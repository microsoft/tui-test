import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import * as sdk from "../dist/index.js";
import { TuiTest } from "../dist/index.js";

const BIN = process.env.TUI_TEST_BIN || "tui-test";

function agentContext() {
  const out = execFileSync(BIN, ["agent-context"], { encoding: "utf8" });
  return JSON.parse(out);
}

let schema;
try {
  schema = agentContext();
} catch {
  schema = null;
}

const MAPPING = {
  open: [["client", "open"]],
  run: [["client", "run"]],
  close: [["client", "close"], ["module", "closeAll"]],
  sessions: [["module", "sessions"]],
  state: [["client", "state"]],
  text: [["client", "text"]],
  screenshot: [["client", "screenshot"]],
  record: [["client", "startRecording"], ["client", "stopRecording"]],
  cells: [["client", "cells"]],
  get: [
    ["client", "getCommand"],
    ["client", "getOutput"],
    ["client", "getExitCode"],
    ["client", "getCwd"],
    ["client", "getCursor"],
    ["client", "getSize"],
    ["client", "getTitle"],
    ["client", "getBellCount"],
    ["client", "getBellEvents"],
  ],
  type: [["client", "type"]],
  submit: [["client", "submit"]],
  key: [["client", "keyboard"]],
  press: [["client", "press"]],
  mouse: [["client", "mouse"]],
  resize: [["client", "resize"]],
  write: [["client", "write"]],
  signal: [["client", "signal"]],
  kill: [["client", "kill"]],
  wait: [["client", "waitTitle"], ["client", "waitText"], ["client", "waitIdle"], ["client", "waitCommand"], ["client", "waitExit"], ["client", "waitBell"]],
  expect: [["client", "expectTitle"], ["client", "expectText"], ["client", "expectExitCode"], ["client", "expectOutput"], ["client", "expectBellCount"], ["client", "expectSnapshot"]],
  find: [["client", "findText"]],
  locator: [["client", "getByText"]],
  "get-recording": [["module", "getRecording"]],
};

const EXCLUDED = new Set(["monitor", "status", "daemon", "usage", "agent-context", "skill"]);

test("every cli command is mapped or excluded", { skip: !schema }, () => {
  const instance = new TuiTest("conformance");
  for (const command of Object.keys(schema.commands)) {
    if (EXCLUDED.has(command)) {
      continue;
    }
    assert.ok(MAPPING[command], `cli command '${command}' has no SDK mapping`);
    for (const [scope, name] of MAPPING[command]) {
      const target = scope === "client" ? instance : sdk;
      assert.ok(
        typeof target[name] !== "undefined",
        `missing SDK member for '${command}': ${scope}.${name}`,
      );
    }
  }
});

test("keyboard exposes every key action", () => {
  const keyboard = new TuiTest("keyboard-conformance").keyboard;
  for (const method of ["press", "down", "repeat", "up"]) {
    assert.equal(typeof keyboard[method], "function", method);
  }
});

test("error exit codes match the taxonomy", () => {
  assert.equal(new sdk.ExpectationError("x").exitCode, 1);
  assert.equal(new sdk.UsageError("x").exitCode, 2);
  assert.equal(new sdk.NoSessionError("x").exitCode, 3);
  assert.equal(new sdk.InternalError("x").exitCode, 5);
});
