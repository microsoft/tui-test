import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, afterEach, before, beforeEach, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const repo = fileURLToPath(new URL("../../../", import.meta.url));
const embedded = /(<script id="report-data" type="application\/json">)([\s\S]*?)(<\/script>)/;
const expectedFiles = new Map();
let root, browser, context, page, reportPath, source, pageErrors, externalRequests;

before(async () => {
  root = await mkdtemp(path.join(tmpdir(), "tui-test-report-browser-"));
  const fixture = spawnSync("cargo", [
    "test", "-p", "tui-test-rs", "--lib",
    "diagnostics::report::tests::write_browser_fixture", "--", "--exact", "--ignored",
  ], {
    cwd: repo, encoding: "utf8", timeout: 180_000,
    env: { ...process.env, TUI_TEST_REPORT_FIXTURE_DIR: root },
  });
  assert.equal(fixture.status, 0, `${fixture.error || ""}\n${fixture.stdout}\n${fixture.stderr}`);
  const bundle = path.join(root, (await readdir(root)).find((name) => name.startsWith("failure-")));
  for (const name of await readdir(bundle)) expectedFiles.set(name, await readFile(path.join(bundle, name)));
  source = expectedFiles.get("failure.html").toString("utf8");
  await mkdir(path.join(root, "isolated"));
  reportPath = path.join(root, "isolated", "renamed.html");
  await writeFile(reportPath, source);
  await rm(bundle, { recursive: true });
  browser = await chromium.launch({
    headless: true,
    ...(process.env.TUI_TEST_BROWSER_CHANNEL ? { channel: process.env.TUI_TEST_BROWSER_CHANNEL } : {}),
  });
});

beforeEach(async () => {
  pageErrors = [];
  externalRequests = [];
  context = await browser.newContext({ offline: true, viewport: { width: 1440, height: 1000 } });
  page = await context.newPage();
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("request", (request) => {
    if (/^https?:/.test(request.url()) || (request.url().startsWith("file:") && request.resourceType() !== "document")) {
      externalRequests.push(request.url());
    }
  });
  await page.goto(pathToFileURL(reportPath).href);
  await page.waitForFunction(() => document.getElementById("screen-image").naturalWidth > 0);
});

afterEach(async () => {
  await context?.close();
  assert.deepEqual(pageErrors, [], "no unhandled browser errors");
  assert.deepEqual(externalRequests, [], "no remote assets or sibling files");
});

after(async () => {
  await browser?.close();
  if (root) await rm(root, { recursive: true, force: true });
});

const rawCell = async () => JSON.parse(await page.locator("#cell-json").textContent());
const frame = async () => JSON.parse(await page.locator("#frame-details").textContent());
const point = (sequence) => page.locator(`#points [data-operation="${sequence}"]`);

test("explains expected and observed style values next to the pinned failure", async () => {
  assert.match(await page.locator("#frame-meta").textContent(), /Screen 3 \/ failure/);
  assert.equal(await page.locator("#operation").textContent(), "Foreground mismatch");
  assert.match(await page.locator("#expected").textContent(), /"A": foreground equals ANSI 2 \(green slot\)/);
  assert.match(await page.locator("#actual").textContent(), /Text matched.*ANSI 1 \(red slot\).*#112233/);
  assert.match(await page.locator("#summary").textContent(), /ANSI 2/);
  assert.equal((await frame()).frame.size.cols, 24);
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 1);
  assert.equal((await rawCell()).resolved_fg, "#112233");
  assert.equal(await page.evaluate(() => globalThis.reportPwned), undefined);
  assert.equal(await page.locator("script").count(), 2);
  assert.equal(await page.locator('img[src="x"]').count(), 0);
  assert.match(await page.locator("#raw-error").textContent(), /<\/script><script>/);
  assert.equal(await page.locator("#report-error").isVisible(), false);
  assert.equal(await page.locator("#before, #after").count(), 0);
});

test("session, emulator, timeout defaults and elapsed time have labelled metadata", async () => {
  assert.match(await page.locator("#session-summary").textContent(), /Session: deployment-wizard/);
  assert.match(await page.locator("#session-summary").textContent(), /Emulator: alacritty/);
  assert.doesNotMatch(await page.locator(".topbar").textContent(), /timeout/i);
  await page.locator("#metadata-tab").click();
  assert.match(await page.locator("#session-properties").textContent(), /pwsh/);
  assert.match(await page.locator("#session-properties").textContent(), /Process ID123/);
  assert.match(await page.locator("#timeout-properties").textContent(), /text5000 ms/);
  assert.match(await page.locator("#timeout-properties").textContent(), /command30000 ms/);
  assert.match(await page.locator("#session-properties").textContent(), /Assertion timeout20 ms/);
  await page.locator("#metadata-tab").focus();
  await page.keyboard.press("ArrowLeft");
  assert.equal(await page.locator("#actions-tab").getAttribute("aria-selected"), "true");
});

