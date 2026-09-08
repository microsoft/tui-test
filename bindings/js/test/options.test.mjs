import assert from "node:assert/strict";
import { execFile, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { promisify } from "node:util";
import { test } from "node:test";

import {
  ExpectationError,
  NoSessionError,
  TuiTest,
  UsageError,
  uniqueSession,
} from "../dist/index.js";
import {
  backendPayload,
  envPairs,
  profilePayload,
  recordingPayload,
  resolveMonitoring,
  resolveTimeout,
  timeoutsPayload,
} from "../dist/config.js";
import { NativeRuntime } from "../dist/native.js";

const shell = process.platform === "win32" ? "pwsh" : undefined;
const evalArgs =
  typeof globalThis.Deno === "undefined"
    ? ["-e", "console.log('ready'); setInterval(() => {}, 1000)"]
    : ["eval", "console.log('ready'); setInterval(() => {}, 1000)"];

const ALL_TIMEOUT_ENV_VARS = [
  "TUI_TEST_TIMEOUT_MS",
  "TUI_TEST_EXPECT_TIMEOUT_MS",
  "TUI_TEST_TIMEOUT_TEXT_MS",
  "TUI_TEST_TIMEOUT_IDLE_MS",
  "TUI_TEST_TIMEOUT_COMMAND_MS",
  "TUI_TEST_TIMEOUT_EXIT_MS",
  "TUI_TEST_TIMEOUT_READY_MS",
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
    assert.equal(resolveTimeout(cls), undefined);
    assert.equal(resolveTimeout(cls, undefined, { timeouts: {} }), undefined);
  }
});

test("resolveTimeout precedence is per-call then client class", () => {
  assert.equal(resolveTimeout("text", 111, { timeouts: { text: 333 } }), 111);
  assert.equal(resolveTimeout("text", undefined, { timeouts: { text: 333 } }), 333);
  assert.equal(resolveTimeout("text", undefined, { timeouts: { idle: 333 } }), undefined);
  assert.equal(resolveTimeout("text", 0), 0);
});

test("resolveTimeout leaves environment fallback to the engine", () => {
  const vars = Object.fromEntries(ALL_TIMEOUT_ENV_VARS.map((name) => [name, "1234"]));
  withEnv(vars, () => {
    for (const cls of CLASSES) {
      assert.equal(resolveTimeout(cls), undefined);
    }
  });
});

test("timeoutsPayload omits empty values and keeps known classes", () => {
  assert.equal(timeoutsPayload(undefined), undefined);
  assert.equal(timeoutsPayload({}), undefined);
  assert.deepEqual(timeoutsPayload({ text: 1000, command: 2000 }), {
    text: 1000,
    command: 2000,
  });
  assert.deepEqual(timeoutsPayload({ ready: 45000 }), { ready: 45000 });
});

test("envPairs coerces records and preserves pair arrays", () => {
  assert.deepEqual(envPairs({ A: "1", B: 2, C: true, D: false }), [
    ["A", "1"],
    ["B", "2"],
    ["C", "true"],
    ["D", "false"],
  ]);
  assert.deepEqual(envPairs([["X", "Y"]]), [["X", "Y"]]);
  assert.deepEqual(envPairs(), []);
});

test("backendPayload validates backend names", () => {
  assert.equal(backendPayload(), undefined);
  assert.equal(backendPayload("alacritty"), "alacritty");
  assert.equal(backendPayload("ghostty"), "ghostty");
  assert.equal(backendPayload("rio"), "rio");
  assert.throws(() => backendPayload("xterm"), /unknown backend/);
  assert.throws(() => backendPayload("libghostty"), /unknown backend/);
});

test("profilePayload validates profile and color fields", () => {
  assert.deepEqual(profilePayload({ scrollback: 50, colors: { red: "#010203" } }), {
    scrollback: 50,
    colors: [["red", "#010203"]],
  });

  assert.throws(() => profilePayload({ scrollbacks: 50 }), /scrollbacks/);
  assert.throws(
    () => profilePayload({ colors: { chartreuse: "#010203" } }),
    /chartreuse/,
  );
  assert.throws(() => profilePayload({ colors: { red: 123 } }), /must be a string/);
});

