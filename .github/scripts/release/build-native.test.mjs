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
  if (command !== "cargo" && command !== "docker") throw new Error("Unexpected command: " + command);
  fs.writeFileSync(path.join(options.cwd, "invocation.json"), JSON.stringify({
    command, args, rustflags: options.env.RUSTFLAGS, targetDir: options.env.CARGO_TARGET_DIR,
  }));
  const index = args.indexOf("--target");
  const target = command === "docker" ? args.at(-2) : index === -1 ? "" : args[index + 1];
  const profile = command === "docker" ? args.at(-1) : args.includes("--release") ? "release" : "debug";
  const output = path.join(options.cwd, "target", target, profile);
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

test("GNU builds use manylinux 2.28 and the original output target", (t) => {
  const { invocation, files } = build(t, ["--target", "aarch64-unknown-linux-gnu", "-x"]);
  assert.equal(invocation.command, "docker");
  assert.ok(invocation.args.includes("quay.io/pypa/manylinux_2_28_aarch64"));
  assert.ok(invocation.args.includes("linux/arm64"));
  assert.deepEqual(invocation.args.slice(-2), ["aarch64-unknown-linux-gnu", "release"]);
  const script = invocation.args[invocation.args.indexOf("-euc") + 1];
  assert.match(script, /--target "\$1\.2\.28"/);
  assert.match(script, /CARGO_TARGET_DIR=\/io\/target\/manylinux/);
  assert.deepEqual(files, ["tui-test.linux-arm64-gnu.so"]);
  assert.equal(invocation.targetDir, undefined);
  assert.equal(invocation.rustflags, "-C debuginfo=1");
  assert.ok(invocation.args.includes("RUSTFLAGS"));
});

test("implicit debug GNU builds also use the baseline container", (t) => {
  const { invocation, files } = build(t, ["--zig", "--debug"]);
  assert.equal(invocation.command, "docker");
  assert.ok(invocation.args.includes("quay.io/pypa/manylinux_2_28_x86_64"));
  assert.ok(invocation.args.includes("linux/amd64"));
  assert.deepEqual(invocation.args.slice(-2), ["x86_64-unknown-linux-gnu", "debug"]);
  assert.deepEqual(files, ["tui-test.linux-x64-gnu.so"]);
});

test("musl remains dynamic and retains caller Rust flags", (t) => {
  const { invocation } = build(t, ["--target", "x86_64-unknown-linux-musl", "-x"]);
  assert.equal(invocation.command, "cargo");
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
