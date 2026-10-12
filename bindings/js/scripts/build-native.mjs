import { execFileSync } from "node:child_process";
import { copyFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { patchLoader } from "./patch-loader.mjs";

const directory = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(directory, "../..");
const args = process.argv.slice(2);
const debug = args.includes("--debug");
const napiArgs = args.filter((arg) => arg !== "--debug");
const targetIndex = args.indexOf("--target");
const explicitTarget = targetIndex >= 0
  ? args[targetIndex + 1]
  : args.find((arg) => arg.startsWith("--target="))?.slice("--target=".length);
const target = explicitTarget ?? execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .match(/^host: (.+)$/m)?.[1];
const tags = {
  "x86_64-pc-windows-msvc": "win32-x64-msvc",
  "x86_64-apple-darwin": "darwin-x64",
  "aarch64-apple-darwin": "darwin-arm64",
  "x86_64-unknown-linux-gnu": "linux-x64-gnu",
  "aarch64-unknown-linux-gnu": "linux-arm64-gnu",
  "x86_64-unknown-linux-musl": "linux-x64-musl",
  "aarch64-unknown-linux-musl": "linux-arm64-musl",
};
const tag = tags[target];
if (!tag) throw new Error(`Unsupported native target: ${target}`);
const windows = target.includes("windows");
const extension = windows ? "dll" : target.includes("darwin") ? "dylib" : "so";
const stem = `${windows ? "" : "lib"}tui_test_ffi`;
const env = { ...process.env };
delete env.CARGO_TARGET_DIR;
if (target.endsWith("-musl")) {
  env.RUSTFLAGS = `${env.RUSTFLAGS ?? ""} -C target-feature=-crt-static`.trim();
}
const run = (command, commandArgs, cwd = directory) =>
  execFileSync(command, commandArgs, { cwd, env, stdio: "inherit" });

run("cargo", [
  args.includes("-x") || args.includes("--cross-compile") ? "zigbuild" : "build",
  "-p", "tui-test-ffi",
  ...(debug ? [] : ["--release"]),
  ...(explicitTarget ? ["--target", explicitTarget] : []),
], root);
const profile = debug ? "debug" : "release";
copyFileSync(
  resolve(root, "target", explicitTarget ?? "", profile, `${stem}.${extension}`),
  resolve(directory, "native", `${stem}.${tag}.${extension}`),
);
run(process.execPath, [
  resolve(directory, "node_modules/@napi-rs/cli/dist/cli.js"),
  "build", "--platform", "--esm",
  ...(debug ? [] : ["--release", "--strip"]),
  "--output-dir", "native", "--js", "index.js", "--dts", "index.d.ts",
  ...napiArgs,
]);
patchLoader(resolve(directory, "native/index.js"));
