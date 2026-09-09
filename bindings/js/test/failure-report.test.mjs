import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, afterEach, before, beforeEach, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const repo = fileURLToPath(new URL("../../../", import.meta.url));
let root, browser, context, page, reportPath, source;
let pageErrors, networkRequests;
const embedded = /(<script id="report-data" type="application\/json">)([\s\S]*?)(<\/script>)/;

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
  const directory = (await readdir(root)).find((name) => name.startsWith("failure-"));
  assert.ok(directory, "Rust must generate the real artifact bundle");
  reportPath = path.join(root, directory, "failure.html");
  source = await readFile(reportPath, "utf8");
  browser = await chromium.launch({
    headless: true,
    ...(process.env.TUI_TEST_BROWSER_CHANNEL ? { channel: process.env.TUI_TEST_BROWSER_CHANNEL } : {}),
  });
});

beforeEach(async () => {
  pageErrors = [];
  networkRequests = [];
  context = await browser.newContext({ offline: true, viewport: { width: 1440, height: 1000 } });
  page = await context.newPage();
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("request", (request) => {
    if (/^https?:/.test(request.url())) networkRequests.push(request.url());
  });
  await page.goto(pathToFileURL(reportPath).href);
  await page.waitForFunction(() => document.getElementById("screen-image").naturalWidth > 0);
});

afterEach(async () => {
  await context?.close();
  assert.deepEqual(pageErrors, [], "no unhandled browser errors");
  assert.deepEqual(networkRequests, [], "reports must not request remote assets");
});

after(async () => {
  await browser?.close();
  if (root) await rm(root, { recursive: true, force: true });
});

const rawCell = async () => JSON.parse(await page.locator("#cell-json").textContent());
const frame = async () => JSON.parse(await page.locator("#frame-details").textContent());
const point = (sequence) => page.locator(`[data-operation="${sequence}"]`);

test("opens at pinned failure, exposes mismatch evidence, and keeps captured markup inert", async () => {
  assert.match(await page.locator("#frame-meta").textContent(), /Screen 3 \/ PINNED FAILURE/);
  assert.equal((await frame()).frame.size.cols, 24);
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 1);
  assert.equal((await rawCell()).resolved_fg, "#112233");
  assert.match(await page.locator("#cell-mismatches").textContent(), /#00ff00/);
  assert.equal(await page.evaluate(() => globalThis.reportPwned), undefined);
  assert.equal(await page.locator("script").count(), 2);
  assert.equal(await page.locator('img[src="x"]').count(), 0);
  assert.match(await page.locator("#summary").textContent(), /<\/script><script>/);
  assert.equal(await page.locator("#report-error").isVisible(), false);
});

test("navigates repeated assertions, operation boundaries, and intermediate frames", async () => {
  await point(2).click();
  assert.equal((await frame()).frame.sequence, 1);
  await page.locator("#next-point").click();
  assert.match(await page.locator("#frame-heading").textContent(), /ready again/);
  assert.equal((await frame()).frame.sequence, 1);
  await page.locator("#next-frame").click();
  assert.equal((await frame()).frame.sequence, 2);
  assert.equal((await frame()).operation, null, "stepping must not leave stale assertion metadata");
  await page.locator("#failure").click();
  await page.locator("#before").click();
  assert.equal((await frame()).frame.sequence, 2);
  assert.match((await frame()).frame.text, /WORKING/);
  await page.locator("#after").click();
  assert.equal((await frame()).frame.sequence, 3);
  await page.locator("#all-operations").check();
  await point(4).click();
  assert.equal((await frame()).operation.name, "submit");
  await page.locator("#frame-slider").focus();
  await page.keyboard.press("Home");
  assert.equal((await frame()).frame.sequence, 1);
});

