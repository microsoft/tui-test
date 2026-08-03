export type Color = "default" | number | string;

export type Shell =
  | "bash"
  | "powershell"
  | "pwsh"
  | "cmd"
  | "fish"
  | "zsh"
  | "xonsh"
  | "elvish"
  | "nushell";

export interface Cursor {
  x: number;
  y: number;
}

export interface Size {
  cols: number;
  rows: number;
}

export interface Cell {
  x: number;
  y: number;
  char: string;
  fg: Color;
  bg: Color;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  inverse: boolean;
}

export interface State {
  session_shell: string | null;
  cols: number;
  rows: number;
  cursor: Cursor;
  cwd: string | null;
  last_command: string | null;
  last_exit: number | null;
  exited: number | null;
  ready: boolean;
  text: string;
}

export interface OpenResult {
  pid: number;
  shell_pid: number | null;
  session: string;
  ready: boolean;
  recording: string;
}

export interface DaemonStatus {
  session: string;
  /** The daemon process, or `null` when no daemon is running. */
  pid: number | null;
  shell_pid?: number | null;
  cols?: number;
  rows?: number;
  shell?: string | null;
  exited?: number | null;
  log: string | null;
}

export interface Response {
  ok: boolean;
  data?: unknown;
  message?: string;
  kind?: string;
}

export interface SpawnOptions {
  cols?: number;
  rows?: number;
  cwd?: string;
  env?: Record<string, string | number | boolean> | [string, string][];
  waitReady?: boolean;
  retries?: number;
  timeouts?: Timeouts;
}

export interface Timeouts {
  text?: number;
  idle?: number;
  command?: number;
  exit?: number;
  ready?: number;
}

export interface TerminalArtifact {
  text?: string;
  screenshot?: string;
}

export interface ArtifactOptions {
  dir: string;
  onFailure?: "svg" | "text" | "none";
}

export interface ClientOptions {
  binary?: string;
  /** Daemon state directory. Ignored when `isolated` is set. */
  home?: string;
  /** Use a private daemon home, created on first use and removed on close. */
  isolated?: boolean;
  timeouts?: Timeouts;
  artifacts?: ArtifactOptions;
}

/** Module-level helper options; no `isolated` because a fresh private home cannot contain an existing daemon. */
export interface HomeOptions {
  binary?: string;
  home?: string;
}
