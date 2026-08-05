import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  DEFAULT_COLS,
  DEFAULT_ROWS,
  assertTimeoutClasses,
  envPairs,
  resolveSession,
  resolveTimeout,
  timeoutsPayload,
} from "./config.js";
import type { TimeoutClass } from "./config.js";
import { uniqueSession } from "./ephemeral.js";
import { ExpectationError } from "./errors.js";
import { NativeRuntime } from "./native.js";
import type {
  Cell,
  ClientOptions,
  Cursor,
  OpenResult,
  Shell,
  Size,
  SpawnOptions,
  State,
} from "./types.js";

export interface WaitTextOptions {
  regex?: boolean;
  full?: boolean;
  not?: boolean;
  timeout?: number;
}

export interface ExpectTextOptions {
  regex?: boolean;
  full?: boolean;
  strict?: boolean;
  not?: boolean;
  fg?: string;
  bg?: string;
  timeout?: number;
}

export interface MouseButtonOptions {
  button?: number;
}

const TERMINAL_MARKER = "Terminal content:\n";

function extractTerminalContent(message: string): string | undefined {
  const idx = message.indexOf(TERMINAL_MARKER);
  if (idx < 0) {
    return undefined;
  }
  return message.slice(idx + TERMINAL_MARKER.length).replace(/\n+$/, "") || undefined;
}

function withOperation(error: unknown, operation: string): unknown {
  if (!(error instanceof ExpectationError)) {
    return error;
  }
  const previous = error.message;
  error.message = `${operation}: ${previous}`;
  if (error.stack) {
    error.stack = error.stack.replace(
      `${error.name}: ${previous}`,
      `${error.name}: ${error.message}`,
    );
  }
  return error;
}

function optional<T>(value: T | null | undefined): T | undefined {
  return value ?? undefined;
}

class Mouse {
  #runtime: NativeRuntime;

  constructor(runtime: NativeRuntime) {
    this.#runtime = runtime;
  }

  async click(
    x: number | null = null,
    y: number | null = null,
    opts: { onText?: string; button?: number; clicks?: number } = {},
  ): Promise<void> {
    await this.#runtime.mouseClick({
      x: optional(x),
      y: optional(y),
      onText: opts.onText,
      button: opts.button ?? 0,
      clicks: opts.clicks ?? 1,
    });
  }

  async move(x: number, y: number): Promise<void> {
    await this.#runtime.mouseMove(x, y);
  }

  async down(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#runtime.mouseDown(x, y, opts.button ?? 0);
  }

  async up(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#runtime.mouseUp(x, y, opts.button ?? 0);
  }

  async drag(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    opts: MouseButtonOptions = {},
  ): Promise<void> {
    await this.#runtime.mouseDrag(x1, y1, x2, y2, opts.button ?? 0);
  }

  async scroll(direction: "up" | "down", opts: { amount?: number } = {}): Promise<void> {
    await this.#runtime.mouseScroll(direction, opts.amount ?? 3);
  }
}

export class ShellUse {
  readonly session: string;
  readonly mouse: Mouse;
  #runtime: NativeRuntime;
  #options: ClientOptions;
  #artifactCounter = 0;

  constructor(session?: string, opts: ClientOptions = {}) {
    this.session = resolveSession(session);
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    this.#options = opts;
    this.#runtime = new NativeRuntime(this.session);
    this.mouse = new Mouse(this.#runtime);
  }

  static ephemeral(prefix?: string, opts: ClientOptions = {}): ShellUse {
    return new ShellUse(uniqueSession(prefix), opts);
  }

  #timeout(cls: TimeoutClass, callTimeout?: number): number | undefined {
    return resolveTimeout(cls, callTimeout, this.#options);
  }

  async #guard<T>(operation: string, action: () => Promise<T>): Promise<T> {
    try {
      return await action();
    } catch (error) {
      const mapped = withOperation(error, operation);
      await this.#captureArtifact(mapped);
      throw mapped;
    }
  }

