import { TuiTest } from "../client.js";
import { IS_MACOS, IS_WINDOWS } from "../config.js";
import { uniqueSession } from "../ephemeral.js";
import type {
  ArtifactOptions,
  AutomaticRecording,
  Backend,
  ClientOptions,
  Profile,
  Shell,
  SpawnOptions,
  Timeouts,
} from "../types.js";

export type { Profile, Shell } from "../types.js";
export { TuiTest } from "../client.js";

export interface CreateTerminalOptions {
  backend?: Backend;
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
  profile?: Profile;
  artifacts?: ArtifactOptions;
  recording?: AutomaticRecording;
}

let defaults: Partial<CreateTerminalOptions> = {};

export function setTerminalDefaults(values: Partial<CreateTerminalOptions>): void {
  defaults = { ...defaults, ...values };
}

export function resetTerminalDefaults(): void {
  defaults = {};
}

const tracked = new Set<TuiTest>();
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

export function trackTerminal(terminal: TuiTest): void {
  tracked.add(terminal);
  installSafetyNet();
}

export function untrackTerminal(terminal: TuiTest): void {
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
  for (const key of [
    "backend",
    "timeouts",
    "profile",
    "artifacts",
    "recording",
  ] as const) {
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
): Promise<TuiTest> {
  const opts: CreateTerminalOptions = { ...defaults, ...options };
  const session = opts.session ?? uniqueSession(opts.prefix);
  const terminal = new TuiTest(session, clientOptions(opts));
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
  fn: (terminal: TuiTest) => Promise<T> | T,
): Promise<T> {
  const terminal = await createTerminal(options);
  try {
    return await fn(terminal);
  } catch (error) {
    try {
      await terminal.retainRecording();
    } catch (retentionError) {
      if (error instanceof Error && error.cause === undefined) {
        error.cause = retentionError;
      }
    }
    throw error;
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
