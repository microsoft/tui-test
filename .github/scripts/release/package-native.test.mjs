import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { gzipSync } from "node:zlib";
import { Header } from "tar";
import * as packages from "./package-native.mjs";
import * as elf from "./check-native-elf.mjs";
import { createFixture, libraryFixture, targets } from "./release-fixtures.mjs";

test("native CLI creates deterministic archives for every platform", async t => {
  const fixture = createFixture(t);
  for (const target of Object.keys(targets)) {
    const library = path.join(fixture.root, packages.libraryName(target));
    fs.writeFileSync(library, libraryFixture(target));
    const args = ["--library", library, "--target", target];
    fixture.run("native", args);
    const filename = path.join(fixture.root, "target/native-packages", packages.nativeFilename("1.2.3", target));
    const first = fs.readFileSync(filename);
    fixture.run("native", args);
    assert.deepEqual(fs.readFileSync(filename), first);
    assert.deepEqual(first.subarray(4, 8), Buffer.alloc(4));
    assert.equal(fs.readFileSync(`${filename}.sha256`, "utf8"), `${packages.sha256(first)}  ${path.basename(filename)}\n`);
    assert.equal((await packages.readNative(first, "1.2.3", fixture.revision, target)).target, target);
    const members = await packages.readArchive(first);
    assert.equal(members.length, 3);
    for (const member of members) {
      assert.deepEqual([member.uid, member.gid, +member.mtime, member.mode], [0, 0, 0, 0o644]);
    }
  }
});

test("wrong platform and truncated library headers are rejected", t => {
  const fixture = createFixture(t);
  const filename = path.join(fixture.root, "wrong.so");
  fs.writeFileSync(filename, libraryFixture("aarch64-apple-darwin"));
  fixture.run("native", ["--library", filename, "--target", "aarch64-unknown-linux-gnu"], false);
  assert.equal(fs.existsSync(path.join(fixture.root, "target/native-packages")), false);
  for (const target of Object.keys(targets)) {
    assert.throws(() => packages.validateLibrary(Buffer.alloc(0), target), /format or architecture/);
  }
});

test("module assembly includes all targets without changing source", async t => {
  const fixture = createFixture(t);
  const before = packages.directoryEntries(fixture.source);
  fixture.assemble();
  assert.deepEqual(packages.directoryEntries(fixture.source), before);
  const contents = packages.directoryEntries(fixture.output);
  assert.deepEqual(await packages.verifyModule(fixture.root, fixture.output), contents);
  assert.deepEqual(contents.get("LICENSE"), fs.readFileSync(path.join(fixture.root, "LICENSE")));
  assert.equal([...contents.keys()].filter(name => /bundled_.*\.go$/.test(name)).length, 5);
  assert.equal([...contents.keys()].filter(name => name.endsWith(".tar.gz")).length, 7);
  const filename = path.join(fixture.root, "tui-test-go-1.2.3.tar.gz");
  assert.ok(fs.existsSync(`${filename}.sha256`));
  const archived = await packages.readArchive(fs.readFileSync(filename));
  assert.deepEqual(new Map(archived.map(entry => [entry.name, entry.data])), contents);
  fixture.run("go", ["--artifacts", fixture.artifacts, "--output", fixture.output], false);
});

test("generated embeds select and build for every Go platform", t => {
  const fixture = createFixture(t);
  fixture.assemble();
  const scratch = path.join(fixture.root, "go-tmp");
  fs.mkdirSync(scratch);
  for (const [platform, arch] of [
    ["linux", "amd64"], ["linux", "arm64"], ["darwin", "amd64"],
    ["darwin", "arm64"], ["windows", "amd64"],
  ]) {
    const env = {
      ...process.env, GOOS: platform, GOARCH: arch, CGO_ENABLED: "0", GOWORK: "off",
      GOTMPDIR: scratch, TMPDIR: scratch, GOCACHE: path.join(scratch, "cache"),
    };
    const details = JSON.parse(execFileSync("go", ["list", "-json", "./internal/native"], {
      cwd: fixture.output, env, encoding: "utf8",
    }));
    assert.ok(details.GoFiles.includes(`bundled_${platform}_${arch}.go`));
    assert.equal(details.EmbedFiles.length, platform === "linux" ? 2 : 1);
    execFileSync("go", ["build", "./..."], { cwd: fixture.output, env, stdio: "pipe" });
  }
});

