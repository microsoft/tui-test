import assert from "node:assert/strict";
import { test } from "node:test";

import { ExpectationError, ShellUse, uniqueSession } from "../dist/index.js";
import { resolveTimeout, timeoutsPayload } from "../dist/config.js";
import { envPairs } from "../dist/protocol.js";

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

const ALL_TIMEOUT_ENV_VARS = [
  "SHELL_USE_TIMEOUT_MS",
  "SHELL_USE_EXPECT_TIMEOUT_MS",
  "SHELL_USE_TIMEOUT_TEXT_MS",
  "SHELL_USE_TIMEOUT_IDLE_MS",
  "SHELL_USE_TIMEOUT_COMMAND_MS",
  "SHELL_USE_TIMEOUT_EXIT_MS",
  "SHELL_USE_TIMEOUT_READY_MS",
];

const CLASSES = ["text", "idle", "command", "exit", "ready"];

function withEnv(vars, fn) {
  const saved = {};
  for (const key of Object.keys(vars)) {
    saved[key] = process.env[key];
    if (vars[key] === undefined) {
      delete process.env[key];
    } else {
      process.env[key] = vars[key];
    }
  }
  try {
    return fn();
  } finally {
    for (const key of Object.keys(vars)) {
      if (saved[key] === undefined) {
        delete process.env[key];
      } else {
        process.env[key] = saved[key];
      }
    }
  }
}

test("resolveTimeout returns undefined when nothing is configured", () => {
  for (const cls of CLASSES) {
    assert.equal(resolveTimeout(cls), undefined, `expected ${cls} -> undefined`);
    assert.equal(resolveTimeout(cls, undefined, {}), undefined);
    assert.equal(resolveTimeout(cls, undefined, { timeouts: {} }), undefined);
  }
});

test("resolveTimeout precedence: per-call beats timeouts[class] beats omitted", () => {
  assert.equal(resolveTimeout("text", 111, { timeouts: { text: 333 } }), 111);
  assert.equal(resolveTimeout("text", undefined, { timeouts: { text: 333 } }), 333);
  assert.equal(resolveTimeout("text", undefined, { timeouts: { idle: 333 } }), undefined);
});

test("resolveTimeout never reads an environment variable (engine owns ranks 3-5)", () => {
  const vars = Object.fromEntries(ALL_TIMEOUT_ENV_VARS.map((name) => [name, "1234"]));
  withEnv(vars, () => {
    for (const cls of CLASSES) {
      assert.equal(resolveTimeout(cls), undefined, `expected ${cls} to ignore env`);
    }
  });
});

test("resolveTimeout honours an explicit zero per-call timeout", () => {
  assert.equal(resolveTimeout("text", 0), 0);
});

test("timeoutsPayload is undefined when empty so the field is omitted", () => {
  assert.equal(timeoutsPayload(undefined), undefined);
  assert.equal(timeoutsPayload({}), undefined);
  assert.equal(timeoutsPayload({ text: undefined, ready: undefined }), undefined);
});

test("timeoutsPayload keeps only the classes that are set", () => {
  assert.deepEqual(timeoutsPayload({ text: 1000, command: 2000 }), {
    text: 1000,
    command: 2000,
  });
  assert.deepEqual(timeoutsPayload({ ready: 45000 }), { ready: 45000 });
});

test("wait/expect omit timeout_ms when no client timeout is configured", async () => {
  const c = new CapturingClient("s");
  await c.waitText("x");
  await c.waitIdle();
  await c.waitCommand();
  await c.waitExit();
  await c.waitReady();
  await c.expectText("x");
  await c.expectExitCode(0);
  for (const payload of c.sent) {
    assert.ok(
      !Object.prototype.hasOwnProperty.call(payload, "timeout_ms"),
      `${payload.kind} should omit timeout_ms`,
    );
  }
});

test("client-level timeouts are sent as an explicit timeout_ms per class", async () => {
  const c = new CapturingClient("s", {
    timeouts: { text: 1000, idle: 2000, command: 3000, exit: 4000, ready: 5000 },
  });
  await c.waitText("x");
  await c.waitIdle();
  await c.waitCommand();
  await c.waitExit();
  await c.waitReady();
  await c.expectText("x");
  await c.expectExitCode(0);
  const byKind = Object.fromEntries(c.sent.map((p) => [p.kind, p.timeout_ms]));
  assert.deepEqual(byKind, {
    wait_text: 1000,
    wait_idle: 2000,
    wait_command: 3000,
    wait_exit: 4000,
    wait_ready: 5000,
    expect_text: 1000, // expectText resolves through the `text` class
    expect_exit_code: 3000, // expectExitCode resolves through the `command` class
  });
});

