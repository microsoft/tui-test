import { test as base, expect } from "@playwright/test";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
import { attachment, fixture, hostile, query, renderReport, writeReport } from "./fixture.mjs";

const test = base.extend({
  openReport: async ({ page }, use, testInfo) => {
    const errors = [];
    const requests = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (!/^(file|data|blob):/.test(request.url())) requests.push(request.url());
    });
    await use(async (data = fixture(), hash = "") => {
      const file = await writeReport(testInfo.outputPath("portable", "failure.html"), data);
      await page.goto("about:blank");
      await page.goto(pathToFileURL(file).href + hash);
      await expect(page.locator("#frame-heading")).toBeVisible();
      await expect(page.locator("#report-error")).toBeHidden();
      return file;
    });
    expect(errors).toEqual([]);
    expect(requests).toEqual([]);
  },
});

async function clickCell(page, column, row) {
  const box = await page.locator("#terminal").boundingBox();
  const data = await page.locator("#report-data").textContent();
  const frames = JSON.parse(data).timeline.frames;
  const index = Number(await page.locator("#frame-slider").inputValue());
  const size = frames[index].size;
  // Fit can make cells barely one CSS pixel wide; round the absolute center before Chromium floors click coordinates.
  await page.locator("#terminal").click({ position: {
    x: Math.round(box.x + (column + .5) * box.width / size.cols) - box.x,
    y: Math.round(box.y + (row + .5) * box.height / size.rows) - box.y,
  } });
}

test("the HTML still works alone after its original directory is removed", async ({ page, openReport }, testInfo) => {
  const original = await openReport();
  const portable = testInfo.outputPath("moved", "report with spaces.html");
  await mkdir(path.dirname(portable), { recursive: true });
  await copyFile(original, portable);
  await page.goto("about:blank");
  await rm(path.dirname(original), { recursive: true });
  await page.goto(pathToFileURL(portable).href);
  await expect(page.locator("#operation")).toHaveText("Foreground mismatch");
  await expect(page.locator("#screen-image")).toBeVisible();
  await expect(page.locator("script[src], link[href], iframe, object")).toHaveCount(0);
  await expect(page.locator("#session-summary")).toContainText("deployment-wizard");
  expect(await page.evaluate(() => globalThis.reportPwned)).toBeUndefined();
});

test("failure explanation and highlights use only the decisive locator stage", async ({ page, openReport }) => {
  await openReport();
  await expect(page.locator("#summary")).toContainText("Failed:");
  await expect(page.locator("#summary-observed")).toContainText("#112233");
  await expect(page.locator("#error-locator")).toContainText('getByText("A").getByStyle({ foreground: "2" })');
  await expect(page.locator(".mismatch-cell")).toHaveCount(1);
  await page.locator("#cell-tab").click();
  await expect(page.locator("#cell-mismatches")).toContainText("foreground / stage 1");
  await expect(page.locator("#cell-mismatches")).toContainText("underline_color / stage 1");
  await expect(page.locator("#cell-properties")).toContainText("1 -> #112233");
});

test("passing actions sharing a failure frame do not inherit mismatches", async ({ page, openReport }) => {
  await openReport();
  await page.locator('#points [data-operation="5"]').click();
  await expect(page.locator("#frame-meta")).toContainText("Screen 3 / also used by failure");
  await expect(page.locator("#expectation-banner")).toHaveAttribute("data-outcome", "passed");
  await expect(page.locator("#call-panel")).toBeVisible();
  await expect(page.locator(".mismatch-cell")).toHaveCount(0);
  await expect(page.locator("#cell-mismatches")).toBeHidden();
  await page.locator("#failure").click();
  await expect(page.locator("#errors-panel")).toBeVisible();
  await expect(page.locator(".mismatch-cell")).toHaveCount(1);
});

test("evicted completion checkpoints never substitute a retained frame", async ({ page, openReport }) => {
  await openReport();
  await page.locator('#points [data-operation="1"]').click();
  await expect(page.locator("#frame-meta")).toContainText("Screen 999 was not retained");
  await expect(page.locator("#screen-image")).toHaveCount(0);
  await expect(page.locator("#screen-text")).toContainText("evicted");
  await expect(page.locator("#frame-count")).toHaveText("Unavailable");
  await expect(page.locator("#inspect")).toBeDisabled();
  await expect(page.locator("#play")).toBeDisabled();
  await expect(page.locator("#call-properties")).toContainText("evidence limit");
  await page.locator("#failure").click();
  await expect(page.locator("#frame-count")).toHaveText("3 / 3");
});

