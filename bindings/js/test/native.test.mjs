import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

import { TuiTest } from "../dist/index.js";
import {
  ExpectationError,
  InternalError,
  NoSessionError,
  UsageError,
} from "../dist/index.js";
import { NativeRuntime, mapNativeError } from "../dist/native.js";

const ERROR_PREFIX = "__tui_test_native_error__:";

function runWithGc(script) {
  const args = typeof globalThis.Deno !== "undefined"
    ? ["eval", "--v8-flags=--expose-gc", script]
    : typeof globalThis.Bun !== "undefined"
      ? ["-e", script]
      : ["--expose-gc", "--input-type=module", "-e", script];
  const child = spawnSync(process.execPath, args, { encoding: "utf8", env: process.env });
  assert.equal(child.status, 0, child.stderr);
}

test("the native function table retains its library through garbage collection", () => {
  runWithGc(`
    import assert from "node:assert/strict";
    const binding = await import(${JSON.stringify(new URL("../native/index.js", import.meta.url).href)});
    for (let i = 0; i < 10; i++) {
      if (typeof globalThis.Bun !== "undefined") globalThis.Bun.gc(true);
      else globalThis.gc();
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    assert.deepEqual(await binding.sessions(), []);
  `);
});

test("nested input buffers remain live during native start and marshaling", () => {
  runWithGc(`
    import assert from "node:assert/strict";
    import koffi from "koffi";
    import { types } from ${JSON.stringify(new URL("../native/abi.js", import.meta.url).href)};
    import { createAdapter } from ${JSON.stringify(new URL("../native/adapter.js", import.meta.url).href)};
    const expected = "nested-owned-buffer-".repeat(16000);
    const read = value => Buffer.from(koffi.decode(value.data, "uint8_t", value.len)).toString();
    let calls = 0;
    const { NativeSession: TestSession } = createAdapter({
      tui_run_start(context, options, program, argv, argc) {
        for (let i = 0; i < 4; i++) {
          if (typeof globalThis.Bun !== "undefined") globalThis.Bun.gc(true);
          else globalThis.gc();
          Buffer.alloc(400000, 0x58);
        }
        const arg = koffi.decode(argv, types.TuiString, argc)[0];
        const pair = koffi.decode(options.env, types.TuiPair, options.env_len)[0];
        assert.ok(read(arg) === expected, "argument buffer changed during native start");
        assert.ok(read(pair.value) === expected, "environment buffer changed during native start");
        calls++;
        return null;
      },
    });
    const session = new TestSession("marshal-gc-regression");
    for (let i = 0; i < 100; i++) {
      await assert.rejects(
        session.run({ program: "unused", args: [expected], env: [["TEST", expected]] }),
        /native call returned a null task/,
      );
    }
    assert.equal(calls, 100);
  `);
});

test("Koffi adapter loads the explicit shared library and honors invalid overrides", (t) => {
  const directory = mkdtempSync(fileURLToPath(new URL(".native-loader-", import.meta.url)));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const native = new URL("../native/", import.meta.url);
  for (const file of readdirSync(native).filter((file) => file.endsWith(".js"))) {
    copyFileSync(new URL(file, native), join(directory, file));
  }
  const extension = { darwin: "dylib", linux: "so", win32: "dll" }[process.platform];
  const variant = process.platform === "linux"
    ? (process.report.getReport().header.glibcVersionRuntime ? "gnu" : "musl")
    : process.platform === "win32" ? "msvc" : undefined;
  const tag = [process.platform, process.arch, variant].filter(Boolean).join("-");
  const binary = process.env.TUI_TEST_NATIVE_LIBRARY
    || new URL(`tui-test.${tag}.${extension}`, native);
  const library = join(directory, `chosen.${extension}`);
  copyFileSync(binary, library);
  const loader = join(directory, "index.js");
  const script = `
    (async () => {
      const assert = (await import("node:assert/strict")).default;
      const binding = await import(${JSON.stringify(pathToFileURL(loader).href)});
      assert.equal(binding.configureLibrary, undefined);
      const session = new binding.NativeSession("standalone-" + process.pid);
      try {
        const args = typeof globalThis.Deno === "undefined"
          ? ["-e", "console.log('standalone-addon')"]
          : ["eval", "console.log('standalone-addon')"];
        await session.run({ program: process.execPath, args });
        await session.waitExit(5000);
        assert.match(await session.text(), /standalone-addon/);
      } finally {
        await session.close();
      }
    })().catch(error => { console.error(error); process.exitCode = 1; });
  `;
  const args = typeof globalThis.Deno === "undefined" ? ["-e", script] : ["eval", script];
  const child = spawnSync(process.execPath, args, {
    encoding: "utf8",
    env: {
      ...process.env,
      TUI_TEST_NATIVE_LIBRARY: library,
    },
  });
  assert.equal(child.status, 0, child.stderr);
  const invalid = spawnSync(process.execPath, args, {
    encoding: "utf8",
    env: { ...process.env, TUI_TEST_NATIVE_LIBRARY: join(directory, "does-not-exist") },
  });
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /Failed to load shared library/);
});

