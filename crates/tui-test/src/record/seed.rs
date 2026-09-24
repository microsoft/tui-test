//! State needed after painting a recording's initial viewport. A picture alone
//! cannot reproduce the rendition, pending wrap or scrolling behavior of the
//! next output. Keep a bounded shadow terminal rather than retaining PTY history.

use std::fmt::Write;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Cursor, Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{self, CharsetIndex, Handler, StandardCharset};
use alacritty_terminal::vte::{Params, Parser, Perform};

pub(super) struct State {
    term: Term<VoidListener>,
    parser: ansi::Processor,
    controls: Controls,
    control_parser: ansi::Processor,
    boundary: SequenceBoundary,
}

impl State {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            term: Term::new(
                Config {
                    scrolling_history: 0,
                    ..Config::default()
                },
                &TermSize::new(usize::from(cols), usize::from(rows)),
                VoidListener,
            ),
            parser: ansi::Processor::new(),
            controls: Controls::new(usize::from(rows)),
            control_parser: ansi::Processor::new(),
            boundary: SequenceBoundary::default(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.control_parser.advance(&mut self.controls, bytes);
        self.boundary.process(bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize::new(usize::from(cols), usize::from(rows));
        if self.term.columns() != size.columns || self.term.screen_lines() != size.screen_lines {
            self.term.resize(size);
            self.controls.rows = usize::from(rows);
            self.controls.margins = (1, usize::from(rows));
        }
    }

    pub fn snapshot(&mut self, viewport: &str) -> String {
        let mut output = String::new();
        let modes = *self.term.mode();
        if modes.contains(TermMode::ALT_SCREEN) {
            // Preserve the hidden primary screen as well: leaving the alternate
            // screen during playback must not reveal an empty terminal.
            let active = self.term.grid().clone();
            self.term.swap_alt();
            paint_grid(&mut output, self.term.grid());
            restore_cursor(&mut output, self.term.grid(), &self.term.grid().cursor, 0);
            output.push_str("\x1b[?1049h");
            self.term.swap_alt();
            *self.term.grid_mut() = active;
        }
        output.push_str(viewport);
        restore_wraps(&mut output, self.term.grid());
        let (top, bottom) = self.controls.margins;
        let _ = write!(output, "\x1b[{top};{bottom}r");
        let grid = self.term.grid();
        restore_cursor(&mut output, grid, &grid.saved_cursor, 0);
        if self.controls.saved_blink {
            output.push_str("\x1b[5m");
        }
        output.push_str("\x1b7");

        let origin = if modes.contains(TermMode::ORIGIN) {
            output.push_str("\x1b[?6h");
            top - 1
        } else {
            0
        };
        if !modes.contains(TermMode::LINE_WRAP) {
            output.push_str("\x1b[?7l");
        }
        restore_cursor(&mut output, grid, &grid.cursor, origin);
        if self.controls.blink {
            output.push_str("\x1b[5m");
        }
        for (mode, number, private) in [
            (TermMode::INSERT, 4, false),
            (TermMode::LINE_FEED_NEW_LINE, 20, false),
            (TermMode::APP_CURSOR, 1, true),
            (TermMode::SHOW_CURSOR, 25, true),
            (TermMode::MOUSE_REPORT_CLICK, 1000, true),
            (TermMode::MOUSE_DRAG, 1002, true),
            (TermMode::MOUSE_MOTION, 1003, true),
            (TermMode::FOCUS_IN_OUT, 1004, true),
            (TermMode::UTF8_MOUSE, 1005, true),
            (TermMode::SGR_MOUSE, 1006, true),
            (TermMode::ALTERNATE_SCROLL, 1007, true),
            (TermMode::BRACKETED_PASTE, 2004, true),
        ] {
            let _ = write!(
                output,
                "\x1b[{}{number}{}",
                if private { "?" } else { "" },
                if modes.contains(mode) { "h" } else { "l" },
            );
        }
        output.push_str(if modes.contains(TermMode::APP_KEYPAD) {
            "\x1b="
        } else {
            "\x1b>"
        });
        output.push_str(match self.controls.charset {
            CharsetIndex::G0 => "\x0f",
            CharsetIndex::G1 => "\x0e",
            CharsetIndex::G2 => "\x1bn",
            CharsetIndex::G3 => "\x1bo",
        });
        output.push_str(&String::from_utf8_lossy(&self.boundary.pending));
        output
    }
}

struct Controls {
    rows: usize,
    margins: (usize, usize),
    charset: CharsetIndex,
    blink: bool,
    saved_blink: bool,
}

impl Controls {
    fn new(rows: usize) -> Self {
        Self {
            rows,
            margins: (1, rows),
            charset: CharsetIndex::G0,
            blink: false,
            saved_blink: false,
        }
    }
}

impl Handler for Controls {
    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        let bottom = bottom.unwrap_or(self.rows);
        if top < bottom {
            self.margins = (top.min(self.rows + 1), bottom.min(self.rows));
        }
    }

