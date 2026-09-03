import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  DEFAULT_COLS,
  DEFAULT_ROWS,
  assertTimeoutClasses,
  backendPayload,
  envPairs,
  profilePayload,
  recordingPayload,
  resolveSession,
  resolveTimeout,
  timeoutsPayload,
} from "./config.js";
import type { TimeoutClass } from "./config.js";
import { uniqueSession } from "./ephemeral.js";
import { ExpectationError, TuiTestError, UsageError } from "./errors.js";
import { NativeRuntime } from "./native.js";
import type {
  RuntimeLocatorStage,
  RuntimeLocatorStyle,
} from "./native.js";
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

export interface TitleOptions {
  regex?: boolean;
  not?: boolean;
  timeout?: number;
}

export interface ClipboardWaitOptions {
  timeout?: number;
}

function isRegExp(value: unknown): value is RegExp {
  return Object.prototype.toString.call(value) === "[object RegExp]";
}

type TextOccurrence =
  | "any"
  | "unique"
  | "first"
  | "last"
  | { nth: number };

export type LocatorDirection = "within" | "after" | "before";

export interface TextSelectorOptions {
  regex?: boolean;
  full?: boolean;
  whitespace?: "exact" | "normalize";
}

export interface RelativeTextSelectorOptions extends TextSelectorOptions {
  direction?: LocatorDirection;
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

export type MouseButton = "left" | "middle" | "right";

export interface MouseButtonOptions {
  button?: MouseButton;
  alt?: boolean;
  ctrl?: boolean;
  shift?: boolean;
}

export interface MouseClickOptions extends MouseButtonOptions {
  onText?: string;
  clicks?: number;
}

export interface LocatorClickOptions extends MouseButtonOptions {
  clicks?: number;
  timeout?: number;
}

export interface LocatorHighlightOptions {
  timeout?: number;
}

export interface LocatorExpectOptions {
  not?: boolean;
  timeout?: number;
}

export interface StyleSelectorOptions {
  full?: boolean;
}

export interface RelativeStyleSelectorOptions extends StyleSelectorOptions {
  direction?: LocatorDirection;
}

export interface Locator {
  getByText(text: string, opts?: RelativeTextSelectorOptions): Locator;
  getByStyle(style: TextStyleExpectation, opts?: RelativeStyleSelectorOptions): Locator;
  any(): Locator;
  unique(): Locator;
  first(): Locator;
  last(): Locator;
  nth(index: number): Locator;
  locations(): Promise<TextMatch[]>;
  location(): Promise<TextMatch>;
  count(): Promise<number>;
  all(): Promise<Locator[]>;
  wait(opts?: LocatorWaitOptions): Promise<Locator>;
  click(opts?: LocatorClickOptions): Promise<void>;
  highlight(opts?: LocatorHighlightOptions): Promise<void>;
  expect(opts?: LocatorExpectOptions): Promise<void>;
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

function isMouseButton(value: unknown): value is MouseButton {
  return value === "left" || value === "middle" || value === "right";
}

function mouseModifier(value: boolean | undefined, name: string, bit: number): number {
  if (value !== undefined && typeof value !== "boolean") {
    throw new TypeError(`${name} must be a boolean`);
  }
  return value ? bit : 0;
}

function mouseButtonCode(opts: MouseButtonOptions): number {
  const button = opts.button ?? "left";
  if (!isMouseButton(button)) {
    throw new TypeError(
      `unknown mouse button "${String(button)}"; expected one of left, middle, right`,
    );
  }
  const base = { left: 0, middle: 1, right: 2 }[button];
  return (
    base +
    mouseModifier(opts.shift, "shift", 4) +
    mouseModifier(opts.alt, "alt", 8) +
    mouseModifier(opts.ctrl, "ctrl", 16)
  );
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
        "occurrence must be any, unique, first, last, or { nth: index }",
      );
    }
    return { occurrence };
  }
  if (
    occurrence === null ||
    !Number.isSafeInteger(occurrence.nth) ||
    occurrence.nth < 0
  ) {
    throw new TypeError("nth index must be a non-negative integer");
  }
  return { occurrence: "nth", nth: occurrence.nth };
}

type LocatorQueryValue = RuntimeLocatorStage[];

