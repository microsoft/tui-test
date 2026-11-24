import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

// Zig 0.16.0's std/debug/SelfInfo/Windows.zig omits @alignCast before @ptrCast
// when resolving ntdll procedures in static MSVC libraries, which fails to
// compile for aarch64-windows-msvc where function pointers are 4-byte aligned.
export const UNPATCHED = "@ptrCast(try si.loadNtdllProc(@tagName(proc)))";
export const PATCHED = "@ptrCast(@alignCast(try si.loadNtdllProc(@tagName(proc))))";

export function parseZigEnvField(output, field) {
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = output.match(new RegExp(`(?:\\.${escaped}\\s*=|"${escaped}"\\s*:)\\s*("(?:\\\\.|[^"\\\\])*")`));
  if (!match) throw new Error(`Missing ${field} in zig env output`);
  return JSON.parse(match[1]);
}

export function patchWindowsSelfInfo(filename, version) {
  if (version !== "0.16.0") return "not-needed";
  const source = fs.readFileSync(filename, "utf8");
  if (source.includes(PATCHED)) return "already-patched";
  const parts = source.split(UNPATCHED);
  if (parts.length === 2) {
    fs.writeFileSync(filename, parts.join(PATCHED));
    return "patched";
  }
  throw new Error(`Expected one unpatched loadNtdllProc @ptrCast in ${filename}, found ${parts.length - 1}`);
}

function main() {
  const { values: args } = parseArgs({ options: { zig: { type: "string", default: "zig" } } });
  const envOutput = execFileSync(args.zig, ["env"], { encoding: "utf8" });
  const stdDir = parseZigEnvField(envOutput, "std_dir");
  const version = parseZigEnvField(envOutput, "version");
  const filename = path.join(stdDir, "debug", "SelfInfo", "Windows.zig");
  const status = patchWindowsSelfInfo(filename, version);
  console.log(`Zig ${version} Windows ARM64 SelfInfo check: ${status} (${filename})`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) main();