test("concurrent waits cannot starve input or close", async () => {
  const session = new NativeSession(`koffi-concurrency-${process.pid}`);
  try {
    await session.run({
      program: process.execPath,
      args: [typeof globalThis.Deno === "undefined" ? "-e" : "eval",
        'process.stdin.once("data", () => process.stdout.write("\\x1b]0;koffi-completed\\x07")); setInterval(() => {}, 1000);'],
    });
    let completedWaits = 0;
    const waits = Array.from({ length: 32 }, () => session.waitTitle("koffi-completed", { timeoutMs: 5000 })
      .then(() => { completedWaits++; }));
    const sizes = await Promise.all(Array.from({ length: 300 }, () => session.getSize()));
    assert.ok(sizes.every((size) => size.cols === 80 && size.rows === 30));
    assert.equal(completedWaits, 0, "short reads waited for long waits to complete");
    const start = performance.now();
    await session.submit("koffi-completed");
    await Promise.all(waits);
    assert.ok(performance.now() - start < 2000, "waits blocked the input call");
    const pending = Array.from({ length: 32 }, () => session.waitTitle("never-set", { timeoutMs: 5000 }).catch(mapNativeError));
    const closeStart = performance.now();
    await session.close();
    const failures = await Promise.all(pending);
    assert.ok(performance.now() - closeStart < 2000, "waits blocked the close call");
    assert.ok(failures.every((failure) => failure instanceof NoSessionError || failure instanceof ExpectationError));
  } finally {
    await session.close();
  }
});

test("short reads use bounded asynchronous calls without task polling", async () => {
  const { default: koffi } = await import("koffi");
  const { types } = await import("../native/abi.js");
  const { createAdapter } = await import("../native/adapter.js");
  const result = Buffer.alloc(koffi.sizeof(types.TuiResult));
  koffi.encode(result, koffi.offsetof(types.TuiResult, "size"), types.TuiSize, { cols: 80, rows: 30 });
  let active = 0;
  let maximum = 0;
  let completed = 0;
  let freed = 0;
  const { NativeSession: TestSession } = createAdapter({
    tui_get_size: {
      async(context, callback) {
        active++;
        maximum = Math.max(maximum, active);
        setImmediate(() => {
          active--;
          completed++;
          callback(null, result);
        });
      },
    },
    tui_result_free() { freed++; },
  });
  const session = new TestSession("short-read-queue");
  const sizes = await Promise.all(Array.from({ length: 300 }, () => session.getSize()));
  assert.ok(sizes.every((size) => size.cols === 80 && size.rows === 30));
  assert.equal(maximum, 4);
  assert.equal(completed, 300);
  assert.equal(freed, 300);
});

test("native output copies survive result release and later allocations", async () => {
  const session = new NativeSession(`koffi-copy-${process.pid}`);
  try {
    await session.run({
      program: process.execPath,
      args: [typeof globalThis.Deno === "undefined" ? "-e" : "eval",
        'process.stdout.write("copy-αβγ"); setInterval(() => {}, 1000);'],
    });

    await session.waitLocator({ nodes: [{ kind: "text", text: "copy-αβγ" }], root: 0 }, false, 5000);
    const screen = await session.packedScreen();
    const original = Buffer.from(screen.utf8);
    const state = await session.state();
    const cells = await session.cells(0, 0, 8, 1);
    for (let i = 0; i < 30; i++) {
      await session.packedScreen();
      await session.state();
      Buffer.alloc(64 * 1024, i);
    }
    assert.deepEqual(screen.utf8, original);
    assert.match(state.text, /copy-αβγ/);
    assert.equal(cells.map((cell) => cell.char).join(""), "copy-αβγ");
  } finally {
    await session.close();
  }
});