function directionValue(direction?: LocatorDirection): LocatorDirection {
  if (
    direction === undefined ||
    direction === "within" ||
    direction === "after" ||
    direction === "before"
  ) {
    return direction ?? "within";
  }
  throw new TypeError("locator direction must be within, after, or before");
}

function textStageValue(
  text: string,
  opts: RelativeTextSelectorOptions,
  occurrence: TextOccurrence = "any",
): RuntimeLocatorStage {
  return {
    kind: "text",
    direction: opts.direction ?? "within",
    text,
    regex: opts.regex ?? false,
    full: opts.full ?? false,
    whitespace: opts.whitespace ?? "exact",
    ...occurrenceOptions(occurrence),
  };
}

function styleStageValue(
  style: TextStyleExpectation,
  opts: RelativeStyleSelectorOptions,
  occurrence: TextOccurrence = "any",
): RuntimeLocatorStage {
  if (!Object.values(style).some((value) => value !== undefined)) {
    throw new TypeError("getByStyle requires at least one style property");
  }
  return {
    kind: "style",
    direction: opts.direction ?? "within",
    style: textStyleValue(style),
    full: opts.full ?? false,
    ...occurrenceOptions(occurrence),
  };
}

function textStyleValue(style: TextStyleExpectation): RuntimeLocatorStyle {
  return {
    foreground: style.foreground,
    background: style.background,
    bold: style.bold,
    dim: style.dim,
    italic: style.italic,
    underlineStyle: style.underlineStyle,
    underlineColor: style.underlineColor,
    inverse: style.inverse,
    hidden: style.hidden,
    strikethrough: style.strikethrough,
    blink: style.blink,
  };
}

function textQuery(
  text: string,
  opts: RelativeTextSelectorOptions,
  within: LocatorQueryValue = [],
): LocatorQueryValue {
  rejectOccurrenceOption(opts);
  const direction = directionValue(opts.direction);
  if (within.length === 0 && direction !== "within") {
    throw new TypeError("locator direction requires a parent locator");
  }
  return [...within, textStageValue(text, { ...opts, direction })];
}

function styleQuery(
  style: TextStyleExpectation,
  opts: RelativeStyleSelectorOptions,
  within: LocatorQueryValue = [],
): LocatorQueryValue {
  rejectOccurrenceOption(opts);
  const direction = directionValue(opts.direction);
  if (within.length === 0 && direction !== "within") {
    throw new TypeError("locator direction requires a parent locator");
  }
  return [...within, styleStageValue(style, { ...opts, direction })];
}

function rejectOccurrenceOption(opts: object): void {
  if ("occurrence" in opts) {
    throw new TypeError(
      "select locator occurrences with any(), unique(), first(), last(), or nth()",
    );
  }
}

function cloneQuery(query: LocatorQueryValue): LocatorQueryValue {
  return query.map((stage) => ({
    ...stage,
    style: stage.style ? { ...stage.style } : undefined,
  }));
}

function currentStage(query: LocatorQueryValue): RuntimeLocatorStage {
  const stage = query.at(-1);
  if (!stage) {
    throw new TypeError("locator requires at least one stage");
  }
  return stage;
}

interface LocatorActions {
  locations(query: LocatorQueryValue, operation: string): Promise<TextMatch[]>;
  wait(
    query: LocatorQueryValue,
    hidden: boolean,
    timeout: number | undefined,
    operation: string,
  ): Promise<void>;
  click(query: LocatorQueryValue, action: LocatorClickOptions): Promise<void>;
  highlight(
    query: LocatorQueryValue,
    timeout: number | undefined,
  ): Promise<void>;
  expect(
    query: LocatorQueryValue,
    expectation: LocatorExpectOptions,
    operation: string,
  ): Promise<void>;
  fail(operation: string, message: string): Promise<never>;
}

class LocatorImpl implements Locator {
  readonly #query: LocatorQueryValue;
  readonly #actions: LocatorActions;

  constructor(query: LocatorQueryValue, actions: LocatorActions) {
    this.#query = cloneQuery(query);
    this.#actions = actions;
  }

