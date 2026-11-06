//! Headless WezTerm VT core: no window, renderer, multiplexer, or WezTerm PTY.

use std::cell::RefCell;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use alacritty_terminal::vte::{Params, Parser, Perform};
use compact_str::CompactString;
use termwiz::input::KeyboardEncoding;
use wezterm_surface::{CursorShape as WezCursorShape, CursorVisibility};
use wezterm_term::color::{ColorAttribute, ColorPalette, Palette256, RgbColor, SrgbaTuple};
use wezterm_term::{
    Alert, AlertHandler, Blink, CellAttributes, Clipboard as WezClipboard, ClipboardSelection,
    Intensity, KeyCode, KeyModifiers, StableRowIndex, Terminal, TerminalConfiguration,
    TerminalSize, Underline,
};

use crate::api::{ScrollTarget, Viewport};
use crate::event::BellTracker;
use crate::input::keys::{KeyEventKind, KeyPress};
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, LinkCache, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{
    Clipboard, ClipboardType, ClipboardValidator, ColorTable, CursorShape, Emulator, KeyboardMode,
    TerminalMode,
};

#[derive(Debug)]
struct Configuration(Profile);

fn native_color(rgb: Rgb) -> SrgbaTuple {
    RgbColor::new_8bpc(rgb.r, rgb.g, rgb.b).into()
}

fn rgb(color: SrgbaTuple) -> Rgb {
    Rgb::new(
        (color.0 * 255.0).round() as u8,
        (color.1 * 255.0).round() as u8,
        (color.2 * 255.0).round() as u8,
    )
}

impl TerminalConfiguration for Configuration {
    fn scrollback_size(&self) -> usize {
        self.0.scrollback
    }

    fn color_palette(&self) -> ColorPalette {
        let colors = &self.0.colors;
        ColorPalette {
            colors: Palette256(std::array::from_fn(|index| {
                native_color(colors.rgb(index as u8))
            })),
            foreground: native_color(colors.foreground),
            background: native_color(colors.background),
            cursor_bg: native_color(colors.cursor),
            cursor_border: native_color(colors.cursor),
            ..ColorPalette::default()
        }
    }

    fn enable_kitty_keyboard(&self) -> bool {
        true
    }
}

#[derive(Default)]
struct Captured {
    pending: Vec<u8>,
    title: Option<String>,
    clipboard: Clipboard,
    fence: Option<Vec<u8>>,
    fault: Option<String>,
}

#[derive(Clone, Default)]
struct Capture {
    state: Arc<Mutex<Captured>>,
    complete: Arc<Condvar>,
    bells: BellTracker,
}

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(fence) = state.fence.as_ref() {
            if bytes.ends_with(fence) {
                let end = bytes.len() - fence.len();
                state.pending.extend_from_slice(&bytes[..end]);
                state.fence = None;
                self.complete.notify_all();
                return Ok(bytes.len());
            }
        }
        state.pending.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AlertHandler for Capture {
    fn alert(&mut self, alert: Alert) {
        match alert {
            Alert::Bell => self.bells.ring(),
            Alert::WindowTitleChanged(title) => {
                self.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .title = (!title.is_empty()).then_some(title);
            }
            _ => {}
        }
    }
}

impl WezClipboard for Capture {
    fn set_contents(
        &self,
        selection: ClipboardSelection,
        contents: Option<String>,
    ) -> anyhow::Result<()> {
        let selection = match selection {
            ClipboardSelection::Clipboard => ClipboardType::Clipboard,
            ClipboardSelection::PrimarySelection => ClipboardType::Selection,
        };
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clipboard
            .set(selection, contents.unwrap_or_default());
        Ok(())
    }
}

// WezTerm keeps viewport selection in its GUI, not in the VT core. These
// boundaries let the headless viewport follow a native history clear or a
// return from an alternate screen before later bytes add more output.
#[derive(Default)]
struct ViewportEvents {
    history_clear: bool,
    screen_exit: bool,
}

impl Perform for ViewportEvents {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        if action == 'J' && intermediates.is_empty() {
            self.history_clear |= params.iter().any(|param| param == [3]);
        }
        if action == 'l' && intermediates == b"?" {
            self.screen_exit |= params
                .iter()
                .any(|param| matches!(param, [47 | 1047 | 1049]));
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore && intermediates.is_empty() && byte == b'c' {
            self.history_clear = true;
        }
    }
}

