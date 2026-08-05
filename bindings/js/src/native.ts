import { InternalError, UsageError, makeError } from "./errors.js";
import type {
  Cell,
  Cursor,
  EffectiveTimeouts,
  ExpectTextOptions,
  MouseClickOptions,
  OpenOptions,
  OpenResult,
  PackedScreen,
  RunOptions,
  ScreenshotOptions,
  Size,
  SnapshotOptions,
  State,
  Timeouts,
  WaitTextOptions,
} from "../native/index.js";

type NativeBinding = typeof import("../native/index.js");
type NativeSessionHandle = InstanceType<NativeBinding["NativeSession"]>;
type RuntimeOpenOptions = Omit<OpenOptions, "shell"> & { shell?: string };

const ERROR_PREFIX = "__shell_use_native_error__:";
const USAGE_NAPI_CODES = new Set([
  "InvalidArg",
  "ObjectExpected",
  "StringExpected",
  "NameExpected",
  "FunctionExpected",
  "NumberExpected",
  "BooleanExpected",
  "ArrayExpected",
  "BigintExpected",
  "DateExpected",
  "ArrayBufferExpected",
  "DetachableArraybufferExpected",
]);

let bindingPromise: Promise<NativeBinding> | undefined;
let cachedBinding: NativeBinding | undefined;
let exitHookInstalled = false;

function installExitHook(): void {
  if (exitHookInstalled) {
    return;
  }
  exitHookInstalled = true;
  process.once("exit", () => {
    try {
      cachedBinding?.closeAllSync();
    } catch {}
  });
}

async function importBinding(): Promise<NativeBinding> {
  try {
    const binding = await import("../native/index.js");
    cachedBinding = binding;
    installExitHook();
    return binding;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      "failed to load the @microsoft/shell-use native addon: " +
        `${message}. Build it with \`npm run build:native\` (requires a Rust ` +
        "toolchain), or install a matching prebuilt platform package.",
      { cause: error },
    );
  }
}

