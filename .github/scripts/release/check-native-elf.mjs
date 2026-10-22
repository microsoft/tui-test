import { execFileSync } from "node:child_process";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

export function checkVersions(output) {
  const versions = [...output.matchAll(/\bGLIBC_(\d+(?:\.\d+)+)\b/g)].map(match => match[1]);
  if (!versions.length || output.includes("GLIBC_PRIVATE")) throw new Error("Missing or unsupported glibc symbol versions");
  const newer = [...new Set(versions.filter(version => {
    const [major, minor, ...rest] = version.split(".").map(Number);
    return major > 2 || (major === 2 && (minor > 28 || (minor === 28 && rest.length > 0)));
  }))].sort();
  if (newer.length) throw new Error(`Library requires glibc newer than 2.28: ${newer.join(", ")}`);
}

export function checkDependencies(output, target) {
  const arch = target.startsWith("aarch64-") ? "aarch64" : "x86_64";
  const allowed = new Set(target.endsWith("-gnu") ? [
    "libc.so.6", "libdl.so.2", "libpthread.so.0", "libm.so.6", "librt.so.1", "libutil.so.1",
    "libgcc_s.so.1", "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1",
  ] : ["libc.so", `libc.musl-${arch}.so.1`]);
  const needed = [...output.matchAll(/\(NEEDED\).*?\[([^\]]+)\]/g)].map(match => match[1]);
  if (!needed.length || needed.some(library => !allowed.has(library))) {
    throw new Error(`Unexpected native dependencies for ${target}: ${[...new Set(needed)].sort().join(", ")}`);
  }
  if (/\((?:RPATH|RUNPATH)\)/.test(output)) throw new Error("Native release library must not contain RPATH or RUNPATH");
}

export function checkSymbols(output) {
  for (const line of output.split("\n")) {
    const fields = line.trim().split(/\s+/);
    if (fields.length >= 8 && fields[4] === "GLOBAL" && fields[6] === "UND" && !fields[7].includes("@")) {
      throw new Error(`Unversioned required symbol bypasses the glibc baseline: ${fields[7]}`);
    }
  }
}

function main() {
  const { values: args } = parseArgs({ options: { library: { type: "string" }, target: { type: "string" } } });
  if (!args.library || !/^(x86_64|aarch64)-unknown-linux-(gnu|musl)$/.test(args.target ?? "")) {
    throw new Error("Usage: check-native-elf.mjs --library PATH --target <Linux target>");
  }
  const readelf = (...options) => execFileSync("readelf", [...options, args.library], { encoding: "utf8" });
  checkDependencies(readelf("--dynamic"), args.target);
  if (args.target.endsWith("-gnu")) {
    checkVersions(readelf("--version-info"));
    checkSymbols(readelf("--dyn-syms", "--wide"));
  }
  console.log(`Native dependencies and ABI checks passed: ${args.target}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) main();
