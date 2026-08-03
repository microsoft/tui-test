import { makeError } from "./errors.js";
import type { Response } from "./types.js";

export function unwrap(resp: Response): unknown {
  if (resp.ok) {
    return resp.data;
  }
  throw makeError(resp.kind, resp.message || "shell-use error");
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