test("mouse helpers encode named buttons and modifiers", async () => {
  const calls = [];
  const originals = {
    clickLocator: NativeRuntime.prototype.clickLocator,
    mouseClick: NativeRuntime.prototype.mouseClick,
    mouseDown: NativeRuntime.prototype.mouseDown,
    mouseUp: NativeRuntime.prototype.mouseUp,
    mouseDrag: NativeRuntime.prototype.mouseDrag,
  };
  NativeRuntime.prototype.clickLocator = async (stages, button, clicks, timeout) => {
    calls.push(["locator", stages, button, clicks, timeout]);
  };
  NativeRuntime.prototype.mouseClick = async (options) => {
    calls.push(["click", options]);
  };
  NativeRuntime.prototype.mouseDown = async (x, y, button) => {
    calls.push(["down", x, y, button]);
  };
  NativeRuntime.prototype.mouseUp = async (x, y, button) => {
    calls.push(["up", x, y, button]);
  };
  NativeRuntime.prototype.mouseDrag = async (x1, y1, x2, y2, button) => {
    calls.push(["drag", x1, y1, x2, y2, button]);
  };

  try {
    const su = new TuiTest("mouse-options");
    await su.mouse.click(null, null, {
      onText: "OK",
      button: "right",
      alt: true,
      ctrl: true,
      shift: true,
      clicks: 2,
    });
    await su.mouse.down(1, 2, { button: "middle", ctrl: true });
    await su.mouse.up(3, 4, { button: "right", alt: true });
    await su.mouse.drag(5, 6, 7, 8, { shift: true });

    assert.deepEqual(calls, [
      [
        "click",
        {
          x: undefined,
          y: undefined,
          onText: "OK",
          button: 30,
          clicks: 2,
        },
      ],
      ["down", 1, 2, 17],
      ["up", 3, 4, 10],
      ["drag", 5, 6, 7, 8, 4],
    ]);

    await su.getByText("Open").unique().click({
      button: "middle",
      alt: true,
      ctrl: true,
      shift: true,
      clicks: 2,
      timeout: 50,
    });
    const locatorCall = calls.at(-1);
    assert.equal(locatorCall[0], "locator");
    assert.equal(locatorCall[1].at(-1).occurrence, "unique");
    assert.deepEqual(locatorCall.slice(2), [29, 2, 50]);

    await assert.rejects(
      su.mouse.click(0, 0, { button: "primary" }),
      /unknown mouse button "primary"/,
    );
    await assert.rejects(
      su.mouse.click(0, 0, { ctrl: "yes" }),
      /ctrl must be a boolean/,
    );
    await assert.rejects(
      su.getByText("Open").click({ button: "primary" }),
      /unknown mouse button "primary"/,
    );
  } finally {
    Object.assign(NativeRuntime.prototype, originals);
  }
});

test("recordingPayload accepts only mode and directory", () => {
  assert.deepEqual(recordingPayload({ mode: "on-failure", directory: "casts" }), {
    mode: "on-failure",
    directory: "casts",
  });
  assert.throws(() => recordingPayload({ mode: "sometimes" }), /recording mode/);
  assert.throws(() => recordingPayload({ directory: "" }), /non-empty/);
  assert.throws(() => recordingPayload({ other: 1 }), /other/);
});

test("monitoring is opt-in and resolves explicit and environment settings", () => {
  withEnv(
    {
      TUI_TEST_MONITORING: undefined,
      TUI_TEST_WAIT_AT_END: undefined,
      TUI_TEST_FIRST_ATTACH_TIMEOUT: undefined,
      TUI_TEST_LABEL: undefined,
    },
    () => {
      assert.deepEqual(resolveMonitoring(), {
        enabled: false,
        waitAtEnd: "never",
        firstAttachTimeout: 30000,
        holdWhileAttached: true,
        label: undefined,
        metadata: {},
      });
    },
  );
  withEnv(
    {
      TUI_TEST_MONITORING: "1",
      TUI_TEST_WAIT_AT_END: "failure",
      TUI_TEST_FIRST_ATTACH_TIMEOUT: "25",
      TUI_TEST_LABEL: "env label",
    },
    () => {
      assert.deepEqual(resolveMonitoring(), {
        enabled: true,
        waitAtEnd: "failure",
        firstAttachTimeout: 25,
        holdWhileAttached: true,
        label: "env label",
        metadata: {},
      });
      assert.equal(
        resolveMonitoring({ enabled: false, waitAtEnd: "always" }).enabled,
        false,
      );
      const explicit = resolveMonitoring({
        enabled: false,
        waitAtEnd: "never",
        firstAttachTimeout: null,
        holdWhileAttached: false,
        label: "explicit label",
      });
      assert.deepEqual(
        [explicit.enabled, explicit.waitAtEnd, explicit.firstAttachTimeout,
          explicit.holdWhileAttached, explicit.label],
        [false, "never", null, false, "explicit label"],
      );
    },
  );
  assert.throws(
    () => resolveMonitoring({ firstAttachTimeout: -1 }),
    /non-negative integer or null/,
  );
  assert.equal(
    resolveMonitoring({ firstAttachTimeout: null }).firstAttachTimeout,
    null,
  );
  withEnv({ TUI_TEST_FIRST_ATTACH_TIMEOUT: "infinite" }, () => {
    assert.equal(resolveMonitoring().firstAttachTimeout, null);
    assert.equal(resolveMonitoring({ firstAttachTimeout: 0 }).firstAttachTimeout, 0);
  });
  assert.throws(
    () => resolveMonitoring({ metadata: { unknown: "x" } }),
    /unknown monitoring metadata field/,
  );
  assert.throws(
    () => resolveMonitoring({ metadata: { tags: ["smoke", 1] } }),
    /tags must be an array of strings/,
  );
  assert.throws(
    () => resolveMonitoring({ metadata: { tags: "smoke" } }),
    /tags must be an array of strings/,
  );
  withEnv({ TUI_TEST_LABEL: undefined }, () => {
    const tags = ["smoke", "login"];
    const resolved = resolveMonitoring({
      metadata: { testFile: "login.test.mjs", testName: "rejects expired token", tags },
    });
    assert.equal(resolved.label, "login.test.mjs - rejects expired token");
    assert.deepEqual(resolved.metadata.tags, tags);
    tags.push("later");
    assert.deepEqual(resolved.metadata.tags, ["smoke", "login"]);
  });
});

