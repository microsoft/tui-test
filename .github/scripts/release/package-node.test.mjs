import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const script = fileURLToPath(new URL("./package-node.mjs", import.meta.url));
const targets = ["darwin-arm64", "linux-x64-gnu", "win32-x64-msvc"];

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
  }));
  fs.writeFileSync(path.join(bindings, "index.js"), "export {};\n");
  for (const platform of targets) {
    const pkg = path.join(bindings, "npm", platform);
    fs.mkdirSync(pkg, { recursive: true });
    fs.writeFileSync(path.join(pkg, "package.json"), JSON.stringify({
      name: `@fixture/tui-test-${platform}`,
      version: "1.0.0",
      files: platform === "linux-x64-gnu" ? undefined : ["*.node"],
    }));
    fs.writeFileSync(path.join(pkg, `tui-test.${platform}.node`), "addon fixture");
  }
  return directory;
}

test("platform packages need only the self-contained addon", (t) => {
  const directory = fixture(t);
  const packagePath = path.join(directory, "bindings/js/package.json");
  const original = fs.readFileSync(packagePath, "utf8");
  execFileSync(process.execPath, [script], { cwd: directory, stdio: "pipe" });
  assert.equal(fs.readFileSync(packagePath, "utf8"), original);
  for (const platform of targets) {
    const archive = path.join(directory, "package-artifacts/npm", `fixture-tui-test-${platform}-1.0.0.tgz`);
    const entries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" }).split("\n");
    assert.ok(entries.includes(`package/tui-test.${platform}.node`));
    assert.ok(!entries.some((entry) => /\.(so|dylib|dll)$/.test(entry)));
  }
});

test("packaging rejects a platform package without its addon", (t) => {
  const directory = fixture(t);
  fs.unlinkSync(path.join(directory, "bindings/js/npm/darwin-arm64/tui-test.darwin-arm64.node"));
  assert.throws(
    () => execFileSync(process.execPath, [script], { cwd: directory, stdio: "pipe" }),
    /Missing native addon/,
  );
});
