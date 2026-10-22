import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const directory = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(directory, "../..");
const args = process.argv.slice(2);
let debug = false;
let zigbuild = false;
let explicitTarget;
for (let i = 0; i < args.length; i++) {
  const arg = args[i];
  if (arg === "--debug") debug = true;
  else if (arg === "--zig" || arg === "-x") zigbuild = true;
  else if (arg === "--target" || arg === "-t") {
    explicitTarget = args[++i];
    if (!explicitTarget || explicitTarget.startsWith("-")) throw new Error("Missing --target value");
  } else if (arg.startsWith("--target=") && arg.length > "--target=".length) {
    explicitTarget = arg.slice("--target=".length);
  } else {
    throw new Error(`Unknown build option: ${arg}`);
  }
}
const target = explicitTarget ?? execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .match(/^host: (.+)$/m)?.[1];
const tags = JSON.parse(readFileSync(join(directory, "package.json"), "utf8")).nativeTargets;
const tag = tags[target];
if (!tag) throw new Error(`Unsupported native target: ${target}`);
const env = { ...process.env };
delete env.CARGO_TARGET_DIR;
if (target.endsWith("-musl")) {
  env.RUSTFLAGS = `${env.RUSTFLAGS ?? ""} -C target-feature=-crt-static`.trim();
}
if (zigbuild && target.endsWith("-gnu")) {
  const arch = target.startsWith("aarch64") ? "aarch64" : "x86_64";
  const tools = join(root, "target/manylinux-tools", arch);
  mkdirSync(tools, { recursive: true });
  // Ghostty's native Zig build detects the host libc, independently of cargo-zigbuild.
  execFileSync("docker", [
    "run", "--rm", "--platform", arch === "aarch64" ? "linux/arm64" : "linux/amd64",
    "--volume", `${root}:/io`, "--volume", `${tools}:/root`,
    "--workdir", "/io", "--env", "RUSTFLAGS",
    `quay.io/pypa/manylinux_2_28_${arch}`,
    "bash", "-euc", `
export CARGO_HOME="$HOME/.cargo" RUSTUP_HOME="$HOME/.rustup"
export PATH="$CARGO_HOME/bin:$HOME/.local/bin:$PATH"
bash .github/scripts/release/prepare-linux.sh
cargo install --locked --version 0.23.4 cargo-zigbuild
profile=()
if [[ "$2" == release ]]; then profile=(--release); fi
CARGO_TARGET_DIR=/io/target/manylinux cargo zigbuild --locked -p tui-test-ffi --target "$1.2.28" "\${profile[@]}"
mkdir -p "target/$1/$2"
cp "target/manylinux/$1/$2/libtui_test_ffi.so" "target/$1/$2/"
`, "--", target, debug ? "debug" : "release",
  ], { cwd: root, env, stdio: "inherit" });
} else {
  execFileSync("cargo", [
    zigbuild ? "zigbuild" : "build", "--locked", "-p", "tui-test-ffi",
    ...(debug ? [] : ["--release"]),
    ...(explicitTarget || zigbuild ? ["--target", target] : []),
  ], { cwd: root, env, stdio: "inherit" });
}

const extension = target.includes("windows") ? "dll" : target.includes("apple") ? "dylib" : "so";
const library = extension === "dll" ? "tui_test_ffi.dll" : `libtui_test_ffi.${extension}`;
const output = join(root, "target", ...(explicitTarget || zigbuild ? [target] : []), debug ? "debug" : "release", library);
mkdirSync(join(directory, "native"), { recursive: true });
copyFileSync(output, join(directory, "native", `tui-test.${tag}.${extension}`));