test("typed input buffers survive garbage collection after start", () => {
  const script = `
    import assert from "node:assert/strict";
    import { NativeSession } from ${JSON.stringify(new URL("../native/index.js", import.meta.url).href)};
    const session = new NativeSession("koffi-gc-" + process.pid);
    const expected = "input-copy-" + "αβγ".repeat(1500) + "-input-end";
    const env = [["TUI_COPY_VALUE", expected]];
    const code = 'process.stdout.write(process.env.TUI_COPY_VALUE); setInterval(() => {}, 1000);';
    const args = [typeof globalThis.Deno === "undefined" ? "-e" : "eval", code];
    try {
      const started = session.run({ program: process.execPath, args, env, cols: 120, rows: 60 });
      args[1] = "throw new Error('mutated')";
      env[0][1] = "mutated";
      for (let i = 0; i < 20; i++) {
        Buffer.alloc(1024 * 1024, i);
        if (typeof globalThis.gc === "function") globalThis.gc();
        else globalThis.Bun.gc(true);
      }
      await started;
      await session.waitLocator({ nodes: [{ kind: "text", text: "-input-end" }], root: 0 }, false, 5000);
      const screen = await session.packedScreen();
      assert.ok(Buffer.from(screen.utf8).toString("utf8").replace(/\\s/g, "").includes(expected));
    } finally { await session.close(); }
  `;
  runWithGc(script);
});

test("error and conversion failures free native results and tasks exactly once", async () => {
  const { default: koffi } = await import("koffi");
  const { types } = await import("../native/abi.js");
  const { createAdapter } = await import("../native/adapter.js");
  for (const [method, malformed] of [
    ["state", false], ["state", true], ["getSize", false], ["getSize", true],
  ]) {
    const result = Buffer.alloc(koffi.sizeof(types.TuiResult));
    koffi.encode(result, koffi.offsetof(types.TuiResult, "error_kind"), "uint32_t", 2);
    const message = Buffer.from("test native failure");
    const details = Buffer.from(malformed ? "{invalid-json" : '{"reason":"test"}');
    for (const [field, data] of [["error_message", message], ["error_details", details]]) {
      koffi.encode(result, koffi.offsetof(types.TuiResult, field), types.TuiString, { data, len: data.length });
    }
    let resultsFreed = 0;
    let tasksFreed = 0;
    const { NativeSession: TestSession } = createAdapter({
      tui_state_start: () => result,
      tui_get_size: { async(context, callback) { callback(null, result); } },
      tui_task_poll: () => result,
      tui_task_free: () => { tasksFreed++; },
      tui_result_free: () => { resultsFreed++; },
    });
    await assert.rejects(new TestSession("test")[method]());
    assert.equal(resultsFreed, 1);
    assert.equal(tasksFreed, method === "state" ? 1 : 0);
  }
});

test("native JSON error envelopes expose structured diagnostics", () => {
  const details = {
    schema_version: 1,
    operation: "locator.location",
    reason: "locator_no_match",
    summary: "missing",
    truncated: false,
  };
  const artifact = {
    status: "written",
    directory: "C:\\artifacts\\failure",
    report: "C:\\artifacts\\failure\\failure.md",
    report_html: "C:\\artifacts\\failure\\failure.html",
    timeline: "C:\\artifacts\\failure\\timeline.json",
    screen_text: "C:\\artifacts\\failure\\screen.txt",
    screen_svg: "C:\\artifacts\\failure\\screen.svg",
  };
  const nativeError = new Error(
    `${ERROR_PREFIX}${JSON.stringify({
      kind: "assertion",
      message: "missing",
      details,
      artifact,
    })}`,
  );
  const error = mapNativeError(nativeError);

  assert.ok(error instanceof ExpectationError);
  assert.equal(error.message, "missing");
  assert.deepEqual(error.details, details);
  assert.deepEqual(error.artifact, artifact);
  assert.equal(error.cause, nativeError);
  assert.equal("terminal" in error, false);
});

