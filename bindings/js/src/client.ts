import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  DEFAULT_COLS,
  DEFAULT_ROWS,
  assertTimeoutClasses,
  resolveBinary,
  resolveHome,
  resolveSession,
  resolveTimeout,
  timeoutsPayload,
} from "./config.js";
import type { TimeoutClass } from "./config.js";
import { createTempHome, removeTempHome, uniqueSession } from "./ephemeral.js";
import { DaemonError, ExpectationError, NoSessionError } from "./errors.js";
import { envPairs, unwrap } from "./protocol.js";
import * as transport from "./transport.js";
import { checkVersion } from "./version.js";
import type {
  Cell,
  ClientOptions,
  OpenResult,
  Shell,
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

/** Pulls boxed terminal content from assertion messages, dropping trailing newlines like the Python binding. */
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

class Mouse {
  #client: ShellUse;

  constructor(client: ShellUse) {
    this.#client = client;
  }

  async click(
    x: number | null = null,
    y: number | null = null,
    opts: { onText?: string; button?: number; clicks?: number } = {},
  ): Promise<void> {
    await this.#client.send({
      kind: "mouse",
      action: {
        op: "click",
        x,
        y,
        on_text: opts.onText ?? null,
        button: opts.button ?? 0,
        clicks: opts.clicks ?? 1,
      },
    });
  }

  async move(x: number, y: number): Promise<void> {
    await this.#client.send({ kind: "mouse", action: { op: "move", x, y } });
  }

  async down(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#client.send({
      kind: "mouse",
      action: { op: "down", x, y, button: opts.button ?? 0 },
    });
  }

  async up(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#client.send({
      kind: "mouse",
      action: { op: "up", x, y, button: opts.button ?? 0 },
    });
  }

  async drag(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    opts: MouseButtonOptions = {},
  ): Promise<void> {
    await this.#client.send({
      kind: "mouse",
      action: { op: "drag", x1, y1, x2, y2, button: opts.button ?? 0 },
    });
  }

  async scroll(direction: "up" | "down", opts: { amount?: number } = {}): Promise<void> {
    await this.#client.send({
      kind: "mouse",
      action: { op: "scroll", direction, amount: opts.amount ?? 3 },
    });
  }
}

export class ShellUse {
  readonly session: string;
  readonly mouse: Mouse;
  #binary: string;
  #home?: string;
  #isolated: boolean;
  #tempHomePath?: string;
  #options: ClientOptions;
  #versionChecked = false;
  #closed = false;
  #artifactCounter = 0;

  constructor(session?: string, opts: ClientOptions = {}) {
    this.session = resolveSession(session);
    this.#binary = resolveBinary(opts.binary);
    this.#isolated = opts.isolated ?? false;
    if (!this.#isolated) {
      this.#home = resolveHome(opts.home);
    }
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    this.#options = opts;
    this.mouse = new Mouse(this);
  }

  static ephemeral(prefix?: string, opts: ClientOptions = {}): ShellUse {
    return new ShellUse(uniqueSession(prefix), { ...opts, isolated: true });
  }

  async send(payload: unknown): Promise<unknown> {
    const home = await this.#resolveHome();
    await this.#checkVersion(home);
    const resp = await transport.request(this.session, home, this.#binary, payload);
    return unwrap(resp);
  }

  async #resolveHome(): Promise<string | undefined> {
    if (!this.#isolated) {
      return this.#home;
    }
    if (!this.#tempHomePath) {
      this.#tempHomePath = await createTempHome();
    }
    return this.#tempHomePath;
  }

