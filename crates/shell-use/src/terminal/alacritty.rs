//! [`Emulator`] backend built on `alacritty_terminal`. Translates the
//! alacritty grid into shell-use's neutral cell vocabulary, plus a capture
//! proxy that queues the terminal's replies so the reader can forward them
//! back to the PTY.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as AlacFlags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as AlacConfig, Term};
use alacritty_terminal::vte::ansi;

use compact_str::{CompactString, ToCompactString};

use crate::event::BellTracker;
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::Emulator;

/// Alacritty's palette colors arrive either as a `Named` variant or an index;
/// both funnel through [`Color::from_index`] so a given slot always yields the
/// same value. `Foreground`/`Background` are the terminal defaults, and every
/// other `NamedColor` (`Cursor`, the `Dim*` and `Bright*` aliases) is a UI role
/// with no cell representation, so it falls back to the default too.
fn color_from_alac(c: ansi::Color) -> Option<Color> {
    match c {
        ansi::Color::Named(named) => match named {
            ansi::NamedColor::Black
            | ansi::NamedColor::Red
            | ansi::NamedColor::Green
            | ansi::NamedColor::Yellow
            | ansi::NamedColor::Blue
            | ansi::NamedColor::Magenta
            | ansi::NamedColor::Cyan
            | ansi::NamedColor::White
            | ansi::NamedColor::BrightBlack
            | ansi::NamedColor::BrightRed
            | ansi::NamedColor::BrightGreen
            | ansi::NamedColor::BrightYellow
            | ansi::NamedColor::BrightBlue
            | ansi::NamedColor::BrightMagenta
            | ansi::NamedColor::BrightCyan
            | ansi::NamedColor::BrightWhite => Some(Color::from_index(named as u8)),
            _ => None,
        },
        ansi::Color::Spec(rgb) => Some(Color::Rgb(rgb.r, rgb.g, rgb.b)),
        ansi::Color::Indexed(i) => Some(Color::from_index(i)),
    }
}

fn underline_from_alac(c: &alacritty_terminal::term::cell::Cell) -> UnderlineStyle {
    let flags = c.flags;
    // Ordered widest-to-narrowest: alacritty clears the other underline bits on
    // each SGR, so at most one is ever set.
    if flags.contains(AlacFlags::DOUBLE_UNDERLINE) {
        UnderlineStyle::Double
    } else if flags.contains(AlacFlags::UNDERCURL) {
        UnderlineStyle::Curly
    } else if flags.contains(AlacFlags::DOTTED_UNDERLINE) {
        UnderlineStyle::Dotted
    } else if flags.contains(AlacFlags::DASHED_UNDERLINE) {
        UnderlineStyle::Dashed
    } else if flags.contains(AlacFlags::UNDERLINE) {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    }
}

fn cell_from_alac(c: &alacritty_terminal::term::cell::Cell) -> EmuCell {
    let flags = c.flags;
    // Only WIDE_CHAR_SPACER is a continuation: it is the second column of a
    // wide char on this row. LEADING_WIDE_CHAR_SPACER is the opposite, a filler
    // in the last column when a wide char did not fit and wrapped to the next
    // row, so it owns its column and has to render as a blank.
    let ch = if flags.contains(AlacFlags::WIDE_CHAR_SPACER) {
        CompactString::const_new(CONTINUATION)
    } else if flags.contains(AlacFlags::LEADING_WIDE_CHAR_SPACER) {
        CompactString::const_new(" ")
    } else {
        let mut s = c.c.to_compact_string();
        for zw in c.zerowidth().unwrap_or(&[]) {
            s.push(*zw);
        }
        s
    };

    let mut attrs = Attrs::empty();
    for (flag, attr) in [
        (AlacFlags::BOLD, Attrs::BOLD),
        (AlacFlags::DIM, Attrs::DIM),
        (AlacFlags::ITALIC, Attrs::ITALIC),
        (AlacFlags::INVERSE, Attrs::INVERSE),
        (AlacFlags::HIDDEN, Attrs::INVISIBLE),
        (AlacFlags::STRIKEOUT, Attrs::STRIKE),
    ] {
        attrs.set(attr, flags.contains(flag));
    }
    // `Attrs::BLINK` stays clear: alacritty_terminal parses SGR 5/6/25 and then
    // discards them, its `Flags` has no blink bit, so this backend has nothing
    // to read. The attribute is still part of the vocabulary because ghostty
    // (`Style.Flags.blink`) and xterm.js (`FgFlags.BLINK`) both track it.

    EmuCell {
        ch,
        fg: color_from_alac(c.fg),
        bg: color_from_alac(c.bg),
        underline: underline_from_alac(c),
        underline_color: c.underline_color().and_then(color_from_alac),
        attrs,
    }
}

