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
  colors?: Colors;
}

export interface AutomaticRecording {
  directory?: string;
}

export interface TraceOptions {
  mode?: "on" | "off" | "on-failure";
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
  onFailure?: "all" | "html" | "text" | "none";
  includeRecording?: boolean;
}

export interface ClientOptions {
  backend?: Backend;
  profile?: Profile;
  timeouts?: Timeouts;
  screenHistoryLimit?: number;
  artifacts?: ArtifactOptions;
  recording?: AutomaticRecording;
  trace?: TraceOptions;
}

export type FailureReason =
  | "completed"
  | "test_failed"
  | "timed_out"
  | "session_exited"
  | "cancelled"
  | "locator_no_match"
  | "locator_ambiguous"
  | "unexpected_match"
  | "match_not_actionable"
  | "scalar_mismatch"
  | "snapshot_mismatch"
  | "emulator_fault"
  | "internal_failure";

export type LocatorFailureReason =
  | "anchor_not_found"
  | "anchor_ambiguous"
  | "relative_region_no_match"
  | "style_filter_removed_all"
  | "link_filter_removed_all"
  | "intersection_empty"
  | "union_empty"
  | "filter_removed_all"
  | "nth_out_of_range"
  | "outside_viewport"
  | "matched_no_cells"
  | "no_match"
  | "ambiguous";

export type FailureArtifactStatus = "written" | "partial" | "failed";
export interface FailureTextPosition {
  readonly row: number;
  readonly column: number;
}

export interface FailureCellMismatch {
  readonly location: FailureTextPosition;
  readonly grapheme: string;
  readonly property: string;
  readonly operator: string;
  readonly expected: string;
  readonly actual: string;
  readonly resolved?: string;
  readonly reason: string;
}

export interface FailureLocatorDetails {
  readonly reason?: LocatorFailureReason;
  /** Selector descriptions leading to the failing stage. */
  readonly selectors: readonly string[];
  readonly stage_index?: number;
  /** Sample candidate starts in terminal grid coordinates. */
  readonly locations: readonly FailureTextPosition[];
  readonly mismatches: readonly FailureCellMismatch[];
}

export interface FailureDetails {
  readonly schema_version: number;
  readonly operation: string;
  readonly reason: FailureReason;
  readonly summary: string;
  readonly locator?: FailureLocatorDetails;
  readonly comparison?: {
    readonly kind: string;
    readonly expected?: string;
    readonly actual?: string;
  };
  readonly truncated: boolean;
}

export interface FailureArtifactRef {
  readonly status: FailureArtifactStatus;
  readonly directory: string;
  readonly manifest?: string;
  readonly report?: string;
  readonly report_html?: string;
  readonly timeline?: string;
  readonly screen_text?: string;
  readonly screen_svg?: string;
  readonly recording?: string;
  readonly errors?: readonly string[];
}