test(
  "a monitor wait does not close a restarted same-name session",
  { timeout: 10000 },
  async () => {
    const name = uniqueSession("monitor-restart");
    const monitored = new TuiTest(name, {
      monitoring: {
        enabled: true,
        waitAtEnd: "always",
        firstAttachTimeout: null,
        label: "restart boundary test",
      },
    });
    const replacement = new TuiTest(name);
    try {
      await monitored.run(process.execPath, evalArgs);
      const finishing = monitored.finish({ outcome: "passed" });
      await new Promise((resolve) => setTimeout(resolve, 100));
      await replacement.run(process.execPath, [
        "-e",
        "console.log('replacement-ready'); setInterval(() => {}, 1000)",
      ], { restart: true });
      await finishing;
      await monitored.close();
      await monitored[Symbol.asyncDispose]();
      await replacement.getByText("replacement-ready").wait({ timeout: 2000 });
    } finally {
      await replacement.closeQuiet();
      await monitored.closeQuiet();
    }
  },
);

test("inspection started after replacement cannot adopt the replacement's generation", async () => {
  const name = uniqueSession("monitor-late-finish");
  const original = new TuiTest(name, {
    monitoring: { enabled: true, waitAtEnd: "failure", firstAttachTimeout: 0 },
  });
  const replacement = new TuiTest(name, { monitoring: { enabled: true } });
  const failure = new Error("original session failed");
  try {
    await original.run(process.execPath, evalArgs);
    await replacement.run(process.execPath, evalArgs, { restart: true });
    await assert.rejects(original.inspectFailure(failure), (error) => error === failure);
    await replacement.getByText("ready").wait({ timeout: 5000 });
    await original.close();
    assert.ok((await replacement.state()).cols > 0);
  } finally {
    await replacement.closeQuiet();
    await original.closeQuiet();
  }
});

test("same-instance restart abandons infinite inspection including initialization", { timeout: 15000 }, async () => {
  for (const initializationDelay of [0, 50]) {
    const name = uniqueSession("same-instance-restart");
    const terminal = new TuiTest(name, {
      monitoring: { enabled: true, waitAtEnd: "always", firstAttachTimeout: null },
    });
    const cleanup = new NativeRuntime(name);
    try {
      await terminal.run(process.execPath, evalArgs);
      const finishing = terminal.finish({ outcome: "passed" });
      finishing.catch(() => {});
      if (initializationDelay) await delay(initializationDelay);
      await terminal.run(process.execPath, [
        "-e", "console.log('same-instance-replacement'); setInterval(() => {}, 1000)",
      ], { restart: true });
      await finishing;
      await terminal.getByText("same-instance-replacement").first().expect({ timeout: 5000 });
    } finally {
      await cleanup.cancelMonitorWait();
      await terminal.closeQuiet();
    }
  }
});

test("a stale terminal's normal close cannot close a same-name replacement", async () => {
  const name = uniqueSession("monitor-stale-close");
  const original = new TuiTest(name, { monitoring: { enabled: true } });
  const replacement = new TuiTest(name, { monitoring: { enabled: false } });
  try {
    await original.run(process.execPath, evalArgs);
    await replacement.run(process.execPath, evalArgs, { restart: true });
    await original.close();
    await replacement.getByText("ready").wait({ timeout: 5000 });
  } finally {
    await replacement.closeQuiet();
    await original.closeQuiet();
  }
});

test("a failed initial spawn cannot inspect or close a later same-name replacement", async () => {
  const name = uniqueSession("monitor-unspawnable-replacement");
  const original = new TuiTest(name, {
    monitoring: { enabled: true, waitAtEnd: "failure", firstAttachTimeout: 0 },
  });
  const replacement = new TuiTest(name, { monitoring: { enabled: true } });
  let failure;
  try {
    await assert.rejects(original.run(`__missing_tui_test_program_${process.pid}__`), (error) => {
      failure = error;
      return error instanceof Error;
    });
    const stack = failure.stack;
    await replacement.run(process.execPath, evalArgs);
    await assert.rejects(original.inspectFailure(failure), (error) => {
      assert.equal(error, failure);
      assert.equal(error.stack, stack);
      return true;
    });
    await original.close();
    await replacement.getByText("ready").wait({ timeout: 5000 });
  } finally {
    await replacement.closeQuiet();
    await original.closeQuiet();
  }
});

test("unmonitored handles retain name-based cleanup after finishing or disposal", async () => {
  for (const cleanup of ["finish", "inspectFailure", "dispose"]) {
    const name = uniqueSession("unmonitored-reopen");
    const original = new TuiTest(name, { monitoring: { enabled: false } });
    const replacement = new TuiTest(name, { monitoring: { enabled: false } });
    const primary = new Error("original unmonitored failure");
    try {
      await original.run(process.execPath, evalArgs);
      if (cleanup === "finish") {
        await original.finish({ outcome: "passed" });
      } else if (cleanup === "inspectFailure") {
        await assert.rejects(original.inspectFailure(primary), (error) => error === primary);
      } else {
        await original[Symbol.asyncDispose]();
      }
      await replacement.run(process.execPath, evalArgs);
      await original.close();
      await assert.rejects(replacement.state(), NoSessionError);
    } finally {
      await replacement.closeQuiet();
      await original.closeQuiet();
    }
  }
});

test("never-opened monitored handles cannot inspect or close another same-name session", async () => {
  for (const cleanup of ["close", "inspectFailure"]) {
    const name = uniqueSession("monitored-unopened");
    const original = new TuiTest(name, {
      monitoring: { enabled: true, waitAtEnd: "failure", firstAttachTimeout: 0 },
    });
    const replacement = new TuiTest(name, { monitoring: { enabled: true } });
    const primary = new Error("unopened monitored failure");
    try {
      await replacement.run(process.execPath, evalArgs);
      if (cleanup === "close") {
        await original.close();
      } else {
        await assert.rejects(original.inspectFailure(primary), (error) => error === primary);
      }
      await original[Symbol.asyncDispose]();
      await replacement.getByText("ready").wait({ timeout: 5000 });
    } finally {
      await replacement.closeQuiet();
      await original.closeQuiet();
    }
  }
});

