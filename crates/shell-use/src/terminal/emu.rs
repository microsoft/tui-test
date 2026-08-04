//! The terminal-emulator seam.
//!
//! shell-use drives a PTY and reads back a cell grid. [`Emulator`] is the
//! entire contract between the two, so an emulator backend can be swapped
//! without touching render, assert, monitor, or the daemon.
//!
//! Command/exit/cwd tracking is deliberately *not* part of this trait: it is
//! derived from the raw PTY byte stream by [`crate::terminal::integration`],
//! which is backend-independent. Keeping it out means every backend reports
//! identical shell-integration behavior by construction rather than by
//! reimplementation.

use crate::profile::Rgb;
use crate::terminal::cell::{Color, EmuCell};

/// A headless terminal emulator: bytes in, cell grid out.
///
/// Implementations must be `Send`; the daemon shares the emulator across its
/// reader, request, and monitor threads behind a mutex. A backend whose native
/// handle is `!Send` is expected to confine that handle to its own thread and
/// implement this trait on a `Send` handle.
pub trait Emulator: Send {
    /// Feed PTY output bytes into the emulator.
    fn process(&mut self, bytes: &[u8]);

    /// Drain bytes the emulator wants written back to the PTY (device
    /// attribute replies, cursor position reports, and similar). The caller
    /// forwards these to the PTY.
    fn take_pending_writes(&mut self) -> Vec<u8>;

    fn resize(&mut self, cols: u16, rows: u16);

    /// Current grid size as `(cols, rows)`.
    fn size(&self) -> (u16, u16);

    /// Cursor position as `(x, y)` (column, row), 0-based, clamped to screen.
    fn cursor(&self) -> (u16, u16);

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
    /// `slot` is a palette index, or one of [`crate::profile::FOREGROUND`],
    /// [`crate::profile::BACKGROUND`], [`crate::profile::CURSOR`].
    fn color(&self, slot: usize) -> Rgb;

    /// Resolve a cell's color, where `None` is the terminal default.
    ///
    /// The grid records which slot a cell chose, never a color, so this is
    /// where a cell becomes something to paint or compare. Provided rather
    /// than required so every backend resolves a cell identically.
    fn resolve(&self, color: Option<Color>, is_fg: bool) -> Rgb {
        match color {
            None => self.color(if is_fg {
                crate::profile::FOREGROUND
            } else {
                crate::profile::BACKGROUND
            }),
            Some(Color::Named(n)) => self.color(n.index() as usize),
            Some(Color::Idx(i)) => self.color(i as usize),
            Some(Color::Rgb(r, g, b)) => Rgb::new(r, g, b),
        }
    }
}
