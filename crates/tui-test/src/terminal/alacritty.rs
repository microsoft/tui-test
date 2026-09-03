//! [`Emulator`] backend built on `alacritty_terminal`. Translates the
//! alacritty grid into tui-test's neutral cell vocabulary, plus a capture
//! proxy that queues the terminal's replies so the reader can forward them
//! back to the PTY.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as AlacFlags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{
    ClipboardType as AlacClipboardType, Config as AlacConfig, Osc52, Term, TermMode,
};
use alacritty_terminal::vte::ansi;
use alacritty_terminal::vte::ansi::CursorShape as AlacCursorShape;
use alacritty_terminal::vte::ansi::NamedColor;
use alacritty_terminal::vte::ansi::Rgb as AlacRgb;

use compact_str::{CompactString, ToCompactString};

use crate::event::BellTracker;
use crate::profile::{xterm_color, ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, Hyperlink, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{
    Clipboard, ClipboardType, ClipboardValidator, CursorShape, Emulator, KeyboardMode,
};

fn clipboard_type(clipboard: AlacClipboardType) -> ClipboardType {
    match clipboard {
        AlacClipboardType::Clipboard => ClipboardType::Clipboard,
        AlacClipboardType::Selection => ClipboardType::Selection,
    }
}

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

/// alacritty invents an id for a link that arrived without one, appending
/// `_alacritty` to a process-wide counter. That is a rendering aid, not
/// something the child sent, and letting it through would put a value in
/// snapshots that changes with the order links were parsed in and that no
/// other backend produces. A link with no `id=` reports `None`.
///
/// A child that really sent `id=7_alacritty` is indistinguishable from a
/// synthesized one and reports `None` too. Telling them apart would mean
/// parsing OSC 8 ourselves, which is a lot of machinery for a collision
/// nobody will hit.
fn hyperlink_from_alac(link: &alacritty_terminal::term::cell::Hyperlink) -> Arc<Hyperlink> {
    let id = link.id();
    Arc::new(Hyperlink {
        id: (!id.ends_with("_alacritty")).then(|| id.to_compact_string()),
        uri: link.uri().to_compact_string(),
    })
}

fn cell_from_alac(c: &alacritty_terminal::term::cell::Cell, hyperlinks: bool) -> EmuCell {
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
        hyperlink: hyperlinks
            .then(|| c.hyperlink())
            .flatten()
            .as_ref()
            .map(hyperlink_from_alac),
    }
}

/// Formats a color query's reply once the color is known. alacritty builds
/// this closure with the query's own prefix and terminator already captured,
/// so the answer echoes the form the program asked in.
type ReplyFormat = Arc<dyn Fn(AlacRgb) -> String + Send + Sync>;

/// One thing the terminal wants to say back to the PTY.
///
/// A color query cannot be answered inside the listener: the color lives in
/// the terminal that owns this listener. It is parked as a [`Reply::Color`]
/// until `process` regains access to the terminal at the query boundary.
enum Reply {
    Bytes(Vec<u8>),
    Color(usize, ReplyFormat),
}

/// Queues what the terminal wants to say back to the PTY, in the order it
/// decided to say it.
///
/// Answers and other replies share one queue because a program may pipeline
/// several requests in a single write and match the answers up by position.
/// The common idiom ends a batch of queries with a device attributes request,
/// whose reply every terminal sends, and treats that reply as the end of the
/// batch: an answer that arrived after it would look like the query went
/// unanswered, and would then be read as though the user had typed it.
#[derive(Default, Clone)]
struct CaptureProxy {
    pending: Arc<Mutex<Vec<Reply>>>,
    title: Arc<Mutex<Option<String>>>,
    clipboard: Arc<Mutex<Clipboard>>,
    bells: BellTracker,
    color_query_pending: Arc<AtomicBool>,
}

