import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as packages from "./package-native.mjs";

export const scripts = fileURLToPath(new URL("./", import.meta.url));
export const root = fileURLToPath(new URL("../../../", import.meta.url));
export const targets = JSON.parse(fs.readFileSync(path.join(root, "bindings/js/package.json"), "utf8")).nativeTargets;

export function libraryFixture(target) {
  const data = Buffer.alloc(128);
  const arm = target.startsWith("aarch64");
  if (target.includes("linux")) {
    data.write("\x7fELF\x02\x01", 0, "binary");
    data.writeUInt16LE(3, 16);
    data.writeUInt16LE(arm ? 183 : 62, 18);
  } else if (target.includes("apple")) {
    data.writeUInt32LE(0xfeedfacf, 0);
    data.writeUInt32LE(arm ? 0x100000c : 0x1000007, 4);
    data.writeUInt32LE(6, 12);
  } else {
    data.write("MZ");
    data.writeUInt32LE(64, 60);
    data.write("PE\0\0", 64);
    data.writeUInt16LE(0x8664, 68);
    data.writeUInt16LE(0x2000, 86);
  }
  return data;
}

export function createFixture(t) {
  fs.mkdirSync(path.join(root, "target"), { recursive: true });
  const directory = fs.mkdtempSync(path.join(root, "target/release-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  fs.mkdirSync(path.join(directory, "bindings/js"), { recursive: true });
  fs.writeFileSync(path.join(directory, "bindings/js/package.json"), JSON.stringify({
    version: "1.2.3", nativeTargets: targets,
  }));
  fs.writeFileSync(path.join(directory, "LICENSE"), "License fixture\n");
  const source = path.join(directory, "bindings/go");
  fs.mkdirSync(path.join(source, "internal/native"), { recursive: true });
  fs.writeFileSync(path.join(source, "go.mod"), "module example.com/fixture\n\ngo 1.23\n");
  fs.writeFileSync(path.join(source, "internal/native/registry.go"),
    "package native\n"
    + "type bundledLibrary struct { name string; checksum string; archive []byte }\n"
    + "var bundledLibraries = map[string]bundledLibrary{}\n");
  const fixture = {
    root: directory, source, artifacts: path.join(directory, "artifacts"),
    output: path.join(directory, "assembled"),
    revision: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
    writeNative(target) {
      const data = libraryFixture(target);
      const manifest = {
        version: "1.2.3", revision: this.revision, target,
        library: packages.libraryName(target), sha256: packages.sha256(data),
      };
      const filename = path.join(this.artifacts, packages.nativeFilename("1.2.3", target));
      packages.writeArchive(filename, new Map([
        [manifest.library, data], ["LICENSE", Buffer.from("License fixture\n")],
        ["manifest.json", Buffer.from(JSON.stringify(manifest))],
      ]));
      return filename;
    },
    run(command, args = [], succeeds = true) {
      const result = spawnSync(process.execPath, [path.join(scripts, "package-native.mjs"), command, ...args], {
        cwd: this.root, encoding: "utf8",
      });
      if (result.error) throw result.error;
      if (succeeds) assert.equal(result.status, 0, result.stdout + result.stderr);
      else assert.notEqual(result.status, 0, result.stdout);
      return result;
    },
    assemble() {
      return this.run("go", ["--artifacts", this.artifacts, "--output", this.output]);
    },
  };
  fs.mkdirSync(fixture.artifacts);
  for (const target of Object.keys(targets)) fixture.writeNative(target);
  return fixture;
}
