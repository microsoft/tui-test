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

use crate::profile::{Palette, Rgb};
use crate::terminal::cell::EmuCell;

/// Runtime color slots: the 256-color palette, then the three dynamic colors.
///
/// The numbering is not ours — both emulators already address their special
/// colors this way, so a backend can hand its own table straight through.
pub const FOREGROUND: usize = 256;
pub const BACKGROUND: usize = 257;
pub const CURSOR: usize = 258;
pub const COLOR_SLOTS: usize = 259;

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

    /// The color a slot currently shows.
    ///
    /// Programs move these with `OSC 4` (palette) and `OSC 10/11/12` (default
    /// foreground, background, cursor), and put them back with `OSC 104` and
    /// `OSC 110/111/112`. A reset restores the color the session was configured
    /// with; nothing a program sends can change that configured value, so
    /// there is always something to fall back to.
    ///
    /// Backends answer color *queries* themselves, through
    /// [`Emulator::take_pending_writes`], because each one already parses the
    /// sequence and knows which terminator the query used. This method is how
    /// the screenshot renderer and `expect --fg/--bg` see the same answer.
    ///
    /// `slot` is a palette index, or one of [`FOREGROUND`], [`BACKGROUND`],
    /// [`CURSOR`].
    fn color(&self, slot: usize) -> Rgb;

    /// Every slot at once, so a consumer can resolve colors without holding
    /// the session lock or knowing which backend produced them.
    fn palette(&self) -> Palette {
        let mut slots = [Rgb::new(0, 0, 0); COLOR_SLOTS];
        for (slot, out) in slots.iter_mut().enumerate() {
            *out = self.color(slot);
        }
        Palette::new(slots)
    }
}