#[derive(Default, Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<u8>>>,
    bells: BellTracker,
}

impl EventListener for CaptureProxy {
    fn send_event(&self, ev: Event) {
        match ev {
            Event::PtyWrite(bytes) => {
                if let Ok(mut buf) = self.pending.lock() {
                    buf.extend_from_slice(bytes.as_bytes());
                }
            }
            Event::Bell => self.bells.ring(),
            _ => {}
        }
    }
}

pub struct AlacrittyEmu {
    term: Term<CaptureProxy>,
    processor: ansi::Processor,
    cols: u16,
    rows: u16,
    pending: Arc<Mutex<Vec<u8>>>,
}

impl AlacrittyEmu {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        Self::with_bell_tracker(cols, rows, scrollback, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        scrollback: usize,
        bells: BellTracker,
    ) -> Self {
        let size = TermSize::new(cols as usize, rows as usize);
        let config = AlacConfig {
            scrolling_history: scrollback,
            ..Default::default()
        };
        let pending: Arc<Mutex<Vec<u8>>> = Arc::default();
        let proxy = CaptureProxy {
            pending: pending.clone(),
            bells,
        };
        AlacrittyEmu {
            term: Term::new(config, &size, proxy),
            processor: ansi::Processor::new(),
            cols,
            rows,
            pending,
        }
    }

    fn rows_in_range(&self, start: i32, end: i32) -> Vec<Vec<EmuCell>> {
        let grid = self.term.grid();
        let mut out = Vec::with_capacity((end - start).max(0) as usize);
        for line in start..end {
            let mut row = Vec::with_capacity(self.cols as usize);
            for col in 0..self.cols as usize {
                let cell = &grid[Line(line)][Column(col)];
                row.push(cell_from_alac(cell));
            }
            out.push(row);
        }
        out
    }
}

impl Emulator for AlacrittyEmu {
    fn process(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        match self.pending.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => Vec::new(),
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term
            .resize(TermSize::new(cols as usize, rows as usize));
        self.cols = cols;
        self.rows = rows;
    }

    fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn cursor(&self) -> (u16, u16) {
        let p = self.term.grid().cursor.point;
        let y = p.line.0.max(0).min(self.rows as i32 - 1) as u16;
        let x = (p.column.0 as u16).min(self.cols.saturating_sub(1));
        (x, y)
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(0, self.rows as i32)
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        let grid = self.term.grid();
        let total = grid.total_lines() as i32;
        let screen = grid.screen_lines() as i32;
        let history = (total - screen).max(0);
        self.rows_in_range(-history, screen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::emulator_conformance_tests!(|c, r, s| Box::new(AlacrittyEmu::new(c, r, s)));

    #[test]
    fn multiple_bells_in_one_chunk_are_counted_individually() {
        let bells = BellTracker::default();
        let mut emulator = AlacrittyEmu::with_bell_tracker(80, 24, 100, bells.clone());

        emulator.process(b"\x07\x07");

        assert_eq!(bells.count(), 2);
        assert_eq!(bells.sequence(), 2);
    }

    #[test]
    fn an_osc_bell_terminator_does_not_ring_the_terminal_bell() {
        let bells = BellTracker::default();
        let mut emulator = AlacrittyEmu::with_bell_tracker(80, 24, 100, bells.clone());

        emulator.process(b"\x1b]0;window title\x07");

        assert_eq!(bells.count(), 0);
    }
}