test("a monitored readiness failure retains its exact live target for inspection", async () => {
  const terminal = TuiTest.ephemeral("monitor-ready-failure", {
    monitoring: { enabled: true, waitAtEnd: "failure", firstAttachTimeout: 0 },
  });
  let failure;
  try {
    await assert.rejects(
      terminal.run(process.execPath, evalArgs, {
        waitReady: true,
        timeouts: { ready: 25 },
      }),
      (error) => {
        failure = error;
        return error instanceof ExpectationError;
      },
    );
    const stack = failure.stack;
    await terminal.getByText("ready").wait({ timeout: 5000 });
    assert.equal((await terminal.state()).exited, null);
    await assert.rejects(terminal.inspectFailure(failure), (error) => {
      assert.equal(error, failure);
      assert.equal(error.stack, stack);
      return true;
    });
    await assert.rejects(terminal.state(), NoSessionError);
  } finally {
    await terminal.closeQuiet();
  }
});

test("unknown timeout classes are rejected before native dispatch", async () => {
  assert.throws(() => timeoutsPayload({ comand: 100 }), /comand/);
  assert.throws(() => new TuiTest("s", { timeouts: { txt: 100 } }), /txt/);
  const su = new TuiTest(uniqueSession("bad-open-timeout"));
  await assert.rejects(() => su.open({ timeouts: { txt: 100 } }), /txt/);
  await su.closeQuiet();
});

test("session timeout defaults are visible in typed state", async () => {
  const su = new TuiTest(uniqueSession("typed-timeouts"));
  try {
    await su.open({
      shell,
      timeouts: { text: 1234, idle: 2345, command: 3456, exit: 4567, ready: 5678 },
    });
    assert.deepEqual((await su.state()).timeouts, {
      text: 1234,
      idle: 2345,
      command: 3456,
      exit: 4567,
      ready: 5678,
    });
  } finally {
    await su.closeQuiet();
  }
});

test("constructor and per-run profile objects recolor the terminal", async () => {
  const su = new TuiTest(uniqueSession("typed-profile"), {
    profile: { colors: { red: "#010203" } },
  });
  const argsFor = (marker) =>
    typeof globalThis.Deno === "undefined"
      ? ["-e", `process.stdout.write("\\u001b[31m${marker}\\u001b[0m")`]
      : [
          "eval",
          `Deno.stdout.writeSync(new TextEncoder().encode("\\u001b[31m${marker}\\u001b[0m"))`,
        ];
  try {
    await su.run(process.execPath, argsFor("constructor-profile"));
    await su.getByText("constructor-profile").wait({ timeout: 5000 });
    await su
      .getByText("constructor-profile")
      .getByStyle({ foreground: "#010203" })
      .unique()
      .expect();

    await su.run(process.execPath, argsFor("call-profile"), {
      restart: true,
      profile: { colors: { red: "#040506" } },
    });
    await su.getByText("call-profile").wait({ timeout: 5000 });
    await su
      .getByText("call-profile")
      .getByStyle({ foreground: "#040506" })
      .unique()
      .expect();
  } finally {
    await su.closeQuiet();
  }
});

test("constructor and per-call backends reach native sessions", async () => {
  const su = new TuiTest(uniqueSession("typed-backend"), {
    backend: "ghostty",
  });
  try {
    await su.run(process.execPath, evalArgs);
    await su.getByText("ready").wait({ timeout: 2000 });

    await su.run(process.execPath, evalArgs, { backend: "alacritty" });
    await su.getByText("ready").wait({ timeout: 2000 });
  } finally {
    await su.closeQuiet();
  }
});

test("client and per-call timeout precedence reaches native waits", async () => {
  const su = new TuiTest(uniqueSession("typed-timeout-precedence"), {
    timeouts: { text: 120 },
  });
  try {
    await su.run(process.execPath, evalArgs, { timeouts: { text: 2000 } });
    await su.getByText("ready").wait({ timeout: 2000 });
    await assert.rejects(
      su.getByText("missing-client-timeout").wait(),
      (error) =>
        error instanceof ExpectationError &&
        error.message.includes("timed out after 120ms"),
    );
    await assert.rejects(
      su.getByText("missing-call-timeout").wait({ timeout: 30 }),
      (error) =>
        error instanceof ExpectationError &&
        error.message.includes("timed out after 30ms"),
    );
  } finally {
    await su.closeQuiet();
  }
});

test("wait and expectation failures retain operation names", async () => {
  const su = new TuiTest(uniqueSession("typed-operation-errors"));
  try {
    await su.run(process.execPath, evalArgs);
    await su.getByText("ready").wait({ timeout: 2000 });
    await assert.rejects(
      su.getByText("missing-wait").wait({ timeout: 20 }),
      (error) =>
        error instanceof ExpectationError &&
        error.message.startsWith("locator.wait: "),
    );
    await assert.rejects(
      su.getByText("missing-expect").unique().expect({ timeout: 20 }),
      (error) =>
        error instanceof ExpectationError &&
        error.message.startsWith("locator.expect: "),
    );
  } finally {
    await su.closeQuiet();
  }
});