impl EventListener for CaptureProxy {
    fn send_event(&self, ev: Event) {
        match ev {
            Event::PtyWrite(bytes) => {
                self.push_reply(Reply::Bytes(bytes.as_bytes().to_vec()), false)
            }
            Event::ColorRequest(slot, format) => self.push_reply(Reply::Color(slot, format), true),
            Event::ClipboardStore(clipboard, text) => {
                self.clipboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .set(clipboard_type(clipboard), text);
            }
            Event::ClipboardLoad(clipboard, format) => {
                let text = self
                    .clipboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(clipboard_type(clipboard))
                    .to_string();
                self.push_reply(Reply::Bytes(format(&text).into_bytes()), false);
            }
            // `OSC 0` and `OSC 2` both arrive here, and so does a pop of the
            // title stack (`CSI 23 t`), which alacritty implements by setting
            // the title it popped. An empty payload means the same as a reset:
            // the program is asking for no title rather than for a blank one.
            Event::Title(title) => self.set_title((!title.is_empty()).then_some(title)),
            Event::ResetTitle => self.set_title(None),
            Event::Bell => self.bells.ring(),
            _ => {}
        }
    }
}

impl CaptureProxy {
    fn push_reply(&self, reply: Reply, color_query: bool) {
        if let Ok(mut queue) = self.pending.lock() {
            queue.push(reply);
            if color_query {
                self.color_query_pending.store(true, Ordering::Release);
            }
        }
    }

    fn set_title(&self, title: Option<String>) {
        if let Ok(mut current) = self.title.lock() {
            *current = title;
        }
    }
}

pub struct AlacrittyEmu {
    term: Term<CaptureProxy>,
    processor: ansi::Processor,
    cols: u16,
    rows: u16,
    pending: Arc<Mutex<Vec<Reply>>>,
    title: Arc<Mutex<Option<String>>>,
    clipboard: Arc<Mutex<Clipboard>>,
    clipboard_validator: ClipboardValidator,
    color_query_pending: Arc<AtomicBool>,
    /// The settings this session was opened with. A program can shadow the
    /// colors at runtime but never reach them, so a reset always has a value
    /// to restore.
    profile: Profile,
}

impl AlacrittyEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Self {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> Self {
        let size = TermSize::new(cols as usize, rows as usize);
        let alac_config = AlacConfig {
            scrolling_history: profile.scrollback,
            kitty_keyboard: true,
            osc52: Osc52::CopyPaste,
            ..Default::default()
        };
        let pending: Arc<Mutex<Vec<Reply>>> = Arc::default();
        let title: Arc<Mutex<Option<String>>> = Arc::default();
        let clipboard: Arc<Mutex<Clipboard>> = Arc::default();
        let color_query_pending = Arc::new(AtomicBool::new(false));
        let proxy = CaptureProxy {
            pending: pending.clone(),
            title: title.clone(),
            clipboard: clipboard.clone(),
            bells,
            color_query_pending: color_query_pending.clone(),
        };
        AlacrittyEmu {
            term: Term::new(alac_config, &size, proxy),
            processor: ansi::Processor::new(),
            cols,
            rows,
            pending,
            title,
            clipboard,
            clipboard_validator: ClipboardValidator::new(),
            color_query_pending,
            profile: *profile,
        }
    }

    /// Resolve any color queries parked at the current parser position.
    ///
    /// Each answer replaces the query where it sits in the queue, so replies
    /// leave in the order the program asked for them. The listener cannot read
    /// the terminal palette itself, so `process` calls this immediately after
    /// the byte that completed the query and before parsing any later color
    /// change from the same PTY chunk.
    fn answer_queries(&mut self) {
        let parked: Vec<Reply> = match self.pending.lock() {
            Ok(mut queue) => {
                if !queue.iter().any(|r| matches!(r, Reply::Color(..))) {
                    return;
                }
                std::mem::take(&mut queue)
            }
            Err(_) => return,
        };
        let resolved: Vec<Reply> = parked
            .into_iter()
            .map(|reply| match reply {
                Reply::Bytes(bytes) => Reply::Bytes(bytes),
                Reply::Color(index, format) => {
                    let slot = match index {
                        i if i == NamedColor::Foreground as usize => ColorSlot::Foreground,
                        i if i == NamedColor::Background as usize => ColorSlot::Background,
                        i if i == NamedColor::Cursor as usize => ColorSlot::Cursor,
                        i => ColorSlot::Indexed(i as u8),
                    };
                    let c = self.color(slot);
                    Reply::Bytes(
                        format(AlacRgb {
                            r: c.r,
                            g: c.g,
                            b: c.b,
                        })
                        .into_bytes(),
                    )
                }
            })
            .collect();
        if let Ok(mut queue) = self.pending.lock() {
            // Anything queued meanwhile was asked for later, so it goes after.
            queue.splice(0..0, resolved);
        }
    }

