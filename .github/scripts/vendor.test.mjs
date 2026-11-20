import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { gunzipSync } from "node:zlib";
import { zipSync } from "fflate";
import { prepareConptyAssets, writeConptyAssets } from "./vendor-conpty.mjs";

function directory(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tui-test-vendor-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

function pe(machine, patch = 3002) {
  const data = Buffer.alloc(768);
  data.write("MZ");
  data.writeUInt32LE(128, 0x3c);
  data.write("PE\0\0", 128);
  data.writeUInt16LE(machine, 132);
  Buffer.from([0xbd, 4, 0xef, 0xfe, 0, 0, 1, 0]).copy(data, 512);
  data.writeUInt32LE((1 << 16) | 25, 520);
  data.writeUInt32LE((2603 << 16) | patch, 524);
  return data;
}

function conptyPackage(change = () => {}, rid = "win") {
  const entries = {};
  for (const [arch, machine] of [["x86", 0x14c], ["x64", 0x8664], ["arm64", 0xaa64]]) {
    entries[`runtimes/${rid}-${arch}/native/conpty.dll`] = pe(machine);
    entries[`build/native/runtimes/${arch}/OpenConsole.exe`] = pe(machine);
  }
  change(entries);
  const bytes = Buffer.from(zipSync(entries));
  return {
    bytes,
    pin: {
      version: "1.25.260303002-preview",
      sha256: createHash("sha256").update(bytes).digest("hex"),
    },
  };
}

test("ConPTY vendoring validates all architectures and writes reproducible gzip assets", t => {
  const root = directory(t);
  const { bytes, pin } = conptyPackage();
  const prepared = prepareConptyAssets(bytes, pin);
  assert.equal(prepared.manifest.version, pin.version);
  assert.equal(prepared.manifest.file_version, "1.25.2603.3002");
  assert.equal(prepared.manifest.assets.length, 6);
  writeConptyAssets(prepared, root);
  for (const asset of prepared.manifest.assets) {
    const filename = path.join(root, `${asset.path}.gz`);
    const compressed = fs.readFileSync(filename);
    assert.equal(compressed[9], 255);
    assert.deepEqual(compressed.subarray(4, 8), Buffer.alloc(4));
    const decoded = gunzipSync(compressed);
    assert.equal(decoded.length, asset.size);
    assert.equal(createHash("sha256").update(decoded).digest("hex"), asset.sha256);
  }
  const again = prepareConptyAssets(bytes, pin);
  assert.deepEqual(again.files, prepared.files);
});

test("ConPTY rejects a package checksum mismatch before reading the ZIP", () => {
  const { bytes, pin } = conptyPackage();
  assert.throws(
    () => prepareConptyAssets(bytes, { ...pin, sha256: "0".repeat(64) }),
    /checksum mismatch/
  );
});

test("ConPTY accepts the legacy win10 runtime layout", () => {
  const { bytes, pin } = conptyPackage(() => {}, "win10");
  assert.equal(prepareConptyAssets(bytes, pin).manifest.assets.length, 6);
});

test("ConPTY rejects missing, truncated, mismatched-architecture, and mixed-version assets", () => {
  const cases = [
    [entries => { delete entries["runtimes/win-x86/native/conpty.dll"]; }, /Missing NuGet asset/],
    [entries => { entries["runtimes/win-x64/native/conpty.dll"] = Buffer.alloc(1); }, /Invalid PE header/],
    [entries => { entries["runtimes/win-x64/native/conpty.dll"] = pe(0x14c); }, /PE architecture/],
    [entries => { entries["runtimes/win-x64/native/conpty.dll"] = pe(0x8664, 3003); }, /different file versions/],
  ];
  for (const [change, message] of cases) {
    const { bytes, pin } = conptyPackage(change);
    assert.throws(() => prepareConptyAssets(bytes, pin), message);
  }
});
