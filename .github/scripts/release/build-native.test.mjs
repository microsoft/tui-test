import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("../../../", import.meta.url));

function build(t, args, host = "x86_64-unknown-linux-gnu") {
  fs.mkdirSync(path.join(root, "target"), { recursive: true });
  const directory = fs.mkdtempSync(path.join(root, "target/release-build-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const bindings = path.join(directory, "bindings/js");
  fs.mkdirSync(path.join(bindings, "scripts"), { recursive: true });
  for (const name of ["package.json", "scripts/build-native.mjs"]) {
    fs.copyFileSync(path.join(root, "bindings/js", name), path.join(bindings, name));
  }
  const mock = path.join(directory, "mock.mjs");
  fs.writeFileSync(mock, `
import childProcess from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { syncBuiltinESMExports } from "node:module";
childProcess.execFileSync = (command, args, options) => {
  if (command === "rustc") return "host: ${host}\\n";
  if (command !== "cargo") throw new Error("Unexpected command: " + command);
  fs.writeFileSync(path.join(options.cwd, "invocation.json"), JSON.stringify({
    args, rustflags: options.env.RUSTFLAGS, targetDir: options.env.CARGO_TARGET_DIR,
  }));
  const index = args.indexOf("--target");
  const target = index === -1 ? "" : args[index + 1].replace(/\\.2\\.28$/, "");
  const output = path.join(options.cwd, "target", target, args.includes("--release") ? "release" : "debug");
  fs.mkdirSync(output, { recursive: true });
  const library = target.includes("windows") ? "tui_test_ffi.dll"
    : target.includes("apple") ? "libtui_test_ffi.dylib" : "libtui_test_ffi.so";
  fs.writeFileSync(path.join(output, library), "native build fixture");
};
syncBuiltinESMExports();
`);
  execFileSync(process.execPath, ["--import", mock, path.join(bindings, "scripts/build-native.mjs"), ...args], {
    cwd: directory,
    env: { ...process.env, RUSTFLAGS: "-C debuginfo=1", CARGO_TARGET_DIR: "caller-target" },
    stdio: "pipe",
  });
  return {
    invocation: JSON.parse(fs.readFileSync(path.join(directory, "invocation.json"), "utf8")),
    files: fs.readdirSync(path.join(bindings, "native")),
  };
}

test("GNU zigbuild uses glibc 2.28 and the original output target", (t) => {
  const { invocation, files } = build(t, ["--target", "aarch64-unknown-linux-gnu", "-x"]);
  assert.deepEqual(invocation.args, [
    "zigbuild", "--locked", "-p", "tui-test-ffi", "--release",
    "--target", "aarch64-unknown-linux-gnu.2.28",
  ]);
  assert.deepEqual(files, ["tui-test.linux-arm64-gnu.so"]);
  assert.equal(invocation.targetDir, undefined);
});

test("implicit GNU zigbuild also selects the baseline target", (t) => {
  const { invocation, files } = build(t, ["--zig"]);
  assert.equal(invocation.args.at(-1), "x86_64-unknown-linux-gnu.2.28");
  assert.deepEqual(files, ["tui-test.linux-x64-gnu.so"]);
});

test("musl remains dynamic and retains caller Rust flags", (t) => {
  const { invocation } = build(t, ["--target", "x86_64-unknown-linux-musl", "-x"]);
  assert.equal(invocation.args.at(-1), "x86_64-unknown-linux-musl");
  assert.equal(invocation.rustflags, "-C debuginfo=1 -C target-feature=-crt-static");
});

test("ordinary native and explicit target builds stay unchanged", (t) => {
  const native = build(t, ["--debug"]);
  assert.deepEqual(native.invocation.args, ["build", "--locked", "-p", "tui-test-ffi"]);
  const explicit = build(t, ["--target", "x86_64-unknown-linux-gnu"]);
  assert.equal(explicit.invocation.args[0], "build");
  assert.equal(explicit.invocation.args.at(-1), "x86_64-unknown-linux-gnu");
  assert.equal(explicit.invocation.rustflags, "-C debuginfo=1");
});
