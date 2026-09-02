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
  onFailure?: "bundle" | "svg" | "text" | "json" | "none";
  includeRecording?: boolean;
}

export interface ClientOptions {
  backend?: Backend;
  profile?: Profile;
  timeouts?: Timeouts;
  screenHistoryLimit?: number;
  artifacts?: ArtifactOptions;
  recording?: AutomaticRecording;
}

export type FailureReason =
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
  | "nth_out_of_range"
  | "outside_viewport"
  | "matched_no_cells"
  | "no_match"
  | "ambiguous";

export type FailureArtifactStatus = "written" | "partial" | "failed";
export type FailureMatchOccurrence =
  | "any"
  | "unique"
  | "first"
  | "last"
  | { readonly nth: number };

export interface FailureTextPosition {
  readonly row: number;
  readonly column: number;
}

export interface FailureTextSpan {
  readonly row: number;
  readonly start: number;
  readonly end: number;
}

export interface FailureTextMatch {
  readonly text: string;
  readonly start: FailureTextPosition;
  readonly end: FailureTextPosition;
  readonly spans: readonly FailureTextSpan[];
}

export interface FailureTextStyle {
  readonly foreground?: string;
  readonly background?: string;
  readonly bold?: boolean;
  readonly dim?: boolean;
  readonly italic?: boolean;
  readonly underline_style?: string;
  readonly underline_color?: string;
  readonly inverse?: boolean;
  readonly hidden?: boolean;
  readonly strikethrough?: boolean;
  readonly blink?: boolean;
}

export interface FailureTextAnchor {
  readonly text: string;
  readonly regex: boolean;
  readonly occurrence: FailureMatchOccurrence;
}

export interface FailureTextSelector {
  readonly text: string;
  readonly regex: boolean;
  readonly full: boolean;
  readonly whitespace: "exact" | "normalize";
  readonly scope: {
    readonly after?: FailureTextAnchor | null;
    readonly before?: FailureTextAnchor | null;
  };
}

export interface FailureStyleSelector {
  readonly style: FailureTextStyle;
  readonly full: boolean;
}

export type FailureLocatorSelector =
  | { readonly kind: "text"; readonly selector: FailureTextSelector }
  | { readonly kind: "style"; readonly selector: FailureStyleSelector };

export interface FailureOperationDetails {
  readonly name: string;
  readonly timeout_ms?: number;
  readonly elapsed_ms: number;
  readonly started_screen_sequence: number;
  readonly failed_screen_sequence: number;
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

export interface FailureLocatorStageDetails {
  readonly stage_index: number;
  readonly mode: "text" | "contiguous_style_runs" | "parent_style_filter";
  readonly selector: FailureLocatorSelector;
  readonly direction: "within" | "after" | "before";
  readonly requested_occurrence: FailureMatchOccurrence;
  readonly effective_occurrence: FailureMatchOccurrence;
  readonly occurrence_source: "explicit" | "action_default";
  readonly input_candidate_count: number;
  readonly raw_candidate_count: number;
  readonly style_candidate_count: number;
  readonly selected_count: number;
  readonly candidates?: readonly FailureTextMatch[];
  readonly candidates_truncated: boolean;
  readonly mismatches?: readonly FailureCellMismatch[];
  readonly mismatches_truncated: boolean;
}

export interface FailureLocatorDetails {
  readonly search_scope: string;
  readonly viewport_origin_y: number;
  readonly stages: readonly FailureLocatorStageDetails[];
  readonly final_candidate_count: number;
  readonly selected?: readonly FailureTextMatch[];
  readonly failure_stage?: number;
  readonly failure_reason?: LocatorFailureReason;
}

export interface FailureEvaluationTransition {
  readonly elapsed_ms: number;
  readonly screen_sequence: number;
  readonly outcome: string;
  readonly stage_index?: number;
  readonly stage_counts: readonly number[];
}

export interface FailureOperationEvent {
  readonly sequence: number;
  readonly name: string;
  readonly started_ms: number;
  readonly ended_ms: number;
  readonly result: string;
  readonly screen_before: number;
  readonly screen_at_return: number;
  readonly safe_summary: string;
}

export interface FailureCursorDetails {
  readonly column: number;
  readonly row: number;
  readonly visible: boolean;
  readonly shape: string;
}

export interface FailureScreenSnapshot {
  readonly sequence: number;
  readonly first_seen_ms: number;
  readonly last_seen_ms: number;
  readonly repeat_count: number;
  readonly changes: readonly string[];
  readonly size: Size;
  readonly cursor: FailureCursorDetails;
  readonly title?: string;
  readonly text: string;
}

export interface FailureTerminalDetails {
  readonly size: Size;
  readonly title?: string;
  readonly cursor: FailureCursorDetails;
  readonly last_visual_change_ms: number;
  readonly unchanged_for_ms: number;
  readonly screen_history: {
    readonly limit: number;
    readonly dropped_screen_count: number;
    readonly dropped_row_count: number;
    readonly screens: readonly FailureScreenSnapshot[];
  };
}

export interface FailureProcessDetails {
  readonly pid?: number;
  readonly state: string;
  readonly exit_code?: number;
  readonly status_error?: string;
  readonly cancelled: boolean;
  readonly ready: boolean;
  readonly command_running: boolean;
  readonly last_command_exit?: number;
}

export interface FailureRuntimeDetails {
  readonly tui_test_version: string;
  readonly backend: string;
  readonly target_os: string;
  readonly target_arch: string;
  readonly terminal_profile_fingerprint: string;
}

export interface FailureRecordingDetails {
  readonly mode: "disabled" | "on-failure" | "always";
  readonly status: "disabled" | "unavailable" | "live" | "copied" | "omitted" | "failed";
  readonly failure_offset_ms: number;
  readonly last_committed_ms?: number;
  readonly path?: string;
  readonly bytes?: number;
  readonly reason?: string;
  readonly ephemeral: boolean;
}

export interface FailureDetails {
  readonly schema_version: number;
  readonly signature: string;
  readonly operation: FailureOperationDetails;
  readonly reason: FailureReason;
  readonly summary: string;
  readonly locator?: FailureLocatorDetails;
  readonly comparison?: {
    readonly kind: string;
    readonly expected?: string;
    readonly actual?: string;
  };
  readonly evaluation_transitions?: readonly FailureEvaluationTransition[];
  readonly recent_operations?: readonly FailureOperationEvent[];
  readonly terminal?: FailureTerminalDetails;
  readonly process?: FailureProcessDetails;
  readonly runtime?: FailureRuntimeDetails;
  readonly recording?: FailureRecordingDetails;
  readonly hints?: readonly {
    readonly code: string;
    readonly message: string;
  }[];
  readonly context?: Readonly<Record<string, string>>;
  readonly truncated: boolean;
}

export interface FailureArtifactRef {
  readonly status: FailureArtifactStatus;
  readonly directory: string;
  readonly manifest?: string;
  readonly report?: string;
  readonly screen_text?: string;
  readonly screen_svg?: string;
  readonly recording?: string;
  readonly errors?: readonly string[];
}
