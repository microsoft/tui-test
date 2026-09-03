//! The terminal-emulator seam.
//!
//! tui-test drives a PTY and reads back a cell grid. [`Emulator`] is the
//! entire contract between the two, so an emulator backend can be swapped
//! without touching render, assert, monitor, or the daemon.
//!
//! Command/exit/cwd tracking is deliberately *not* part of this trait: it is
//! derived from the raw PTY byte stream by [`crate::terminal::integration`],
//! which is backend-independent. Keeping it out means every backend reports
//! identical shell-integration behavior by construction rather than by
//! reimplementation.

use alacritty_terminal::vte::{Parser, Perform};

use crate::profile::{ColorSlot, Rgb};
use crate::terminal::cell::{Color, EmuCell};

/// The shape a terminal draws its cursor as, set with `DECSCUSR` (`CSI Ps SP q`).
///
/// The specification defines three, each in a blinking and a steady form. The
/// blink is not represented: a screenshot is a single moment, and a blinking
/// cursor is drawn in the half of that cycle where it is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

bitflags::bitflags! {
    /// Kitty keyboard protocol flags currently requested by the child.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct KeyboardMode: u8 {
        const DISAMBIGUATE_ESC_CODES = 1;
        const REPORT_EVENT_TYPES = 1 << 1;
        const REPORT_ALTERNATE_KEYS = 1 << 2;
        const REPORT_ALL_KEYS_AS_ESC = 1 << 3;
        const REPORT_ASSOCIATED_TEXT = 1 << 4;
    }
}

/// Clipboard target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardType {
    Clipboard,
    Selection,
}

/// The session-local clipboard state shared with a backend's event listener.
#[derive(Debug, Default)]
pub(crate) struct Clipboard {
    clipboard: String,
    selection: String,
    clipboard_revision: u64,
    selection_revision: u64,
}

impl Clipboard {
    pub(crate) fn get(&self, clipboard: ClipboardType) -> &str {
        match clipboard {
            ClipboardType::Clipboard => &self.clipboard,
            ClipboardType::Selection => &self.selection,
        }
    }

    pub(crate) fn set(&mut self, clipboard: ClipboardType, text: String) {
        match clipboard {
            ClipboardType::Clipboard if self.clipboard != text => {
                self.clipboard = text;
                self.clipboard_revision = self.clipboard_revision.wrapping_add(1);
            }
            ClipboardType::Selection if self.selection != text => {
                self.selection = text;
                self.selection_revision = self.selection_revision.wrapping_add(1);
            }
            _ => {}
        }
    }

    pub(crate) fn revision(&self, clipboard: ClipboardType) -> u64 {
        match clipboard {
            ClipboardType::Clipboard => self.clipboard_revision,
            ClipboardType::Selection => self.selection_revision,
        }
    }
}

#[derive(Default)]
struct ClipboardValidation {
    fault: Option<String>,
    unsupported: bool,
}

impl Perform for ClipboardValidation {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.first().copied() != Some(b"52") {
            return;
        }
        if self.unsupported {
            if self.fault.is_none() {
                self.fault = Some("clipboard access is unavailable".to_string());
            }
            return;
        }
        let selection = params.get(1).copied().unwrap_or_default();
        if !matches!(selection, b"c" | b"p" | b"s") && self.fault.is_none() {
            self.fault = Some(format!(
                "clipboard selection {:?} is unavailable",
                String::from_utf8_lossy(selection)
            ));
        }
    }
}

/// Tracks unsupported OSC 52 destinations across arbitrary PTY read splits.
pub(crate) struct ClipboardValidator {
    parser: Parser,
    state: ClipboardValidation,
}

impl ClipboardValidator {
    pub(crate) fn new() -> Self {
        Self {
            parser: Parser::new(),
            state: ClipboardValidation::default(),
        }
    }

    #[cfg(feature = "xtermjs")]
    pub(crate) fn unsupported() -> Self {
        Self {
            parser: Parser::new(),
            state: ClipboardValidation {
                unsupported: true,
                ..Default::default()
            },
        }
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    pub(crate) fn fault(&self) -> Option<String> {
        self.state.fault.clone()
    }
}

/// A headless terminal emulator: bytes in, cell grid out.
///
/// Implementations must be `Send`; the daemon shares the emulator across its
/// reader, request, and monitor threads behind a mutex. A backend whose native
/// handle is `!Send` is expected to confine that handle to its own thread and
/// implement this trait on a `Send` handle.
pub trait Emulator: Send {
    /// Feed PTY output bytes into the emulator.
    fn process(&mut self, bytes: &[u8]);

    /// A failure that left the grid no longer a faithful account of the bytes
    /// fed to it, if one has happened.
    ///
    /// Backends that parse in-process cannot fail this way and never report
    /// one. A backend driving a separate engine can, and the grid it hands
    /// back afterwards is a guess rather than an answer, so callers surface
    /// this instead of reading on.
    fn fault(&self) -> Option<String> {
        None
    }

