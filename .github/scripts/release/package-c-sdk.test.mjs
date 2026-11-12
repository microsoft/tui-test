import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { archiveBytes, readArchive, sha256 } from "./package-native.mjs";
import { bundleEntries, sdkEntries } from "./package-c-sdk.mjs";
import { createFixture, libraryFixture, root, targets } from "./release-fixtures.mjs";

function fixture(t) {
  const f = createFixture(t);
  for (const relative of ["bindings/c", "crates/tui-test-ffi/include",
    "crates/tui-test/assets/jetbrains-mono/OFL.txt", "crates/tui-test/assets/nerd-fonts/LICENSE",
    "crates/tui-test/assets/xtermjs/LICENSE", "crates/tui-test/assets/conpty/LICENSE"]) {
    fs.mkdirSync(path.dirname(path.join(f.root, relative)), { recursive: true });
    fs.cpSync(path.join(root, relative), path.join(f.root, relative), { recursive: true });
  }
  return f;
}

const importLibrary = Buffer.from("!<arch>\nimport library fixture");

test("SDK archives include matching headers, libraries, CMake, examples, and checksums for every target", async t => {
  const f = fixture(t);
  for (const target of Object.keys(targets)) {
    const windows = target.includes("windows");
    const entries = sdkEntries(f.root, target, libraryFixture(target), windows ? importLibrary : undefined);
    const manifest = JSON.parse(entries.get("manifest.json"));
    assert.equal(manifest.abiVersion, 7);
    assert.equal(manifest.target, target);
    assert.equal(manifest.revision, f.revision);
    assert.equal(entries.has("lib/tui_test_ffi.lib"), windows);
    assert.equal(entries.has("licenses/conpty.txt"), windows);
    for (const name of ["include/tui_test.h", "examples/basic.c", "examples/async.c",
      "examples/cpp.cpp", "lib/cmake/tui-test/tui-test-config.cmake"]) assert.ok(entries.has(name), name);
    for (const [name, hash] of Object.entries(manifest.files)) assert.equal(sha256(entries.get(name)), hash);
    const archive = archiveBytes(entries);
    assert.deepEqual(archiveBytes(entries), archive);
    assert.equal((await readArchive(archive)).length, entries.size);
  }
});

test("SDK packaging rejects wrong architectures and missing Windows import libraries", t => {
  const f = fixture(t);
  assert.throws(() => sdkEntries(f.root, "unknown", Buffer.alloc(0)), /Unknown SDK target/);
  assert.throws(() => sdkEntries(f.root, "x86_64-apple-darwin",
    libraryFixture("aarch64-apple-darwin")), /architecture/);
  assert.throws(() => sdkEntries(f.root, "x86_64-pc-windows-msvc",
    libraryFixture("x86_64-pc-windows-msvc")), /import library/);
});

test("combined archives preserve SDK files and make the CLI executable", async t => {
  const f = fixture(t);
  const target = "x86_64-unknown-linux-musl";
  const entries = sdkEntries(f.root, target, libraryFixture(target));
  const cli = Buffer.from("CLI fixture");
  const bundle = await bundleEntries(f.root, target, archiveBytes(entries), cli);
  for (const [name, data] of entries) {
    if (name !== "manifest.json") assert.deepEqual(bundle.entries.get(name), data);
  }
  const members = await readArchive(archiveBytes(bundle.entries, new Set([bundle.executable])));
  assert.equal(members.find(member => member.name === "bin/tui-test").mode, 0o755);
  assert.equal(JSON.parse(bundle.entries.get("manifest.json")).files["bin/tui-test"], sha256(cli));
});

test("bundle assembly rejects corrupt SDK content and mismatched releases", async t => {
  const f = fixture(t);
  const target = "x86_64-unknown-linux-gnu";
  const entries = sdkEntries(f.root, target, libraryFixture(target));
  const cli = Buffer.from("CLI fixture");
  const altered = new Map(entries);
  altered.set("include/tui_test.h", Buffer.from("wrong header"));
  await assert.rejects(bundleEntries(f.root, target, archiveBytes(altered), cli), /checksum mismatch/);
  await assert.rejects(bundleEntries(f.root, "aarch64-unknown-linux-gnu", archiveBytes(entries), cli), /mismatch/);
  altered.set("../escape", Buffer.from("unsafe"));
  await assert.rejects(bundleEntries(f.root, target, archiveBytes(altered), cli), /Unexpected SDK archive member/);
});
