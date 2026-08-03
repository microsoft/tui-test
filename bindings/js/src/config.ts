import { createHash } from "node:crypto";
import os from "node:os";
import path from "node:path";

import type { Timeouts } from "./types.js";

export const DEFAULT_COLS = 80;
export const DEFAULT_ROWS = 30;

export const IS_WINDOWS = process.platform === "win32";
export const IS_MACOS = process.platform === "darwin";

const SOCKET_PATH_MAX = 100;
const SOCKET_DIGEST_HEX_LEN = 16;

export function resolveSession(session?: string): string {
  return session || process.env.SHELL_USE_SESSION || "default";
}

export function resolveBinary(binary?: string): string {
  return binary || process.env.SHELL_USE_BIN || "shell-use";
}

export function resolveHome(home?: string): string | undefined {
  return home || process.env.SHELL_USE_HOME || undefined;
}

export function homeDir(home?: string): string {
  return home || path.join(os.homedir(), ".shell-use");
}

export function socketPathIn(directory: string, session: string): string {
  const candidate = path.join(directory, `${session}.sock`);
  if (Buffer.byteLength(candidate) <= SOCKET_PATH_MAX) {
    return candidate;
  }
  const digest = createHash("sha256").update(session, "utf8").digest("hex");
  return path.join(directory, `${digest.slice(0, SOCKET_DIGEST_HEX_LEN)}.sock`);
}

export function socketPath(session: string, home?: string): string {
  if (IS_WINDOWS) {
    return `\\\\.\\pipe\\shell-use-${session}.sock`;
  }
  return socketPathIn(homeDir(home), session);
}

function cacheDir(): string {
  if (IS_WINDOWS) {
    return process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
  }
  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Caches");
  }
  return process.env.XDG_CACHE_HOME || path.join(os.homedir(), ".cache");
}

export function recordingDir(home?: string): string {
  if (home) {
    return path.join(home, "recordings");
  }
  return path.join(cacheDir(), "shell-use");
}

export function recordingPath(session: string, home?: string): string {
  return path.join(recordingDir(home), `${session}.cast`);
}

export type TimeoutClass = "text" | "idle" | "command" | "exit" | "ready";

const TIMEOUT_CLASSES: readonly TimeoutClass[] = [
  "text",
  "idle",
  "command",
  "exit",
  "ready",
];

/** Resolves a client-side timeout; returns `undefined` so callers omit `timeout_ms` and let the daemon decide. */
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

/** Builds an open/run timeout payload; returns `undefined` when empty and throws on an unrecognised class. */
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
