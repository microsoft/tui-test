//! [`Emulator`] backend built on `rio-vt`.

use std::sync::{Arc, Mutex, PoisonError};

use compact_str::{CompactString, ToCompactString};
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::clipboard::ClipboardType as RioClipboardType;
use rio_vt::config::colors::term::COUNT as COLOR_COUNT;
use rio_vt::config::colors::{AnsiColor, ColorRgb, NamedColor as RioNamedColor};
use rio_vt::crosswords::grid::ExtrasTable;
use rio_vt::crosswords::pos::Line;
use rio_vt::crosswords::square::{ContentTag, Square, Wide};
use rio_vt::crosswords::style::{Style, StyleFlags, StyleSet};
use rio_vt::crosswords::{Crosswords, CrosswordsSize, Mode};
use rio_vt::event::{EventListener, RioEvent, WindowId};
use rio_vt::performer::handler::Processor;

use crate::event::BellTracker;
use crate::profile::{xterm_color, ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{
    Clipboard, ClipboardType, ClipboardValidator, CursorShape, Emulator, KeyboardMode,
};

fn clipboard_type(clipboard: RioClipboardType) -> ClipboardType {
    match clipboard {
        RioClipboardType::Clipboard => ClipboardType::Clipboard,
        RioClipboardType::Selection => ClipboardType::Selection,
    }
}

fn color_from_rio(color: AnsiColor) -> Option<Color> {
    match color {
        AnsiColor::Named(RioNamedColor::Foreground | RioNamedColor::Background) => None,
        AnsiColor::Named(named) if (named as usize) < 16 => Some(Color::from_index(named as u8)),
        AnsiColor::Named(_) => None,
        AnsiColor::Indexed(index) => Some(Color::from_index(index)),
        AnsiColor::Spec(rgb) => Some(Color::Rgb(rgb.r, rgb.g, rgb.b)),
    }
}

fn underline_from_rio(flags: StyleFlags) -> UnderlineStyle {
    if flags.contains(StyleFlags::DOUBLE_UNDERLINE) {
        UnderlineStyle::Double
    } else if flags.contains(StyleFlags::UNDERCURL) {
        UnderlineStyle::Curly
    } else if flags.contains(StyleFlags::DOTTED_UNDERLINE) {
        UnderlineStyle::Dotted
    } else if flags.contains(StyleFlags::DASHED_UNDERLINE) {
        UnderlineStyle::Dashed
    } else if flags.contains(StyleFlags::UNDERLINE) {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    }
}

fn styled_cell(square: Square, style: Style, extras: &ExtrasTable) -> EmuCell {
    let ch = match square.wide() {
        Wide::Spacer => CompactString::const_new(CONTINUATION),
        Wide::LeadingSpacer => CompactString::const_new(" "),
        Wide::Narrow | Wide::Wide => {
            let c = square.c();
            if matches!(c, '\0' | '\t' | ' ') {
                CompactString::const_new(" ")
            } else {
                let mut text = c.to_compact_string();
                if let Some(extra) = square.extras_id().and_then(|id| extras.get(id)) {
                    for c in &extra.zerowidth {
                        text.push(*c);
                    }
                }
                text
            }
        }
    };

    let mut attrs = Attrs::empty();
    for (flag, attr) in [
        (StyleFlags::BOLD, Attrs::BOLD),
        (StyleFlags::DIM, Attrs::DIM),
        (StyleFlags::ITALIC, Attrs::ITALIC),
        (StyleFlags::INVERSE, Attrs::INVERSE),
        (StyleFlags::HIDDEN, Attrs::INVISIBLE),
        (StyleFlags::STRIKEOUT, Attrs::STRIKE),
    ] {
        attrs.set(attr, style.flags.contains(flag));
    }

    EmuCell {
        ch,
        fg: color_from_rio(style.fg),
        bg: color_from_rio(style.bg),
        underline: underline_from_rio(style.flags),
        underline_color: style.underline_color.and_then(color_from_rio),
        attrs,
    }
}

fn cell_from_rio(square: Square, styles: &StyleSet, extras: &ExtrasTable) -> EmuCell {
    match square.content_tag() {
        ContentTag::Codepoint => styled_cell(square, styles.get(square.style_id()), extras),
        ContentTag::BgPalette => EmuCell {
            bg: Some(Color::from_index(square.bg_palette_index())),
            ..EmuCell::blank()
        },
        ContentTag::BgRgb => {
            let (r, g, b) = square.bg_rgb();
            EmuCell {
                bg: Some(Color::Rgb(r, g, b)),
                ..EmuCell::blank()
            }
        }
    }
}

#[derive(Clone)]
struct ColorState {
    profile: Profile,
    overrides: Arc<Mutex<[Option<Rgb>; COLOR_COUNT]>>,
}

impl ColorState {
    fn new(profile: &Profile) -> Self {
        Self {
            profile: *profile,
            overrides: Arc::new(Mutex::new([None; COLOR_COUNT])),
        }
    }

    fn slot_index(slot: ColorSlot) -> usize {
        match slot {
            ColorSlot::Indexed(index) => index as usize,
            ColorSlot::Foreground => RioNamedColor::Foreground as usize,
            ColorSlot::Background => RioNamedColor::Background as usize,
            ColorSlot::Cursor => RioNamedColor::Cursor as usize,
        }
    }

    fn configured(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.profile.colors.ansi()[index],
            16..=255 => xterm_color(index as u8),
            index if index == RioNamedColor::Foreground as usize => self.profile.colors.foreground,
            index if index == RioNamedColor::Background as usize => self.profile.colors.background,
            index if index == RioNamedColor::Cursor as usize => self.profile.colors.cursor,
            _ => self.profile.colors.foreground,
        }
    }

    fn get(&self, index: usize) -> Rgb {
        self.overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index]
            .unwrap_or_else(|| self.configured(index))
    }

    fn set(&self, index: usize, color: Option<ColorRgb>) {
        self.overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index] =
            color.map(|color| Rgb::new(color.r, color.g, color.b));
    }
}

