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

use crate::terminal::cell::EmuCell;

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

    /// The window title a program set with `OSC 0` or `OSC 2`, or `None` when
    /// none is set.
    ///
    /// A program clears the title by sending an empty one, so an empty string
    /// is reported as `None` rather than as a title that happens to be blank.
    /// Callers therefore never have to distinguish the two.
    fn title(&self) -> Option<String>;

    /// Visible screen as rows of cells. Always `rows` entries of `cols` cells.
    fn viewable_rows(&self) -> Vec<Vec<EmuCell>>;

    /// Scrollback history followed by the visible screen.
    fn full_rows(&self) -> Vec<Vec<EmuCell>>;
}
