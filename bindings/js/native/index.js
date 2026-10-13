import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import koffi from "koffi";
import { abiVersion, bind } from "./abi.js";
import { createAdapter } from "./adapter.js";

export {
  Backend, Shell, RecordingFormat, SnapshotResult, UnderlineStyle,
  LocatorNodeKind, LocatorStageDirection,
} from "./models.js";

function libraryPath() {
  if (process.env.TUI_TEST_NATIVE_LIBRARY) return process.env.TUI_TEST_NATIVE_LIBRARY;
  const platform = process.platform;
  const arch = process.arch;
  const libc = platform === "linux"
    ? (process.report.getReport().header.glibcVersionRuntime ? "gnu" : "musl")
    : undefined;
  const tag = [platform, arch, platform === "win32" ? "msvc" : libc].filter(Boolean).join("-");
  const extension = { darwin: "dylib", linux: "so", win32: "dll" }[platform];
  if (!extension) throw new Error(`unsupported native platform: ${platform}`);
  const local = fileURLToPath(new URL(`tui-test.${tag}.${extension}`, import.meta.url));
  if (existsSync(local)) return local;
  return createRequire(import.meta.url).resolve(`@microsoft/tui-test-${tag}`);
}

const library = koffi.load(libraryPath());
// Keep the library handle alive with its functions. Bun cannot safely finalize
// Koffi's handle during garbage collection, even while native functions remain.
const functions = { ...bind(library), library };
const version = functions.tui_abi_version();
if (version !== abiVersion) {
  throw new Error(`tui-test native ABI mismatch: expected ${abiVersion}, got ${version}`);
}
export const { NativeSession, sessions, recording, closeAll, closeAllSync } = createAdapter(functions);
