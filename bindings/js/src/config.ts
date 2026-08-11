import type { Timeouts } from "./types.js";

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