test("filters search locator and input text and drive action navigation", async ({ page, openReport }) => {
  await openReport();
  await page.locator("#action-filter").fill('getByText("READY")');
  await expect(page.locator("#points .point")).toHaveCount(2);
  await page.locator("#next-point").click();
  await expect(page.locator('#points [data-operation="2"]')).toHaveAttribute("aria-current", "true");
  await page.locator("#next-point").click();
  await expect(page.locator('#points [data-operation="3"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#next-point")).toBeDisabled();
  await page.locator("#action-filter").fill("DEPLOY staging");
  await expect(page.locator("#points .point")).toHaveCount(1);
  await page.locator("#assertions-only").check();
  await expect(page.locator("#points")).toHaveText("No matching actions.");
  await expect(page.locator("#previous-point")).toBeDisabled();
  await expect(page.locator("#next-point")).toBeDisabled();
  await page.locator("#action-filter").fill("");
  await expect(page.locator("#points .point")).toHaveCount(5);
});

test("repeated checkpoints remain separate actions and input bytes remain exact", async ({ page, openReport }) => {
  await openReport();
  await page.locator('#action-timeline [data-operation="2"]').click();
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await page.locator("#next-point").click();
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await expect(page.locator("#frame-heading")).toHaveText("ready again");
  await page.locator("#next-point").click();
  await expect(page.locator("#frame-count")).toHaveText("2 / 3");
  await expect(page.locator("#call-properties")).toContainText('"DEPLOY staging"');
  await expect(page.locator("#call-properties")).toContainText("41 0d");
});

test("filmstrip, slider and frame buttons select retained frames independently", async ({ page, openReport }) => {
  await openReport();
  await page.locator('#filmstrip [data-frame="0"]').click();
  await expect(page.locator("#previous-frame")).toBeDisabled();
  await expect(page.locator('#points [aria-current="true"]')).toHaveCount(0);
  await page.locator("#next-frame").click();
  await expect(page.locator("#frame-count")).toHaveText("2 / 3");
  await page.locator("#frame-slider").focus();
  await page.keyboard.press("End");
  await expect(page.locator("#frame-count")).toHaveText("3 / 3");
  await expect(page.locator("#next-frame")).toBeDisabled();
  await page.keyboard.press("Home");
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
});

test("playback stops at the next retained assertion, not the next operation", async ({ page, openReport }) => {
  await page.clock.install();
  await openReport();
  await page.locator('#points [data-operation="2"]').click();
  await page.locator("#play").click();
  await expect(page.locator("#play")).toHaveText("Pause");
  await page.clock.runFor(800);
  await expect(page.locator('#points [data-operation="5"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#play")).toHaveText("Play to next point");
  await expect(page.locator(".mismatch-cell")).toHaveCount(0);
  await page.clock.runFor(2000);
  await expect(page.locator('#points [data-operation="5"]')).toHaveAttribute("aria-current", "true");
});

test("manual selection and hidden documents cancel playback timers", async ({ page, openReport }) => {
  await page.clock.install();
  await openReport();
  await page.locator('#points [data-operation="2"]').click();
  await page.locator("#play").click();
  await page.locator('#points [data-operation="3"]').click();
  await page.clock.runFor(1200);
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await page.locator("#play").click();
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.clock.runFor(1200);
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await expect(page.locator("#play")).toHaveText("Play to next point");
});

test("the pause button and cell inspection stop playback without changing the action", async ({ page, openReport }) => {
  await page.clock.install();
  await openReport();
  await page.locator('#points [data-operation="2"]').click();
  await page.locator("#play").click();
  await page.locator("#play").click();
  await page.clock.runFor(1200);
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await page.locator("#play").click();
  await clickCell(page, 1, 0);
  await page.clock.runFor(1200);
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await expect(page.locator('#points [data-operation="2"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#cell-tab")).toHaveAttribute("aria-selected", "true");
});

test("mouse inputs select their captured pointer rather than the terminal cursor", async ({ page, openReport }) => {
  const data = fixture();
  data.details.recent_operations[3].input = {
    arguments: { kind: "mouse_move", x: 1, y: 0 }, mouse_position: { column: 1, row: 0 }, sent_bytes: [27],
  };
  await openReport(data);
  await page.locator('#points [data-operation="4"]').click();
  await expect(page.locator("#call-properties")).toContainText("Mouse cell (column, row)");
  await page.locator("#cell-tab").click();
  await expect(page.locator("#cell-heading")).toHaveText("Column 1, row 0");
});

test("mouse inspection preserves Unicode, continuation cells and raw metadata", async ({ page, openReport }) => {
  await openReport();
  await clickCell(page, 2, 0);
  await expect(page.locator("#cell-panel")).toBeVisible();
  await expect(page.locator("#cell-heading")).toHaveText("Column 2, row 0 / continuation");
  await expect(page.locator("#cell-properties")).toContainText("(none)");
  const cell = JSON.parse(await page.locator("#cell-json").textContent());
  expect(cell).toMatchObject({ char: "", width: 0, column: 2, row: 0, lead_column: 1 });
  await clickCell(page, 1, 0);
  await expect(page.locator("#cell-properties")).toContainText("U+4F60");
  await clickCell(page, 3, 0);
  await expect(page.locator("#cell-properties")).toContainText("U+0065 U+0301");
  await expect(page.locator("#cell-properties a")).toHaveCount(0);
  expect(await page.evaluate(() => globalThis.reportPwned)).toBeUndefined();
});

test("cell coordinates, keyboard bounds and missing metadata are explicit", async ({ page, openReport }) => {
  await openReport();
  await page.locator("#cell-tab").click();
  await page.locator("#column").fill("999");
  await page.locator("#inspect").click();
  await expect(page.locator("#inspect")).toBeFocused();
  await expect(page.locator("#cell-heading")).toContainText("unavailable");
  await expect(page.locator(".selected-cell")).toHaveCount(0);
  await page.locator("#column").fill("");
  await page.locator("#inspect").click();
  await expect(page.locator("#cell-heading")).toContainText("unavailable");
  await page.locator("#terminal").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#column")).toHaveValue("0");
  await page.keyboard.press("ArrowUp");
  await expect(page.locator("#row")).toHaveValue("0");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#column")).toHaveValue("1");
  await page.locator("#column").fill("0.5");
  await page.locator("#inspect").click();
  await expect(page.locator("#cell-heading")).toContainText("unavailable");
  await page.locator("#terminal").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#column")).toHaveValue("0");
});

test("the terminal's native button activation inspects the selected cell", async ({ page, openReport }) => {
  await openReport();
  await page.locator("#terminal").focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#cell-heading")).toHaveText("Column 1, row 0");
  for (const key of ["Enter", "Space"]) {
    await page.locator("#call-tab").click();
    await page.locator("#terminal").focus();
    await page.keyboard.press(key);
    await expect(page.locator("#cell-panel")).toBeVisible();
    await expect(page.locator("#cell-heading")).toHaveText("Column 1, row 0");
  }
});

test("zoom and responsive fit retain the same cell hit testing and crop", async ({ page, openReport }) => {
  const data = fixture();
  data.timeline.frames[2].size = { cols: 80, rows: 24 };
  await openReport(data);
  for (const zoom of ["1", "1.5", "fit"]) {
    await page.locator("#zoom").selectOption(zoom);
    if (zoom === "fit") await page.setViewportSize({ width: 800, height: 720 });
    await clickCell(page, 2, 0);
    await expect(page.locator("#cell-heading")).toContainText("Column 2, row 0");
    await expect(page.locator("#overlay")).toHaveAttribute("viewBox", "7 11 800 480");
    if (zoom !== "fit") {
      await expect.poll(async () => (await page.locator("#terminal").boundingBox()).width).toBe(800 * Number(zoom));
    } else {
      const dimensions = await page.locator("#viewport").evaluate((viewport) => {
        const terminal = viewport.querySelector("#terminal").getBoundingClientRect();
        const padding = getComputedStyle(viewport);
        return {
          fitsWidth: terminal.width <= viewport.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight) + 1,
          fitsHeight: terminal.height <= viewport.clientHeight - parseFloat(padding.paddingTop) - parseFloat(padding.paddingBottom) + 1,
        };
      });
      expect(dimensions).toEqual({ fitsWidth: true, fitsHeight: true });
    }
  }
});

test("tabs expose metadata and support roving keyboard focus", async ({ page, openReport }) => {
  await openReport();
  await expect(page.locator("#cell-tab")).toHaveAttribute("aria-controls", "cell-panel");
  await page.locator("#actions-tab").focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#metadata-tab")).toBeFocused();
  await expect(page.locator("#metadata-panel")).toBeVisible();
  await expect(page.locator("#session-properties")).toContainText("pwsh");
  await expect(page.locator("#timeout-properties")).toContainText("30000 ms");
  await expect(page.locator("#retention")).toContainText("2 checkpoints evicted");
  await page.keyboard.press("Home");
  await expect(page.locator("#actions-tab")).toBeFocused();
  await page.locator("#errors-tab").focus();
  await page.keyboard.press("End");
  await expect(page.locator("#attachments-tab")).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#errors-tab")).toBeFocused();
});

test("attachments preview inert text and download original bytes offline", async ({ page, openReport }, testInfo) => {
  const data = fixture();
  await openReport(data);
  await page.locator("#attachments-tab").click();
  await page.locator('[data-attachment="failure.md"]').click();
  await expect(page.locator("#attachment-preview pre")).toHaveText(hostile);
  await expect(page.locator("#attachment-preview script")).toHaveCount(0);
  for (const file of data.attachments) {
    const downloaded = page.waitForEvent("download");
    await page.locator(`[data-download="${file.name}"]`).click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toBe(file.name);
    const target = testInfo.outputPath("downloads", file.name);
    await download.saveAs(target);
    expect(await readFile(target)).toEqual(Buffer.from(file.data, "base64"));
  }
  await page.locator('[data-attachment="current.svg"]').click();
  await expect(page.locator("#attachment-preview img")).toBeVisible();
  expect(await page.evaluate(() => globalThis.reportPwned)).toBeUndefined();
});

test("captured SVG markup stays inert in both terminal and attachment previews", async ({ page, openReport }) => {
  const data = fixture();
  const svg = data.timeline.frames[2].svg.replace("</svg>", '<script>globalThis.reportPwned=true</script></svg>');
  data.timeline.frames[2].svg = svg;
  data.attachments[0] = attachment("current.svg", svg);
  await openReport(data);
  await expect(page.locator("#screen-image")).toBeVisible();
  await page.locator("#attachments-tab").click();
  await page.locator('[data-attachment="current.svg"]').click();
  await expect(page.locator("#attachment-preview img")).toBeVisible();
  expect(await page.evaluate(() => globalThis.reportPwned)).toBeUndefined();
});

test("large recording previews are bounded but downloads remain complete", async ({ page, openReport }, testInfo) => {
  const data = fixture();
  const text = '{"version":2}\n' + "\u00e9".repeat(300_000);
  data.attachments.push(attachment("session.cast", text));
  await openReport(data);
  await page.locator("#attachments-tab").click();
  await page.locator('[data-attachment="session.cast"]').click();
  await expect(page.locator("#attachment-preview")).toContainText("Preview limited to 512 KiB");
  await expect(page.locator("#recording-note")).toContainText("complete included session.cast");
  const preview = await page.locator("#attachment-preview pre").textContent();
  expect(Buffer.byteLength(preview)).toBeLessThanOrEqual(512 * 1024);
  expect(preview).not.toContain("\ufffd");
  const downloaded = page.waitForEvent("download");
  await page.locator('[data-download="session.cast"]').click();
  const target = testInfo.outputPath("session.cast");
  await (await downloaded).saveAs(target);
  expect(await readFile(target, "utf8")).toBe(text);
});

for (const [name, svg] of [["malformed SVG", "<svg><invalid"], ["non-SVG XML", "<text/>"],
  ["undisplayable SVG", '<svg xmlns="urn:invalid"/>']]) {
  test(`${name} falls back to terminal text without losing cell evidence`, async ({ page, openReport }) => {
    const data = fixture();
    data.timeline.frames[2].svg = svg;
    await openReport(data);
    await expect(page.locator("#frame-notice")).toContainText("Captured SVG could not");
    await expect(page.locator("#screen-text")).toHaveText("A\u4f60e\u0301");
    await page.locator("#cell-tab").click();
    await expect(page.locator("#cell-properties")).toContainText("#112233");
  });
}

test("omitted grids and screenshots remain explicit and noninteractive", async ({ page, openReport }) => {
  const data = fixture();
  delete data.timeline.frames[2].svg;
  data.timeline.frames[2].grid = [];
  data.timeline.frames[2].cells = [];
  data.timeline.frames[2].omission = "Screen exceeds the byte limit";
  await openReport(data);
  await expect(page.locator("#frame-notice")).toHaveText("Screen exceeds the byte limit");
  await expect(page.locator("#screen-text")).toHaveText("A\u4f60e\u0301");
  await page.locator("#cell-tab").click();
  await expect(page.locator("#inspect")).toBeDisabled();
  await expect(page.locator("#cell-heading")).toContainText("unavailable");
});

test("passed traces use Result and Jump to end without failure annotations", async ({ page, openReport }) => {
  const data = fixture();
  data.details.outcome = "passed";
  data.details.recent_operations.at(-1).result = "ok";
  data.explanation.title = "Trace completed";
  await openReport(data);
  await expect(page.locator("#errors-tab")).toHaveText("Result");
  await expect(page.locator("#failure")).toHaveText("Jump to end");
  await expect(page.locator("#comparison")).toBeHidden();
  await expect(page.locator(".mismatch-cell")).toHaveCount(0);
  await page.locator('#points [data-operation="2"]').click();
  await page.locator("#failure").click();
  await expect(page.locator("#errors-panel")).toBeVisible();
  await expect(page.locator("#frame-count")).toHaveText("3 / 3");
});

test("deep links select exact frames and unknown links return to failure", async ({ page, openReport }) => {
  await openReport(fixture(), "#screen-1");
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  await expect(page.locator('#points [aria-current="true"]')).toHaveCount(0);
  await openReport(fixture(), "#screen-999");
  await expect(page.locator("#frame-count")).toHaveText("3 / 3");
});

test("empty histories and missing optional metadata still render explicitly", async ({ page, openReport }) => {
  const data = fixture();
  data.timeline.frames = [];
  data.details.recent_operations = [];
  delete data.details.runtime;
  delete data.details.terminal;
  delete data.details.process;
  await openReport(data);
  await expect(page.locator("#failure")).toBeDisabled();
  await expect(page.locator("#frame-slider")).toBeDisabled();
  await expect(page.locator("#next-frame")).toBeDisabled();
  await expect(page.locator("#points")).toHaveText("No matching actions.");
  await page.locator("#metadata-tab").click();
  await expect(page.locator("#timeout-properties")).toContainText("Not captured");
});

test("composed locators and input event descriptions remain readable and searchable", async ({ page, openReport }) => {
  const data = fixture();
  const left = query("Parent");
  left.occurrence = { nth: 1 };
  const right = { selector: { kind: "link", selector: { uri: "https://example.invalid", full: false } }, occurrence: "any", direction: "within", style: {} };
  data.details.recent_operations[1].expectation.query = {
    selector: { kind: "filter", selector: {
      input: { selector: { kind: "and", selector: { left, right } }, occurrence: "any", direction: "within", style: {} },
      has_not: query("Disabled"),
    } }, occurrence: "any", direction: "within", style: {},
  };
  data.details.recent_operations[3].input = { arguments: { kind: "key", keys: ["Control", "A"], action: "up" }, sent_bytes: [] };
  await openReport(data);
  await page.locator('#points [data-operation="2"]').click();
  await expect(page.locator("#call-locator")).toContainText('getByText("Parent").nth(1).and(getByLink("https://example.invalid")).filter({ hasNot: getByText("Disabled") })');
  await page.locator('#points [data-operation="4"]').click();
  await expect(page.locator("#frame-heading")).toHaveText("keyboard.up");
  await expect(page.locator("#call-properties")).toContainText("did not emit this event");
});

test("corrupt report data produces a visible error, not a blank successful report", async ({ page }, testInfo) => {
  const html = (await renderReport(fixture())).replace(/(<script id="report-data" type="application\/json">)[\s\S]*?(<\/script>)/, "$1{invalid$2");
  const file = testInfo.outputPath("invalid.html");
  await writeFile(file, html);
  await page.goto(pathToFileURL(file).href);
  await expect(page.locator("#report-error")).toContainText("Report viewer error:");
  await expect(page.locator("#report-error")).toContainText("embedded report-data");
});

test("invalid structured data identifies the offending field in the report", async ({ page }, testInfo) => {
  const data = fixture();
  data.timeline.geometry.cell_height = 0;
  const file = await writeReport(testInfo.outputPath("invalid-shape.html"), data);
  await page.goto(pathToFileURL(file).href);
  await expect(page.locator("#report-error")).toContainText("report.timeline.geometry.cell_height");
  await expect(page.locator("#workbench")).toBeEmpty();
});

test("Svelte updates preserve input focus, caret, keyed actions and native disclosures", async ({ page, openReport }) => {
  await openReport();
  const filter = page.locator("#action-filter");
  await filter.focus();
  await page.keyboard.type("ready");
  await expect(filter).toBeFocused();
  expect(await filter.evaluate((input) => input.selectionStart)).toBe(5);
  await page.locator('#points [data-operation="2"]').click();
  const point = await page.locator('#points [data-operation="2"]').elementHandle();
  await page.locator("#call-panel summary").click();
  await page.locator("#next-point").click();
  await expect(page.locator("#call-panel details")).toHaveAttribute("open", "");
  expect(await point.evaluate((element) => element.isConnected)).toBe(true);
  await page.locator("#cell-tab").click();
  const column = page.locator("#column");
  await column.fill("1");
  await expect(column).toBeFocused();
});

test("display failures retain text and cell evidence, and attachment failures retain downloads", async ({ page, openReport }) => {
  const data = fixture();
  data.attachments[0] = attachment("current.svg", '<svg xmlns="urn:invalid"/>');
  await openReport(data);
  await page.locator("#screen-image").evaluate((image) => image.dispatchEvent(new Event("error")));
  await expect(page.locator("#frame-notice")).toContainText("Captured SVG could not");
  await expect(page.locator("#screen-text")).toHaveText("A\u4f60e\u0301");
  await page.locator("#cell-tab").click();
  await expect(page.locator("#cell-properties")).toContainText("#112233");
  await page.locator("#attachments-tab").click();
  await page.locator('[data-attachment="current.svg"]').click();
  await expect(page.locator("#attachment-preview")).toContainText("Download retains the original attachment bytes");
  await expect(page.locator('[data-download="current.svg"]')).toHaveAttribute("download", "current.svg");
});

test("the Svelte lifecycle cancels playback and revokes its URLs on disposal", async ({ page, openReport }) => {
  await page.clock.install();
  await page.addInitScript(() => {
    const create = URL.createObjectURL.bind(URL);
    const revoke = URL.revokeObjectURL.bind(URL);
    globalThis.viewerUrls = { created: [], revoked: [] };
    URL.createObjectURL = (blob) => { const url = create(blob); globalThis.viewerUrls.created.push(url); return url; };
    URL.revokeObjectURL = (url) => { globalThis.viewerUrls.revoked.push(url); revoke(url); };
  });
  await openReport();
  await page.locator('#points [data-operation="2"]').click();
  await page.locator("#play").click();
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pagehide", { persisted: false })));
  await page.clock.runFor(1200);
  await expect(page.locator("#frame-count")).toHaveText("1 / 3");
  const urls = await page.evaluate(() => globalThis.viewerUrls);
  expect(urls.created.length).toBeGreaterThan(0);
  expect(urls.revoked).toEqual(urls.created);
});

test("the shipped shell stays within budget with no external assets or development runtime", async () => {
  const html = await renderReport(null);
  expect(Buffer.byteLength(html)).toBeLessThanOrEqual(144 * 1024);
  expect(gzipSync(html, { level: 9 }).length).toBeLessThanOrEqual(48 * 1024);
  expect(html.match(/<\/script>/g)).toHaveLength(2);
  expect(html).toContain("connect-src 'none'");
  const manifest = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  expect(Object.keys(manifest.dependencies ?? {})).toEqual(["svelte"]);
  expect(manifest.dependencies.svelte).toMatch(/^5\./);
  expect(html).toContain("Permission is hereby granted");
  expect(html).toContain("Svelte");
  expect(html).not.toMatch(/react-dom|react\.production|react\.development|__REACT/);
  expect(html).not.toContain("sourceMappingURL");
  expect(html).not.toMatch(/<script[^>]+src=|<link[^>]+href=/);
});
