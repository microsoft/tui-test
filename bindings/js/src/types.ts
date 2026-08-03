export type Color = "default" | number | string;

/** `"none"` is a value, not an absence: an un-underlined cell reports it. */
export type UnderlineStyle =
  | "none"
  | "single"
  | "double"
  | "curly"
  | "dotted"
  | "dashed";

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
  /** The cell's grapheme; `" "` when blank, `""` for the second column of a double-width character. */
  char: string;
  fg: Color;
  bg: Color;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  inverse: boolean;
  invisible: boolean;
  strike: boolean;
  /** Always `false` from the alacritty backend, which cannot report blink. */
  blink: boolean;
  /** Shorthand for `underline_style !== "none"`. */
  underline: boolean;
  underline_style: UnderlineStyle;
  /**
   * `"default"` means the underline follows the text color. Tracked
   * independently of `underline_style`, so a cell that set SGR 58 without an
   * underline still reports the color it would use.
   */
  underline_color: Color;
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
  session: string;
  recording: string;
}

export interface DaemonStatus {
  session: string;
  pid: number | null;
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
  env?: Record<string, string> | [string, string][];
}

export interface ClientOptions {
  binary?: string;
  home?: string;
}