fn color(color: ColorAttribute) -> Option<Color> {
    match color {
        ColorAttribute::Default => None,
        ColorAttribute::PaletteIndex(index) => Some(Color::from_index(index)),
        ColorAttribute::TrueColorWithPaletteFallback(value, _)
        | ColorAttribute::TrueColorWithDefaultFallback(value) => {
            let value = rgb(value);
            Some(Color::Rgb(value.r, value.g, value.b))
        }
    }
}

fn cell(text: &str, native: &CellAttributes, links: &mut LinkCache) -> EmuCell {
    let mut attrs = Attrs::empty();
    for (enabled, attr) in [
        (native.intensity() == Intensity::Bold, Attrs::BOLD),
        (native.intensity() == Intensity::Half, Attrs::DIM),
        (native.italic(), Attrs::ITALIC),
        (native.reverse(), Attrs::INVERSE),
        (native.invisible(), Attrs::INVISIBLE),
        (native.strikethrough(), Attrs::STRIKE),
        (native.blink() != Blink::None, Attrs::BLINK),
    ] {
        attrs.set(attr, enabled);
    }
    EmuCell {
        ch: CompactString::new(text),
        fg: color(native.foreground()),
        bg: color(native.background()),
        underline: match native.underline() {
            Underline::None => UnderlineStyle::None,
            Underline::Single => UnderlineStyle::Single,
            Underline::Double => UnderlineStyle::Double,
            Underline::Curly => UnderlineStyle::Curly,
            Underline::Dotted => UnderlineStyle::Dotted,
            Underline::Dashed => UnderlineStyle::Dashed,
        },
        underline_color: color(native.underline_color()),
        attrs,
        hyperlink: native
            .hyperlink()
            .map(|link| links.get(link.params().get("id").map(String::as_str), link.uri())),
    }
}

pub struct WezTermEmu {
    // The native key encoder mutates its writer through an &self trait call.
    // Emulator is Send, not Sync; the session mutex serializes all access.
    term: RefCell<Terminal>,
    capture: Capture,
    anchor: Option<StableRowIndex>,
    viewport_parser: Parser,
    viewport_events: ViewportEvents,
    clipboard_validator: ClipboardValidator,
}

impl WezTermEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Self {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> Self {
        let capture = Capture {
            bells,
            ..Capture::default()
        };
        let mut term = Terminal::new(
            TerminalSize {
                cols: usize::from(cols),
                rows: usize::from(rows),
                ..TerminalSize::default()
            },
            Arc::new(Configuration(*profile)),
            "WezTerm",
            "e68a8ec27727fbbda15e07f0a0737cd5e16481f6",
            Box::new(capture.clone()),
        );
        term.set_notification_handler(Box::new(capture.clone()));
        let clipboard: Arc<dyn WezClipboard> = Arc::new(capture.clone());
        term.set_clipboard(&clipboard);
        Self {
            term: RefCell::new(term),
            capture,
            anchor: None,
            viewport_parser: Parser::new(),
            viewport_events: ViewportEvents::default(),
            clipboard_validator: ClipboardValidator::new(),
        }
    }

    fn normalize_viewport(&mut self) {
        let term = self.term.borrow();
        if term.is_alt_screen_active() {
            return;
        }
        let screen = term.screen();
        let live = screen.visible_row_to_stable_row(0);
        self.anchor = self
            .anchor
            .map(|anchor| anchor.max(screen.phys_to_stable_row_index(0)))
            .filter(|anchor| *anchor < live);
    }

    fn fault_writer(&self, message: impl Into<String>) {
        self.capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fault
            .get_or_insert_with(|| message.into());
    }