  async #captureArtifact(error: unknown): Promise<void> {
    const artifacts = this.#options.artifacts;
    if (!artifacts || !(error instanceof ExpectationError)) {
      return;
    }
    const mode = artifacts.onFailure ?? "svg";
    if (mode === "none") {
      return;
    }
    try {
      const terminal = error.terminal ?? {};
      const text = extractTerminalContent(error.message);
      if (text !== undefined) {
        terminal.text = text;
      }
      if (mode === "svg") {
        await mkdir(artifacts.dir, { recursive: true });
        const file = path.join(
          artifacts.dir,
          `${this.session}-${Date.now()}-${this.#artifactCounter++}.svg`,
        );
        terminal.screenshot = await this.screenshot(file);
      }
      if (terminal.text !== undefined || terminal.screenshot !== undefined) {
        error.terminal = terminal;
      }
    } catch {}
  }

  async #spawn(action: () => Promise<OpenResult>, retries: number): Promise<OpenResult> {
    let lastError: unknown;
    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        return await action();
      } catch (error) {
        lastError = error;
        if (attempt < retries) {
          await this.closeQuiet();
        }
      }
    }
    throw lastError;
  }

  async open(opts: SpawnOptions & { shell?: Shell } = {}): Promise<OpenResult> {
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    const options = {
      shell: opts.shell,
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd,
      env: envPairs(opts.env),
      waitReady: opts.waitReady,
      timeouts: timeoutsPayload(opts.timeouts),
    };
    return this.#spawn(() => this.#runtime.open(options), opts.retries ?? 0);
  }

  async run(program: string, args: string[] = [], opts: SpawnOptions = {}): Promise<OpenResult> {
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    const options = {
      program,
      args,
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd,
      env: envPairs(opts.env),
      waitReady: opts.waitReady,
      timeouts: timeoutsPayload(opts.timeouts),
    };
    return this.#spawn(() => this.#runtime.run(options), opts.retries ?? 0);
  }

  async close(): Promise<void> {
    await this.#runtime.close();
  }

  async closeQuiet(): Promise<void> {
    try {
      await this.close();
    } catch {}
  }

  async type(text: string): Promise<void> {
    await this.#runtime.type(text);
  }

  async write(data: string): Promise<void> {
    await this.#runtime.write(data);
  }

  async submit(text: string | null = null): Promise<void> {
    await this.#runtime.submit(optional(text));
  }

  async press(...keys: string[]): Promise<void> {
    await this.#runtime.press(keys);
  }

  async keys(combo: string): Promise<void> {
    await this.#runtime.press([combo]);
  }

  async resize(cols: number, rows: number): Promise<void> {
    await this.#runtime.resize(cols, rows);
  }

  async signal(name: string): Promise<void> {
    await this.#runtime.signal(name);
  }

  async kill(): Promise<void> {
    await this.#runtime.signal("KILL");
  }

  async state(): Promise<State> {
    return this.#runtime.state();
  }

  async text(opts: { full?: boolean } = {}): Promise<string> {
    return this.#runtime.text(opts.full ?? false);
  }

  async cells(x: number, y: number, w = 1, h = 1): Promise<Cell[]> {
    return this.#runtime.cells(x, y, w, h);
  }

  async getCommand(): Promise<string | null> {
    return this.#runtime.getCommand();
  }

  async getOutput(): Promise<string | null> {
    return this.#runtime.getOutput();
  }

  async getExitCode(): Promise<number | null> {
    return this.#runtime.getExitCode();
  }

  async getCwd(): Promise<string | null> {
    return this.#runtime.getCwd();
  }

  async getCursor(): Promise<Cursor> {
    return this.#runtime.getCursor();
  }

  async getSize(): Promise<Size> {
    return this.#runtime.getSize();
  }

  async getBellCount(): Promise<number> {
    return this.#runtime.getBellCount();
  }

  async screenshot(path: string | null = null, opts: { full?: boolean } = {}): Promise<string> {
    return this.#runtime.screenshot({
      full: opts.full ?? false,
      path: optional(path),
    });
  }

  async waitText(text: string, opts: WaitTextOptions = {}): Promise<void> {
    await this.#guard("waitText", () =>
      this.#runtime.waitText(text, {
        regex: opts.regex ?? false,
        full: opts.full ?? false,
        not: opts.not ?? false,
        timeoutMs: this.#timeout("text", opts.timeout),
      }),
    );
  }

  async waitIdle(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitIdle", () =>
      this.#runtime.waitIdle(this.#timeout("idle", opts.timeout)),
    );
  }

  async waitCommand(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitCommand", () =>
      this.#runtime.waitCommand(this.#timeout("command", opts.timeout)),
    );
  }

  async waitExit(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitExit", () =>
      this.#runtime.waitExit(this.#timeout("exit", opts.timeout)),
    );
  }

  async waitReady(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitReady", () =>
      this.#runtime.waitReady(this.#timeout("ready", opts.timeout)),
    );
  }

  async waitBell(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitBell", () =>
      this.#runtime.waitBell(this.#timeout("text", opts.timeout)),
    );
  }

  async expectText(text: string, opts: ExpectTextOptions = {}): Promise<void> {
    await this.#guard("expectText", () =>
      this.#runtime.expectText(text, {
        regex: opts.regex ?? false,
        full: opts.full ?? false,
        strict: opts.strict ?? true,
        not: opts.not ?? false,
        fg: opts.fg,
        bg: opts.bg,
        timeoutMs: this.#timeout("text", opts.timeout),
      }),
    );
  }

  async expectExitCode(code: number, opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("expectExitCode", () =>
      this.#runtime.expectExitCode(code, this.#timeout("command", opts.timeout)),
    );
  }

  async expectOutput(text: string, opts: { regex?: boolean } = {}): Promise<void> {
    await this.#guard("expectOutput", () =>
      this.#runtime.expectOutput(text, opts.regex ?? false),
    );
  }

  async expectBellCount(count: number, opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("expectBellCount", () =>
      this.#runtime.expectBellCount(count, this.#timeout("text", opts.timeout)),
    );
  }

  async expectSnapshot(
    name: string,
    opts: { update?: boolean; includeColors?: boolean } = {},
  ): Promise<string> {
    return this.#guard("expectSnapshot", () =>
      this.#runtime.snapshot(name, {
        update: opts.update ?? false,
        includeColors: opts.includeColors ?? false,
        cwd: process.cwd(),
      }),
    );
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.closeQuiet();
  }
}
