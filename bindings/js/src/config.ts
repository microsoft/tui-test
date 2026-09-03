import type { AutomaticRecording, Backend, Profile, Timeouts } from "./types.js";

export const DEFAULT_COLS = 80;
export const DEFAULT_ROWS = 30;

export const IS_WINDOWS = process.platform === "win32";
export const IS_MACOS = process.platform === "darwin";

export function resolveSession(session?: string): string {
  return session || process.env.TUI_TEST_SESSION || "default";
}

export type TimeoutClass = "text" | "idle" | "command" | "exit" | "ready";

const TIMEOUT_CLASSES: readonly TimeoutClass[] = [
  "text",
  "idle",
  "command",
  "exit",
  "ready",
];
const BACKENDS: readonly Backend[] = ["alacritty", "ghostty", "rio", "xtermjs"];
const PROFILE_FIELDS = new Set(["scrollback", "kittyKeyboard", "colors"]);
const RECORDING_MODES = new Set(["disabled", "on-failure", "always"]);
const COLOR_FIELDS = new Map([
  ["foreground", "foreground"],
  ["background", "background"],
  ["cursor", "cursor"],
  ["black", "black"],
  ["red", "red"],
  ["green", "green"],
  ["yellow", "yellow"],
  ["blue", "blue"],
  ["magenta", "magenta"],
  ["cyan", "cyan"],
  ["white", "white"],
  ["brightBlack", "bright_black"],
  ["brightRed", "bright_red"],
  ["brightGreen", "bright_green"],
  ["brightYellow", "bright_yellow"],
  ["brightBlue", "bright_blue"],
  ["brightMagenta", "bright_magenta"],
  ["brightCyan", "bright_cyan"],
  ["brightWhite", "bright_white"],
]);

export function resolveTimeout(
  cls: TimeoutClass,
  callTimeout?: number,
  options: { timeouts?: Timeouts } = {},
): number | undefined {
  if (callTimeout !== undefined && callTimeout !== null) {
    return callTimeout;
  }
  const scoped = options.timeouts?.[cls];
  if (scoped !== undefined && scoped !== null) {
    return scoped;
  }
  return undefined;
}

export function backendPayload(backend?: unknown): Backend | undefined {
  if (backend === undefined || backend === null) {
    return undefined;
  }
  if (typeof backend !== "string") {
    throw new TypeError("backend must be a string");
  }
  const normalized = backend.trim().toLowerCase();
  if ((BACKENDS as readonly string[]).includes(normalized)) {
    return normalized as Backend;
  }
  throw new TypeError(
    `unknown backend "${backend}"; expected one of ${BACKENDS.join(", ")}`,
  );
}

export function timeoutsPayload(
  timeouts?: Timeouts,
): Record<TimeoutClass, number> | undefined {
  if (!timeouts) {
    return undefined;
  }
  assertTimeoutClasses(timeouts);
  const out: Partial<Record<TimeoutClass, number>> = {};
  let any = false;
  for (const cls of TIMEOUT_CLASSES) {
    const value = timeouts[cls];
    if (value !== undefined && value !== null) {
      out[cls] = value;
      any = true;
    }
  }
  return any ? (out as Record<TimeoutClass, number>) : undefined;
}

export function assertTimeoutClasses(timeouts: Timeouts): void {
  const unknown = Object.keys(timeouts).filter(
    (key) => !(TIMEOUT_CLASSES as readonly string[]).includes(key),
  );
  if (unknown.length > 0) {
    throw new TypeError(
      `unknown timeout class ${unknown.map((k) => `"${k}"`).join(", ")}; ` +
        `expected one of ${TIMEOUT_CLASSES.join(", ")}`,
    );
  }
}

function profileObject(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

export interface ProfilePayload {
  scrollback?: number;
  kittyKeyboard?: boolean;
  colors: [string, string][];
}

export function recordingPayload(
  recording?: AutomaticRecording,
): AutomaticRecording | undefined {
  if (recording === undefined) {
    return undefined;
  }
  const raw = profileObject(recording, "recording");
  const unknown = Object.keys(raw).filter(
    (key) => key !== "mode" && key !== "directory",
  );
  if (unknown.length > 0) {
    throw new TypeError(`unknown recording field ${unknown.join(", ")}`);
  }
  if (raw.mode !== undefined && !RECORDING_MODES.has(String(raw.mode))) {
    throw new TypeError(
      `unknown recording mode "${String(raw.mode)}"; expected disabled, on-failure, or always`,
    );
  }
  if (
    raw.directory !== undefined &&
    (typeof raw.directory !== "string" || raw.directory.length === 0)
  ) {
    throw new TypeError("recording.directory must be a non-empty string");
  }
  return raw as AutomaticRecording;
}

export function profilePayload(profile?: Profile): ProfilePayload | undefined {
  if (profile === undefined) {
    return undefined;
  }
  const raw = profileObject(profile, "profile");
  const unknown = Object.keys(raw).filter((key) => !PROFILE_FIELDS.has(key));
  if (unknown.length > 0) {
    throw new TypeError(`unknown profile field ${unknown.join(", ")}`);
  }
  if (raw.colors !== undefined) {
    const colors = profileObject(raw.colors, "profile.colors");
    const unknownColors = Object.keys(colors).filter((key) => !COLOR_FIELDS.has(key));
    if (unknownColors.length > 0) {
      throw new TypeError(`unknown profile color ${unknownColors.join(", ")}`);
    }
    const payloadColors: [string, string][] = [];
    for (const [name, value] of Object.entries(colors)) {
      if (value !== undefined && typeof value !== "string") {
        throw new TypeError(`profile.colors.${name} must be a string`);
      }
      if (typeof value === "string") {
        payloadColors.push([COLOR_FIELDS.get(name) as string, value]);
      }
    }
    return {
      scrollback: raw.scrollback as number | undefined,
      kittyKeyboard: raw.kittyKeyboard as boolean | undefined,
      colors: payloadColors,
    };
  }
  return {
    scrollback: raw.scrollback as number | undefined,
    kittyKeyboard: raw.kittyKeyboard as boolean | undefined,
    colors: [],
  };
}

export function envPairs(
  env?: Record<string, string | number | boolean> | [string, string][],
): [string, string][] {
  if (!env) {
    return [];
  }
  if (Array.isArray(env)) {
    return env;
  }
  return Object.entries(env).map(([key, value]) => [key, coerceEnvValue(value)]);
}

function coerceEnvValue(value: string | number | boolean): string {
  if (typeof value === "boolean") {
    return value ? "true" : "false";
  }
  return String(value);
}
