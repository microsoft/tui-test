import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  DEFAULT_COLS,
  DEFAULT_ROWS,
  assertTimeoutClasses,
  backendPayload,
  envPairs,
  profilePayload,
  resolveSession,
  resolveTimeout,
  timeoutsPayload,
} from "./config.js";
import type { TimeoutClass } from "./config.js";
import { uniqueSession } from "./ephemeral.js";
import { ExpectationError, TuiTestError } from "./errors.js";
import { NativeRuntime } from "./native.js";
import type { NativeTextSelectorOptions } from "./native.js";
import type {
  BellEvent,
  Cell,
  ClientOptions,
  Cursor,
  OpenResult,
  Shell,
  Size,
  SpawnOptions,
  State,
  TextMatch,
} from "./types.js";

export interface WaitTextOptions extends TextSelectorOptions {
  not?: boolean;
  timeout?: number;
}

export interface TitleOptions {
  regex?: boolean;
  not?: boolean;
  timeout?: number;
}

export type TextOccurrence =
  | "any"
  | "unique"
  | "first"
  | "last"
  | { nth: number };

export interface TextAnchor {
  text: string;
  regex?: boolean;
  occurrence?: TextOccurrence;
}

export interface TextSelectorOptions {
  regex?: boolean;
  full?: boolean;
  whitespace?: "exact" | "normalize";
  scope?: {
    after?: TextAnchor;
    before?: TextAnchor;
  };
  occurrence?: TextOccurrence;
}

export interface TextStyleExpectation {
  foreground?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underlineStyle?: "none" | "single" | "double" | "curly" | "dotted" | "dashed";
  underlineColor?: string;
  inverse?: boolean;
  hidden?: boolean;
  strikethrough?: boolean;
  blink?: boolean;
}

export interface LocatorWaitOptions {
  state?: "visible" | "hidden";
  timeout?: number;
}

export interface LocatorClickOptions {
  button?: number;
  clicks?: number;
  timeout?: number;
}

export interface LocatorHighlightOptions {
  timeout?: number;
}

export interface LocatorExpectOptions {
  not?: boolean;
  style?: TextStyleExpectation;
  timeout?: number;
}

export interface TextLocator {
  locator(text: string, opts?: TextSelectorOptions): TextLocator;
  any(): TextLocator;
  unique(): TextLocator;
  first(): TextLocator;
  last(): TextLocator;
  nth(index: number): TextLocator;
  locations(): Promise<TextMatch[]>;
  location(): Promise<TextMatch>;
  count(): Promise<number>;
  all(): Promise<TextLocator[]>;
  wait(opts?: LocatorWaitOptions): Promise<TextLocator>;
  click(opts?: LocatorClickOptions): Promise<void>;
  highlight(opts?: LocatorHighlightOptions): Promise<void>;
  expect(opts?: LocatorExpectOptions): Promise<void>;
}

export interface ExpectTextOptions extends TextSelectorOptions {
  strict?: boolean;
  not?: boolean;
  fg?: string;
  bg?: string;
  style?: TextStyleExpectation;
  timeout?: number;
}

export interface MouseButtonOptions {
  button?: number;
}

export type RecordingFormat = "apng" | "gif" | "mp4" | "cast";

export interface RecordingOptions {
  format?: RecordingFormat;
  fps?: number;
  speed?: number;
  idleTimeLimit?: number;
  zoom?: number;
}

export interface ScreenshotOptions {
  full?: boolean;
  zoom?: number;
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

class Keyboard {
  #runtime: NativeRuntime;

  constructor(runtime: NativeRuntime) {
    this.#runtime = runtime;
  }

  async press(...keys: string[]): Promise<void> {
    await this.#runtime.press(keys);
  }

  async down(...keys: string[]): Promise<void> {
    await this.#runtime.keyDown(keys);
  }

  async repeat(...keys: string[]): Promise<void> {
    await this.#runtime.repeat(keys);
  }