    fn reset_state(&mut self) {
        *self = Self::new(self.rows);
    }

    fn set_active_charset(&mut self, charset: CharsetIndex) {
        self.charset = charset;
    }

    fn terminal_attribute(&mut self, attr: ansi::Attr) {
        match attr {
            ansi::Attr::BlinkSlow | ansi::Attr::BlinkFast => self.blink = true,
            ansi::Attr::Reset | ansi::Attr::CancelBlink => self.blink = false,
            _ => {}
        }
    }

    fn save_cursor_position(&mut self) {
        self.saved_blink = self.blink;
    }

    fn restore_cursor_position(&mut self) {
        self.blink = self.saved_blink;
    }
}

fn paint_grid(output: &mut String, grid: &Grid<Cell>) {
    output.push_str("\x1b[0m\x1b[?7l\x1b[H\x1b[J");
    for y in 0..grid.screen_lines() {
        for x in 0..grid.columns() {
            let cell = &grid[Line(y as i32)][Column(x)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let _ = write!(output, "\x1b[{};{}H", y + 1, x + 1);
            write_cell(output, cell);
        }
    }
    restore_wraps(output, grid);
}

fn restore_wraps(output: &mut String, grid: &Grid<Cell>) {
    output.push_str("\x1b[?6l\x1b[4l\x1b[?7h\x1b[r\x1b(B\x0f");
    for y in 0..grid.screen_lines().saturating_sub(1) {
        let mut x = grid.columns() - 1;
        let last = &grid[Line(y as i32)][Column(x)];
        if !last.flags.contains(Flags::WRAPLINE) {
            continue;
        }
        if x > 0 && last.flags.contains(Flags::WIDE_CHAR_SPACER) {
            x -= 1;
        }
        let _ = write!(output, "\x1b[{};{}H", y + 1, x + 1);
        write_cell(output, &grid[Line(y as i32)][Column(x)]);
        write_cell(output, &grid[Line(y as i32 + 1)][Column(0)]);
    }
}

fn restore_cursor(output: &mut String, grid: &Grid<Cell>, cursor: &Cursor<Cell>, origin: usize) {
    // Printing a cell at the margin is the portable way to re-arm delayed
    // wrapping; a CUP alone always clears that state. Rewrite the existing
    // glyph (including wide/combining characters), then restore the live SGR.
    output.push_str("\x1b(B\x0f");
    let y = cursor.point.line.0.max(0) as usize;
    let mut x = cursor.point.column.0;
    if cursor.input_needs_wrap
        && x > 0
        && grid[Line(y as i32)][Column(x)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    {
        x -= 1;
    }
    let _ = write!(output, "\x1b[{};{}H", y.saturating_sub(origin) + 1, x + 1);
    if cursor.input_needs_wrap {
        write_cell(output, &grid[Line(y as i32)][Column(x)]);
    }
    write_style(output, &cursor.template);
    for (index, selector) in [
        (CharsetIndex::G0, '('),
        (CharsetIndex::G1, ')'),
        (CharsetIndex::G2, '*'),
        (CharsetIndex::G3, '+'),
    ] {
        let _ = write!(
            output,
            "\x1b{selector}{}",
            match cursor.charsets[index] {
                StandardCharset::Ascii => 'B',
                StandardCharset::SpecialCharacterAndLineDrawing => '0',
            }
        );
    }
}

fn write_cell(output: &mut String, cell: &Cell) {
    write_style(output, cell);
    output.push(if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        ' '
    } else {
        cell.c
    });
    for ch in cell.zerowidth().unwrap_or_default() {
        output.push(*ch);
    }
}

fn write_style(output: &mut String, cell: &Cell) {
    output.push_str("\x1b[0");
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
        (Flags::UNDERLINE, "4"),
        (Flags::DOUBLE_UNDERLINE, "4:2"),
        (Flags::UNDERCURL, "4:3"),
        (Flags::DOTTED_UNDERLINE, "4:4"),
        (Flags::DASHED_UNDERLINE, "4:5"),
    ] {
        if cell.flags.contains(flag) {
            let _ = write!(output, ";{code}");
        }
    }
    write_color(output, cell.fg, 38);
    write_color(output, cell.bg, 48);
    if let Some(color) = cell.underline_color() {
        write_color(output, color, 58);
    }
    output.push('m');
    if let Some(link) = cell.hyperlink() {
        let id = link.id();
        let id = if id.ends_with("_alacritty") { "" } else { id };
        let _ = write!(output, "\x1b]8;id={id};{}\x1b\\", link.uri());
    } else {
        output.push_str("\x1b]8;;\x1b\\");
    }
}