    /// The core wraps every writer in an asynchronous BufWriter and exposes
    /// no flush barrier. send_paste only writes; it never enters the parser or
    /// changes terminal state. Fence that FIFO with a NUL-delimited marker,
    /// stripped by Capture before any bytes can reach the PTY. Native replies
    /// cannot produce this marker, and bracketed-paste wrappers are stripped
    /// with it. This also works while a VT sequence is split across reads.
    fn synchronize_writer(&self) -> bool {
        const FENCE: &str = "\0tui-test-wezterm-writer-fence\0";
        let mut term = self.term.borrow_mut();
        {
            let mut state = self
                .capture
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.fault.is_some() {
                return false;
            }
            state.fence = Some(if term.bracketed_paste_enabled() {
                format!("\x1b[200~{FENCE}\x1b[201~").into_bytes()
            } else {
                FENCE.as_bytes().to_vec()
            });
        }
        if let Err(error) = term.send_paste(FENCE) {
            self.fault_writer(format!("WezTerm writer failed: {error:#}"));
            return false;
        }
        let state = self
            .capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (state, _) = self
            .capture
            .complete
            .wait_timeout_while(state, Duration::from_secs(5), |state| state.fence.is_some())
            .unwrap_or_else(PoisonError::into_inner);
        if state.fence.is_some() {
            drop(state);
            self.fault_writer("WezTerm writer did not acknowledge the output fence");
            return false;
        }
        true
    }

    fn rows_in_range(&self, start: usize, end: usize) -> Vec<Vec<EmuCell>> {
        let term = self.term.borrow();
        let cols = term.screen().physical_cols;
        let mut links = LinkCache::default();
        term.screen()
            .lines_in_phys_range(start..end)
            .into_iter()
            .map(|line| {
                let mut row = vec![EmuCell::blank(); cols];
                for native in line.visible_cells() {
                    let column = native.cell_index();
                    if column >= cols {
                        break;
                    }
                    let mapped = cell(native.str(), native.attrs(), &mut links);
                    row[column] = mapped.clone();
                    for target in row
                        .iter_mut()
                        .take(column.saturating_add(native.width()).min(cols))
                        .skip(column + 1)
                    {
                        *target = mapped.clone();
                        target.ch = CompactString::const_new(CONTINUATION);
                    }
                }
                row
            })
            .collect()
    }
}

impl Emulator for WezTermEmu {
    fn process(&mut self, bytes: &[u8]) {
        self.clipboard_validator.process(bytes);
        let mut start = 0;
        for index in 0..bytes.len() {
            self.viewport_parser
                .advance(&mut self.viewport_events, &bytes[index..=index]);
            let history_clear = std::mem::take(&mut self.viewport_events.history_clear);
            let screen_exit = std::mem::take(&mut self.viewport_events.screen_exit);
            if history_clear || screen_exit {
                self.term.get_mut().advance_bytes(&bytes[start..=index]);
                if history_clear && !self.term.get_mut().is_alt_screen_active() {
                    self.anchor = None;
                }
                self.normalize_viewport();
                start = index + 1;
            }
        }
        if start < bytes.len() {
            self.term.get_mut().advance_bytes(&bytes[start..]);
        }
        self.normalize_viewport();
    }

    fn fault(&self) -> Option<String> {
        self.capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fault
            .clone()
            .or_else(|| self.clipboard_validator.fault())
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        if !self.synchronize_writer() {
            return Vec::new();
        }
        std::mem::take(
            &mut self
                .capture
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending,
        )
    }

    fn clipboard(&self, clipboard: ClipboardType) -> anyhow::Result<String> {
        Ok(self
            .capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clipboard
            .get(clipboard)
            .to_string())
    }

    fn clipboard_revision(&self, clipboard: ClipboardType) -> anyhow::Result<u64> {
        Ok(self
            .capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clipboard
            .revision(clipboard))
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        match self.term.borrow().get_keyboard_encoding() {
            KeyboardEncoding::Kitty(flags) => KeyboardMode::from_bits_truncate(flags.bits() as u8),
            _ => KeyboardMode::empty(),
        }
    }

    fn cursor_key_application(&self) -> bool {
        self.term.borrow().application_cursor_keys_enabled()
    }

