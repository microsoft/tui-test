import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  PATCHED,
  UNPATCHED,
  parseZigEnvField,
  patchWindowsSelfInfo,
} from "./patch-zig-windows-arm64.mjs";

test("parseZigEnvField reads ZON and JSON zig env output", () => {
  const zon = `.{
    .std_dir = "C:\\\\zig-0.16.0\\\\lib\\\\std",
    .version = "0.16.0",
}`;
  assert.equal(parseZigEnvField(zon, "std_dir"), "C:\\zig-0.16.0\\lib\\std");
  assert.equal(parseZigEnvField(zon, "version"), "0.16.0");

  const json = JSON.stringify({ std_dir: "/opt/zig/lib/std", version: "0.16.0" });
  assert.equal(parseZigEnvField(json, "std_dir"), "/opt/zig/lib/std");
  assert.throws(() => parseZigEnvField(zon, "missing"), /Missing missing/);
});

test("patchWindowsSelfInfo is idempotent and enforces 0.16.0 expectations", t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "zig-patch-test-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const filename = path.join(dir, "Windows.zig");

  fs.writeFileSync(filename, `return if (load)\n    ${UNPATCHED}\nelse\n    null;\n`);
  assert.equal(patchWindowsSelfInfo(filename, "0.16.0"), "patched");
  assert.ok(fs.readFileSync(filename, "utf8").includes(PATCHED));

  assert.equal(patchWindowsSelfInfo(filename, "0.16.0"), "already-patched");

  fs.writeFileSync(filename, "// refactored upstream\n");
  assert.throws(() => patchWindowsSelfInfo(filename, "0.16.0"), /Expected one unpatched/);
  assert.equal(patchWindowsSelfInfo(filename, "0.16.1"), "not-needed");
});