async function loadBinding(): Promise<NativeBinding> {
  if (!bindingPromise) {
    bindingPromise = importBinding();
  }
  try {
    return await bindingPromise;
  } catch (error) {
    bindingPromise = undefined;
    throw error;
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function mapNativeError(error: unknown): Error {
  const message = errorMessage(error);
  const encodedAt = message.indexOf(ERROR_PREFIX);
  if (encodedAt >= 0) {
    const encoded = message.slice(encodedAt + ERROR_PREFIX.length);
    const newline = encoded.indexOf("\n");
    if (newline >= 0) {
      const mapped = makeError(encoded.slice(0, newline), encoded.slice(newline + 1));
      Object.defineProperty(mapped, "cause", {
        configurable: true,
        value: error,
      });
      return mapped;
    }
  }
  const code =
    typeof error === "object" && error !== null && "code" in error
      ? String(error.code)
      : undefined;
  const isTypeError =
    error instanceof TypeError ||
    (typeof error === "object" &&
      error !== null &&
      "name" in error &&
      error.name === "TypeError");
  return isTypeError || (code !== undefined && USAGE_NAPI_CODES.has(code))
    ? new UsageError(message)
    : new InternalError(message);
}

async function invoke<T>(action: () => Promise<T>): Promise<T> {
  try {
    return await action();
  } catch (error) {
    throw mapNativeError(error);
  }
}

async function createSession(name: string): Promise<NativeSessionHandle> {
  const binding = await loadBinding();
  return new binding.NativeSession(name);
}

export class NativeRuntime {
  #session: Promise<NativeSessionHandle>;

  constructor(name: string) {
    this.#session = createSession(name);
  }

  async #call<T>(action: (session: NativeSessionHandle) => Promise<T>): Promise<T> {
    const session = await this.#session;
    return invoke(() => action(session));
  }

  open(options?: RuntimeOpenOptions): Promise<OpenResult> {
    return this.#call((session) => session.open(options as OpenOptions | undefined));
  }

  run(options: RunOptions): Promise<OpenResult> {
    return this.#call((session) => session.run(options));
  }

  close(): Promise<void> {
    return this.#call((session) => session.close());
  }

  state(): Promise<State> {
    return this.#call((session) => session.state());
  }

  text(full = false): Promise<string> {
    return this.#call((session) => session.text(full));
  }

  /**
   * Private packed snapshot. The detached Uint8Array is read-only by contract
   * and contains newline-delimited full logical rows, including trailing spaces
   * and blank rows. UTF-8 byte offsets are not terminal cell offsets.
   */
  async packedScreen(full = false): Promise<PackedScreen> {
    const screen = await this.#call((session) => session.packedScreen(full));
    return Object.freeze(screen);
  }

  cells(x: number, y: number, w = 1, h = 1): Promise<Cell[]> {
    return this.#call((session) => session.cells(x, y, w, h));
  }

  getCommand(): Promise<string | null> {
    return this.#call((session) => session.getCommand());
  }

  getOutput(): Promise<string | null> {
    return this.#call((session) => session.getOutput());
  }

  getExitCode(): Promise<number | null> {
    return this.#call((session) => session.getExitCode());
  }

  getCwd(): Promise<string | null> {
    return this.#call((session) => session.getCwd());
  }

  getCursor(): Promise<Cursor> {
    return this.#call((session) => session.getCursor());
  }

  getSize(): Promise<Size> {
    return this.#call((session) => session.getSize());
  }

  getBellCount(): Promise<number> {
    return this.#call((session) => session.getBellCount());
  }

  write(data: string): Promise<void> {
    return this.#call((session) => session.write(data));
  }

  type(text: string): Promise<void> {
    return this.#call((session) => session.type(text));
  }

  submit(data?: string): Promise<void> {
    return this.#call((session) => session.submit(data));
  }

  press(keys: string[]): Promise<void> {
    return this.#call((session) => session.press(keys));
  }

  mouseClick(options?: MouseClickOptions): Promise<void> {
    return this.#call((session) => session.mouseClick(options));
  }

  mouseMove(x: number, y: number): Promise<void> {
    return this.#call((session) => session.mouseMove(x, y));
  }

  mouseDown(x: number, y: number, button = 0): Promise<void> {
    return this.#call((session) => session.mouseDown(x, y, button));
  }

  mouseUp(x: number, y: number, button = 0): Promise<void> {
    return this.#call((session) => session.mouseUp(x, y, button));
  }

  mouseDrag(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    button = 0,
  ): Promise<void> {
    return this.#call((session) => session.mouseDrag(x1, y1, x2, y2, button));
  }

  mouseScroll(direction: "up" | "down", amount = 3): Promise<void> {
    return this.#call((session) => session.mouseScroll(direction, amount));
  }

  resize(cols: number, rows: number): Promise<void> {
    return this.#call((session) => session.resize(cols, rows));
  }

  signal(name: string): Promise<void> {
    return this.#call((session) => session.signal(name));
  }

  waitText(text: string, options?: WaitTextOptions): Promise<void> {
    return this.#call((session) => session.waitText(text, options));
  }

  waitIdle(timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.waitIdle(timeoutMs));
  }

  waitCommand(timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.waitCommand(timeoutMs));
  }

  waitExit(timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.waitExit(timeoutMs));
  }

  waitReady(timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.waitReady(timeoutMs));
  }

  waitBell(timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.waitBell(timeoutMs));
  }

  expectText(text: string, options?: ExpectTextOptions): Promise<void> {
    return this.#call((session) => session.expectText(text, options));
  }

  expectExitCode(code: number, timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.expectExitCode(code, timeoutMs));
  }

  expectOutput(text: string, regex = false): Promise<void> {
    return this.#call((session) => session.expectOutput(text, regex));
  }

  expectBellCount(count: number, timeoutMs?: number): Promise<void> {
    return this.#call((session) => session.expectBellCount(count, timeoutMs));
  }

  async snapshot(name: string, options?: SnapshotOptions): Promise<string> {
    return this.#call((session) => session.snapshot(name, options));
  }

  screenshot(options?: ScreenshotOptions): Promise<string> {
    return this.#call((session) => session.screenshot(options));
  }

  panicProbe(): Promise<void> {
    return this.#call((session) => session.panicProbe());
  }
}

export async function sessions(): Promise<string[]> {
  const binding = await loadBinding();
  return invoke(() => binding.sessions());
}

export async function closeAll(): Promise<void> {
  const binding = await loadBinding();
  await invoke(() => binding.closeAll());
}

export async function recording(name: string): Promise<string> {
  const binding = await loadBinding();
  return invoke(() => binding.recording(name));
}

export type {
  Cell as NativeCell,
  Cursor as NativeCursor,
  EffectiveTimeouts as NativeEffectiveTimeouts,
  ExpectTextOptions as NativeExpectTextOptions,
  MouseClickOptions as NativeMouseClickOptions,
  OpenOptions as NativeOpenOptions,
  OpenResult as NativeOpenResult,
  PackedScreen as NativePackedScreen,
  RunOptions as NativeRunOptions,
  ScreenshotOptions as NativeScreenshotOptions,
  Size as NativeSize,
  SnapshotOptions as NativeSnapshotOptions,
  State as NativeState,
  Timeouts as NativeTimeouts,
  WaitTextOptions as NativeWaitTextOptions,
};