test("typed validation and engine usage errors map to UsageError", async () => {
  const locator = new TuiTest(uniqueSession("invalid-locator-direction"));
  assert.throws(
    () => locator.getByText("parent").getByText("child", { direction: "sideways" }),
    /locator direction must be within, after, or before/,
  );
  assert.throws(
    () => locator.getByText("root", { direction: "after" }),
    /locator direction requires a parent locator/,
  );

  const invalid = new TuiTest(uniqueSession("typed-invalid-size"));
  await assert.rejects(
    invalid.open({ cols: -1 }),
    (error) => error instanceof UsageError && error.kind === "usage",
  );
  await invalid.closeQuiet();

  const invalidShell = new TuiTest(uniqueSession("typed-invalid-shell"));
  await assert.rejects(
    invalidShell.open({ shell: "definitely-not-a-shell" }),
    (error) => error instanceof UsageError && error.kind === "usage",
  );
  await invalidShell.closeQuiet();

  const su = new TuiTest(uniqueSession("typed-invalid-regex"));
  try {
    await su.run(process.execPath, evalArgs);
    await assert.rejects(
      su.getByText("(", { regex: true }).unique().expect({ timeout: 20 }),
      (error) => error instanceof UsageError && error.kind === "usage",
    );
  } finally {
    await su.closeQuiet();
  }
});

test("expectExitCode rejects unsafe JavaScript numbers as UsageError", async () => {
  const su = TuiTest.ephemeral("invalid-exit-code");
  try {
    for (const code of [
      1.5,
      Number.NaN,
      Number.POSITIVE_INFINITY,
      Number.NEGATIVE_INFINITY,
      2_147_483_648,
      -2_147_483_649,
    ]) {
      await assert.rejects(
        su.expectExitCode(code),
        (error) =>
          error instanceof UsageError &&
          error.message.includes("code must be an integer"),
        `expected ${String(code)} to be rejected`,
      );
    }
  } finally {
    await su.closeQuiet();
  }
});

test("N-API argument conversion errors map to UsageError", async () => {
  const runtime = new NativeRuntime(uniqueSession("native-argument-errors"));
  const invalidCalls = [
    () => runtime.run(null),
    () => runtime.write(42),
    () => runtime.resize("80", 24),
    () => runtime.text("false"),
    () => runtime.press("Enter"),
  ];
  try {
    for (const call of invalidCalls) {
      await assert.rejects(
        call,
        (error) => error instanceof UsageError && error.exitCode === 2,
      );
    }
  } finally {
    await runtime.close();
  }
});

test("close remains idempotent without a prior open", async () => {
  const su = TuiTest.ephemeral("never-opened");
  await su.close();
  await su.close();
  await su.closeQuiet();
});

test("inspectFailure never replaces the original error with cleanup failure", async () => {
  const originalClose = NativeRuntime.prototype.close;
  const primary = new Error("primary");
  NativeRuntime.prototype.close = async () => {
    throw new Error("cleanup");
  };
  try {
    const terminal = new TuiTest("preserve-primary");
    await assert.rejects(
      terminal.inspectFailure(primary),
      (error) => error === primary,
    );
  } finally {
    NativeRuntime.prototype.close = originalClose;
  }
});

test("finish reports cleanup failure after a successful test", async () => {
  const originalClose = NativeRuntime.prototype.close;
  const cleanup = new Error("cleanup");
  NativeRuntime.prototype.close = async () => {
    throw cleanup;
  };
  try {
    const terminal = new TuiTest("report-cleanup");
    await assert.rejects(
      terminal.finish({ outcome: "passed" }),
      (error) => error === cleanup,
    );
  } finally {
    NativeRuntime.prototype.close = originalClose;
  }
});

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function monitorInfo(generation, session = "session") {
  return {
    id: randomUUID(),
    command: `tui-test --session ${session} monitor --interactive`,
    generation,
  };
}

async function withRuntimeMethods(methods, fn) {
  const originals = Object.fromEntries(
    Object.keys(methods).map((name) => [name, NativeRuntime.prototype[name]]),
  );
  Object.assign(NativeRuntime.prototype, methods);
  try {
    return await fn();
  } finally {
    Object.assign(NativeRuntime.prototype, originals);
  }
}

test("close and disposal await inspection initialization and share one targeted cleanup", async () => {
  const begin = deferred();
  const waiting = deferred();
  const started = deferred();
  const attached = deferred();
  const calls = [];
  const primary = new Error("original inspection failure");
  const stack = primary.stack;
  await withRuntimeMethods({
    async beginMonitorWait(outcome, timeoutMs, holdWhileAttached) {
      calls.push(["begin", outcome, timeoutMs, holdWhileAttached]);
      started.resolve();
      return begin.promise;
    },
    async waitForMonitor(...args) {
      calls.push(["wait", ...args]);
      attached.resolve();
      return waiting.promise;
    },
    async closeMonitorTarget(generation) {
      calls.push(["closeTarget", generation]);
    },
    async close() {
      assert.fail("inspection must not close a session by name");
    },
    async write(data) {
      calls.push(["write", data]);
    },
    async resize(cols, rows) {
      calls.push(["resize", cols, rows]);
    },
  }, async () => {
    const terminal = new TuiTest("inspection-race", {
      monitoring: { waitAtEnd: "failure", firstAttachTimeout: 42, holdWhileAttached: false },
    });
    const inspected = assert.rejects(terminal.inspectFailure(primary), (error) => {
      assert.equal(error, primary);
      assert.equal(error.stack, stack);
      return true;
    });
    let closed = false;
    const closing = terminal.close().then(() => { closed = true; });
    const disposed = terminal[Symbol.asyncDispose]();
    const finished = terminal.finish({ outcome: "passed" });
    await started.promise;
    assert.equal(closed, false);
    assert.deepEqual(calls, [["begin", "failed", 42, false]]);
    begin.resolve(monitorInfo("7", terminal.session));
    await attached.promise;
    await terminal.write("input during inspection");
    await terminal.resize(100, 40);
    assert.equal(closed, false);
    waiting.resolve(false);
    await Promise.all([inspected, closing, disposed, finished]);
    await terminal.close();
    assert.deepEqual(calls, [
      ["begin", "failed", 42, false],
      ["wait", "7", 42, false],
      ["write", "input during inspection"],
      ["resize", 100, 40],
      ["closeTarget", "7"],
    ]);
  });
});

