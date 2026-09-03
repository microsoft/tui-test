import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ExpectationError,
  TuiTest,
  UsageError,
  uniqueSession,
} from "../dist/index.js";
import {
  backendPayload,
  envPairs,
  profilePayload,
  recordingPayload,
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
    hyperlinks: undefined,
    colors: [["red", "#010203"]],
  });

  assert.deepEqual(profilePayload({ hyperlinks: false }), {
    scrollback: undefined,
    hyperlinks: false,
    colors: [],
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