    fn encode_key(&self, press: &KeyPress) -> Option<Vec<u8>> {
        // WezTerm's API has no repeat or associated-text fields. The shared
        // encoder can represent those Kitty events without losing information.
        if !self.keyboard_mode().is_empty()
            || press.event == KeyEventKind::Repeat
            || press.mods.hyper
            || press.mods.meta
        {
            return None;
        }
        let key = match press.key.as_str() {
            "up" => KeyCode::UpArrow,
            "down" => KeyCode::DownArrow,
            "left" => KeyCode::LeftArrow,
            "right" => KeyCode::RightArrow,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "insert" => KeyCode::Insert,
            "delete" => KeyCode::Delete,
            "backspace" => KeyCode::Backspace,
            "tab" => KeyCode::Tab,
            "enter" | "return" => KeyCode::Enter,
            "escape" | "esc" => KeyCode::Escape,
            "space" => KeyCode::Char(' '),
            name if name.starts_with('f') && name.len() > 1 => {
                KeyCode::Function(name[1..].parse().ok()?)
            }
            name => {
                let mut chars = press.text.as_deref().unwrap_or(name).chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                KeyCode::Char(ch)
            }
        };
        let mut mods = KeyModifiers::NONE;
        for (enabled, flag) in [
            (press.mods.ctrl, KeyModifiers::CTRL),
            (press.mods.alt, KeyModifiers::ALT),
            (press.mods.shift, KeyModifiers::SHIFT),
            (press.mods.super_key, KeyModifiers::SUPER),
        ] {
            mods.set(flag, enabled);
        }
        if !self.synchronize_writer() {
            return Some(Vec::new());
        }
        let before = self
            .capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .len();
        let result = if press.event == KeyEventKind::Release {
            self.term.borrow_mut().key_up(key, mods)
        } else {
            self.term.borrow_mut().key_down(key, mods)
        };
        if !self.synchronize_writer() {
            return Some(Vec::new());
        }
        let bytes = self
            .capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .split_off(before);
        result.ok().map(|()| bytes)
    }

