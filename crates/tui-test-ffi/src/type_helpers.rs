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
pub struct TuiQuery {
    pub json: TuiString,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TuiMode {
    pub name: TuiString,
    pub enabled: bool,
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
/// The function called determines the populated success field. error_kind is
/// 0 on success, 1 assertion, 2 usage, 3 no-session, 5 internal.
/// snapshot: 0 passed, 1 written, 2 updated. Free exactly once.
#[repr(C)]
#[derive(Default)]
pub struct TuiResult {
    pub modes: *const TuiMode,
    pub modes_len: usize,
    pub colors: TuiTerminalColors,
    pub error_kind: u32,
    pub error_message: TuiString,
    pub error_details: TuiString,
    pub error_artifact: TuiString,
    pub text: TuiString,
    pub number: u64,
    pub exit_code: TuiOptionalI32,
    pub open: TuiOpenResult,
    pub state: TuiState,
    pub cursor: TuiCursor,
    pub size: TuiSize,
    pub viewport: TuiViewport,
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