test("a per-call timeout beats the client-level class default", async () => {
  const c = new CapturingClient("s", { timeouts: { text: 1000, command: 9000 } });
  await c.waitText("x", { timeout: 50 });
  await c.expectExitCode(0, { timeout: 75 });
  assert.equal(c.sent[0].timeout_ms, 50);
  assert.equal(c.sent[1].timeout_ms, 75);
});

test("expectExitCode sends a timeout only when given one", async () => {
  const c = new CapturingClient("s");
  await c.expectExitCode(0);
  assert.deepEqual(c.sent[0], { kind: "expect_exit_code", code: 0 });
  await c.expectExitCode(1, { timeout: 250 });
  assert.deepEqual(c.sent[1], { kind: "expect_exit_code", code: 1, timeout_ms: 250 });
});

test("open omits the timeouts object when no session defaults are set", async () => {
  const c = new CapturingClient("s");
  await c.open();
  assert.ok(
    !Object.prototype.hasOwnProperty.call(c.sent[0], "timeouts"),
    "open should omit an empty timeouts object",
  );
});

test("open seeds only the session-default classes that are set", async () => {
  const c = new CapturingClient("s");
  await c.open({ timeouts: { command: 60000, ready: 45000 } });
  assert.deepEqual(c.sent[0].timeouts, { command: 60000, ready: 45000 });
});

test("run seeds session-default timeouts too", async () => {
  const c = new CapturingClient("s");
  await c.run("vim", [], { timeouts: { text: 1500 } });
  assert.deepEqual(c.sent[0].timeouts, { text: 1500 });
});

test("envPairs coerces record values to strings", () => {
  assert.deepEqual(envPairs({ A: "1", B: 2, C: true, D: false }), [
    ["A", "1"],
    ["B", "2"],
    ["C", "true"],
    ["D", "false"],
  ]);
});

test("envPairs passes array form through and handles empty input", () => {
  assert.deepEqual(envPairs([["X", "Y"]]), [["X", "Y"]]);
  assert.deepEqual(envPairs(), []);
});

test("close is idempotent and needs no prior open", async () => {
  const su = new ShellUse(uniqueSession("close-idempotency"));
  await su.close();
  await su.close();
  await su.closeQuiet();
});

test("an ephemeral client closes cleanly without ever opening", async () => {
  const su = ShellUse.ephemeral("never-opened");
  await su.close();
  await su.close();
});

test("timeoutsPayload rejects an unknown class", () => {
  assert.throws(() => timeoutsPayload({ comand: 100 }), /comand/);
});

test("open rejects an unknown timeout class", async () => {
  const c = new CapturingClient("s");
  await assert.rejects(() => c.open({ timeouts: { txt: 100 } }), /txt/);
});

test("the constructor rejects an unknown timeout class", () => {
  assert.throws(() => new ShellUse("s", { timeouts: { txt: 100 } }), /txt/);
  assert.doesNotThrow(() => new ShellUse("s", { timeouts: { text: 100 } }));
  assert.doesNotThrow(() => new ShellUse("s"));
});

test("timeoutsPayload keeps every known class", () => {
  assert.deepEqual(timeoutsPayload({ text: 1, ready: 2 }), { text: 1, ready: 2 });
});

test("every wait and expect method tags its failure with the operation", async () => {
  const cases = {
    waitText: ["x"],
    waitIdle: [],
    waitCommand: [],
    waitExit: [],
    waitReady: [],
    expectText: ["x"],
    expectExitCode: [0],
    expectOutput: ["x"],
    expectSnapshot: ["x"],
  };
  for (const [name, args] of Object.entries(cases)) {
    const c = new CapturingClient("s");
    c.send = async () => {
      throw new ExpectationError("boom");
    };
    await assert.rejects(
      () => c[name](...args),
      (error) =>
        error instanceof ExpectationError && error.message.startsWith(`${name}: `),
      `${name} did not tag its failure`,
    );
  }
});