    fn mode(&self, mode: TerminalMode) -> bool {
        let term = self.term.borrow();
        match mode {
            TerminalMode::ApplicationCursorKeys => term.application_cursor_keys_enabled(),
            TerminalMode::ApplicationKeypad => term.application_keypad_enabled(),
            TerminalMode::Origin => term.dec_origin_mode_enabled(),
            TerminalMode::Wraparound => term.dec_auto_wrap_enabled(),
            TerminalMode::Insert => term.insert_mode_enabled(),
            TerminalMode::FocusEvents => term.focus_tracking_enabled(),
            TerminalMode::BracketedPaste => term.bracketed_paste_enabled(),
            TerminalMode::AlternateScreen => term.is_alt_screen_active(),
            TerminalMode::CursorVisible => {
                term.cursor_pos().visibility == CursorVisibility::Visible
            }
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term.get_mut().resize(TerminalSize {
            cols: usize::from(cols),
            rows: usize::from(rows),
            ..TerminalSize::default()
        });
        self.normalize_viewport();
    }

    fn scroll(&mut self, target: ScrollTarget) {
        let term = self.term.borrow();
        if term.is_alt_screen_active() {
            return;
        }
        let viewport = self.viewport();
        let offset = match target.clamped(viewport) {
            ScrollTarget::Lines(lines) => viewport.offset.saturating_add_signed(lines as isize),
            ScrollTarget::Top => viewport.history,
            ScrollTarget::Bottom => 0,
        }
        .min(viewport.history);
        self.anchor = (offset != 0).then(|| {
            term.screen()
                .phys_to_stable_row_index(viewport.history - offset)
        });
    }

    fn viewport(&self) -> Viewport {
        let term = self.term.borrow();
        if term.is_alt_screen_active() {
            return Viewport::default();
        }
        let screen = term.screen();
        let history = screen.scrollback_rows() - screen.physical_rows;
        let live = screen.visible_row_to_stable_row(0);
        let offset = self
            .anchor
            .map(|anchor| (live - anchor).max(0) as usize)
            .unwrap_or_default()
            .min(history);
        Viewport { offset, history }
    }

    fn size(&self) -> (u16, u16) {
        let term = self.term.borrow();
        (
            term.screen().physical_cols as u16,
            term.screen().physical_rows as u16,
        )
    }

    fn cursor(&self) -> (u16, u16) {
        let cursor = self.term.borrow().cursor_pos();
        let (cols, rows) = self.size();
        (
            cursor.x.min(usize::from(cols.saturating_sub(1))) as u16,
            cursor.y.clamp(0, i64::from(rows.saturating_sub(1))) as u16,
        )
    }

    fn cursor_shape(&self) -> CursorShape {
        match self.term.borrow().cursor_pos().shape {
            WezCursorShape::BlinkingUnderline | WezCursorShape::SteadyUnderline => {
                CursorShape::Underline
            }
            WezCursorShape::BlinkingBar | WezCursorShape::SteadyBar => CursorShape::Bar,
            _ => CursorShape::Block,
        }
    }

    fn title(&self) -> Option<String> {
        self.capture
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .title
            .clone()
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        let viewport = self.viewport();
        let start = viewport.history - viewport.offset;
        self.rows_in_range(start, start + usize::from(self.size().1))
    }

    fn active_rows(&self) -> Vec<Vec<EmuCell>> {
        let screen = self.term.borrow();
        let end = screen.screen().scrollback_rows();
        self.rows_in_range(end - screen.screen().physical_rows, end)
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(0, self.term.borrow().screen().scrollback_rows())
    }

    fn color(&self, slot: ColorSlot) -> Rgb {
        let palette = self.term.borrow().palette();
        rgb(match slot {
            ColorSlot::Indexed(index) => palette.colors.0[index as usize],
            ColorSlot::Foreground => palette.foreground,
            ColorSlot::Background => palette.background,
            ColorSlot::Cursor => palette.cursor_bg,
        })
    }

    fn colors(&self) -> ColorTable {
        let palette = self.term.borrow().palette();
        ColorTable {
            foreground: rgb(palette.foreground),
            background: rgb(palette.background),
            cursor: rgb(palette.cursor_bg),
            palette: std::array::from_fn(|index| rgb(palette.colors.0[index])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::KeyAction;
    use crate::input::keys::token_to_presses;

    // Verified against the pinned core's Performer, Window dispatcher, and
    // CellAttributes intensity enum; these are not adapter approximations.
    crate::emulator_conformance_tests!(
        |cols, rows, profile| Box::new(WezTermEmu::new(cols, rows, profile)),
        &[
            crate::terminal::conformance::Divergence::ClipboardQueriesUnsupported,
            crate::terminal::conformance::Divergence::ColorRepliesUseSt,
            crate::terminal::conformance::Divergence::BoldAndDimAreExclusive,
            crate::terminal::conformance::Divergence::PaletteResetResetsDynamicColors,
            crate::terminal::conformance::Divergence::NoTitleStack,
            crate::terminal::conformance::Divergence::WideCharOverflowsRightMargin,
        ]
    );

    #[test]
    fn native_key_encoding_preserves_queued_terminal_replies() {
        let mut emu = WezTermEmu::new(20, 4, &Profile::default());
        emu.process(b"\x1b[?1h\x1b[6n");
        let press = token_to_presses("up", KeyAction::Down).unwrap().remove(0);
        assert_eq!(emu.encode_key(&press), Some(b"\x1bOA".to_vec()));
        assert_eq!(emu.take_pending_writes(), b"\x1b[1;1R");
    }

    #[test]
    fn kitty_repeats_keep_the_shared_encoders_event_information() {
        let mut emu = WezTermEmu::new(20, 4, &Profile::default());
        emu.process(b"\x1b[>3u");
        let press = token_to_presses("up", KeyAction::Repeat).unwrap().remove(0);
        assert!(emu.encode_key(&press).is_none());
        assert!(emu
            .keyboard_mode()
            .contains(KeyboardMode::REPORT_EVENT_TYPES));
        assert!(emu.take_pending_writes().is_empty());
    }

    #[test]
    fn writer_fence_never_becomes_pty_input_even_during_bracketed_paste() {
        let mut emu = WezTermEmu::new(20, 4, &Profile::default());
        emu.process(b"\x1b[?2004h\x1b[3");
        assert!(emu.take_pending_writes().is_empty());
        emu.process(b"1mX\x1b[6n");
        assert_eq!(emu.viewable_rows()[0][0].fg, Some(Color::from_index(1)));
        assert_eq!(emu.take_pending_writes(), b"\x1b[1;2R");
        assert!(emu.take_pending_writes().is_empty());
        assert_eq!(emu.fault(), None);
    }
}