#[derive(Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<u8>>>,
    colors: ColorState,
    clipboard: Arc<Mutex<Clipboard>>,
    bells: BellTracker,
}

impl EventListener for CaptureProxy {
    fn event(&self) -> (Option<RioEvent>, bool) {
        (None, false)
    }

    fn send_event(&self, event: RioEvent, _id: WindowId) {
        match event {
            RioEvent::PtyWrite(_, text) => self.push(text.into_bytes()),
            RioEvent::ColorRequest(_, index, format) => {
                let color = self.colors.get(index);
                self.push(
                    format(ColorRgb {
                        r: color.r,
                        g: color.g,
                        b: color.b,
                    })
                    .into_bytes(),
                );
            }
            RioEvent::ColorChange(_, index, color) => self.colors.set(index, color),
            RioEvent::ClipboardStore(clipboard, text) => {
                self.clipboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .set(clipboard_type(clipboard), text);
            }
            RioEvent::ClipboardLoad(_, clipboard, format) => {
                let text = self
                    .clipboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(clipboard_type(clipboard))
                    .to_string();
                self.push(format(&text).into_bytes());
            }
            RioEvent::Bell => self.bells.ring(),
            _ => {}
        }
    }
}

impl CaptureProxy {
    fn push(&self, bytes: Vec<u8>) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(bytes);
    }
}

pub struct RioEmu {
    term: Crosswords<CaptureProxy>,
    processor: Processor,
    pending: Arc<Mutex<Vec<u8>>>,
    colors: ColorState,
    clipboard: Arc<Mutex<Clipboard>>,
    clipboard_validator: ClipboardValidator,
    cols: u16,
    rows: u16,
}