test("restart cancels inspection only after initialization and before replacing its target", async () => {
  for (const phase of ["initializing", "waiting"]) {
    const begin = deferred();
    const waiting = deferred();
    const started = deferred();
    const calls = [];
    await withRuntimeMethods({
      async beginMonitorWait() {
        calls.push("begin");
        return begin.promise;
      },
      async waitForMonitor(generation) {
        calls.push(["wait", generation]);
        started.resolve();
        return waiting.promise;
      },
      async cancelMonitorWait() {
        calls.push("cancel");
        waiting.resolve(false);
      },
      async closeMonitorTarget(generation) {
        calls.push(["closeTarget", generation]);
      },
      async run() {
        calls.push("run");
        return { session: "restart-race", ready: true, shell_pid: 1, recording: "" };
      },
      async close() { assert.fail("cleanup must use the inspected generation"); },
    }, async () => {
      const terminal = new TuiTest("restart-race", {
        monitoring: { waitAtEnd: "always", firstAttachTimeout: null },
      });
      const finishing = terminal.finish({ outcome: "passed" });
      if (phase === "waiting") {
        begin.resolve(monitorInfo("17", terminal.session));
        await started.promise;
      }
      const restarting = terminal.run("replacement", [], { restart: true });
      if (phase === "initializing") {
        await Promise.resolve();
        assert.deepEqual(calls, ["begin"], "restart must wait for the exact target");
        begin.resolve(monitorInfo("17", terminal.session));
      }
      await Promise.all([finishing, restarting]);
      assert.deepEqual(calls, phase === "initializing"
        ? ["begin", "cancel", ["wait", "17"], ["closeTarget", "17"], "run"]
        : ["begin", ["wait", "17"], "cancel", ["closeTarget", "17"], "run"]);
    });
  }
});

test("restart also releases hold-aware cleanup without an inspection wait", async () => {
  for (const phase of ["initializing", "closing"]) {
    const started = deferred();
    const closing = deferred();
    const calls = [];
    await withRuntimeMethods({
      async beginMonitorWait() { assert.fail("ordinary cleanup must not start inspection"); },
      async cancelMonitorWait() {
        calls.push("cancel");
        closing.resolve();
      },
      async close() {
        calls.push("close");
        started.resolve();
        await closing.promise;
      },
      async run() {
        calls.push("run");
        return { session: "restart-close", ready: true, shell_pid: 1, recording: "" };
      },
    }, async () => {
      const terminal = new TuiTest("restart-close", { monitoring: { enabled: true } });
      const closed = terminal.close();
      if (phase === "closing") await started.promise;
      const restarted = terminal.run("replacement", [], { restart: true });
      await Promise.all([closed, restarted]);
      assert.deepEqual(calls, phase === "initializing"
        ? ["cancel", "close", "run"]
        : ["close", "cancel", "run"]);
    });
  }
});

test("successful finish propagates inspection errors and still cleans up", async () => {
  for (const phase of ["begin", "wait"]) {
    const failure = new Error(`${phase} failed`);
    const closed = [];
    await withRuntimeMethods({
      async beginMonitorWait() {
        if (phase === "begin") throw failure;
        return monitorInfo("11");
      },
      async waitForMonitor() {
        throw failure;
      },
      async close() {
        closed.push("name");
      },
      async closeMonitorTarget(generation) {
        closed.push(generation);
      },
    }, async () => {
      const terminal = new TuiTest("inspection-error", {
        monitoring: { waitAtEnd: "always" },
      });
      await assert.rejects(terminal.finish({ outcome: "passed" }), (error) => error === failure);
      assert.deepEqual(closed, [phase === "begin" ? "name" : "11"]);
    });
  }
});

test("successful finish composes inspection and cleanup errors without losing either", async () => {
  const inspection = new Error("inspection failed");
  const cleanup = new Error("cleanup failed");
  await withRuntimeMethods({
    async beginMonitorWait() { throw inspection; },
    async close() { throw cleanup; },
  }, async () => {
    const terminal = new TuiTest("multiple-finish-errors", {
      monitoring: { waitAtEnd: "always" },
    });
    await assert.rejects(terminal.finish({ outcome: "passed" }), (error) => {
      assert.ok(error instanceof AggregateError);
      assert.deepEqual(error.errors, [inspection, cleanup]);
      return true;
    });
  });
});