fn write_color(output: &mut String, color: ansi::Color, code: u8) {
    match color {
        ansi::Color::Named(named) if (named as usize) < 16 => {
            let _ = write!(output, ";{code};5;{}", named as u8);
        }
        ansi::Color::Named(_) => {}
        ansi::Color::Indexed(index) => {
            let _ = write!(output, ";{code};5;{index}");
        }
        ansi::Color::Spec(rgb) => {
            let _ = write!(output, ";{code};2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
    }
}

#[derive(Default)]
struct SequenceBoundary {
    parser: Parser,
    pending: Vec<u8>,
}

impl SequenceBoundary {
    fn process(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == 0x1b {
                self.pending.clear();
            }
            self.pending.push(byte);
            let mut completion = Completion::default();
            self.parser.advance(&mut completion, &[byte]);
            if completion.dispatched {
                self.pending.clear();
                if byte == 0x1b {
                    self.pending.push(byte);
                }
            } else if completion.executed {
                if matches!(byte, 0x18 | 0x1a) {
                    self.pending.clear();
                } else {
                    // A C0 control inside an unfinished escape has already
                    // affected the cursor; replay only the unfinished escape.
                    self.pending.pop();
                }
            }
        }
    }
}

#[derive(Default)]
struct Completion {
    dispatched: bool,
    executed: bool,
}

impl Perform for Completion {
    fn print(&mut self, _: char) {
        self.dispatched = true;
    }

    fn execute(&mut self, _: u8) {
        self.executed = true;
    }

    fn csi_dispatch(&mut self, _: &Params, _: &[u8], _: bool, _: char) {
        self.dispatched = true;
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {
        self.dispatched = true;
    }

    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
        self.dispatched = true;
    }

    fn unhook(&mut self) {
        self.dispatched = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::record::cast::snapshot_to_ansi;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::emu::{Emulator, TerminalMode};

    #[test]
    fn seeds_preserve_the_behavior_of_subsequent_output() {
        for (prefix, suffix) in [
            ("\x1b[31;1;4:3;58;2;4;5;6mhi", "!"),
            ("\x1b[31mA\x1b[39m", "B"),
            ("\x1b[58;2;4;5;6m", "\x1b[4mX"),
            ("abcde", "f"),
            ("abc你", "X"),
            ("abcdé\u{301}", "X"),
            ("abcde\x1b[?7l", "XY"),
            ("1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H", "\nZ"),
            ("\x1b[2;3r\x1b[?6h\x1b[1;2H", "a\x1b[2;1Hb\nc"),
            ("abcde\x1b[1;2H\x1b[4h", "X"),
            ("main\x1b[?1049halt", "\x1b[?1049lX"),
            ("\x1b[31m\x1b7\x1b[32mX", "\x1b8Y"),
            ("\x1b(0q", "qx"),
            ("\x1b[20h\x1b[?1h\x1b=\x1b[?2004hA", "\nB"),
            ("\x1b]8;id=test;https://example.com\x1b\\X", "Y"),
            ("\x1b[31", "mX"),
            ("\x1b[3\n1", "mX"),
            ("\x1b]2;part", "ial\x07X"),
            ("\x1b]2;title\x1b", "\\X"),
            ("\x1b(", "0qx"),
        ] {
            let mut state = State::new(5, 4);
            let mut source = AlacrittyEmu::new(5, 4, &Profile::default());
            state.process(prefix.as_bytes());
            source.process(prefix.as_bytes());
            let seed = state.snapshot(&snapshot_to_ansi(&source));
            let mut replay = AlacrittyEmu::new(5, 4, &Profile::default());
            replay.process(seed.as_bytes());
            assert_eq!(replay.viewable_rows(), source.viewable_rows(), "{prefix:?}");
            assert_eq!(replay.cursor(), source.cursor(), "{prefix:?}");
            source.process(suffix.as_bytes());
            replay.process(suffix.as_bytes());
            assert_eq!(
                replay.viewable_rows(),
                source.viewable_rows(),
                "{prefix:?} -> {suffix:?}"
            );
            assert_eq!(replay.cursor(), source.cursor(), "{prefix:?} -> {suffix:?}");
            assert_eq!(replay.title(), source.title(), "{prefix:?} -> {suffix:?}");
            for mode in TerminalMode::ALL {
                assert_eq!(replay.mode(mode), source.mode(mode), "{prefix:?}: {mode:?}");
            }
        }
    }

    #[test]
    fn resize_resets_margins_before_a_later_seed() {
        let mut state = State::new(5, 4);
        let mut source = AlacrittyEmu::new(5, 4, &Profile::default());
        let prefix = b"1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H";
        state.process(prefix);
        source.process(prefix);
        state.resize(6, 5);
        source.resize(6, 5);
        let mut replay = AlacrittyEmu::new(6, 5, &Profile::default());
        replay.process(state.snapshot(&snapshot_to_ansi(&source)).as_bytes());
        for suffix in [b"\nA".as_slice(), b"\nB", b"\nC"] {
            source.process(suffix);
            replay.process(suffix);
            assert_eq!(replay.viewable_rows(), source.viewable_rows());
            assert_eq!(replay.cursor(), source.cursor());
        }
    }

    #[test]
    fn seeds_preserve_soft_wrapping_when_playback_resizes() {
        for prefix in ["abcdefgh", "abc你好e\u{301}", "main\x1b[?1049habcdefgh"] {
            let mut state = State::new(5, 4);
            let mut source = AlacrittyEmu::new(5, 4, &Profile::default());
            state.process(prefix.as_bytes());
            source.process(prefix.as_bytes());
            let mut replay = AlacrittyEmu::new(5, 4, &Profile::default());
            replay.process(state.snapshot(&snapshot_to_ansi(&source)).as_bytes());
            for (cols, rows) in [(7, 4), (4, 5)] {
                source.resize(cols, rows);
                replay.resize(cols, rows);
                assert_eq!(replay.viewable_rows(), source.viewable_rows(), "{prefix:?}");
                assert_eq!(replay.cursor(), source.cursor(), "{prefix:?}");
            }
        }
    }
}
