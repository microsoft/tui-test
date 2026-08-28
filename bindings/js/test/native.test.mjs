import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { TuiTest } from "../dist/index.js";

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
    /findLocator\(stages: Array<LocatorStage>\)/,
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
  assert.match(declarations, /waitText\([\s\S]*Promise<Locator>/);
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
});