  async up(...keys: string[]): Promise<void> {
    await this.#runtime.keyUp(keys);
  }
}

function occurrenceOptions(occurrence?: TextOccurrence): {
  occurrence?: string;
  nth?: number;
} {
  if (occurrence === undefined) {
    return {};
  }
  if (typeof occurrence === "string") {
    if (!["any", "unique", "first", "last"].includes(occurrence)) {
      throw new TypeError(
        "text occurrence must be any, unique, first, last, or a non-negative integer",
      );
    }
    return { occurrence };
  }
  if (
    occurrence === null ||
    !Number.isSafeInteger(occurrence.nth) ||
    occurrence.nth < 0
  ) {
    throw new TypeError("text occurrence nth must be a non-negative integer");
  }
  return { occurrence: "nth", nth: occurrence.nth };
}

function cloneOccurrence(occurrence?: TextOccurrence): TextOccurrence | undefined {
  occurrenceOptions(occurrence);
  return typeof occurrence === "object" && occurrence !== null
    ? { nth: occurrence.nth }
    : occurrence;
}

function cloneSelectorOptions(opts: TextSelectorOptions): TextSelectorOptions {
  return {
    ...opts,
    occurrence: cloneOccurrence(opts.occurrence),
    scope: opts.scope
      ? {
          after: opts.scope.after
            ? {
                ...opts.scope.after,
                occurrence: cloneOccurrence(opts.scope.after.occurrence),
              }
            : undefined,
          before: opts.scope.before
            ? {
                ...opts.scope.before,
                occurrence: cloneOccurrence(opts.scope.before.occurrence),
              }
            : undefined,
        }
      : undefined,
  };
}

function selectorOptions(
  opts: TextSelectorOptions,
  withinJson?: string,
): NativeTextSelectorOptions {
  const after = opts.scope?.after;
  const before = opts.scope?.before;
  const afterOccurrence = occurrenceOptions(after?.occurrence);
  const beforeOccurrence = occurrenceOptions(before?.occurrence);
  return {
    regex: opts.regex ?? false,
    full: opts.full ?? false,
    whitespace: opts.whitespace ?? "exact",
    ...occurrenceOptions(opts.occurrence),
    afterText: after?.text,
    afterRegex: after?.regex,
    afterOccurrence: afterOccurrence.occurrence,
    afterNth: afterOccurrence.nth,
    beforeText: before?.text,
    beforeRegex: before?.regex,
    beforeOccurrence: beforeOccurrence.occurrence,
    beforeNth: beforeOccurrence.nth,
    withinJson,
  };
}

function selectorJson(
  text: string,
  opts: TextSelectorOptions,
  withinJson?: string,
): string {
  const occurrence = occurrenceOptions(opts.occurrence);
  const anchor = (value: TextAnchor | undefined): object | null => {
    if (!value) {
      return null;
    }
    const selected = occurrenceOptions(value.occurrence);
    return {
      text: value.text,
      regex: value.regex ?? false,
      occurrence:
        selected.nth === undefined
          ? selected.occurrence ?? "unique"
          : { nth: selected.nth },
    };
  };
  return JSON.stringify({
    text,
    regex: opts.regex ?? false,
    full: opts.full ?? false,
    whitespace: opts.whitespace ?? "exact",
    scope: {
      after: anchor(opts.scope?.after),
      before: anchor(opts.scope?.before),
    },
    occurrence:
      occurrence.nth === undefined
        ? occurrence.occurrence ?? "any"
        : { nth: occurrence.nth },
    within: withinJson === undefined ? null : JSON.parse(withinJson),
  });
}

interface LocatorActions {
  locations(
    text: string,
    options: TextSelectorOptions,
    withinJson: string | undefined,
    operation: string,
  ): Promise<TextMatch[]>;
  wait(
    text: string,
    options: TextSelectorOptions,
    withinJson: string | undefined,
    hidden: boolean,
    timeout: number | undefined,
    operation: string,
  ): Promise<void>;
  click(
    text: string,
    options: TextSelectorOptions,
    withinJson: string | undefined,
    action: LocatorClickOptions,
  ): Promise<void>;
  highlight(
    text: string,
    options: TextSelectorOptions,
    withinJson: string | undefined,
    timeout: number | undefined,
  ): Promise<void>;
  expect(
    text: string,
    options: TextSelectorOptions,
    withinJson: string | undefined,
    expectation: LocatorExpectOptions,
  ): Promise<void>;
  fail(operation: string, message: string): Promise<never>;
}

class TextLocatorImpl implements TextLocator {
  readonly #text: string;
  readonly #options: TextSelectorOptions;
  readonly #actions: LocatorActions;
  readonly #withinJson: string | undefined;