test("partial target selection creates only the selected platform", t => {
  const fixture = createFixture(t);
  fixture.run("go", ["--artifacts", fixture.artifacts, "--target", "aarch64-apple-darwin"]);
  const output = path.join(fixture.root, "target/go-package/internal/native");
  assert.equal(fs.readdirSync(path.join(output, "bundled")).length, 1);
  assert.ok(fs.existsSync(path.join(output, "bundled_darwin_arm64.go")));
});

test("missing, extra, duplicate, and unknown targets are rejected", t => {
  const fixture = createFixture(t);
  const filename = fixture.writeNative("aarch64-apple-darwin");
  const saved = fs.readFileSync(filename);
  fs.unlinkSync(filename);
  const args = ["--artifacts", fixture.artifacts, "--output", fixture.output];
  fixture.run("go", args, false);
  assert.equal(fs.existsSync(fixture.output), false);
  fs.writeFileSync(filename, saved);
  const extra = path.join(fixture.artifacts, "extra.tar.gz");
  fs.writeFileSync(extra, saved);
  fixture.run("go", args, false);
  fs.unlinkSync(extra);
  for (const selected of [["unsupported"], ["aarch64-apple-darwin", "aarch64-apple-darwin"]]) {
    fixture.run("go", [...args, ...selected.flatMap(target => ["--target", target])], false);
  }
  assert.equal(fs.existsSync(fixture.output), false);
});

test("manifest, library, and archive checksum tampering is rejected", async t => {
  const fixture = createFixture(t);
  const target = "aarch64-unknown-linux-gnu";
  const filename = fixture.writeNative(target);
  const original = new Map((await packages.readArchive(fs.readFileSync(filename))).map(entry => [entry.name, entry.data]));
  const args = ["--artifacts", fixture.artifacts, "--output", fixture.output];
  for (const field of ["version", "revision", "target", "library", "sha256", "extra"]) {
    const contents = new Map(original);
    const manifest = JSON.parse(contents.get("manifest.json"));
    manifest[field] = "tampered";
    contents.set("manifest.json", Buffer.from(JSON.stringify(manifest)));
    packages.writeArchive(filename, contents);
    fixture.run("go", args, false);
    assert.equal(fs.existsSync(fixture.output), false);
  }
  const contents = new Map(original);
  const library = packages.libraryName(target);
  contents.set(library, Buffer.concat([contents.get(library), Buffer.from("tampered")]));
  packages.writeArchive(filename, contents);
  fixture.run("go", args, false);
  fixture.writeNative(target);
  fs.writeFileSync(`${filename}.sha256`, "0".repeat(64));
  fixture.run("go", args, false);
});

test("unsafe archive members are rejected without extraction", async t => {
  const fixture = createFixture(t);
  const target = "x86_64-unknown-linux-musl";
  for (const kind of ["traversal", "symlink", "hardlink", "duplicate", "extra", "absolute"]) {
    const filename = fixture.writeNative(target);
    const members = await packages.readArchive(fs.readFileSync(filename));
    const blocks = [];
    for (const member of members) {
      const header = new Header({
        path: member.name, size: member.data.length, type: "File", mode: 0o644,
        uid: 0, gid: 0, mtime: new Date(0),
      });
      let data = member.data;
      if (member.name === packages.libraryName(target)) {
        if (kind === "traversal") header.path = "../outside";
        if (kind === "absolute") header.path = "/outside";
        if (kind === "symlink" || kind === "hardlink") {
          header.type = kind === "symlink" ? "SymbolicLink" : "Link";
          header.linkpath = "../outside";
          header.size = 0;
          data = Buffer.alloc(0);
        }
        if (kind === "duplicate") {
          header.encode();
          blocks.push(Buffer.from(header.block), data, Buffer.alloc((512 - data.length % 512) % 512));
        }
      }
      header.encode();
      blocks.push(header.block, data, Buffer.alloc((512 - data.length % 512) % 512));
    }
    if (kind === "extra") {
      const header = new Header({ path: "extra", size: 0, type: "File" });
      header.encode();
      blocks.push(header.block);
    }
    blocks.push(Buffer.alloc(1024));
    fs.writeFileSync(filename, gzipSync(Buffer.concat(blocks)));
    fs.unlinkSync(`${filename}.sha256`);
    fixture.run("go", ["--artifacts", fixture.artifacts, "--output", fixture.output], false);
    assert.equal(fs.existsSync(fixture.output), false);
  }
});