test("missing checkpoints show an explicit gap and clear the previous cell and terminal", async () => {
  await point(1).click();
  assert.match(await page.locator("#frame-meta").textContent(), /999 was not retained/);
  assert.equal(await page.locator("#terminal").isVisible(), false);
  assert.equal(await page.locator("#cell-json").textContent(), "");
  assert.equal(await page.locator("#cell-properties dd").count(), 0);
  assert.equal(await page.locator("#inspect").isDisabled(), true);
  await page.locator("#failure").click();
  assert.equal(await page.locator("#terminal").isVisible(), true);
});

test("passing assertions sharing the failure screen do not inherit its mismatch markers", async () => {
  await variant("shared-screen.html", (payload) => {
    payload.details.recent_operations[2].screen_at_return = payload.timeline.failure_screen_sequence;
  });
  await point(3).click();
  assert.equal((await frame()).frame.sequence, 3);
  assert.equal((await frame()).operation.result, "ok");
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 0);
  assert.equal(await page.locator("#cell-mismatches").isVisible(), false);
  assert.match(await page.locator("#frame-meta").textContent(), /also used by failure/);
  await page.locator("#failure").click();
  assert.equal(await page.locator("#overlay .mismatch-cell").count(), 1);
});

test("cell hit testing stays aligned after resize and responsive scaling", async () => {
  for (const width of [1440, 560]) {
    await page.setViewportSize({ width, height: 1000 });
    const geometry = await page.evaluate(() => {
      const { timeline } = JSON.parse(document.getElementById("report-data").textContent);
      const terminal = document.getElementById("terminal");
      const bounds = terminal.getBoundingClientRect();
      const viewBox = document.getElementById("overlay").viewBox.baseVal;
      return {
        x: bounds.x + (timeline.geometry.grid_x + 2.5 * timeline.geometry.cell_width) * bounds.width / viewBox.width,
        y: bounds.y + (timeline.geometry.grid_y + 0.5 * timeline.geometry.cell_height) * bounds.height / viewBox.height,
      };
    });
    await page.mouse.click(geometry.x, geometry.y);
    const cell = await rawCell();
    assert.equal(cell.column, 2);
    assert.equal(cell.row, 0);
    assert.equal(cell.char, "");
    assert.equal(cell.width, 0);
    assert.equal(cell.lead_column, 1);
  }
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

test("sampled playback visits intermediate frames and stops at the next retained assertion", async () => {
  await point(2).click();
  await page.locator("#play").click();
  await page.waitForFunction(() => document.getElementById("frame-meta").textContent.startsWith("Screen 2 "));
  await page.waitForFunction(() => document.getElementById("play").textContent === "Play to next point");
  assert.equal((await frame()).frame.sequence, 3);
  assert.equal((await frame()).operation.sequence, 5);
});

async function variant(name, change) {
  const payload = JSON.parse(source.match(embedded)[2]);
  change(payload);
  const encoded = JSON.stringify(payload).replace(/[<>&]/g, (char) =>
    `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`);
  const filename = path.join(root, name);
  await writeFile(filename, source.replace(embedded, (_, start, data, end) => start + encoded + end));
  await page.goto(pathToFileURL(filename).href);
}

test("self-contained report works without siblings, and omitted SVG still permits coordinate inspection", async () => {
  await variant("isolated.html", (payload) => {
    const frame = payload.timeline.frames.at(-1);
    delete frame.svg;
    frame.omission = "SVG omitted: timeline byte limit. Cell metadata is retained.";
  });
  assert.equal(await page.locator("#terminal").isVisible(), false);
  assert.equal(await page.locator("#screen-text").isVisible(), true);
  assert.match(await page.locator("#frame-notice").textContent(), /SVG omitted/);
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

test("viewer has no horizontal page overflow and can produce a visual reference", async () => {
  for (const width of [1440, 560]) {
    await page.setViewportSize({ width, height: 1000 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  }
  if (process.env.TUI_TEST_REPORT_SCREENSHOT) {
    await page.setViewportSize({ width: 1600, height: 1100 });
    await page.screenshot({ path: process.env.TUI_TEST_REPORT_SCREENSHOT, fullPage: true });
  }
});
