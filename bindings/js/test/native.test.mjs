import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { TuiTest } from "../dist/index.js";
import {
  ExpectationError,
  InternalError,
} from "../dist/index.js";
import { mapNativeError } from "../dist/native.js";

const ERROR_PREFIX = "__tui_test_native_error__:";

test("native JSON error envelopes expose structured diagnostics", () => {
  const details = {
    schema_version: 1,
    signature: "sha256:test",
    operation: {
      name: "locator.location",
      elapsed_ms: 12,
      started_screen_sequence: 1,
      failed_screen_sequence: 2,
    },
    reason: "locator_no_match",
    summary: "missing",
    terminal: {
      size: { cols: 80, rows: 24 },
      cursor: { column: 0, row: 0, visible: true, shape: "block" },
      last_visual_change_ms: 1,
      unchanged_for_ms: 2,
      screen_history: {
        limit: 10,
        dropped_screen_count: 0,
        dropped_row_count: 0,
        screens: [
          {
            sequence: 2,
            first_seen_ms: 1,
            last_seen_ms: 2,
            repeat_count: 1,
            changes: [],
            size: { cols: 80, rows: 24 },
            cursor: { column: 0, row: 0, visible: true, shape: "block" },
            text: "current screen",
          },
        ],
      },
    },
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
  const error = mapNativeError(
    new Error(
      `${ERROR_PREFIX}${JSON.stringify({
        kind: "assertion",
        message: "missing",
        details,
        artifact,
      })}`,
    ),
  );

  assert.ok(error instanceof ExpectationError);
  assert.equal(error.message, "missing");
  assert.deepEqual(error.details, details);
  assert.deepEqual(error.artifact, artifact);
  assert.deepEqual(error.terminal, {
    text: "current screen",
    screenshot: artifact.screen_svg,
  });
});

test("native error parsing retains the legacy newline fallback", () => {
  const error = mapNativeError(
    new Error(`${ERROR_PREFIX}assertion\nlegacy failure`),
  );
  assert.ok(error instanceof ExpectationError);
  assert.equal(error.message, "legacy failure");
  assert.equal(error.details, undefined);
  assert.equal(error.artifact, undefined);
});

test("malformed JSON error envelopes become internal transport errors", () => {
  const error = mapNativeError(new Error(`${ERROR_PREFIX}{not-json`));
  assert.ok(error instanceof InternalError);
  assert.match(error.message, /malformed native error envelope/);
});

import { NativeSession } from "../native/index.js";

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
