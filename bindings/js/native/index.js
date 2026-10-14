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

const path = libraryPath();
const library = koffi.load(path);
let platformLibrary;
if (process.platform === "win32") {
  // Rust thread-local destructors can outlive the JavaScript environment.
  platformLibrary = koffi.load("kernel32.dll");
  const pin = platformLibrary.func("int32_t __stdcall GetModuleHandleExW(uint32_t flags, void *address, _Out_ void **module)");
  const GET_MODULE_HANDLE_EX_FLAG_PIN = 1;
  const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS = 4;
  if (!pin(
    GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    library.symbol("tui_abi_version"),
    [null],
  )) {
    throw new Error("Failed to retain the tui-test native library for process lifetime");
  }
} else {
  platformLibrary = koffi.load(null);
  const open = platformLibrary.func("void *dlopen(const char *path, int flags)");
  const close = platformLibrary.func("int dlclose(void *handle)");
  const lastError = platformLibrary.func("const char *dlerror()");
  const RTLD_NOW = 2;
  const RTLD_NODELETE = process.platform === "darwin" ? 0x80 : 0x1000;
  // Keep code mapped after the last environment exits, including detached tasks.
  const retained = open(path, RTLD_NOW | RTLD_NODELETE);
  if (retained === null) {
    throw new Error(`Failed to retain the tui-test native library: ${lastError()}`);
  }
  if (close(retained) !== 0) {
    throw new Error(`Failed to release the temporary native library handle: ${lastError()}`);
  }
}
// Keep the library handle alive with its functions. Bun cannot safely finalize
// Koffi's handle during garbage collection, even while native functions remain.
const functions = { ...bind(library), library, platformLibrary };
const version = functions.tui_abi_version();
if (version !== abiVersion) {
  throw new Error(`tui-test native ABI mismatch: expected ${abiVersion}, got ${version}`);
}
export const { NativeSession, sessions, recording, closeAll, closeAllSync } = createAdapter(functions);
