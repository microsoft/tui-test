use std::ffi::c_void;

/// A borrowed UTF-8 byte string. NULL denotes absence; a non-NULL pointer
/// with zero length denotes an explicitly empty string. Inputs live through
/// the call only. All output pointers live until tui_result_free.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiString {
    pub data: *const u8,
    pub len: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOptionalU64 {
    pub present: bool,
    pub value: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOptionalI32 {
    pub present: bool,
    pub value: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOptionalBool {
    pub present: bool,
    pub value: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOptionalF64 {
    pub present: bool,
    pub value: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiPair {
    pub key: TuiString,
    pub value: TuiString,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiTimeouts {
    pub text: TuiOptionalU64,
    pub idle: TuiOptionalU64,
    pub command: TuiOptionalU64,
    pub exit: TuiOptionalU64,
    pub ready: TuiOptionalU64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOpenOptions {
    pub backend: TuiString,
    pub shell: TuiString,
    pub cols: TuiOptionalU64,
    pub rows: TuiOptionalU64,
    pub cwd: TuiString,
    pub env: *const TuiPair,
    pub env_len: usize,
    pub wait_ready: TuiOptionalBool,
    pub restart: bool,
    pub scrollback: TuiOptionalU64,
    pub colors: *const TuiPair,
    pub colors_len: usize,
    pub timeouts: TuiTimeouts,
    pub recording_mode: TuiString,
    pub recording_directory: TuiString,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiTextStyle {
    pub foreground: TuiString,
    pub background: TuiString,
    pub bold: TuiOptionalBool,
    pub dim: TuiOptionalBool,
    pub italic: TuiOptionalBool,
    pub underline_style: TuiString,
    pub underline_color: TuiString,
    pub inverse: TuiOptionalBool,
    pub hidden: TuiOptionalBool,
    pub strikethrough: TuiOptionalBool,
    pub blink: TuiOptionalBool,
}
/// occurrence: 0 any, 1 unique, 2 first, 3 last, 4 nth.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiAnchor {
    pub text: TuiString,
    pub regex: bool,
    pub occurrence: u32,
    pub index: usize,
}
/// kind: 0 text, 1 style. direction: 0 within, 1 after, 2 before.
/// whitespace: 0 exact, 1 normalize. Stages are ordered parent first.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiLocatorStage {
    pub kind: u32,
    pub text: TuiString,
    pub regex: bool,
    pub full: bool,
    pub whitespace: u32,
    pub after: TuiAnchor,
    pub before: TuiAnchor,
    pub style: TuiTextStyle,
    pub occurrence: u32,
    pub index: usize,
    pub direction: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiQuery {
    pub stages: *const TuiLocatorStage,
    pub len: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiWaitOptions {
    pub timeout_ms: TuiOptionalU64,
    pub regex: bool,
    pub not: bool,
}
/// button: 0 left, 1 middle, 2 right.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiMouseOptions {
    pub button: u32,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiRecordingOptions {
    pub path: TuiString,
    pub format: TuiString,
    pub fps: TuiOptionalU64,
    pub speed: TuiOptionalF64,
    pub idle_time_limit: TuiOptionalF64,
    pub zoom: TuiOptionalF64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiCursor {
    pub x: u16,
    pub y: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiSize {
    pub cols: u16,
    pub rows: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiOpenResult {
    pub shell_pid: TuiOptionalU64,
    pub session: TuiString,
    pub ready: bool,
    pub recording: TuiString,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiState {
    pub session_shell: TuiString,
    pub cols: u16,
    pub rows: u16,
    pub cursor: TuiCursor,
    pub title: TuiString,
    pub cwd: TuiString,
    pub last_command: TuiString,
    pub last_exit: TuiOptionalI32,
    pub exited: TuiOptionalI32,
    pub ready: bool,
    pub bell_count: u64,
    pub timeouts: TuiTimeouts,
    pub text: TuiString,
}
/// kind: 0 default, 1 indexed (index), 2 RGB (red/green/blue).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiColor {
    pub kind: u32,
    pub index: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiCell {
    pub x: u16,
    pub y: u16,
    pub character: TuiString,
    pub fg: TuiColor,
    pub bg: TuiColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strike: bool,
    pub blink: bool,
    pub underline: bool,
    pub underline_style: TuiString,
    pub underline_color: TuiColor,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiPosition {
    pub row: u32,
    pub column: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiSpan {
    pub row: u32,
    pub start: u16,
    pub end: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiMatch {
    pub text: TuiString,
    pub start: TuiPosition,
    pub end: TuiPosition,
    pub spans: *const TuiSpan,
    pub spans_len: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiBellEvent {
    pub sequence: u64,
    pub elapsed_ms: u64,
}
/// The function called determines the populated success field. error_kind is
/// 0 on success, 1 assertion, 2 usage, 3 no-session, 5 internal.
/// snapshot: 0 passed, 1 written, 2 updated. Free exactly once.
#[repr(C)]
#[derive(Default)]
pub struct TuiResult {
    pub error_kind: u32,
    pub error_message: TuiString,
    pub text: TuiString,
    pub number: u64,
    pub exit_code: TuiOptionalI32,
    pub open: TuiOpenResult,
    pub state: TuiState,
    pub cursor: TuiCursor,
    pub size: TuiSize,
    pub cells: *const TuiCell,
    pub cells_len: usize,
    pub matches: *const TuiMatch,
    pub matches_len: usize,
    pub bells: *const TuiBellEvent,
    pub bells_len: usize,
    pub strings: *const TuiString,
    pub strings_len: usize,
    pub snapshot: u32,
    pub private_data: *mut c_void,
}