test("filters actions and navigates repeated checkpoints, filmstrip and duration bars", async () => {
  assert.equal(await page.locator("#points button").count(), 5);
  await page.locator("#action-filter").fill("ready");
  assert.equal(await page.locator("#points button").count(), 2);
  await page.locator("#action-filter").fill("");
  await page.locator("#assertions-only").check();
  assert.equal(await page.locator("#points button").count(), 4);
  await point(2).click();
  await page.locator("#next-point").click();
  assert.match(await page.locator("#frame-heading").textContent(), /ready again/);
  assert.equal((await frame()).frame.sequence, 1);
  await page.locator('#filmstrip [data-frame="1"]').click();
  assert.equal((await frame()).frame.sequence, 2);
  assert.equal((await frame()).operation, null);
  assert.match((await frame()).frame.text, /WORKING/);
  await page.locator('#action-timeline [data-operation="4"]').click();
  assert.equal((await frame()).operation.name, "submit");
  await page.locator("#failure").click();
  assert.equal((await frame()).frame.sequence, 3);
  await page.locator("#frame-slider").focus();
  await page.keyboard.press("Home");
  assert.equal((await frame()).frame.sequence, 1);
});

test("missing checkpoints clear stale terminal and cell evidence", async () => {
  await point(1).click();
  assert.match(await page.locator("#frame-meta").textContent(), /999 was not retained/);
  assert.equal(await page.locator("#terminal").isVisible(), false);
  assert.equal(await page.locator("#cell-json").textContent(), "");
  assert.equal(await page.locator("#cell-properties dd").count(), 0);
  assert.equal(await page.locator("#inspect").isDisabled(), true);
  await page.locator("#failure").click();
  assert.equal(await page.locator("#terminal").isVisible(), true);
});

test("passing assertions sharing a failure frame do not inherit failure annotations", async () => {
  await variant("shared-screen.html", (payload) => {
    payload.details.recent_operations[2].screen_at_return = payload.timeline.failure_screen_sequence;
  });
  await point(3).click();
  assert.equal((await frame()).operation.result, "ok");
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 0);
  assert.equal(await page.locator("#expectation-banner").getAttribute("data-outcome"), "passed");
  assert.match(await page.locator("#summary").textContent(), /Passed: Expect text "READY" to be visible/);
  assert.doesNotMatch(await page.locator("#summary").textContent(), /foreground|ANSI/);
  assert.match(await page.locator("#frame-meta").textContent(), /also used by failure/);
  await page.locator("#failure").click();
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 1);
});

test("passing locator expectations are visible, searchable, and included in Call details", async () => {
  await point(2).click();
  assert.match(await page.locator("#summary").textContent(), /Expect text "READY" to be visible/);
  await page.locator("#call-tab").click();
  assert.match(await page.locator("#call-properties").textContent(), /ExpectedExpect text "READY" to be visible/);
  await page.locator("#action-filter").fill('"READY"');
  assert.equal(await page.locator("#points button").count(), 2);
});

test("style, relative scope, occurrence and negation remain explicit for passing locators", async () => {
  await variant("style-expectation.html", (payload) => {
    const original = payload.details.recent_operations[1].expectation.query;
    payload.details.recent_operations[1].expectation = {
      kind: "locator", outcome: "hidden",
      query: {
        selector: { kind: "style", selector: { style: { foreground: "2", bold: true, link: "https://example.test/" }, full: true } },
        style: { italic: false }, direction: "after", occurrence: { nth: 1 }, within: original,
      },
    };
  });
  await point(2).click();
  const expected = await page.locator("#summary").textContent();
  for (const fragment of ["cells matching", "ANSI 2 (green slot)", "bold=true", 'link="https://example.test/"', "full scrollback", "italic=false", "nth(1)", 'after [text "READY"]', "to be absent"]) {
    assert.ok(expected.includes(fragment), `${fragment} is part of the recorded assertion`);
  }
});

test("older and oversized expectations are explicitly unavailable rather than inferred", async () => {
  await variant("old-expectation.html", (payload) => {
    delete payload.details.recent_operations[1].expectation;
    payload.details.recent_operations[2].expectation = { kind: "unavailable", reason: "Expectation exceeded the 8192-byte retention limit" };
  });
  await point(2).click();
  assert.match(await page.locator("#summary").textContent(), /Expectation not captured/);
  assert.doesNotMatch(await page.locator("#summary").textContent(), /foreground|ANSI/);
  await point(3).click();
  assert.match(await page.locator("#summary").textContent(), /8192-byte retention limit/);
});