test("current native envelopes preserve error kinds without failure details", () => {
  for (const [kind, ErrorClass, exitCode] of [
    ["assertion", ExpectationError, 1],
    ["usage", UsageError, 2],
    ["no_session", NoSessionError, 3],
    ["internal", InternalError, 5],
  ]) {
    const nativeError = new Error(
      `${ERROR_PREFIX}${JSON.stringify({ kind, message: "current native failure" })}`,
    );
    const error = mapNativeError(nativeError);
    assert.ok(error instanceof ErrorClass);
    assert.equal(error.kind, kind);
    assert.equal(error.exitCode, exitCode);
    assert.equal(error.message, "current native failure");
    assert.equal(error.details, undefined);
    assert.equal(error.artifact, undefined);
    assert.equal("terminal" in error, false);
    assert.equal(error.cause, nativeError);
  }
});

test("only valid JSON error envelopes are accepted", () => {
  for (const encoded of [
    "{not-json",
    "assertion\nobsolete failure",
    "",
    "null",
    "[]",
    "{}",
    '{"kind":"unknown","message":"failure"}',
    '{"kind":"assertion","message":1}',
  ]) {
    const nativeError = new Error(`${ERROR_PREFIX}${encoded}`);
    const error = mapNativeError(nativeError);
    assert.ok(error instanceof InternalError);
    assert.match(error.message, /malformed native error envelope/);
    assert.equal(error.cause, nativeError);
  }
});

test("unstructured N-API errors retain their current taxonomy", () => {
  for (const nativeError of [
    new TypeError("invalid argument"),
    Object.assign(new Error("invalid argument"), { code: "InvalidArg" }),
    Object.assign(new Error("invalid argument"), { code: "StringExpected" }),
  ]) {
    const error = mapNativeError(nativeError);
    assert.ok(error instanceof UsageError);
    assert.equal(error.message, "invalid argument");
  }
  const error = mapNativeError(new Error("native operation failed"));
  assert.ok(error instanceof InternalError);
  assert.equal(error.message, "native operation failed");
});

test("native constructor validation errors use the public error taxonomy", async () => {
  const runtime = new NativeRuntime(
    `invalid-native-trace-${process.pid}`,
    undefined,
    undefined,
    { mode: "invalid" },
  );
  await assert.rejects(runtime.state(), (error) => {
    assert.ok(error instanceof UsageError);
    assert.match(error.message, /unknown trace mode/);
    assert.equal(error.details, undefined);
    assert.equal(error.artifact, undefined);
    return true;
  });
});

import { NativeSession } from "../native/index.js";

test("owned native inputs preserve numeric and operation validation", async () => {
  const session = new NativeSession(`owned-validation-${process.pid}`);
  const failures = [
    [() => session.open({ cols: 65536 }), /cols must be an integer between 0 and 65535/],
    [() => session.open({ timeouts: { text: Number.NaN } }), /timeouts.text must be an integer/],
    [() => session.run({ program: "" }), /program must not be empty/],
    [() => session.run({ program: "unused", screenHistoryLimit: 65535 }), /screen history limit/],
    [() => session.resize(1.5, 20), /cols must be an integer/],
    [() => session.waitIdle(Number.MAX_SAFE_INTEGER + 1), /timeoutMs must be an integer/],
    [() => session.mouseDown(0, 0, 3), /invalid mouse button code 3/],
    [() => session.mouseClick({ x: -1 }), /x must be an integer/],
    [() => session.expectExitCode(2147483648), /code must be an integer/],
    [() => session.startRecording({ path: "unused", fps: 256 }), /fps must be an integer/],
    [() => session.screenshot({ background: "#000000", transparent: true }), /background and transparent options conflict/],
    [() => session.waitClipboard(undefined, { regex: true }), /clipboard regex requires text/],
    [() => session.waitClipboard("[", { regex: true }), /invalid regex/],
  ];
  for (const [call, message] of failures) {
    await assert.rejects(call, (error) => {
      const mapped = mapNativeError(error);
      assert.ok(mapped instanceof UsageError, mapped.message);
      assert.match(mapped.message, message);
      return true;
    });
  }
  await session.close(false);
  await session.close(true);
});