  #withOccurrence(occurrence: TextOccurrence): LocatorImpl {
    const query = cloneQuery(this.#query);
    const selected = occurrenceOptions(occurrence);
    const stage = currentStage(query);
    stage.occurrence =
      selected.nth === undefined
        ? selected.occurrence ?? "any"
        : "nth";
    stage.nth = selected.nth;
    return new LocatorImpl(query, this.#actions);
  }

  #strictQuery(): LocatorQueryValue {
    const query = cloneQuery(this.#query);
    const stage = currentStage(query);
    if (stage.occurrence === "any") {
      stage.occurrence = "unique";
    }
    return query;
  }

  any(): Locator {
    return this.#withOccurrence("any");
  }

  unique(): Locator {
    return this.#withOccurrence("unique");
  }

  first(): Locator {
    return this.#withOccurrence("first");
  }

  last(): Locator {
    return this.#withOccurrence("last");
  }

  nth(index: number): Locator {
    if (!Number.isSafeInteger(index) || index < 0) {
      throw new TypeError("locator nth index must be a non-negative integer");
    }
    return this.#withOccurrence({ nth: index });
  }

  getByText(text: string, opts: RelativeTextSelectorOptions = {}): Locator {
    return new LocatorImpl(
      textQuery(text, opts, this.#query),
      this.#actions,
    );
  }

  getByStyle(
    style: TextStyleExpectation,
    opts: RelativeStyleSelectorOptions = {},
  ): Locator {
    return new LocatorImpl(
      styleQuery(style, opts, this.#query),
      this.#actions,
    );
  }

  locations(): Promise<TextMatch[]> {
    return this.#actions.locations(this.#query, "locator.locations");
  }

  async location(): Promise<TextMatch> {
    const matches = await this.#actions.locations(
      this.#strictQuery(),
      "locator.location",
    );
    if (matches.length !== 1) {
      const current = currentStage(this.#query);
      const description =
        current.kind === "text"
          ? JSON.stringify(current.text)
          : "style";
      return this.#actions.fail(
        "locator.location",
        `no match found for ${description}`,
      );
    }
    return matches[0];
  }

  async count(): Promise<number> {
    return (await this.locations()).length;
  }

  async all(): Promise<Locator[]> {
    const matches = await this.locations();
    if (currentStage(this.#query).occurrence === "any") {
      return matches.map((_, index) => this.nth(index));
    }
    return matches.map(() => new LocatorImpl(this.#query, this.#actions));
  }

  async wait(opts: LocatorWaitOptions = {}): Promise<Locator> {
    const state = opts.state ?? "visible";
    if (state !== "visible" && state !== "hidden") {
      throw new TypeError("locator state must be 'visible' or 'hidden'");
    }
    await this.#actions.wait(
      this.#query,
      state === "hidden",
      opts.timeout,
      "locator.wait",
    );
    return this;
  }

  click(opts: LocatorClickOptions = {}): Promise<void> {
    return this.#actions.click(this.#strictQuery(), opts);
  }

  highlight(opts: LocatorHighlightOptions = {}): Promise<void> {
    return this.#actions.highlight(this.#query, opts.timeout);
  }

  expect(opts: LocatorExpectOptions = {}): Promise<void> {
    if ("style" in opts) {
      throw new TypeError(
        "refine the locator with getByStyle() before calling expect()",
      );
    }
    return this.#actions.expect(this.#query, opts, "locator.expect");
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
    opts: MouseClickOptions = {},
  ): Promise<void> {
    await this.#runtime.mouseClick({
      x: optional(x),
      y: optional(y),
      onText: opts.onText,
      button: mouseButtonCode(opts),
      clicks: opts.clicks ?? 1,
    });
  }

  async move(x: number, y: number): Promise<void> {
    await this.#runtime.mouseMove(x, y);
  }

  async down(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#runtime.mouseDown(x, y, mouseButtonCode(opts));
  }

  async up(x: number, y: number, opts: MouseButtonOptions = {}): Promise<void> {
    await this.#runtime.mouseUp(x, y, mouseButtonCode(opts));
  }

  async drag(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    opts: MouseButtonOptions = {},
  ): Promise<void> {
    await this.#runtime.mouseDrag(x1, y1, x2, y2, mouseButtonCode(opts));
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
    this.#runtime = new NativeRuntime(this.session, recordingPayload(opts.recording));
    this.keyboard = new Keyboard(this.#runtime);
    this.mouse = new Mouse(this.#runtime);
  }

  static ephemeral(prefix?: string, opts: ClientOptions = {}): TuiTest {
    return new TuiTest(uniqueSession(prefix), opts);
  }

  #timeout(cls: TimeoutClass, callTimeout?: number): number | undefined {
    return resolveTimeout(cls, callTimeout, this.#options);
  }

  #makeLocator(query: LocatorQueryValue): LocatorImpl {
    const actions: LocatorActions = {
      locations: (value, operation) =>
        this.#guard(operation, () =>
          this.#runtime.findLocator(value),
        ),
      wait: (value, hidden, timeout, operation) =>
        this.#guard(operation, () =>
          this.#runtime.waitLocator(
            value,
            hidden,
            this.#timeout("text", timeout),
          ),
        ),
      click: (value, action) =>
        this.#guard("locator.click", () =>
          this.#runtime.clickLocator(
            value,
            mouseButtonCode(action),
            action.clicks ?? 1,
            this.#timeout("text", action.timeout),
          ),
        ),
      highlight: (value, timeout) =>
        this.#guard("locator.highlight", async () => {
          await this.#runtime.highlightLocator(
            value,
            this.#timeout("text", timeout),
          );
        }),
      expect: (value, expectation, operation) => {
        return this.#guard(operation, () =>
          this.#runtime.expectLocator(
            value,
            expectation.not ?? false,
            this.#timeout("text", expectation.timeout),
          ),
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
    return new LocatorImpl(query, actions);
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

  async #waitClipboardRegex(
    pattern: RegExp,
    timeoutMs: number | undefined,
  ): Promise<void> {
    if (
      timeoutMs !== undefined &&
      (!Number.isSafeInteger(timeoutMs) || timeoutMs < 0)
    ) {
      throw new UsageError(
        `timeoutMs must be an integer between 0 and ${Number.MAX_SAFE_INTEGER}`,
      );
    }
    const effectiveTimeout =
      timeoutMs ?? (await this.#runtime.state()).timeouts.text;
    const deadline = Date.now() + effectiveTimeout;
    const matcher = new RegExp(pattern.source, pattern.flags);
    const initialLastIndex = pattern.lastIndex;

    while (true) {
      const value = await this.#runtime.getClipboard();
      matcher.lastIndex = initialLastIndex;
      if (matcher.test(value)) {
        return;
      }
      await this.#runtime.waitClipboard(undefined, {
        regex: false,
        timeoutMs: Math.max(0, deadline - Date.now()),
      });
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
      profileKittyKeyboard: profile?.kittyKeyboard,
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
      profileKittyKeyboard: profile?.kittyKeyboard,
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

  async getClipboard(): Promise<string> {
    return this.#runtime.getClipboard();
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

  getByText(text: string, opts: TextSelectorOptions = {}): Locator {
    return this.#makeLocator(textQuery(text, opts));
  }

  getByStyle(
    style: TextStyleExpectation,
    opts: StyleSelectorOptions = {},
  ): Locator {
    return this.#makeLocator(styleQuery(style, opts));
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

  async waitClipboard(opts?: ClipboardWaitOptions): Promise<void>;
  async waitClipboard(
    pattern: string | RegExp,
    opts?: ClipboardWaitOptions,
  ): Promise<void>;
  async waitClipboard(
    patternOrOpts: string | RegExp | ClipboardWaitOptions = {},
    opts: ClipboardWaitOptions = {},
  ): Promise<void> {
    const text = typeof patternOrOpts === "string" ? patternOrOpts : undefined;
    const options =
      typeof patternOrOpts === "string" || isRegExp(patternOrOpts)
        ? opts
        : patternOrOpts;
    await this.#guard("waitClipboard", async () => {
      if (Object.prototype.hasOwnProperty.call(options, "regex")) {
        throw new UsageError(
          "pass a RegExp instead of regex: true",
        );
      }
      const timeoutMs = this.#timeout("text", options.timeout);
      if (isRegExp(patternOrOpts)) {
        return this.#waitClipboardRegex(patternOrOpts, timeoutMs);
      }
      return this.#runtime.waitClipboard(text, {
        regex: false,
        timeoutMs,
      });
    });
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