    fn rows_in_range(&self, start: i32, end: i32) -> Vec<Vec<EmuCell>> {
        let grid = self.term.grid();
        let mut out = Vec::with_capacity((end - start).max(0) as usize);
        for line in start..end {
            let mut row = Vec::with_capacity(self.cols as usize);
            for col in 0..self.cols as usize {
                let cell = &grid[Line(line)][Column(col)];
                row.push(cell_from_alac(cell, self.profile.hyperlinks));
            }
            out.push(row);
        }
        out
    }
}

impl Emulator for AlacrittyEmu {
    fn process(&mut self, bytes: &[u8]) {
        self.clipboard_validator.process(bytes);
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            // OSC replies are emitted when BEL, ST (ESC \), or C1 ST closes
            // the sequence. A backslash is also a cheap split point when it
            // is ordinary text, and catches ST split across two PTY reads.
            if !matches!(*byte, b'\x07' | b'\\' | b'\x9c') {
                continue;
            }
            self.processor
                .advance(&mut self.term, &bytes[start..=index]);
            if self.color_query_pending.swap(false, Ordering::AcqRel) {
                self.answer_queries();
            }
            start = index + 1;
        }
        if start < bytes.len() {
            self.processor.advance(&mut self.term, &bytes[start..]);
            if self.color_query_pending.swap(false, Ordering::AcqRel) {
                self.answer_queries();
            }
        }
    }

    fn fault(&self) -> Option<String> {
        self.clipboard_validator.fault()
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        let queued = match self.pending.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => return Vec::new(),
        };
        queued
            .into_iter()
            .flat_map(|reply| match reply {
                Reply::Bytes(bytes) => bytes,
                // `answer_queries` runs at every query boundary, so a query is
                // always resolved before anything can drain it.
                Reply::Color(..) => Vec::new(),
            })
            .collect()
    }

    fn clipboard(&self, clipboard: ClipboardType) -> anyhow::Result<String> {
        Ok(self
            .clipboard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(clipboard)
            .to_string())
    }

    fn clipboard_revision(&self, clipboard: ClipboardType) -> anyhow::Result<u64> {
        Ok(self
            .clipboard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision(clipboard))
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        let mode = self.term.mode();
        let mut keyboard_mode = KeyboardMode::empty();
        for (term_mode, keyboard_flag) in [
            (
                TermMode::DISAMBIGUATE_ESC_CODES,
                KeyboardMode::DISAMBIGUATE_ESC_CODES,
            ),
            (
                TermMode::REPORT_EVENT_TYPES,
                KeyboardMode::REPORT_EVENT_TYPES,
            ),
            (
                TermMode::REPORT_ALTERNATE_KEYS,
                KeyboardMode::REPORT_ALTERNATE_KEYS,
            ),
            (
                TermMode::REPORT_ALL_KEYS_AS_ESC,
                KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
            ),
            (
                TermMode::REPORT_ASSOCIATED_TEXT,
                KeyboardMode::REPORT_ASSOCIATED_TEXT,
            ),
        ] {
            keyboard_mode.set(keyboard_flag, mode.contains(term_mode));
        }
        keyboard_mode
    }

    fn title(&self) -> Option<String> {
        self.title.lock().ok()?.clone()
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

    fn cursor_visible(&self) -> bool {
        // `Hidden` is a shape alacritty uses for a cursor it will not draw, so
        // it means the same thing as the mode being off.
        self.term.mode().contains(TermMode::SHOW_CURSOR)
            && self.term.cursor_style().shape != AlacCursorShape::Hidden
    }

    fn cursor_shape(&self) -> CursorShape {
        match self.term.cursor_style().shape {
            AlacCursorShape::Underline => CursorShape::Underline,
            AlacCursorShape::Beam => CursorShape::Bar,
            // `HollowBlock` is what alacritty draws for an unfocused window,
            // which a headless terminal has no notion of.
            _ => CursorShape::Block,
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

    #[test]
    fn multiple_bells_in_one_chunk_are_counted_individually() {
        let bells = BellTracker::default();
        let mut emulator =
            AlacrittyEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone());

        emulator.process(b"\x07\x07");

        assert_eq!(bells.count(), 2);
        assert_eq!(bells.sequence(), 2);
    }

    #[test]
    fn an_osc_bell_terminator_does_not_ring_the_terminal_bell() {
        let bells = BellTracker::default();
        let mut emulator =
            AlacrittyEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone());

        emulator.process(b"\x1b]0;window title\x07");

        assert_eq!(bells.count(), 0);
    }

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

    #[test]
    fn queries_pushes_and_pops_kitty_keyboard_modes() {
        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());

        emu.process(b"\x1b[?u");
        assert_eq!(emu.take_pending_writes(), b"\x1b[?0u");

        emu.process(b"\x1b[>3u");
        assert_eq!(
            emu.keyboard_mode(),
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_EVENT_TYPES
        );

        emu.process(b"\x1b[?u");
        assert_eq!(emu.take_pending_writes(), b"\x1b[?3u");

        emu.process(b"\x1b[>8u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::REPORT_ALL_KEYS_AS_ESC);
        emu.process(b"\x1b[<u");
        assert_eq!(
            emu.keyboard_mode(),
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_EVENT_TYPES
        );
        emu.process(b"\x1b[<u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::empty());
    }

    #[test]
    fn sets_kitty_keyboard_modes_with_each_apply_behavior() {
        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());

        emu.process(b"\x1b[=1u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::DISAMBIGUATE_ESC_CODES);

        emu.process(b"\x1b[=2;2u");
        assert_eq!(
            emu.keyboard_mode(),
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_EVENT_TYPES
        );

        emu.process(b"\x1b[=1;3u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::REPORT_EVENT_TYPES);

        emu.process(b"\x1b[=8u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::REPORT_ALL_KEYS_AS_ESC);
    }

    #[test]
    fn main_and_alternate_screens_keep_independent_keyboard_modes() {
        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());

        emu.process(b"\x1b[>1u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::DISAMBIGUATE_ESC_CODES);

        emu.process(b"\x1b[?1049h");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::empty());
        emu.process(b"\x1b[>2u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::REPORT_EVENT_TYPES);

        emu.process(b"\x1b[?1049l");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::DISAMBIGUATE_ESC_CODES);
        emu.process(b"\x1b[?1049h");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::REPORT_EVENT_TYPES);

        emu.process(b"\x1b[<u");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::empty());
        emu.process(b"\x1b[?1049l");
        assert_eq!(emu.keyboard_mode(), KeyboardMode::DISAMBIGUATE_ESC_CODES);
    }

    #[test]
    fn key_encoding_follows_the_active_negotiated_mode() {
        use crate::input::keys::{token_to_seq_for_action_with_mode, token_to_seq_with_mode};
        use crate::KeyAction;

        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());
        emu.process(b"\x1b[>1u");
        assert_eq!(
            token_to_seq_with_mode("Ctrl+i", emu.keyboard_mode()).unwrap(),
            "\x1b[105;5u"
        );

        emu.process(b"\x1b[<u");
        assert_eq!(
            token_to_seq_with_mode("Ctrl+i", emu.keyboard_mode()).unwrap(),
            "\t"
        );

        emu.process(b"\x1b[>10u");
        assert_eq!(
            token_to_seq_for_action_with_mode("a", KeyAction::Repeat, emu.keyboard_mode()).unwrap(),
            "\x1b[97;1:2u"
        );
    }
}
