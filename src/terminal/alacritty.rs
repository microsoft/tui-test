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

use crate::terminal::cell::{Color, EmuCell};
use crate::terminal::emu::Emulator;

fn color_from_alac(c: ansi::Color) -> Color {
    match c {
        ansi::Color::Named(named) => match named {
            ansi::NamedColor::Foreground | ansi::NamedColor::Background => Color::Default,
            other => Color::Idx(other as u8),
        },
        ansi::Color::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        ansi::Color::Indexed(i) => Color::Idx(i),
    }
}

fn cell_from_alac(c: &alacritty_terminal::term::cell::Cell) -> EmuCell {
    let flags = c.flags;
    let spacer = flags.contains(AlacFlags::WIDE_CHAR_SPACER)
        || flags.contains(AlacFlags::LEADING_WIDE_CHAR_SPACER);
    let ch = if spacer || (c.c == ' ' && !flags.contains(AlacFlags::WIDE_CHAR)) {
        String::new()
    } else {
        c.c.to_string()
    };
    EmuCell {
        ch,
        fg: color_from_alac(c.fg),
        bg: color_from_alac(c.bg),
        bold: flags.contains(AlacFlags::BOLD),
        dim: flags.contains(AlacFlags::DIM),
        italic: flags.contains(AlacFlags::ITALIC),
        underline: flags.intersects(
            AlacFlags::UNDERLINE
                | AlacFlags::DOUBLE_UNDERLINE
                | AlacFlags::DOTTED_UNDERLINE
                | AlacFlags::DASHED_UNDERLINE
                | AlacFlags::UNDERCURL,
        ),
        inverse: flags.contains(AlacFlags::INVERSE),
        invisible: flags.contains(AlacFlags::HIDDEN),
        strike: flags.contains(AlacFlags::STRIKEOUT),
    }
}

#[derive(Default, Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<u8>>>,
}

impl EventListener for CaptureProxy {
    fn send_event(&self, ev: Event) {
        if let Event::PtyWrite(bytes) = ev {
            if let Ok(mut buf) = self.pending.lock() {
                buf.extend_from_slice(bytes.as_bytes());
            }
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
        let size = TermSize::new(cols as usize, rows as usize);
        let config = AlacConfig {
            scrolling_history: scrollback,
            ..Default::default()
        };
        let pending: Arc<Mutex<Vec<u8>>> = Arc::default();
        let proxy = CaptureProxy {
            pending: pending.clone(),
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