test("explicit no-hold policy reaches native cleanup even without end-of-test inspection", async () => {
  const calls = [];
  await withRuntimeMethods({
    async beginMonitorWait(...args) {
      calls.push(["begin", ...args]);
      return monitorInfo("5", "no-hold");
    },
    async waitForMonitor(...args) {
      calls.push(["wait", ...args]);
      return false;
    },
    async closeMonitorTarget(generation) { calls.push(["closeTarget", generation]); },
    async close() { assert.fail("configured cleanup must remain target-bound"); },
  }, async () => {
    for (const [method, waitAtEnd] of [
      ["finish", "never"],
      ["finish", "failure"],
      ["close", "always"],
      ["dispose", "always"],
    ]) {
      calls.length = 0;
      const terminal = TuiTest.ephemeral("no-hold", {
        monitoring: {
          enabled: true, waitAtEnd, firstAttachTimeout: 1234, holdWhileAttached: false,
        },
      });
      if (method === "finish") {
        await terminal.finish({ outcome: "passed" });
      } else if (method === "dispose") {
        await terminal[Symbol.asyncDispose]();
      } else {
        await terminal.close();
      }
      assert.deepEqual(calls, [
        ["begin", "passed", 0, false],
        ["wait", "5", 0, false],
        ["closeTarget", "5"],
      ]);
    }
  });
});

test("no-hold close remains idempotent without a monitorable session", async () => {
  let closes = 0;
  await withRuntimeMethods({
    async beginMonitorWait() { throw new NoSessionError("no active session"); },
    async close() { closes++; },
  }, async () => {
    const terminal = TuiTest.ephemeral("no-hold-unopened", {
      monitoring: { enabled: true, holdWhileAttached: false },
    });
    await terminal.close();
    await terminal.close();
    assert.equal(closes, 1);
  });
});

test("explicit primary failures including undefined survive inspection and cleanup errors", async () => {
  const original = new Error("primary stack");
  const stack = original.stack;
  await withRuntimeMethods({
    async beginMonitorWait() { throw new Error("cannot inspect"); },
    async close() { throw new Error("cannot clean up"); },
  }, async () => {
    for (const primary of [original, undefined, null, "primitive failure"]) {
      const terminal = TuiTest.ephemeral("exact-failure", {
        monitoring: { waitAtEnd: "failure" },
      });
      await terminal.inspectFailure(primary).then(
        () => assert.fail("inspectFailure must reject"),
        (error) => assert.equal(error, primary),
      );
      await terminal.finish({ outcome: "failed", error: primary }).then(
        () => assert.fail("finish with an explicit error must reject"),
        (error) => assert.equal(error, primary),
      );
      await terminal.close();
      await terminal[Symbol.asyncDispose]();
    }
    assert.equal(original.stack, stack);
  });
});

test("concurrent disposal cannot replace an inspection's primary failure with secondary errors", async () => {
  const primary = new Error("authoritative original failure");
  const stack = primary.stack;
  await withRuntimeMethods({
    async beginMonitorWait() { throw new Error("secondary initialization error"); },
    async close() { throw new Error("secondary cleanup error"); },
  }, async () => {
    const terminal = TuiTest.ephemeral("primary-disposal", {
      monitoring: { waitAtEnd: "failure" },
    });
    const inspection = assert.rejects(terminal.inspectFailure(primary), (error) => {
      assert.equal(error, primary);
      assert.equal(error.stack, stack);
      return true;
    });
    await Promise.all([inspection, terminal.close(), terminal[Symbol.asyncDispose]()]);
  });
});

test("async disposal propagates cleanup errors", async () => {
  const cleanup = new Error("dispose failed");
  await withRuntimeMethods({
    async close() { throw cleanup; },
  }, async () => {
    const terminal = new TuiTest("dispose-error");
    await assert.rejects(terminal[Symbol.asyncDispose](), (error) => error === cleanup);
  });
});

test("unmonitored close, finish and disposal share only their in-flight cleanup", async () => {
  const closing = deferred();
  const started = deferred();
  let closes = 0;
  await withRuntimeMethods({
    async close() {
      closes++;
      started.resolve();
      await closing.promise;
    },
  }, async () => {
    const terminal = new TuiTest("shared-cleanup", { monitoring: { enabled: false } });
    const closed = terminal.close();
    const finished = terminal.finish({ outcome: "passed" });
    const disposed = terminal[Symbol.asyncDispose]();
    await started.promise;
    assert.equal(closes, 1);
    closing.resolve();
    await Promise.all([closed, finished, disposed]);
    await terminal.close();
    assert.equal(closes, 2, "later name-based cleanup must not reuse the completed operation");
  });
});

test("a terminal can reopen after finishing without reusing its previous cleanup promise", async () => {
  const calls = [];
  await withRuntimeMethods({
    async run() {
      calls.push("run");
      return { session: "reopen", ready: true, shell_pid: 1, recording: "" };
    },
    async close() { calls.push("close"); },
  }, async () => {
    const terminal = new TuiTest("reopen", { monitoring: { enabled: false } });
    await terminal.run("unused");
    await terminal.finish({ outcome: "passed" });
    await terminal.run("unused");
    await terminal.finish({ outcome: "passed" });
    assert.deepEqual(calls, ["run", "close", "run", "close"]);
  });
});

test("a finite first-attachment timeout finishes without a monitor", { timeout: 10000 }, async () => {
  const terminal = TuiTest.ephemeral("inspection-timeout", {
    monitoring: { enabled: true, waitAtEnd: "always", firstAttachTimeout: 50 },
  });
  try {
    await terminal.run(process.execPath, evalArgs);
    const start = Date.now();
    await terminal.finish({ outcome: "passed" });
    assert.ok(Date.now() - start >= 40, "should wait for the first-attachment timeout");
    await assert.rejects(terminal.state(), NoSessionError);
  } finally {
    await terminal.closeQuiet();
  }
});