    /// Drain bytes the emulator wants written back to the PTY (device
    /// attribute replies, cursor position reports, and similar). The caller
    /// forwards these to the PTY.
    fn take_pending_writes(&mut self) -> Vec<u8>;

    /// Read a clipboard value.
    fn clipboard(&self, _clipboard: ClipboardType) -> anyhow::Result<String> {
        anyhow::bail!("clipboard access is unavailable")
    }

    #[doc(hidden)]
    fn clipboard_revision(&self, _clipboard: ClipboardType) -> anyhow::Result<u64> {
        anyhow::bail!("clipboard access is unavailable")
    }

    /// Active Kitty keyboard protocol flags negotiated by the child.
    fn keyboard_mode(&self) -> KeyboardMode {
        KeyboardMode::empty()
    }

    /// Encode one key event with the backend's own key encoder.
    ///
    /// `None` means the backend has no encoder, or has one that cannot express
    /// this event, and the caller falls back to [`crate::input::keys`]. Only
    /// ghostty ships an encoder: alacritty and rio keep theirs in their GUI
    /// crates rather than their VT libraries, and the xterm.js headless bundle
    /// omits `evaluateKeyboardEvent` entirely.
    ///
    /// Preferring the backend matters because encoding depends on more terminal
    /// state than the shared encoder models, and a backend's own encoder reads
    /// that state directly. Ghostty's, for one, applies keypad application
    /// mode, `modifyOtherKeys`, and the alt-escape prefix, none of which are
    /// visible through this trait.
    ///
    /// An empty `Vec` is a real answer, not an absence: some events encode to
    /// nothing at all, such as a bare modifier press.
    fn encode_key(&self, _press: &crate::input::keys::KeyPress) -> Option<Vec<u8>> {
        None
    }

    /// Whether the cursor keys are in application mode (`DECCKM`, `CSI ?1h`).
    ///
    /// A child in this mode expects `SS3 A` from the up arrow rather than
    /// `CSI A`, and readline, vim, and less all turn it on. It is part of this
    /// trait for the same reason [`Emulator::keyboard_mode`] is: key encoding
    /// is shared, so anything it has to branch on has to be readable here.
    fn cursor_key_application(&self) -> bool {
        false
    }

    fn resize(&mut self, cols: u16, rows: u16);

    /// Current grid size as `(cols, rows)`.
    fn size(&self) -> (u16, u16);

    /// Cursor position as `(x, y)` (column, row), 0-based, clamped to screen.
    ///
    /// Always relative to the visible screen, never to the scrollback, so a
    /// caller drawing over `full_rows` has to offset it by the history above.
    fn cursor(&self) -> (u16, u16);

    /// The window title a program set with `OSC 0` or `OSC 2`, or `None` when
    /// none is set.
    ///
    /// A program clears the title by sending an empty one, so an empty string
    /// is reported as `None` rather than as a title that happens to be blank.
    /// Callers therefore never have to distinguish the two.
    fn title(&self) -> Option<String>;

    /// Whether the cursor is being drawn, which programs toggle with
    /// `DECTCEM` (`CSI ?25 h` and `l`). Full-screen programs routinely hide it
    /// while repainting, so a screenshot that ignored this would show a cursor
    /// parked wherever the last write happened to leave it.
    fn cursor_visible(&self) -> bool;

    /// The shape the cursor is currently drawn as.
    fn cursor_shape(&self) -> CursorShape;

    /// Visible screen as rows of cells. Always `rows` entries of `cols` cells.
    fn viewable_rows(&self) -> Vec<Vec<EmuCell>>;

    /// Scrollback history followed by the visible screen.
    fn full_rows(&self) -> Vec<Vec<EmuCell>>;

    /// The color a slot is currently showing.
    ///
    /// Programs move these with `OSC 4` (palette) and `OSC 10/11/12` (default
    /// foreground, background, cursor), and put them back with `OSC 104` and
    /// `OSC 110/111/112`. A slot nothing has overridden shows the color the
    /// session's profile gives it, so a reset always has something to restore
    /// and this always has an answer.
    ///
    /// Backends answer color *queries* themselves, through
    /// [`Emulator::take_pending_writes`], because each one already parses the
    /// sequence and knows which terminator the query used. This reports the
    /// same colors, so a screenshot and `expect --fg/--bg` agree with what a
    /// program was told.
    ///
    fn color(&self, slot: ColorSlot) -> Rgb;

    /// Resolve a cell's color, where `None` is the terminal default.
    ///
    /// The grid records which slot a cell chose, never a color, so this is
    /// where a cell becomes something to paint or compare. Provided rather
    /// than required so every backend resolves a cell identically.
    fn resolve(&self, color: Option<Color>, is_fg: bool) -> Rgb {
        match color {
            None => self.color(if is_fg {
                ColorSlot::Foreground
            } else {
                ColorSlot::Background
            }),
            Some(Color::Named(n)) => self.color(ColorSlot::Indexed(n.index())),
            Some(Color::Idx(i)) => self.color(ColorSlot::Indexed(i)),
            Some(Color::Rgb(r, g, b)) => Rgb::new(r, g, b),
        }
    }
}
