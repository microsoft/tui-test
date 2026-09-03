import type {
  BellEvent as NativeBellEvent,
  Cell as NativeCell,
  Cursor as NativeCursor,
  EffectiveTimeouts as NativeEffectiveTimeouts,
  OpenResult as NativeOpenResult,
  Size as NativeSize,
  State as NativeState,
  TextMatch as NativeTextMatch,
  Timeouts as NativeTimeouts,
} from "../native/index.js";

export type Color = "default" | number | string;
export type Backend = "alacritty" | "ghostty" | "rio" | "xtermjs";

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

export type Cursor = NativeCursor;

export type Size = NativeSize;

export interface Cell extends Omit<NativeCell, "fg" | "bg" | "underline_style" | "underline_color"> {
  /** The cell's grapheme; `" "` when blank, `""` for the second column of a double-width character. */
  fg: Color;
  bg: Color;
  /** Always `false` from the alacritty and rio backends, which cannot report blink. */
  /** Shorthand for `underline_style !== "none"`. */
  underline_style: UnderlineStyle;
  /**
   * `"default"` means the underline follows the text color. Tracked
   * independently of `underline_style`, so a cell that set SGR 58 without an
   * underline still reports the color it would use.
   */
  underline_color: Color;
}

export type EffectiveTimeouts = NativeEffectiveTimeouts;

export type BellEvent = NativeBellEvent;

export type State = NativeState;

export type TextMatch = NativeTextMatch;

export type OpenResult = NativeOpenResult;

export interface Colors {
  foreground?: string;
  background?: string;
  cursor?: string;
  black?: string;
  red?: string;
  green?: string;
  yellow?: string;
  blue?: string;
  magenta?: string;
  cyan?: string;
  white?: string;
  brightBlack?: string;
  brightRed?: string;
  brightGreen?: string;
  brightYellow?: string;
  brightBlue?: string;
  brightMagenta?: string;
  brightCyan?: string;
  brightWhite?: string;
}

export interface Profile {
  scrollback?: number;
  /** Honor the Kitty keyboard protocol. Defaults to true. */
  kittyKeyboard?: boolean;
  colors?: Colors;
}

export interface AutomaticRecording {
  mode?: "disabled" | "on-failure" | "always";
  directory?: string;
}

export interface SpawnOptions {
  backend?: Backend;
  cols?: number;
  rows?: number;
  cwd?: string;
  env?: Record<string, string | number | boolean> | [string, string][];
  waitReady?: boolean;
  restart?: boolean;
  retries?: number;
  profile?: Profile;
  timeouts?: Timeouts;
}

export type Timeouts = NativeTimeouts;

export interface TerminalArtifact {
  text?: string;
  screenshot?: string;
}

export interface ArtifactOptions {
  dir: string;
  onFailure?: "svg" | "text" | "none";
}

export interface ClientOptions {
  backend?: Backend;
  profile?: Profile;
  timeouts?: Timeouts;
  artifacts?: ArtifactOptions;
  recording?: AutomaticRecording;
}
