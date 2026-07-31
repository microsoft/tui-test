//! Terminal-emulator wrapper around `rio-vt`. Exposes the small
//! grid/cell/color surface the rest of shell-use consumes, plus a capture
//! proxy that queues the terminal's replies so the reader can forward them
//! back to the PTY.

use std::sync::{Arc, Mutex};

use rio_vt::ansi::CursorShape;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::grid::Dimensions;
use rio_vt::crosswords::pos::{Column, Line};
use rio_vt::crosswords::square::{ContentTag, Square, Wide};
use rio_vt::crosswords::style::{Style, StyleFlags};
use rio_vt::crosswords::{Crosswords, CrosswordsSize};
use rio_vt::event::{EventListener, RioEvent, WindowId};
use rio_vt::performer::handler::Processor;

use crate::terminal::integration::CommandTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    fn from_vt(c: AnsiColor) -> Self {
        match c {
            AnsiColor::Named(NamedColor::Foreground | NamedColor::Background) => Color::Default,
            AnsiColor::Named(named) => {
                let index = named as u32;
                if index < 16 {
                    Color::Idx(index as u8)
                } else {
                    Color::Default
                }
            }
            AnsiColor::Indexed(i) => Color::Idx(i),
            AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmuCell {
    /// Empty string means a blank cell (rendered as a space).
    pub ch: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strike: bool,
}

fn cell_from_square(square: &Square, styles: &[Style]) -> EmuCell {
    let spacer = !matches!(square.wide(), Wide::Narrow | Wide::Wide);
    let c = square.c();
    let ch = if spacer || c == ' ' || c == '\0' {
        String::new()
    } else {
        c.to_string()
    };
    let (fg, bg, flags) = match square.content_tag() {
        ContentTag::Codepoint => {
            let style = styles
                .get(square.style_id() as usize)
                .copied()
                .unwrap_or_default();
            (Color::from_vt(style.fg), Color::from_vt(style.bg), style.flags)
        }
        ContentTag::BgPalette => (
            Color::Default,
            Color::Idx(square.bg_palette_index()),
            StyleFlags::empty(),
        ),
        ContentTag::BgRgb => {
            let (r, g, b) = square.bg_rgb();
            (Color::Default, Color::Rgb(r, g, b), StyleFlags::empty())
        }
    };
    EmuCell {
        ch,
        fg,
        bg,
        bold: flags.contains(StyleFlags::BOLD),
        dim: flags.contains(StyleFlags::DIM),
        italic: flags.contains(StyleFlags::ITALIC),
        underline: flags.intersects(StyleFlags::ALL_UNDERLINES),
        inverse: flags.contains(StyleFlags::INVERSE),
        invisible: flags.contains(StyleFlags::HIDDEN),
        strike: flags.contains(StyleFlags::STRIKEOUT),
    }
}

#[derive(Default, Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<u8>>>,
}

impl EventListener for CaptureProxy {
    fn event(&self) -> (Option<RioEvent>, bool) {
        (None, false)
    }

    fn send_event(&self, event: RioEvent, _id: WindowId) {
        if let RioEvent::PtyWrite(_route, text) = event {
            if let Ok(mut buf) = self.pending.lock() {
                buf.extend_from_slice(text.as_bytes());
            }
        }
    }
}

pub struct Emu {
    term: Crosswords<CaptureProxy>,
    processor: Processor,
    tracker: CommandTracker,
    cols: u16,
    rows: u16,
    pending: Arc<Mutex<Vec<u8>>>,
}

impl Emu {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let pending: Arc<Mutex<Vec<u8>>> = Arc::default();
        let proxy = CaptureProxy {
            pending: pending.clone(),
        };
        let size = CrosswordsSize::new(cols.max(1) as usize, rows.max(1) as usize);
        Emu {
            term: Crosswords::new(
                size,
                CursorShape::Block,
                proxy,
                WindowId::from(0),
                0,
                scrollback,
            ),
            processor: Processor::default(),
            tracker: CommandTracker::new(),
            cols,
            rows,
            pending,
        }
    }

    /// Feed PTY bytes through the terminal emulator and the command tracker.
    pub fn process(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
        self.tracker.feed(bytes);
    }

    /// Access the command tracker.
    pub fn tracker(&self) -> &CommandTracker {
        &self.tracker
    }

    pub fn take_pending_writes(&mut self) -> Vec<u8> {
        match self.pending.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => Vec::new(),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term
            .resize(CrosswordsSize::new(cols.max(1) as usize, rows.max(1) as usize));
        self.cols = cols;
        self.rows = rows;
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// Cursor position as `(x, y)` (column, row), 0-based, clamped to screen.
    pub fn cursor(&self) -> (u16, u16) {
        let p = self.term.cursor().pos;
        let y = p.row.0.max(0).min(self.rows as i32 - 1) as u16;
        let x = (p.col.0 as u16).min(self.cols.saturating_sub(1));
        (x, y)
    }

    /// Visible screen as rows of cells.
    pub fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(0, self.rows as i32)
    }

    /// History + visible screen as rows of cells.
    pub fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        let history = self.term.history_size() as i32;
        let screen = self.term.grid.screen_lines() as i32;
        self.rows_in_range(-history, screen)
    }

    fn rows_in_range(&self, start: i32, end: i32) -> Vec<Vec<EmuCell>> {
        let styles = self.term.grid.style_set.styles();
        let mut out = Vec::with_capacity((end - start).max(0) as usize);
        for line in start..end {
            let row = &self.term.grid[Line(line)];
            let mut cells = Vec::with_capacity(self.cols as usize);
            for col in 0..self.cols as usize {
                cells.push(cell_from_square(&row[Column(col)], styles));
            }
            out.push(cells);
        }
        out
    }
}

/// Join a grid of cells into one string per row (blank cells → spaces).
pub fn rows_to_strings(rows: &[Vec<EmuCell>]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|c| if c.ch.is_empty() { " " } else { c.ch.as_str() })
                .collect::<String>()
        })
        .collect()
}
