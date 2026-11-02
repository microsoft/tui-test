//! Private, versioned C ABI. Allocation and destruction stay in the native library.

use std::ffi::{c_char, c_int, c_void};

pub(super) const ABI_VERSION: u32 = 1;
pub(super) const COLOR_COUNT: usize = 259;

#[repr(C)]
pub(super) struct Terminal {
    _private: [u8; 0],
}

#[repr(C)]
pub(super) struct Error {
    pub message: [c_char; 1024],
}

impl Default for Error {
    fn default() -> Self {
        Self { message: [0; 1024] }
    }
}

impl Error {
    pub fn check(&self, status: c_int) -> anyhow::Result<()> {
        anyhow::ensure!(
            status == 0,
            "Windows Terminal: {}",
            String::from_utf8_lossy(
                &self
                    .message
                    .iter()
                    .take_while(|&&byte| byte != 0)
                    .map(|&byte| byte as u8)
                    .collect::<Vec<_>>()
            )
        );
        Ok(())
    }
}

#[repr(C)]
pub(super) struct Config {
    pub abi_version: u32,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: u32,
    pub colors: [u32; COLOR_COUNT],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Span {
    pub offset: usize,
    pub length: usize,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct State {
    pub cols: u32,
    pub rows: u32,
    pub history: u32,
    pub offset: u32,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub cursor_shape: u32,
    pub modes: u32,
    pub kitty_flags: u32,
    pub colors: [u32; COLOR_COUNT],
}

#[repr(C)]
pub(super) struct Cell {
    pub text: Span,
    pub uri: Span,
    pub link_id: Span,
    pub foreground: u32,
    pub background: u32,
    pub underline_color: u32,
    pub attrs: u32,
    pub underline_style: u32,
}

#[repr(C)]
pub(super) struct Snapshot {
    pub abi_version: u32,
    pub state: State,
    pub cells: *const Cell,
    pub cell_count: usize,
    pub strings: *const u16,
    pub string_count: usize,
    pub title: Span,
    pub clipboard: Span,
    pub clipboard_revision: u64,
    pub owner: *mut c_void,
}

#[repr(C)]
pub(super) struct Output {
    pub text: *const u16,
    pub length: usize,
    pub owner: *mut c_void,
}

unsafe extern "C" {
    pub fn wt_create(
        config: *const Config,
        terminal: *mut *mut Terminal,
        error: *mut Error,
    ) -> c_int;
    pub fn wt_destroy(terminal: *mut Terminal);
    pub fn wt_write(
        terminal: *mut Terminal,
        text: *const u16,
        length: usize,
        bells: *mut u64,
        error: *mut Error,
    ) -> c_int;
    pub fn wt_resize(terminal: *mut Terminal, cols: u16, rows: u16, error: *mut Error) -> c_int;
    pub fn wt_scroll(terminal: *mut Terminal, offset: u32, error: *mut Error) -> c_int;
    pub fn wt_read(
        terminal: *mut Terminal,
        rows: u32,
        snapshot: *mut *mut Snapshot,
        error: *mut Error,
    ) -> c_int;
    pub fn wt_snapshot_free(snapshot: *mut Snapshot);
    pub fn wt_take_output(
        terminal: *mut Terminal,
        output: *mut *mut Output,
        error: *mut Error,
    ) -> c_int;
    pub fn wt_output_free(output: *mut Output);
}
