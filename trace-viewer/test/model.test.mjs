import { test } from "node:test";
import assert from "node:assert/strict";
import { attachment, fixture } from "./fixture.mjs";
import { parseReport, cellAt, selection, visibleActions, inspectCell } from "../src/model.js";
import { initialState, update } from "../src/state.js";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { sveltePlugin } from "../svelte-plugin.mjs";

const parse = (data = fixture()) => parseReport(JSON.stringify(data));
function freeze(value) {
  if (!value || typeof value !== "object" || ArrayBuffer.isView(value)) return value;
  for (const child of Object.values(value)) freeze(child);
  return Object.freeze(value);
}

test("normalization exposes display models, preserves exact input, and never guesses a missing screen", async () => {
  const model = parse();
  assert.equal(model.title, "tui-test: deployment-wizard");
  assert.equal(model.frames[0].viewBox, "7 11 200 80");
  assert.equal(model.actions[0].frameIndex, undefined);
  assert.equal(model.actions[3].label, '"DEPLOY staging"');
  assert.equal(model.actions[3].properties["Bytes (hex)"], "41 0d");
  assert.deepEqual(model.actions.slice(1, 3).map((action) => action.frameIndex), [0, 0]);
  assert.equal(model.failureActionId, 6);
  assert.equal(model.mismatches.length, 2);
  assert.ok(model.mismatches.every((entry) => entry.stage === 1 && entry.x === 0 && entry.y === 0));
  assert.equal(model.result.locator, 'getByText("A").getByStyle({ foreground: "2" })');
  assert.deepEqual(new Uint8Array(await model.attachments[0].blob.arrayBuffer()), new Uint8Array(Buffer.from(fixture().attachments[0].data, "base64")));
});

test("optional histories and metadata normalize to explicit empty or unavailable values", () => {
  const data = fixture();
  delete data.details.recent_operations;
  delete data.details.runtime;
  delete data.details.terminal;
  delete data.details.locator;
  delete data.details.process;
  data.timeline.frames = [];
  const model = parse(data);
  assert.deepEqual(model.actions, []);
  assert.deepEqual(model.mismatches, []);
  assert.equal(model.failureFrameIndex, undefined);
  assert.equal(model.metadata.timeouts.Defaults, "Not captured");
  assert.equal(selection(model, initialState(model)).frame, undefined);
});

for (const [name, change, error] of [
  ["unsupported report version", (data) => { data.details.schema_version = 2; }, /report.details.schema_version/],
  ["unsupported timeline version", (data) => { data.timeline.schema_version = 2; }, /report.timeline.schema_version/],
  ["missing frame text", (data) => { delete data.timeline.frames[0].text; }, /frames\[0\].text/],
  ["invalid geometry", (data) => { data.timeline.geometry.cell_width = 0; }, /geometry.cell_width/],
  ["invalid dimensions", (data) => { data.timeline.frames[0].size.cols = 1.5; }, /size.cols/],
  ["invalid grid", (data) => { data.timeline.frames[0].grid[0][0] = 999; }, /invalid cell grid/],
  ["duplicate frame IDs", (data) => { data.timeline.frames[1].sequence = 1; }, /Duplicate screen sequence/],
  ["duplicate operation IDs", (data) => { data.details.recent_operations[1].sequence = 1; }, /Duplicate action sequence/],
  ["invalid operation time", (data) => { data.details.recent_operations[0].ended_ms = 0; }, /end precedes start/],
  ["invalid expectation", (data) => { data.details.recent_operations[0].expectation.kind = "unknown"; }, /expectation.kind/],
  ["invalid input variant", (data) => { data.details.recent_operations[3].input.arguments.kind = "unknown"; }, /arguments.kind/],
  ["invalid input bytes", (data) => { data.details.recent_operations[3].input.sent_bytes = [256]; }, /sent_bytes\[0\]/],
  ["invalid locator kind", (data) => { data.details.recent_operations[1].expectation.query.selector.kind = "unknown"; }, /selector.kind/],
  ["invalid cell color", (data) => { data.timeline.frames[0].cells[0].fg = {}; }, /cells\[0\].fg/],
  ["invalid base64", (data) => { data.attachments[0].data = "????"; }, /invalid base64/],
  ["wrong attachment size", (data) => { data.attachments[0].bytes = 0; }, /byte count does not match/],
  ["duplicate attachments", (data) => { data.attachments.push(data.attachments[0]); }, /Duplicate attachment/],
  ["null optional object", (data) => { data.details.runtime = null; }, /report.details.runtime/],
]) {
  test(`malformed evidence fails explicitly: ${name}`, () => {
    const data = JSON.parse(JSON.stringify(fixture()));
    change(data);
    assert.throws(() => parse(data), error);
  });
}

test("attachment preview limits do not change original bytes or split a UTF-8 character", async () => {
  const data = fixture();
  const content = "x" + "\u00e9".repeat(300_000);
  data.attachments.push(attachment("session.cast", content));
  const file = parse(data).attachments.at(-1);
  assert.equal(file.truncated, true);
  assert.ok(Buffer.byteLength(file.preview) <= 512 * 1024);
  assert.ok(!file.preview.includes("\ufffd"));
  assert.equal(await file.blob.text(), content);
});

test("the full 64 MiB recording allowance uses a single Blob and bounded text", async () => {
  const data = fixture();
  const size = 64 * 1024 * 1024;
  data.attachments = [attachment("session.cast", "x".repeat(size))];
  const file = parse(data).attachments[0];
  assert.equal(file.byteLength, size);
  assert.equal(file.blob.size, size);
  assert.equal(await file.blob.slice(-1).text(), "x");
  assert.equal(Object.values(file).some((value) => ArrayBuffer.isView(value)), false);
  assert.equal(file.preview.length, 512 * 1024);
  assert.equal(file.truncated, true);
});