test("owned session contexts validate each call without changing caller options", async () => {
  const trace = { mode: "invalid", directory: "" };
  const session = new NativeSession(`owned-context-${process.pid}`, undefined, undefined, trace);
  for (const call of [() => session.state(), () => session.open(), () => session.close(false)]) {
    await assert.rejects(call, (error) => {
      const mapped = mapNativeError(error);
      assert.ok(mapped instanceof UsageError);
      assert.match(mapped.message, /unknown trace mode/);
      return true;
    });
  }
  assert.deepEqual(trace, { mode: "invalid", directory: "" });
});

test("locator expressions reject invalid fields and references", async () => {
  const session = new NativeSession(`invalid-expression-${process.pid}`);
  for (const node of [
    { kind: "style", style: { bold: true, link: "test:link" } },
    { kind: "text", text: "x", style: { bold: true } },
    { kind: "link" },
    { kind: "and", left: 0, right: 0 },
    { kind: "filter", hasText: "x" },
  ]) {
    await assert.rejects(async () => session.findLocator({ nodes: [node], root: 0 }));
  }
});

test("composition validates ownership and locator-only filters without reading", () => {
  const term = new TuiTest(`compose-${process.pid}`);
  const other = new TuiTest(term.session);
  const bold = term.getByStyle({ bold: true });
  const link = term.getByLink("test:link");
  assert.doesNotThrow(() => bold.and(link).or(term.getByText("fallback")).filter({ has: link }));
  assert.throws(() => bold.and(other.getByLink("test:link")), /same terminal owner/);
  assert.throws(() => bold.or(other.getByText("x")), /same terminal owner/);
  assert.throws(() => bold.filter({ has: other.getByText("x") }), /same terminal owner/);
  assert.throws(() => bold.filter({}), /requires has or hasNot/);
  assert.throws(() => bold.filter({ has: link, link: "test:link" }), /unknown filter property/);
  assert.throws(() => term.getByStyle({ bold: true, link: "test:link" }), /unknown style property/);
  assert.throws(() => term.getByLink(undefined), /URI string/);
});

