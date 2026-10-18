import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const script = fileURLToPath(new URL("./package-node.mjs", import.meta.url));
const { nativeTargets } = JSON.parse(fs.readFileSync(new URL("../../../bindings/js/package.json", import.meta.url), "utf8"));
const targets = Object.values(nativeTargets);
const library = (platform) => `tui-test.${platform}.${platform.startsWith("darwin") ? "dylib" : platform.startsWith("win32") ? "dll" : "so"}`;

function fixture(t) {
  const target = path.resolve("target");
  fs.mkdirSync(target, { recursive: true });
  const directory = fs.mkdtempSync(path.join(target, "package-node-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const bindings = path.join(directory, "bindings/js");
  fs.mkdirSync(path.join(bindings, "artifacts"), { recursive: true });
  fs.writeFileSync(path.join(bindings, "package.json"), JSON.stringify({
    name: "@fixture/tui-test",
    version: "1.0.0",
    files: ["index.js"],
    dependencies: { koffi: "^3.3.3" },
    nativeTargets,
  }));
  fs.writeFileSync(path.join(bindings, "index.js"), "export {};\n");
  for (const platform of targets) {
    fs.writeFileSync(path.join(bindings, "artifacts", library(platform)), "shared library fixture");
  }
  return directory;
}

test("platform packages include the shared library at the release version", (t) => {
  const directory = fixture(t);
  const packagePath = path.join(directory, "bindings/js/package.json");
  const original = fs.readFileSync(packagePath, "utf8");
  execFileSync(process.execPath, [script], { cwd: directory, stdio: "pipe" });
  assert.equal(fs.readFileSync(packagePath, "utf8"), original);
  for (const platform of targets) {
    const archive = path.join(directory, "package-artifacts/npm", `fixture-tui-test-${platform}-1.0.0.tgz`);
    const entries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" }).split("\n");
    assert.ok(entries.includes(`package/${library(platform)}`));
    assert.ok(!entries.some((entry) => entry.endsWith(".node")));
    const manifest = JSON.parse(execFileSync("tar", ["-xOf", archive, "package/package.json"], { encoding: "utf8" }));
    assert.equal(manifest.version, "1.0.0");
    assert.deepEqual(manifest.os, [platform.split("-")[0]]);
    assert.deepEqual(manifest.cpu, [platform.split("-")[1]]);
    if (platform.startsWith("linux")) assert.deepEqual(manifest.libc, [platform.endsWith("gnu") ? "glibc" : "musl"]);
  }
  const archive = path.join(directory, "package-artifacts/npm/fixture-tui-test-1.0.0.tgz");
  const manifest = JSON.parse(execFileSync("tar", ["-xOf", archive, "package/package.json"], { encoding: "utf8" }));
  assert.equal(manifest.dependencies.koffi, "^3.3.3");
  assert.deepEqual(manifest.optionalDependencies, Object.fromEntries(targets.map((platform) => [`@fixture/tui-test-${platform}`, "1.0.0"])));
});

test("packaging rejects a missing shared library artifact", (t) => {
  const directory = fixture(t);
  fs.unlinkSync(path.join(directory, "bindings/js/artifacts", library("darwin-arm64")));
  assert.throws(
    () => execFileSync(process.execPath, [script], { cwd: directory, stdio: "pipe" }),
    /Missing shared library/,
  );
});