  constructor(
    text: string,
    options: TextSelectorOptions,
    actions: LocatorActions,
    withinJson?: string,
  ) {
    this.#text = text;
    this.#options = cloneSelectorOptions(options);
    this.#actions = actions;
    this.#withinJson = withinJson;
  }

  #withOccurrence(occurrence: TextOccurrence): TextLocatorImpl {
    return new TextLocatorImpl(
      this.#text,
      { ...this.#options, occurrence },
      this.#actions,
      this.#withinJson,
    );
  }

  #strictOptions(): TextSelectorOptions {
    return {
      ...cloneSelectorOptions(this.#options),
      occurrence:
        (this.#options.occurrence ?? "any") === "any"
          ? "unique"
          : cloneOccurrence(this.#options.occurrence),
    };
  }

  any(): TextLocator {
    return this.#withOccurrence("any");
  }

  unique(): TextLocator {
    return this.#withOccurrence("unique");
  }

  first(): TextLocator {
    return this.#withOccurrence("first");
  }

  last(): TextLocator {
    return this.#withOccurrence("last");
  }

  nth(index: number): TextLocator {
    if (!Number.isSafeInteger(index) || index < 0) {
      throw new TypeError("locator nth index must be a non-negative integer");
    }
    return this.#withOccurrence({ nth: index });
  }

  locator(text: string, opts: TextSelectorOptions = {}): TextLocator {
    return new TextLocatorImpl(
      text,
      { ...opts, occurrence: opts.occurrence ?? "any" },
      this.#actions,
      selectorJson(this.#text, this.#options, this.#withinJson),
    );
  }

  locationsWithOperation(operation: string): Promise<TextMatch[]> {
    return this.#actions.locations(
      this.#text,
      this.#options,
      this.#withinJson,
      operation,
    );
  }

  locations(): Promise<TextMatch[]> {
    return this.locationsWithOperation("locator.locations");
  }

  async location(): Promise<TextMatch> {
    const matches = await this.#actions.locations(
      this.#text,
      this.#strictOptions(),
      this.#withinJson,
      "locator.location",
    );
    if (matches.length !== 1) {
      return this.#actions.fail(
        "locator.location",
        `no match found for ${JSON.stringify(this.#text)}`,
      );
    }
    return matches[0];
  }

  async count(): Promise<number> {
    return (await this.locations()).length;
  }

  async all(): Promise<TextLocator[]> {
    const matches = await this.locations();
    if ((this.#options.occurrence ?? "any") === "any") {
      return matches.map((_, index) => this.nth(index));
    }
    return matches.map(
      () =>
        new TextLocatorImpl(
          this.#text,
          this.#options,
          this.#actions,
          this.#withinJson,
        ),
    );
  }

  waitWithOperation(
    opts: LocatorWaitOptions,
    operation: string,
  ): Promise<TextLocator> {
    return this.#wait(opts, operation);
  }

  async #wait(
    opts: LocatorWaitOptions,
    operation: string,
  ): Promise<TextLocator> {
    const state = opts.state ?? "visible";
    if (state !== "visible" && state !== "hidden") {
      throw new TypeError("locator state must be 'visible' or 'hidden'");
    }
    await this.#actions.wait(
      this.#text,
      this.#options,
      this.#withinJson,
      state === "hidden",
      opts.timeout,
      operation,
    );
    return this;
  }

  wait(opts: LocatorWaitOptions = {}): Promise<TextLocator> {
    return this.#wait(opts, "locator.wait");
  }

