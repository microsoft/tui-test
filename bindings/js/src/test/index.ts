import { ShellUse } from "../client.js";
import { IS_MACOS, IS_WINDOWS } from "../config.js";
import { uniqueSession } from "../ephemeral.js";
import type {
  ArtifactOptions,
  ClientOptions,
  Shell,
  SpawnOptions,
  Timeouts,
} from "../types.js";

export type { Shell } from "../types.js";
export { ShellUse } from "../client.js";

export interface CreateTerminalOptions {
  shell?: Shell;
  program?: string[];
  cols?: number;
  rows?: number;
  cwd?: string;
  env?: Record<string, string | number | boolean> | [string, string][];
  session?: string;
  prefix?: string;
  retries?: number;
  waitReady?: boolean;
  timeouts?: Timeouts;
  artifacts?: ArtifactOptions;
}

let defaults: Partial<CreateTerminalOptions> = {};

export function setTerminalDefaults(values: Partial<CreateTerminalOptions>): void {
  defaults = { ...defaults, ...values };
}

export function resetTerminalDefaults(): void {
  defaults = {};
}

const tracked = new Set<ShellUse>();
let safetyNetInstalled = false;

function installSafetyNet(): void {
  if (safetyNetInstalled) {
    return;
  }
  safetyNetInstalled = true;
  const proc: NodeJS.Process | undefined =
    typeof process !== "undefined" ? process : undefined;
  proc?.once?.("beforeExit", () => {
    void closeAllTracked();
  });
}

export function trackTerminal(terminal: ShellUse): void {
  tracked.add(terminal);
  installSafetyNet();
}

export function untrackTerminal(terminal: ShellUse): void {
  tracked.delete(terminal);
}

export async function closeAllTracked(): Promise<void> {
  const pending = [...tracked];
  tracked.clear();
  await Promise.all(pending.map((terminal) => terminal.closeQuiet()));
}

export function trackedCount(): number {
  return tracked.size;
}

function clientOptions(opts: CreateTerminalOptions): ClientOptions {
  const client: ClientOptions = {};
  for (const key of ["timeouts", "artifacts"] as const) {
    const value = opts[key];
    if (value !== undefined) {
      Object.assign(client, { [key]: value });
    }
  }
  return client;
}

function spawnOptions(opts: CreateTerminalOptions): SpawnOptions {
  const spawn: SpawnOptions = { retries: opts.retries ?? 2 };
  for (const key of ["cols", "rows", "cwd", "env", "waitReady"] as const) {
    const value = opts[key];
    if (value !== undefined) {
      Object.assign(spawn, { [key]: value });
    }
  }
  return spawn;
}

export async function createTerminal(
  options: CreateTerminalOptions = {},
): Promise<ShellUse> {
  const opts: CreateTerminalOptions = { ...defaults, ...options };
  const session = opts.session ?? uniqueSession(opts.prefix);
  const terminal = new ShellUse(session, clientOptions(opts));
  trackTerminal(terminal);
  const spawn = spawnOptions(opts);
  try {
    if (opts.program && opts.program.length > 0) {
      const [command, ...args] = opts.program;
      await terminal.run(command, args, spawn);
    } else {
      await terminal.open({ shell: opts.shell, ...spawn });
    }
  } catch (error) {
    await terminal.closeQuiet();
    untrackTerminal(terminal);
    throw error;
  }
  return terminal;
}

export async function withTerminal<T>(
  options: CreateTerminalOptions,
  fn: (terminal: ShellUse) => Promise<T> | T,
): Promise<T> {
  const terminal = await createTerminal(options);
  try {
    return await fn(terminal);
  } finally {
    await terminal.closeQuiet();
    untrackTerminal(terminal);
  }
}

export const defaultShell: Shell = IS_WINDOWS ? "powershell" : IS_MACOS ? "zsh" : "bash";

export function terminalSnapshot(text: string): string {
  const lines = text.split("\n").map((line) => line.replace(/\s+$/u, ""));
  while (lines.length > 0 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines.join("\n");
}