test("generated native declarations expose typed operations", async () => {
  const declarations = await readFile(
    new URL("../native/index.d.ts", import.meta.url),
    "utf8",
  );
  for (const type of [
    "BellEvent",
    "OpenResult",
    "State",
    "Cursor",
    "Size",
    "Timeouts",
    "Cell",
    "PackedScreen",
    "RecordingOptions",
    "TextMatch",
  ]) {
    assert.match(declarations, new RegExp(`export (?:interface|type) ${type}\\b`));
  }
  for (const method of [
    "open",
    "run",
    "restart",
    "close",
    "state",
    "text",
    "findLocator",
    "waitLocator",
    "clickLocator",
    "highlightLocator",
    "expectLocator",
    "cells",
    "getCommand",
    "getClipboard",
    "getBellCount",
    "getBellEvents",
    "write",
    "type",
    "submit",
    "press",
    "keyDown",
    "repeat",
    "keyUp",
    "mouseClick",
    "resize",
    "signal",
    "waitClipboard",
    "waitBell",
    "expectBellCount",
    "snapshot",
    "screenshot",
    "startRecording",
    "stopRecording",
    "packedScreen",
    "panicProbe",
  ]) {
    assert.match(declarations, new RegExp(`\\b${method}\\(`));
  }
  assert.doesNotMatch(declarations, /\brequest\(/);
  assert.doesNotMatch(declarations, /Promise<unknown>/);
  assert.match(
    declarations,
    /findLocator\(expression: LocatorExpression, requireOne\?: boolean/,
  );
  assert.match(declarations, /constructor\(name: string, recording\?.*artifacts\?/);
  assert.match(declarations, /export interface FailureArtifactOptions\b/);
  assert.match(
    declarations,
    /interface OpenOptions \{[\s\S]*screenHistoryLimit\?: number/,
  );
  assert.match(
    declarations,
    /interface RunOptions \{[\s\S]*screenHistoryLimit\?: number/,
  );
  assert.doesNotMatch(declarations, /(?:queryJson|requestJson): string/);
  assert.match(
    declarations,
    /interface PackedScreen \{[\s\S]*readonly cols: number[\s\S]*readonly rows: number[\s\S]*readonly utf8: Uint8Array[\s\S]*\}/,
  );
  assert.doesNotMatch(declarations, /\bBuffer\b/);
  assert.doesNotMatch(declarations, /interface PackedScreen \{[\s\S]*\bbuffer:/);
});

test("public declarations expose reusable get-by locators", async () => {
  const declarations = await readFile(
    new URL("../dist/client.d.ts", import.meta.url),
    "utf8",
  );
  for (const method of [
    "getByText",
    "getByStyle",
    "getByLink",
    "and",
    "or",
    "filter",
    "any",
    "unique",
    "first",
    "last",
    "nth",
    "locations",
    "location",
    "count",
    "all",
    "wait",
    "click",
    "highlight",
    "expect",
  ]) {
    assert.match(declarations, new RegExp(`\\b${method}\\(`));
  }
  assert.match(declarations, /\bgetByText\(text: string/);
  assert.match(declarations, /\bgetByStyle\(/);
  assert.doesNotMatch(declarations, /\b(?:findText|waitText|expectText)\(/);
  const textOptions = declarations.match(
    /export interface TextSelectorOptions \{([^}]*)\}/s,
  )?.[1];
  const styleOptions = declarations.match(
    /export interface StyleSelectorOptions \{([^}]*)\}/s,
  )?.[1];
  const expectOptions = declarations.match(
    /export interface LocatorExpectOptions \{([^}]*)\}/s,
  )?.[1];
  assert.doesNotMatch(textOptions ?? "", /occurrence/);
  assert.doesNotMatch(styleOptions ?? "", /occurrence/);
  assert.doesNotMatch(expectOptions ?? "", /style/);
  const filterOptions = declarations.match(/export interface LocatorFilterOptions \{([^}]*)\}/s)?.[1];
  assert.match(filterOptions ?? "", /has\?: Locator/);
  assert.match(filterOptions ?? "", /hasNot\?: Locator/);
  assert.doesNotMatch(filterOptions ?? "", /(?:style|link|hasText|hasNotText)\??:/);
  const styleFields = declarations.match(/export interface TextStyleExpectation \{([^}]*)\}/s)?.[1];
  assert.doesNotMatch(styleFields ?? "", /\blink\??:/);
});

test("public client options expose screenHistoryLimit", async () => {
  const declarations = await readFile(
    new URL("../dist/types.d.ts", import.meta.url),
    "utf8",
  );
  assert.match(
    declarations,
    /interface ClientOptions \{[\s\S]*screenHistoryLimit\?: number/,
  );
  assert.doesNotMatch(declarations, /\bTerminalArtifact\b/);
  const errors = await readFile(
    new URL("../dist/errors.d.ts", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(errors, /\bterminal\??:/);
  const entrypoint = await readFile(
    new URL("../dist/index.d.ts", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(entrypoint, /\bTerminalArtifact\b/);
});

test("public facade omits generic request dispatchers", async () => {
  const declarations = await readFile(
    new URL("../dist/client.d.ts", import.meta.url),
    "utf8",
  );
  assert.equal("send" in TuiTest.prototype, false);
  assert.equal("get" in TuiTest.prototype, false);
  assert.doesNotMatch(declarations, /\bsend\(/);
  assert.doesNotMatch(declarations, /\bget\(/);
  assert.doesNotMatch(declarations, /payload|request dispatcher/);
  assert.match(
    declarations,
    /export type MouseButton = "left" \| "middle" \| "right"/,
  );
  assert.match(
    declarations,
    /interface MouseButtonOptions \{[\s\S]*button\?: MouseButton[\s\S]*alt\?: boolean[\s\S]*ctrl\?: boolean[\s\S]*shift\?: boolean[\s\S]*\}/,
  );
  assert.match(
    declarations,
    /interface LocatorClickOptions extends MouseButtonOptions/,
  );
  assert.doesNotMatch(declarations, /interface MouseButtonOptions \{\s*button\?: number/);
  assert.doesNotMatch(declarations, /interface LocatorClickOptions \{\s*button\?: number/);
});