test("header labels have explicit spacing without overlap at narrower widths", async () => {
  for (const width of [1440, 850, 560]) {
    await page.setViewportSize({ width, height: 1000 });
    const fits = await page.locator(".metadata-item").evaluateAll((items) => items.every((item) => {
      const label = item.querySelector(".metadata-label").getBoundingClientRect();
      const value = item.querySelector(".metadata-value").getBoundingClientRect();
      return value.left >= label.right + 5 && value.right <= innerWidth;
    }));
    assert.equal(fits, true);
    assert.doesNotMatch(await page.locator(".topbar").textContent(), /timeout/i);
  }
});

test("header text shares a vertical center across font sizes and zoom levels", async () => {
  for (const zoom of [1, 1.25, 1.5]) {
    const measurements = await page.evaluate((zoom) => {
      document.body.style.zoom = String(zoom);
      const header = document.querySelector(".topbar");
      const bounds = header.getBoundingClientRect();
      const style = getComputedStyle(header);
      const center = bounds.y + (bounds.height - parseFloat(style.borderBottomWidth) * zoom) / 2;
      const walker = document.createTreeWalker(header, NodeFilter.SHOW_TEXT);
      const offsets = [];
      while (walker.nextNode()) {
        if (!walker.currentNode.textContent.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(walker.currentNode);
        const rect = range.getBoundingClientRect();
        if (rect.height) offsets.push({
          text: walker.currentNode.textContent.trim(),
          offset: Math.abs(rect.y + rect.height / 2 - center) / zoom,
        });
      }
      return { lineHeight: style.lineHeight, offsets };
    }, zoom);
    assert.equal(measurements.lineHeight, "20px");
    assert.ok(measurements.offsets.length >= 8);
    for (const { text, offset } of measurements.offsets) {
      assert.ok(offset <= 1, `${text} is ${offset}px off center at ${zoom}x zoom`);
    }
  }
});

test("the standalone report executes the committed minified assets", async () => {
  const javascript = await readFile(path.join(repo, "crates", "tui-test", "src", "diagnostics", "report.min.js"), "utf8");
  const stylesheet = await readFile(path.join(repo, "crates", "tui-test", "src", "diagnostics", "report.min.css"), "utf8");
  assert.ok(source.includes(javascript));
  assert.ok(source.includes(stylesheet));
  assert.ok(javascript.split("\n").length < 5);
  assert.ok(stylesheet.split("\n").length < 5);
});

test("cropped terminal hit testing handles zoom, responsive scaling and Unicode widths", async () => {
  for (const width of [1440, 560]) {
    await page.setViewportSize({ width, height: 1000 });
    const bounds = await page.locator("#terminal").boundingBox();
    await page.mouse.click(bounds.x + 2.5 * bounds.width / 24, bounds.y + .5 * bounds.height / 5);
    const cell = await rawCell();
    assert.equal(cell.column, 2);
    assert.equal(cell.row, 0);
    assert.equal(cell.char, "");
    assert.equal(cell.width, 0);
    assert.equal(cell.lead_column, 1);
    assert.equal(await page.locator("#cell-tab").getAttribute("aria-selected"), "true");
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.locator("#zoom").selectOption("1.5");
  assert.equal(await page.locator("#screen-image").evaluate((img) => img.naturalWidth), 240);
  await page.locator("#terminal").focus();
  await page.keyboard.press("ArrowLeft");
  assert.equal((await rawCell()).char, "\u4f60");
  assert.equal((await rawCell()).width, 2);
  await page.locator("#column").fill("3");
  await page.locator("#row").fill("0");
  await page.locator("#inspect").click();
  assert.equal((await rawCell()).char, "e\u0301");
  assert.deepEqual((await rawCell()).codepoints, ["U+0065", "U+0301"]);
  await page.locator("#row").fill("1");
  await page.locator("#inspect").click();
  assert.equal((await rawCell()).char, " ");
});

test("playback visits intermediate frames and stops at the next retained assertion", async () => {
  await point(2).click();
  await page.evaluate(() => {
    globalThis.visitedFrames = [];
    new MutationObserver(() => globalThis.visitedFrames.push(document.getElementById("frame-meta").textContent))
      .observe(document.getElementById("frame-meta"), { childList: true });
  });
  await page.locator("#play").click();
  await page.waitForFunction(() => document.getElementById("play").textContent === "Play to next point");
  assert.ok(await page.evaluate(() => globalThis.visitedFrames.some((value) => value.startsWith("Screen 2 "))));
  assert.equal((await frame()).frame.sequence, 3);
  assert.equal((await frame()).operation.sequence, 5);
});

test("Fit keeps every terminal row visible when the trace pane becomes shorter", async () => {
  await page.setViewportSize({ width: 1440, height: 720 });
  await page.waitForFunction(() => {
    const viewport = document.getElementById("viewport");
    return viewport.scrollHeight <= viewport.clientHeight && viewport.scrollWidth <= viewport.clientWidth;
  });
  const cell = await page.locator("#terminal").boundingBox();
  assert.ok(cell.height < 105, "the five-row terminal must scale down, not clip its bottom row");
});

test("standalone HTML previews and downloads every embedded file byte-for-byte", async () => {
  await page.locator("#attachments-tab").click();
  for (const name of ["current.txt", "current.svg", "failure.md", "timeline.json", "session.cast"]) {
    const downloadEvent = page.waitForEvent("download");
    await page.locator(`[data-download="${name}"]`).click();
    const download = await downloadEvent;
    assert.equal(download.suggestedFilename(), name);
    const destination = path.join(root, `download-${name}`);
    await download.saveAs(destination);
    assert.deepEqual(await readFile(destination), expectedFiles.get(name));
  }
  await page.locator('[data-attachment="current.svg"]').click();
  await page.waitForFunction(() => document.querySelector("#attachment-preview img")?.naturalWidth > 0);
  await page.locator('[data-attachment="session.cast"]').click();
  assert.match(await page.locator("#attachment-preview").textContent(), /"version":2/);
  await page.locator('[data-attachment="failure.json"]').click();
  const snapshot = JSON.parse(await page.locator("#attachment-preview pre").textContent());
  assert.equal(snapshot.runtime.session_name, "deployment-wizard");
  assert.ok(snapshot.files.some((file) => file.path === "session.cast"));
  assert.ok(!snapshot.files.some((file) => file.path === "failure.html"), "manifest snapshot excludes its own HTML hash");
  assert.equal(await page.locator('a[href^="http"], a[href="current.svg"], a[href="failure.md"]').count(), 0);
});

async function variant(name, change) {
  const payload = JSON.parse(source.match(embedded)[2]);
  change(payload);
  const encoded = JSON.stringify(payload).replace(/[<>&]/g, (char) =>
    `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`);
  const filename = path.join(root, "isolated", name);
  await writeFile(filename, source.replace(embedded, (_, start, data, end) => start + encoded + end));
  await page.goto(pathToFileURL(filename).href);
}

test("omitted SVG permits coordinate inspection and empty timelines explain missing evidence", async () => {
  await variant("no-svg.html", (payload) => {
    const frame = payload.timeline.frames.at(-1);
    delete frame.svg;
    frame.omission = "SVG omitted: timeline byte limit. Cell metadata is retained.";
  });
  assert.equal(await page.locator("#terminal").isVisible(), false);
  assert.equal(await page.locator("#screen-text").isVisible(), true);
  assert.match(await page.locator("#frame-notice").textContent(), /SVG omitted/);
  await page.locator("#cell-tab").click();
  await page.locator("#column").fill("2");
  await page.locator("#row").fill("0");
  await page.locator("#inspect").click();
  assert.equal((await rawCell()).width, 0);
  await variant("empty.html", (payload) => {
    payload.timeline.frames = [];
    payload.details.recent_operations = [];
  });
  assert.equal(await page.locator("#failure").isDisabled(), true);
  assert.match(await page.locator("#screen-text").textContent(), /could not be captured/);
});

test("one invalid screenshot does not prevent inspection of the rest of the trace", async () => {
  await variant("invalid-svg.html", (payload) => { payload.timeline.frames[0].svg = "<svg invalid"; });
  assert.match(await page.locator("#frame-meta").textContent(), /Screen 3 \/ failure/);
  await point(2).click();
  assert.match(await page.locator("#frame-notice").textContent(), /SVG could not be rendered/);
  assert.equal(await page.locator("#terminal").isVisible(), false);
  assert.match(await page.locator("#screen-text").textContent(), /READY/);
  await page.locator("#failure").click();
  assert.equal(await page.locator("#terminal").isVisible(), true);
});

test("trace panes fit the viewport in both color schemes", async () => {
  for (const colorScheme of ["light", "dark"]) {
    await page.emulateMedia({ colorScheme });
    for (const width of [1440, 560]) {
      await page.setViewportSize({ width, height: 1000 });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    }
  }
  if (process.env.TUI_TEST_REPORT_SCREENSHOT) {
    await page.setViewportSize({ width: 1600, height: 1000 });
    await page.emulateMedia({ colorScheme: "light" });
    await page.screenshot({ path: process.env.TUI_TEST_REPORT_SCREENSHOT, fullPage: true });
  }
});
