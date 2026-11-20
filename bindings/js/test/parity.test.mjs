import assert from "node:assert/strict";
import { test } from "node:test";
import koffi from "koffi";
import { createAdapter } from "../native/adapter.js";

const text = (value) => value.data === null
  ? null : Buffer.from(koffi.decode(value.data, "uint8_t", value.len)).toString();

test("scope anchors and composite match styles retain their typed transport", async () => {
  let actual;
  const { NativeSession } = createAdapter({
    tui_find_locator_start(context, expression) {
      actual = JSON.parse(text(expression.json));
      return null;
    },
  });
  const expression = {
    nodes: [
      { kind: "text", text: "Header", occurrence: "unique" },
      { kind: "text", text: "Save", scope: {
        after: { text: "Start", regex: true, occurrence: "nth", nth: 1 },
        before: { text: "End" },
      }, matchStyle: { bold: true } },
      { kind: "or", left: 1, right: 1, within: 0, direction: "after",
        matchStyle: { italic: false } },
    ], root: 2,
  };
  const original = structuredClone(expression);
  await assert.rejects(new NativeSession("parity").findLocator(expression), /null task/);
  assert.deepEqual(expression, original);
  assert.deepEqual(actual.nodes[1].scope.after.occurrence, { nth: 1 });
  assert.equal(actual.nodes[1].scope.before.occurrence, "unique");
  assert.deepEqual(actual.nodes[1].match_style, { bold: true });
  assert.equal(actual.nodes[2].within, 0);
  assert.deepEqual(actual.nodes[2].match_style, { italic: false });
});

test("per-spawn context and recording overrides do not change constructor defaults", async () => {
  const calls = [];
  const { NativeSession } = createAdapter({
    tui_open_start(context, options) {
      calls.push({ context: JSON.parse(text(context)), directory: text(options.recording_directory), mode: text(options.recording_mode) });
      return null;
    },
    tui_finish_trace_start(context, failed) {
      calls.push({ context: JSON.parse(text(context)), failed });
      return null;
    },
    tui_interrupt_start(context) {
      calls.push({ context: JSON.parse(text(context)), interrupt: true });
      return null;
    },
  });
  const context = { operationName: "custom", diagnosticContext: { case: "save" }, screenHistoryLimit: 5 };
  const session = new NativeSession("parity", { directory: "default", mode: "on-failure" }, undefined, undefined, context);
  context.diagnosticContext.case = "changed";
  await assert.rejects(session.open({
    recording: { directory: "override", mode: "always" },
    context: { diagnosticContext: { phase: "open" }, trace: { mode: "off" } },
  }), /null task/);
  await assert.rejects(session.open({}), /null task/);
  await assert.rejects(session.open({ recording: {} }), /null task/);
  await assert.rejects(session.finishTrace(true), /null task/);
  await assert.rejects(session.interrupt(), /null task/);
  assert.equal(calls[0].directory, "override");
  assert.equal(calls[0].mode, "always");
  assert.deepEqual(calls[0].context.diagnostic_context, { phase: "open" });
  assert.equal(calls[0].context.trace_mode, "off");
  assert.equal(calls[1].directory, "default");
  assert.equal(calls[1].mode, "on-failure");
  assert.deepEqual(calls[1].context.diagnostic_context, { case: "save" });
  assert.equal(calls[1].context.screen_history_limit, 5);
  assert.equal(calls[2].directory, null);
  assert.equal(calls[2].mode, null);
  assert.equal(calls[3].failed, true);
  assert.equal(calls[4].interrupt, true);
});
