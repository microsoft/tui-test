import assert from "node:assert/strict";
import { test } from "node:test";

import { ShellUse, VersionMismatchError } from "../dist/index.js";
import { VERSION, checkVersion } from "../dist/version.js";

class CapturingClient extends ShellUse {
  constructor(...args) {
    super(...args);
    this.sent = [];
    this.reply = undefined;
  }
  async send(payload) {
    this.sent.push(payload);
    return this.reply;
  }
}

test("open builds a complete open payload", async () => {
  const c = new CapturingClient("s");
  await c.open({ cols: 120, rows: 40, env: { K: "V" } });
  assert.deepEqual(c.sent[0], {
    kind: "open",
    shell: null,
    program: null,
    cols: 120,
    rows: 40,
    cwd: null,
    env: [["K", "V"]],
  });
});

test("run sets program and null shell", async () => {
  const c = new CapturingClient("s");
  await c.run("vim", ["file.txt"]);
  assert.deepEqual(c.sent[0].program, ["vim", "file.txt"]);
  assert.equal(c.sent[0].shell, null);
});

test("keys wraps a single combo, press forwards tokens", async () => {
  const c = new CapturingClient("s");
  await c.keys("Ctrl+a");
  await c.press("Escape", "Enter");
  assert.deepEqual(c.sent[0], { kind: "press", keys: ["Ctrl+a"] });
  assert.deepEqual(c.sent[1], { kind: "press", keys: ["Escape", "Enter"] });
});

test("kill maps to signal KILL", async () => {
  const c = new CapturingClient("s");
  await c.kill();
  assert.deepEqual(c.sent[0], { kind: "signal", name: "KILL" });
});

test("getExitCode uses the kebab field name", async () => {
  const c = new CapturingClient("s");
  c.reply = { value: 0 };
  await c.getExitCode();
  assert.deepEqual(c.sent[0], { kind: "get", field: "exit-code" });
});

test("mouse click builds a nested action", async () => {
  const c = new CapturingClient("s");
  await c.mouse.click(null, null, { onText: "OK", clicks: 2 });
  assert.deepEqual(c.sent[0], {
    kind: "mouse",
    action: { op: "click", x: null, y: null, on_text: "OK", button: 0, clicks: 2 },
  });
});

test("waitText omits timeout_ms when no client timeout is configured", async () => {
  const c = new CapturingClient("s");
  await c.waitText("done");
  assert.deepEqual(c.sent[0], {
    kind: "wait_text",
    text: "done",
    regex: false,
    full: false,
    not: false,
  });
});

test("waitText carries a client-level text timeout as timeout_ms", async () => {
  const c = new CapturingClient("s", { timeouts: { text: 1500 } });
  await c.waitText("done");
  assert.deepEqual(c.sent[0], {
    kind: "wait_text",
    text: "done",
    regex: false,
    full: false,
    not: false,
    timeout_ms: 1500,
  });
});

test("waitCommand omits timeout_ms when unset", async () => {
  const c = new CapturingClient("s");
  await c.waitCommand();
  assert.deepEqual(c.sent[0], { kind: "wait_command" });
});

test("expectText is strict by default and forwards colors", async () => {
  const c = new CapturingClient("s");
  await c.expectText("ERR", { fg: "#ff0000" });
  assert.equal(c.sent[0].strict, true);
  assert.equal(c.sent[0].fg, "#ff0000");
  assert.ok(!("timeout_ms" in c.sent[0]));
});

test("checkVersion passes when versions match", () => {
  checkVersion(VERSION);
});

test("checkVersion throws on a version mismatch", () => {
  assert.throws(() => checkVersion("9.9.9"), VersionMismatchError);
});

test("checkVersion throws when the daemon reports no version", () => {
  assert.throws(() => checkVersion(undefined), VersionMismatchError);
});
