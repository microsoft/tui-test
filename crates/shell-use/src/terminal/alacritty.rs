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
use alacritty_terminal::vte::ansi::NamedColor;
use alacritty_terminal::vte::ansi::Rgb as AlacRgb;

use compact_str::{CompactString, ToCompactString};

use crate::profile::{xterm_color, ColorSlot, Profile, Rgb};
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

/// Formats a color query's reply once the color is known. alacritty builds
/// this closure with the query's own prefix and terminator already captured,
/// so the answer echoes the form the program asked in.
type ReplyFormat = Arc<dyn Fn(AlacRgb) -> String + Send + Sync>;

/// Queues what the terminal wants to say back to the PTY.
///
/// Color queries cannot be answered here: the color lives in the terminal's
/// own palette, and this listener is constructed before the terminal it
/// listens to. They are parked instead, and [`AlacrittyEmu::answer_queries`]
/// resolves them once `process` returns, where the terminal is in scope.
#[derive(Default, Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<u8>>>,
    queries: Arc<Mutex<Vec<(usize, ReplyFormat)>>>,
}

impl EventListener for CaptureProxy {
    fn send_event(&self, ev: Event) {
        match ev {
            Event::PtyWrite(bytes) => {
                if let Ok(mut buf) = self.pending.lock() {
                    buf.extend_from_slice(bytes.as_bytes());
                }
            }
            Event::ColorRequest(slot, format) => {
                if let Ok(mut queries) = self.queries.lock() {
                    queries.push((slot, format));
                }
            }
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
    queries: Arc<Mutex<Vec<(usize, ReplyFormat)>>>,
    /// The settings this session was opened with. A program can shadow the
    /// colors at runtime but never reach them, so a reset always has a value
    /// to restore.
    profile: Profile,
}

impl AlacrittyEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Self {
        let size = TermSize::new(cols as usize, rows as usize);
        let alac_config = AlacConfig {
            scrolling_history: profile.scrollback,
            ..Default::default()
        };
        let pending: Arc<Mutex<Vec<u8>>> = Arc::default();
        let queries: Arc<Mutex<Vec<(usize, ReplyFormat)>>> = Arc::default();
        let proxy = CaptureProxy {
            pending: pending.clone(),
            queries: queries.clone(),
        };
        AlacrittyEmu {
            term: Term::new(alac_config, &size, proxy),
            processor: ansi::Processor::new(),
            cols,
            rows,
            pending,
            queries,
            profile: *profile,
        }
    }

    /// Answer any color queries parked while the last chunk was parsed.
    ///
    fn answer_queries(&mut self) {
        let parked: Vec<(usize, ReplyFormat)> = match self.queries.lock() {
            Ok(mut queries) => queries.drain(..).collect(),
            Err(_) => return,
        };
        if parked.is_empty() {
            return;
        }
        let replies: String = parked
            .into_iter()
            .map(|(index, format)| {
                let slot = match index {
                    i if i == NamedColor::Foreground as usize => ColorSlot::Foreground,
                    i if i == NamedColor::Background as usize => ColorSlot::Background,
                    i if i == NamedColor::Cursor as usize => ColorSlot::Cursor,
                    i => ColorSlot::Indexed(i as u8),
                };
                let c = self.color(slot);
                format(AlacRgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                })
            })
            .collect();
        if let Ok(mut buf) = self.pending.lock() {
            buf.extend_from_slice(replies.as_bytes());
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
        // Queries are answered here rather than in the listener because the
        // terminal holding the palette is only in scope once parsing is done.
        self.answer_queries();
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

    /// alacritty stores only what a program set, leaving every other slot
    /// empty, so an empty slot falls through to the profile, and then to the
    /// xterm table for an index the profile does not name.
    ///
    /// The indices are alacritty's own: it lays its table out as the
    /// 256-color palette followed by the dynamic colors, which is why the
    /// three are 256, 257, 258. That layout stops here.
    fn color(&self, slot: ColorSlot) -> Rgb {
        let colors = &self.profile.colors;
        let (index, configured) = match slot {
            ColorSlot::Indexed(index) => (
                index as usize,
                colors
                    .ansi()
                    .get(index as usize)
                    .copied()
                    .unwrap_or_else(|| xterm_color(index)),
            ),
            ColorSlot::Foreground => (NamedColor::Foreground as usize, colors.foreground),
            ColorSlot::Background => (NamedColor::Background as usize, colors.background),
            ColorSlot::Cursor => (NamedColor::Cursor as usize, colors.cursor),
        };
        match self.term.colors()[index] {
            Some(set) => Rgb::new(set.r, set.g, set.b),
            None => configured,
        }
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

    crate::emulator_conformance_tests!(|c, r, p| Box::new(AlacrittyEmu::new(c, r, p)));

    /// Every slot a program can address resolves, so a query always has an
    /// answer and a reset always has something to restore. The profile names
    /// sixteen; everything above falls through to the xterm table.
    #[test]
    fn every_slot_resolves_through_the_profile_then_xterm() {
        use crate::profile::{Colors, Rgb};
        let profile = Profile {
            colors: Colors {
                red: Rgb::new(1, 2, 3),
                background: Rgb::new(4, 5, 6),
                ..Default::default()
            },
            ..Default::default()
        };
        let emu = AlacrittyEmu::new(10, 2, &profile);

        assert_eq!(
            emu.color(ColorSlot::Indexed(1)),
            Rgb::new(1, 2, 3),
            "the profile names slot 1"
        );
        assert_eq!(emu.color(ColorSlot::Background), Rgb::new(4, 5, 6));
        assert_eq!(
            emu.color(ColorSlot::Foreground),
            Colors::default().foreground,
            "an unset profile color keeps its default"
        );
        for index in 16u8..=255 {
            assert_eq!(
                emu.color(ColorSlot::Indexed(index)),
                xterm_color(index),
                "slot {index} is not the profile's to name"
            );
        }
    }

    /// A program's color outranks the profile until it is reset, at which
    /// point the profile shows through again.
    #[test]
    fn a_program_color_outranks_the_profile_until_reset() {
        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let configured = emu.color(ColorSlot::Background);

        emu.process(b"\x1b]11;#123456\x07");
        assert_eq!(emu.color(ColorSlot::Background), Rgb::new(0x12, 0x34, 0x56));

        emu.process(b"\x1b]111\x07");
        assert_eq!(emu.color(ColorSlot::Background), configured);
    }
}
