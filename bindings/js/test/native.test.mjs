import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
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

test("standalone addon works without a companion library and ignores the library override", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "tui-test-node-standalone-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const native = new URL("../native/", import.meta.url);
  for (const file of readdirSync(native).filter((file) => file.endsWith(".node"))) {
    copyFileSync(new URL(file, native), join(directory, file));
  }
  const loader = join(directory, "index.mjs");
  copyFileSync(new URL("index.js", native), loader);
  assert.ok(readdirSync(directory).some((file) => file.endsWith(".node")));
  assert.ok(readdirSync(directory).every((file) => file.endsWith(".node") || file === "index.mjs"));
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
      TUI_TEST_NATIVE_LIBRARY: join(directory, "does-not-exist"),
    },
  });
  assert.equal(child.status, 0, child.stderr);
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