  click(opts: LocatorClickOptions = {}): Promise<void> {
    return this.#actions.click(
      this.#text,
      this.#strictOptions(),
      this.#withinJson,
      opts,
    );
  }

  highlight(opts: LocatorHighlightOptions = {}): Promise<void> {
    return this.#actions.highlight(
      this.#text,
      this.#options,
      this.#withinJson,
      opts.timeout,
    );
  }

  expect(opts: LocatorExpectOptions = {}): Promise<void> {
    return this.#actions.expect(
      this.#text,
      this.#options,
      this.#withinJson,
      opts,
    );
  }
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

export class TuiTest {
  readonly session: string;
  readonly keyboard: Keyboard;
  readonly mouse: Mouse;
  #runtime: NativeRuntime;
  #options: ClientOptions;
  #artifactCounter = 0;

  constructor(session?: string, opts: ClientOptions = {}) {
    this.session = resolveSession(session);
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    backendPayload(opts.backend);
    profilePayload(opts.profile);
    this.#options = opts;
    this.#runtime = new NativeRuntime(this.session);
    this.keyboard = new Keyboard(this.#runtime);
    this.mouse = new Mouse(this.#runtime);
  }

  static ephemeral(prefix?: string, opts: ClientOptions = {}): TuiTest {
    return new TuiTest(uniqueSession(prefix), opts);
  }

  #timeout(cls: TimeoutClass, callTimeout?: number): number | undefined {
    return resolveTimeout(cls, callTimeout, this.#options);
  }

  #textLocator(text: string, opts: TextSelectorOptions): TextLocatorImpl {
    const actions: LocatorActions = {
      locations: (value, options, withinJson, operation) =>
        this.#guard(operation, () =>
          this.#runtime.findText(value, selectorOptions(options, withinJson)),
        ),
      wait: (value, options, withinJson, hidden, timeout, operation) =>
        this.#guard(operation, () =>
          this.#runtime.waitTextSelector(
            value,
            selectorOptions(options, withinJson),
            hidden,
            this.#timeout("text", timeout),
          ),
        ),
      click: (value, options, withinJson, action) =>
        this.#guard("locator.click", () =>
          this.#runtime.clickText(
            value,
            selectorOptions(options, withinJson),
            action.button ?? 0,
            action.clicks ?? 1,
            this.#timeout("text", action.timeout),
          ),
        ),
      highlight: (value, options, withinJson, timeout) =>
        this.#guard("locator.highlight", async () => {
          await this.#runtime.highlightText(
            value,
            selectorOptions(options, withinJson),
            this.#timeout("text", timeout),
          );
        }),
      expect: (value, options, withinJson, expectation) => {
        const style = expectation.style ?? {};
        return this.#guard("locator.expect", () =>
          this.#runtime.expectText(value, {
            ...selectorOptions(options, withinJson),
            strict: true,
            not: expectation.not ?? false,
            fg: style.foreground,
            bg: style.background,
            bold: style.bold,
            dim: style.dim,
            italic: style.italic,
            underlineStyle: style.underlineStyle,
            underlineColor: style.underlineColor,
            inverse: style.inverse,
            hidden: style.hidden,
            strikethrough: style.strikethrough,
            blink: style.blink,
            timeoutMs: this.#timeout("text", expectation.timeout),
          }),
        );
      },
      fail: async (operation, message) => {
        let diagnostic = message;
        try {
          diagnostic += `\n\nTerminal content:\n${await this.text()}`;
        } catch (error) {
          if (!(error instanceof TuiTestError)) {
            throw error;
          }
          diagnostic += `\n\nTerminal content unavailable: ${error.message}`;
        }
        return this.#guard(operation, async () => {
          throw new ExpectationError(diagnostic);
        });
      },
    };
    return new TextLocatorImpl(
      text,
      { ...opts, occurrence: opts.occurrence ?? "any" },
      actions,
    );
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
    const profile = profilePayload(opts.profile ?? this.#options.profile);
    const options = {
      backend: backendPayload(opts.backend ?? this.#options.backend),
      shell: opts.shell,
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd,
      env: envPairs(opts.env),
      waitReady: opts.waitReady,
      restart: opts.restart,
      profileScrollback: profile?.scrollback,
      profileColors: profile?.colors,
      timeouts: timeoutsPayload(opts.timeouts),
    };
    return this.#spawn(() => this.#runtime.open(options), opts.retries ?? 0);
  }

  async run(program: string, args: string[] = [], opts: SpawnOptions = {}): Promise<OpenResult> {
    if (opts.timeouts) {
      assertTimeoutClasses(opts.timeouts);
    }
    const profile = profilePayload(opts.profile ?? this.#options.profile);
    const options = {
      backend: backendPayload(opts.backend ?? this.#options.backend),
      program,
      args,
      cols: opts.cols ?? DEFAULT_COLS,
      rows: opts.rows ?? DEFAULT_ROWS,
      cwd: opts.cwd,
      env: envPairs(opts.env),
      waitReady: opts.waitReady,
      restart: opts.restart,
      profileScrollback: profile?.scrollback,
      profileColors: profile?.colors,
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
    await this.keyboard.press(...keys);
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

  async getTitle(): Promise<string | null> {
    return this.#runtime.getTitle();
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

  async getBellEvents(): Promise<BellEvent[]> {
    return this.#runtime.getBellEvents();
  }

  async screenshot(path: string | null = null, opts: ScreenshotOptions = {}): Promise<string> {
    if (opts.zoom !== undefined && path === null) {
      throw new TypeError("screenshot zoom requires a path");
    }
    return this.#runtime.screenshot({
      full: opts.full ?? false,
      path: optional(path),
      zoom: opts.zoom,
    });
  }

  async startRecording(path: string, opts: RecordingOptions = {}): Promise<void> {
    await this.#runtime.startRecording({
      path,
      format: opts.format,
      fps: opts.fps,
      speed: opts.speed,
      idleTimeLimit: opts.idleTimeLimit,
      zoom: opts.zoom,
    });
  }

  async stopRecording(): Promise<string> {
    return this.#runtime.stopRecording();
  }

  locator(text: string, opts: TextSelectorOptions = {}): TextLocator {
    return this.#textLocator(text, opts);
  }

  async waitText(text: string, opts: WaitTextOptions = {}): Promise<TextLocator> {
    const { not = false, timeout, ...selector } = opts;
    const locator = this.#textLocator(text, selector);
    await locator.waitWithOperation(
      { state: not ? "hidden" : "visible", timeout },
      "waitText",
    );
    return locator;
  }

  async waitTitle(text: string, opts: TitleOptions = {}): Promise<void> {
    await this.#guard("waitTitle", () =>
      this.#runtime.waitTitle(text, {
        regex: opts.regex ?? false,
        not: opts.not ?? false,
        timeoutMs: this.#timeout("text", opts.timeout),
      }),
    );
  }

  async findText(text: string, opts: TextSelectorOptions = {}): Promise<TextMatch[]> {
    return this.#textLocator(text, opts).locationsWithOperation("findText");
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

  async expectTitle(text: string, opts: TitleOptions = {}): Promise<void> {
    await this.#guard("expectTitle", () =>
      this.#runtime.expectTitle(text, {
        regex: opts.regex ?? false,
        not: opts.not ?? false,
        timeoutMs: this.#timeout("text", opts.timeout),
      }),
    );
  }

  async expectText(text: string, opts: ExpectTextOptions = {}): Promise<void> {
    const style = opts.style ?? {};
    await this.#guard("expectText", () =>
      this.#runtime.expectText(text, {
        ...selectorOptions(opts),
        strict: opts.strict ?? true,
        not: opts.not ?? false,
        fg: style.foreground ?? opts.fg,
        bg: style.background ?? opts.bg,
        bold: style.bold,
        dim: style.dim,
        italic: style.italic,
        underlineStyle: style.underlineStyle,
        underlineColor: style.underlineColor,
        inverse: style.inverse,
        hidden: style.hidden,
        strikethrough: style.strikethrough,
        blink: style.blink,
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
    opts: { update?: boolean; includeColors?: boolean; includeTitle?: boolean } = {},
  ): Promise<string> {
    return this.#guard("expectSnapshot", () =>
      this.#runtime.snapshot(name, {
        update: opts.update ?? false,
        includeColors: opts.includeColors ?? false,
        includeTitle: opts.includeTitle ?? false,
        cwd: process.cwd(),
      }),
    );
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.closeQuiet();
  }
}