  #currentHome(): string | undefined {
    return this.#isolated ? this.#tempHomePath : this.#home;
  }

  async #cleanupTempHome(): Promise<void> {
    const dir = this.#tempHomePath;
    if (!dir) {
      return;
    }
    this.#tempHomePath = undefined;
    try {
      await removeTempHome(dir);
    } catch {
      /* best effort; the exit sweeper retries */
    }
  }

  #timeout(cls: TimeoutClass, callTimeout?: number): number | undefined {
    return resolveTimeout(cls, callTimeout, this.#options);
  }

  #withTimeout(
    payload: Record<string, unknown>,
    cls: TimeoutClass,
    callTimeout?: number,
  ): Record<string, unknown> {
    const timeout = this.#timeout(cls, callTimeout);
    if (timeout !== undefined) {
      payload.timeout_ms = timeout;
    }
    return payload;
  }

  async #checkVersion(home: string | undefined): Promise<void> {
    if (this.#versionChecked) {
      return;
    }
    const resp = await transport.request(this.session, home, this.#binary, {
      kind: "status",
    });
    const data = unwrap(resp) as { version?: string } | undefined;
    checkVersion(data?.version);
    this.#versionChecked = true;
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
    } catch {
      /* best effort; never mask the original error */
    }
  }

  async #spawn(payload: Record<string, unknown>, retries: number): Promise<OpenResult> {
    let lastError: unknown;
    for (let attempt = 0; attempt <= retries; attempt++) {
      this.#closed = false;
      try {
        return (await this.send(payload)) as OpenResult;
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
    const payload: Record<string, unknown> = {
      kind: "open",
      shell: opts.shell ?? null,
      program: null,
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd ?? null,
      env: envPairs(opts.env),
    };
    if (opts.waitReady !== undefined) {
      payload.wait_ready = opts.waitReady;
    }
    const timeouts = timeoutsPayload(opts.timeouts);
    if (timeouts !== undefined) {
      payload.timeouts = timeouts;
    }
    return this.#spawn(payload, opts.retries ?? 0);
  }

  async run(program: string, args: string[] = [], opts: SpawnOptions = {}): Promise<OpenResult> {
    const payload: Record<string, unknown> = {
      kind: "open",
      shell: null,
      program: [program, ...args],
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd ?? null,
      env: envPairs(opts.env),
    };
    if (opts.waitReady !== undefined) {
      payload.wait_ready = opts.waitReady;
    }
    const timeouts = timeoutsPayload(opts.timeouts);
    if (timeouts !== undefined) {
      payload.timeouts = timeouts;
    }
    return this.#spawn(payload, opts.retries ?? 0);
  }

  async close(): Promise<void> {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    try {
      if (!this.#isolated || this.#tempHomePath) {
        const home = this.#currentHome();
        if (await transport.canConnect(this.session, home)) {
          const resp = await transport.request(
            this.session,
            home,
            this.#binary,
            { kind: "close" },
            false,
          );
          unwrap(resp);
        }
      }
    } catch (error) {
      if (!(error instanceof DaemonError) && !(error instanceof NoSessionError)) {
        throw error;
      }
    } finally {
      await this.#cleanupTempHome();
    }
  }

  async closeQuiet(): Promise<void> {
    try {
      await this.close();
    } catch {
      /* swallow everything; safe for finally blocks */
    }
  }

  async type(text: string): Promise<void> {
    await this.send({ kind: "write", data: text });
  }

  async write(data: string): Promise<void> {
    await this.send({ kind: "write", data });
  }

  async submit(text: string | null = null): Promise<void> {
    await this.send({ kind: "submit", data: text });
  }

  async press(...keys: string[]): Promise<void> {
    await this.send({ kind: "press", keys });
  }

  async keys(combo: string): Promise<void> {
    await this.send({ kind: "press", keys: [combo] });
  }

  async resize(cols: number, rows: number): Promise<void> {
    await this.send({ kind: "resize", cols, rows });
  }

  async signal(name: string): Promise<void> {
    await this.send({ kind: "signal", name });
  }

  async kill(): Promise<void> {
    await this.send({ kind: "signal", name: "KILL" });
  }

  async state(): Promise<State> {
    return (await this.send({ kind: "state" })) as State;
  }

  async text(opts: { full?: boolean } = {}): Promise<string> {
    const data = (await this.send({ kind: "text", full: opts.full ?? false })) as {
      text: string;
    };
    return data.text;
  }

  async cells(x: number, y: number, w = 1, h = 1): Promise<Cell[]> {
    const data = (await this.send({ kind: "cells", x, y, w, h })) as { cells: Cell[] };
    return data.cells;
  }

  async get(field: string): Promise<unknown> {
    const data = (await this.send({ kind: "get", field })) as { value: unknown };
    return data.value;
  }

  async getCommand(): Promise<string | null> {
    return (await this.get("command")) as string | null;
  }

  async getOutput(): Promise<string | null> {
    return (await this.get("output")) as string | null;
  }

  async getExitCode(): Promise<number | null> {
    return (await this.get("exit-code")) as number | null;
  }

  async getCwd(): Promise<string | null> {
    return (await this.get("cwd")) as string | null;
  }

  async getCursor(): Promise<{ x: number; y: number }> {
    return (await this.get("cursor")) as { x: number; y: number };
  }

  async getSize(): Promise<{ cols: number; rows: number }> {
    return (await this.get("size")) as { cols: number; rows: number };
  }

  async screenshot(path: string | null = null, opts: { full?: boolean } = {}): Promise<string> {
    const data = (await this.send({ kind: "screenshot", full: opts.full ?? false, path })) as {
      path?: string;
      text?: string;
    };
    return (data.path ?? data.text) as string;
  }

  async waitText(text: string, opts: WaitTextOptions = {}): Promise<void> {
    await this.#guard("waitText", () =>
      this.send(
        this.#withTimeout(
          {
            kind: "wait_text",
            text,
            regex: opts.regex ?? false,
            full: opts.full ?? false,
            not: opts.not ?? false,
          },
          "text",
          opts.timeout,
        ),
      ),
    );
  }

  async waitIdle(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitIdle", () =>
      this.send(this.#withTimeout({ kind: "wait_idle" }, "idle", opts.timeout)),
    );
  }

  async waitCommand(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitCommand", () =>
      this.send(this.#withTimeout({ kind: "wait_command" }, "command", opts.timeout)),
    );
  }

  async waitExit(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitExit", () =>
      this.send(this.#withTimeout({ kind: "wait_exit" }, "exit", opts.timeout)),
    );
  }

  async waitReady(opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("waitReady", () =>
      this.send(this.#withTimeout({ kind: "wait_ready" }, "ready", opts.timeout)),
    );
  }

  async expectText(text: string, opts: ExpectTextOptions = {}): Promise<void> {
    await this.#guard("expectText", () =>
      this.send(
        this.#withTimeout(
          {
            kind: "expect_text",
            text,
            regex: opts.regex ?? false,
            full: opts.full ?? false,
            strict: opts.strict ?? true,
            not: opts.not ?? false,
            fg: opts.fg ?? null,
            bg: opts.bg ?? null,
          },
          "text",
          opts.timeout,
        ),
      ),
    );
  }

  async expectExitCode(code: number, opts: { timeout?: number } = {}): Promise<void> {
    await this.#guard("expectExitCode", () =>
      this.send(
        this.#withTimeout({ kind: "expect_exit_code", code }, "command", opts.timeout),
      ),
    );
  }

  async expectOutput(text: string, opts: { regex?: boolean } = {}): Promise<void> {
    await this.#guard("expectOutput", () =>
      this.send({ kind: "expect_output", text, regex: opts.regex ?? false }),
    );
  }

  async expectSnapshot(
    name: string,
    opts: { update?: boolean; includeColors?: boolean } = {},
  ): Promise<string> {
    const data = (await this.#guard("expectSnapshot", () =>
      this.send({
        kind: "snapshot",
        name,
        update: opts.update ?? false,
        include_colors: opts.includeColors ?? false,
        cwd: process.cwd(),
      }),
    )) as { status: string };
    return data.status;
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.closeQuiet();
  }
}