test("an awaited inspection keeps Node alive without another referenced handle", {
  skip: typeof globalThis.Deno !== "undefined" || Boolean(process.versions.bun),
}, () => {
  const clientUrl = new URL("../dist/client.js", import.meta.url).href;
  const runtimeUrl = new URL("../dist/native.js", import.meta.url).href;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", `
    import { TuiTest } from ${JSON.stringify(clientUrl)};
    import { NativeRuntime } from ${JSON.stringify(runtimeUrl)};
    NativeRuntime.prototype.beginMonitorWait = async () => (
      ${JSON.stringify(monitorInfo("1", "keepalive"))}
    );
    NativeRuntime.prototype.waitForMonitor = () => new Promise((resolve) => {
      setTimeout(() => resolve(false), 100).unref();
    });
    NativeRuntime.prototype.closeMonitorTarget = async () => {};
    const terminal = new TuiTest("keepalive", { monitoring: { waitAtEnd: "always" } });
    await terminal.finish({ outcome: "passed" });
    console.log("inspection-completed");
  `], { encoding: "utf8", timeout: 10000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /inspection-completed/);
});

test("the CLI inspects the actual JavaScript-owned PTY without replacing its failure", {
  skip: !process.env.TUI_TEST_BIN,
  timeout: 45000,
}, async () => {
  const cli = process.env.TUI_TEST_BIN;
  const execute = promisify(execFile);
  const target = TuiTest.ephemeral("js-cli-monitor", {
    monitoring: {
      enabled: true,
      waitAtEnd: "failure",
      firstAttachTimeout: 10000,
      metadata: {
        testFile: "options.test.mjs",
        testName: "CLI inspects JavaScript-owned PTY",
        framework: "node:test",
        tags: ["interop"],
      },
    },
  });
  const viewer = TuiTest.ephemeral("js-cli-viewer", {
    monitoring: { enabled: false },
  });
  const primary = new Error("original CLI interoperability failure");
  const stack = primary.stack;
  let inspection;
  let settled = false;
  const eventually = async (check, message) => {
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      const result = await check();
      if (result) return result;
      await delay(25);
    }
    assert.fail(message);
  };
  try {
    const program = `
      let input = "";
      const receive = (text) => {
        input += text;
        if (input.includes("through-cli")) {
          console.log("CLI-INPUT-RECEIVED");
          input = "";
        }
      };
      if (typeof Deno !== "undefined") {
        Deno.stdin.setRaw(true);
        console.log("MONITOR-TARGET-READY");
        for await (const bytes of Deno.stdin.readable) {
          receive(new TextDecoder().decode(bytes));
        }
      } else {
        process.stdin.setRawMode(true);
        process.stdin.on("data", (bytes) => receive(bytes.toString()));
        console.log("MONITOR-TARGET-READY");
      }
    `;
    const args = typeof globalThis.Deno === "undefined"
      ? ["--input-type=module", "-e", program]
      : ["eval", program];
    await target.run(process.execPath, args);
    await target.getByText("MONITOR-TARGET-READY").wait({ timeout: 10000 });
    inspection = target.inspectFailure(primary).then(
      () => ({ rejected: false }),
      (error) => ({ rejected: true, error }),
    ).finally(() => { settled = true; });

    const entry = await eventually(async () => {
      const { stdout } = await execute(cli, ["sessions", "--json", "--waiting"], {
        encoding: "utf8",
        timeout: 5000,
      });
      const parsed = JSON.parse(stdout);
      const entries = parsed.details;
      assert.ok(Array.isArray(entries), "sessions discovery must return a session list");
      return entries.find((candidate) => candidate.session === target.session);
    }, "waiting JavaScript-owned session was not discoverable");
    assert.match(entry.id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    for (const removed of ["owner", "ownerType", "generation"]) {
      assert.equal(removed in entry, false);
    }
    assert.equal(entry.pid, process.pid);
    assert.equal(entry.label, "options.test.mjs - CLI inspects JavaScript-owned PTY");
    assert.deepEqual(entry.tags, ["interop"]);
    assert.equal(settled, false);

    await viewer.run(cli, ["monitor", "--interactive", "--id", entry.id], {
      cols: 100,
      rows: 36,
    });
    await viewer.getByText("MONITOR-TARGET-READY").wait({ timeout: 10000 });
    await viewer.write("through-cli");
    await target.getByText("CLI-INPUT-RECEIVED").wait({ timeout: 10000 });
    assert.equal(settled, false, "input must not release inspection");

    const initialSize = await target.getSize();
    await viewer.resize(112, 40);
    await eventually(async () => {
      const size = await target.getSize();
      return size.cols !== initialSize.cols || size.rows !== initialSize.rows;
    }, "interactive viewer resize did not reach the original PTY");
    assert.equal(settled, false, "resizing must not release inspection");
    await target.getByText("CLI-INPUT-RECEIVED").wait({ timeout: 10000 });
    await viewer.getByText("CLI-INPUT-RECEIVED").wait({ timeout: 10000 });

    await viewer.keyboard.press("Ctrl+]");
    await viewer.waitExit({ timeout: 10000 });
    assert.equal((await viewer.state()).exited, 0);
    await eventually(() => settled, "inspection did not resume after Ctrl+] detached");
    const result = await inspection;
    assert.equal(result.rejected, true);
    assert.equal(result.error, primary);
    assert.equal(result.error.stack, stack);
    await assert.rejects(target.state(), NoSessionError);
  } finally {
    await viewer.closeQuiet();
    if (inspection) await inspection;
    await target.closeQuiet();
  }
});