test("creating and disposing resource URLs does not duplicate the 64 MiB attachment", () => {
  const result = JSON.parse(execFileSync(process.execPath, [
    "--expose-gc", fileURLToPath(new URL("./attachment-memory.mjs", import.meta.url)),
  ], { encoding: "utf8" }));
  assert.ok(result.parsed >= 64 * 1024 * 1024, JSON.stringify(result));
  assert.ok(result.parsed < 65 * 1024 * 1024, JSON.stringify(result));
  assert.ok(result.withResources - result.parsed < 1024 * 1024, JSON.stringify(result));
  assert.ok(result.disposed - result.parsed < 1024 * 1024, JSON.stringify(result));
  assert.equal(result.exact, true);
});

test("selection, cells, filters and playback are pure state transitions", () => {
  const model = freeze(parse());
  const original = freeze(initialState(model));
  assert.equal(selection(model, original).isFailure, true);
  assert.equal(inspectCell(model, original).mismatches.length, 2);
  const passed = update(model, original, { type: "select-action", id: 5 });
  assert.equal(selection(model, passed).sharesFailure, true);
  assert.equal(selection(model, passed).isFailure, false);
  assert.deepEqual(selection(model, passed).mismatches, []);
  const missing = update(model, original, { type: "select-action", id: 1 });
  assert.equal(selection(model, missing).frame, undefined);
  assert.equal(missing.missingScreen, 999);
  assert.equal(update(model, missing, { type: "toggle-playback" }), missing);
  const filtered = update(model, original, { type: "filter", value: 'getByText("READY")' });
  assert.deepEqual(visibleActions(model, filtered).map((action) => action.id), [2, 3]);
  let state = update(model, filtered, { type: "navigate-action", delta: 1 });
  assert.equal(state.actionId, 2);
  state = update(model, state, { type: "navigate-action", delta: 1 });
  assert.equal(state.actionId, 3);
  assert.equal(state.frameIndex, 0);
  state = update(model, state, { type: "filter", value: "" });
  state = update(model, state, { type: "toggle-playback" });
  assert.deepEqual(state.playback, { target: 2, actionId: 5 });
  state = update(model, state, { type: "tick" });
  assert.equal(state.frameIndex, 1);
  assert.ok(state.playback);
  state = update(model, state, { type: "tick" });
  assert.equal(state.actionId, 5);
  assert.equal(state.playback, undefined);
  assert.equal(original.actionId, 6);
});

test("manual selection, filters, pause and inspection cancel playback", () => {
  const model = freeze(parse());
  let state = update(model, initialState(model), { type: "select-action", id: 2 });
  state = freeze(update(model, state, { type: "toggle-playback" }));
  for (const event of [
    { type: "select-frame", index: 1 }, { type: "select-action", id: 3 }, { type: "jump-to-result" },
    { type: "filter", value: "READY" }, { type: "assertions-only", value: true },
    { type: "inspect", x: 0, y: 0, activate: false }, { type: "pause" }, { type: "toggle-playback" },
  ]) assert.equal(update(model, state, event).playback, undefined);
});

test("coordinate drafts are independent from inspection and invalid cells have no overlay", () => {
  const model = parse();
  const initial = initialState(model);
  const draft = update(model, initial, { type: "coordinate", axis: "column", value: "999" });
  assert.equal(draft.column, "999");
  assert.deepEqual(draft.cell, initial.cell);
  let state = update(model, draft, { type: "inspect", x: 999, y: 0, activate: false });
  assert.equal(inspectCell(model, state), undefined);
  state = update(model, state, { type: "move-cell", dx: -1, dy: 0 });
  assert.deepEqual(state.cell, { x: 0, y: 0 });
  assert.equal(cellAt(model.frames[0], .5, 0), undefined);
  assert.equal(cellAt(model.frames[0], -1, 0), undefined);
  state = update(model, state, { type: "inspect", x: 2, y: 0, activate: true });
  assert.equal(inspectCell(model, state).leadColumn, 1);
});

test("deep links and passed traces have explicit defaults", () => {
  const model = parse();
  assert.equal(initialState(model, "#screen-1").frameIndex, 0);
  assert.equal(initialState(model, "#screen-999").frameIndex, 2);
  const data = fixture();
  data.details.outcome = "passed";
  const passed = parse(data);
  assert.equal(selection(passed, initialState(passed)).isFailure, false);
});

test("Svelte components compose and escape the report without mutating its model", async () => {
  const result = await build({
    stdin: {
      contents: 'export { default as App } from "./src/App.svelte"; export { render } from "svelte/server";',
      resolveDir: fileURLToPath(new URL("../", import.meta.url)), sourcefile: "component-test.js",
    },
    bundle: true, write: false, platform: "node", format: "esm", conditions: ["svelte", "production"],
    plugins: [sveltePlugin("server")],
  });
  const { App, render } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`);
  const model = freeze(parse());
  const resources = freeze({ images: model.frames.map(() => ({})), attachments: model.attachments.map(() => "blob:test"), dispose() {} });
  const first = render(App, { props: { model, resources } }).body;
  assert.equal(first, render(App, { props: { model, resources } }).body);
  for (const id of ["session-summary", "actions-panel", "metadata-panel", "viewport", "errors-panel", "cell-panel", "call-panel", "attachments-panel"]) {
    assert.ok(first.includes(`id="${id}"`), id);
  }
  assert.ok(first.includes('getByText("A")'));
  assert.ok(!first.includes("<script>"));
  assert.ok(first.includes("&lt;script>"));
  assert.equal(globalThis.document, undefined);
});