test("invalid gzip trailers fail reading, assembly, and publication verification", async t => {
  const fixture = createFixture(t);
  const target = "aarch64-unknown-linux-gnu";
  const filename = fixture.writeNative(target);
  const original = fs.readFileSync(filename);
  fixture.assemble();
  const crc = Buffer.from(original);
  crc[crc.length - 8] ^= 1;
  const size = Buffer.from(original);
  size[size.length - 4] ^= 1;
  for (const [damage, data] of [["truncated", original.subarray(0, -1)], ["crc", crc], ["size", size]]) {
    await assert.rejects(packages.readNative(data, "1.2.3", fixture.revision, target));
    fs.writeFileSync(filename, data);
    fs.writeFileSync(`${filename}.sha256`, `${packages.sha256(data)}  ${path.basename(filename)}\n`);
    const output = path.join(fixture.root, `invalid-module-${damage}`);
    fixture.run("go", ["--artifacts", fixture.artifacts, "--output", output], false);
    assert.equal(fs.existsSync(output), false);
    fs.writeFileSync(path.join(fixture.output, `internal/native/bundled/${target}.tar.gz`), data);
    await assert.rejects(packages.verifyModule(fixture.root, fixture.output));
  }
});

test("module size and output inside source are rejected", t => {
  const fixture = createFixture(t);
  assert.throws(() => packages.checkModuleSize(new Map([["oversized", { length: packages.MAX_MODULE_SIZE }]])), /500 MiB/);
  const output = path.join(fixture.source, "new-directory");
  fixture.run("go", ["--artifacts", fixture.artifacts, "--output", output], false);
  assert.equal(fs.existsSync(output), false);
  const alias = path.join(fixture.root, "source-alias");
  fs.symlinkSync(fixture.source, alias, "junction");
  fixture.run("go", ["--artifacts", fixture.artifacts, "--output", path.join(alias, "new-directory")], false);
  assert.equal(fs.existsSync(output), false);
});

test("unversioned required symbols are rejected", () => {
  elf.checkSymbols([
    "0: 0000000000000000 0 NOTYPE LOCAL DEFAULT UND",
    "1: 0000000000000000 0 FUNC GLOBAL DEFAULT UND memcpy@GLIBC_2.14 (2)",
    "2: 0000000000000000 0 NOTYPE WEAK DEFAULT UND _ITM_registerTMCloneTable",
    "3: 0000000000000010 8 FUNC GLOBAL DEFAULT 12 tui_open",
  ].join("\n"));
  for (const kind of ["FUNC", "NOTYPE"]) {
    assert.throws(() => elf.checkSymbols(`1: 0000000000000000 0 ${kind} GLOBAL DEFAULT UND arc4random_buf`), /arc4random_buf/);
  }
});

test("optimized GNU dependencies are accepted without C++ runtimes", () => {
  const output = ["libc.so.6", "libdl.so.2", "libm.so.6", "libpthread.so.0", "librt.so.1", "libutil.so.1"]
    .map(library => `(NEEDED) Shared library: [${library}]`).join("\n");
  for (const arch of ["aarch64", "x86_64"]) {
    const target = `${arch}-unknown-linux-gnu`;
    elf.checkDependencies(output, target);
    for (const library of ["libstdc++.so.6", "libc++.so.1", "libcustom.so"]) {
      assert.throws(() => elf.checkDependencies(`${output}\n(NEEDED) Shared library: [${library}]`, target));
    }
  }
});

test("glibc symbol baseline rejects new and private versions", () => {
  elf.checkVersions("GLIBC_2.2.5 GLIBC_2.17 GLIBC_2.28");
  for (const output of ["GLIBC_2.29", "GLIBC_PRIVATE", "GLIBC_2.28 GLIBC_2.34", ""]) {
    assert.throws(() => elf.checkVersions(output));
  }
});

test("dependency checks reject unknown libraries and runtime paths", () => {
  elf.checkDependencies("(NEEDED) Shared library: [libc.so.6]", "x86_64-unknown-linux-gnu");
  elf.checkDependencies("(NEEDED) Shared library: [libc.so]", "aarch64-unknown-linux-musl");
  for (const target of ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-musl"]) {
    for (const library of ["libstdc++.so.6", "libcustom.so"]) {
      assert.throws(() => elf.checkDependencies(`(NEEDED) Shared library: [${library}]`, target));
    }
  }
  assert.throws(() => elf.checkDependencies("(NEEDED) [libc.so.6]\n(RUNPATH) [/build/lib]", "x86_64-unknown-linux-gnu"));
});