impl RioEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Self {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let pending = Arc::new(Mutex::new(Vec::new()));
        let colors = ColorState::new(profile);
        let clipboard = Arc::new(Mutex::new(Clipboard::default()));
        let proxy = CaptureProxy {
            pending: Arc::clone(&pending),
            colors: colors.clone(),
            clipboard: Arc::clone(&clipboard),
            bells,
        };
        Self {
            term: Crosswords::new(
                CrosswordsSize::new(cols as usize, rows as usize),
                RioCursorShape::Block,
                proxy,
                WindowId::from(0),
                0,
                profile.scrollback,
            ),
            processor: Processor::default(),
            pending,
            colors,
            clipboard,
            clipboard_validator: ClipboardValidator::new(),
            cols,
            rows,
        }
    }

    fn rows_in_range(&self, start: i32, end: i32) -> Vec<Vec<EmuCell>> {
        let styles = &self.term.grid.style_set;
        let extras = &self.term.grid.extras_table;
        let mut output = Vec::with_capacity((end - start).max(0) as usize);
        for line in start..end {
            let source = &self.term.grid[Line(line)];
            let mut row = Vec::with_capacity(self.cols as usize);
            for col in 0..self.cols as usize {
                let square = source.inner.get(col).copied().unwrap_or_default();
                row.push(cell_from_rio(square, styles, extras));
            }
            output.push(row);
        }
        output
    }
}

impl Emulator for RioEmu {
    fn process(&mut self, bytes: &[u8]) {
        self.clipboard_validator.process(bytes);
        self.processor.advance(&mut self.term, bytes);
    }

    fn fault(&self) -> Option<String> {
        self.clipboard_validator.fault()
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn clipboard(&self, clipboard: ClipboardType) -> anyhow::Result<String> {
        Ok(self
            .clipboard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(clipboard)
            .to_string())
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        let mode = self.term.mode();
        let mut keyboard_mode = KeyboardMode::empty();
        for (rio_mode, keyboard_flag) in [
            (
                Mode::DISAMBIGUATE_ESC_CODES,
                KeyboardMode::DISAMBIGUATE_ESC_CODES,
            ),
            (Mode::REPORT_EVENT_TYPES, KeyboardMode::REPORT_EVENT_TYPES),
            (
                Mode::REPORT_ALTERNATE_KEYS,
                KeyboardMode::REPORT_ALTERNATE_KEYS,
            ),
            (
                Mode::REPORT_ALL_KEYS_AS_ESC,
                KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
            ),
            (
                Mode::REPORT_ASSOCIATED_TEXT,
                KeyboardMode::REPORT_ASSOCIATED_TEXT,
            ),
        ] {
            keyboard_mode.set(keyboard_flag, mode.contains(rio_mode));
        }
        keyboard_mode
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.term
            .resize(CrosswordsSize::new(self.cols as usize, self.rows as usize));
    }

    fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn cursor(&self) -> (u16, u16) {
        let position = self.term.cursor().pos;
        let x = (position.col.0 as u16).min(self.cols.saturating_sub(1));
        let y = position.row.0.max(0).min(self.rows as i32 - 1) as u16;
        (x, y)
    }

    fn title(&self) -> Option<String> {
        (!self.term.title.is_empty()).then(|| self.term.title.clone())
    }

    fn cursor_visible(&self) -> bool {
        !matches!(self.term.cursor().content, RioCursorShape::Hidden)
    }

    fn cursor_shape(&self) -> CursorShape {
        match self.term.cursor().content {
            RioCursorShape::Underline => CursorShape::Underline,
            RioCursorShape::Beam => CursorShape::Bar,
            RioCursorShape::Block | RioCursorShape::Hidden => CursorShape::Block,
        }
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(0, self.rows as i32)
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(-(self.term.history_size() as i32), self.rows as i32)
    }

    fn color(&self, slot: ColorSlot) -> Rgb {
        self.colors.get(ColorState::slot_index(slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::emulator_conformance_tests!(|cols, rows, profile| {
        Box::new(RioEmu::new(cols, rows, profile))
    });

    #[test]
    fn multiple_bells_in_one_chunk_are_counted_individually() {
        let bells = BellTracker::default();
        let mut emulator = RioEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone());

        emulator.process(b"\x07\x07");

        assert_eq!(bells.count(), 2);
        assert_eq!(bells.sequence(), 2);
    }
}
