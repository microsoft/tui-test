//! Key-name and key-event to terminal input sequence mapping.

use crate::api::KeyAction;
use compact_str::{CompactString, ToCompactString};

use crate::terminal::emu::KeyboardMode;

/// The terminal state key encoding depends on.
///
/// Grouped rather than passed as separate arguments because encoding branches
/// on all of it at once and the set grows: DECCKM joined the Kitty flags here
/// once arrows had to be able to come out as `SS3 A`.
///
/// Public entry points take `impl Into<InputModes>`, so a caller that only has
/// Kitty flags can still pass them directly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputModes {
    pub keyboard: KeyboardMode,
    /// `DECCKM`: the child expects `SS3 A` from the up arrow, not `CSI A`.
    pub cursor_key_application: bool,
}

impl From<KeyboardMode> for InputModes {
    fn from(keyboard: KeyboardMode) -> Self {
        Self {
            keyboard,
            cursor_key_application: false,
        }
    }
}

const ESC: &str = "\u{1b}";
const CSI: &str = "\u{1b}[";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyEventKind {
    #[default]
    Press,
    Repeat,
    Release,
}

impl KeyEventKind {
    fn code(self) -> Option<u8> {
        match self {
            KeyEventKind::Press => None,
            KeyEventKind::Repeat => Some(2),
            KeyEventKind::Release => Some(3),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub hyper: bool,
    pub meta: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.super_key || self.hyper || self.meta
    }

    fn disambiguates_character(self) -> bool {
        self.ctrl || self.alt || self.super_key || self.hyper || self.meta
    }

    pub fn has_kitty_only_modifier(self) -> bool {
        self.super_key || self.hyper || self.meta
    }

    /// Kitty and xterm modifier parameters are one plus the modifier bitfield.
    fn param(self) -> u16 {
        1 + self.shift as u16
            + 2 * self.alt as u16
            + 4 * self.ctrl as u16
            + 8 * self.super_key as u16
            + 16 * self.hyper as u16
            + 32 * self.meta as u16
    }
}

struct ParsedToken<'a> {
    key: &'a str,
    mods: Mods,
}

fn parse_token(token: &str) -> anyhow::Result<ParsedToken<'_>> {
    if !token.contains('+') || token.len() == 1 {
        return Ok(ParsedToken {
            key: token,
            mods: Mods::default(),
        });
    }

    let parts: Vec<&str> = token.split('+').collect();
    let (key, prefixes) = parts
        .split_last()
        .ok_or_else(|| anyhow::anyhow!("invalid key: '{token}'"))?;
    if key.is_empty() {
        anyhow::bail!("invalid key: '{token}'");
    }

    let mut mods = Mods::default();
    for prefix in prefixes {
        match prefix.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "option" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "command" | "cmd" | "win" | "windows" => mods.super_key = true,
            "hyper" => mods.hyper = true,
            "meta" => mods.meta = true,
            other => anyhow::bail!("unknown modifier: {other}"),
        }
    }

    Ok(ParsedToken { key, mods })
}

fn kitty_sequences(mode: InputModes) -> bool {
    mode.keyboard.intersects(
        KeyboardMode::DISAMBIGUATE_ESC_CODES
            | KeyboardMode::REPORT_EVENT_TYPES
            | KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
    )
}

fn named(key: &str, mods: Mods, event: KeyEventKind, mode: InputModes) -> Option<String> {
    // `DECCKM` swaps the cursor keys' introducer from `CSI` to `SS3`, and it
    // reaches exactly the keys `CSI ... A-D`, `H`, and `F` spell: the arrows
    // plus Home and End. Only an unmodified press changes, which is already
    // what `ss3_unmodified` means for F1-F4, so it reuses that rather than
    // adding a second rule that could drift from it.
    let ss3 = mode.cursor_key_application;
    let sequence = match key.to_ascii_lowercase().as_str() {
        "home" => navigation(None, 'H', ss3, mods, event, mode),
        "end" => navigation(None, 'F', ss3, mods, event, mode),
        "up" => navigation(None, 'A', ss3, mods, event, mode),
        "down" => navigation(None, 'B', ss3, mods, event, mode),
        "right" => navigation(None, 'C', ss3, mods, event, mode),
        "left" => navigation(None, 'D', ss3, mods, event, mode),
        "pageup" => navigation(Some(5), '~', false, mods, event, mode),
        "pagedown" => navigation(Some(6), '~', false, mods, event, mode),
        "insert" => navigation(Some(2), '~', false, mods, event, mode),
        "delete" => navigation(Some(3), '~', false, mods, event, mode),
        "backspace" => control_key(ControlKey::Backspace, mods, event, mode),
        "tab" => control_key(ControlKey::Tab, mods, event, mode),
        "enter" | "return" => control_key(ControlKey::Enter, mods, event, mode),
        "space" => character(' ', mods, event, mode),
        "escape" | "esc" => control_key(ControlKey::Escape, mods, event, mode),
        "f1" => navigation(None, 'P', true, mods, event, mode),
        "f2" => navigation(None, 'Q', true, mods, event, mode),
        "f3" if kitty_sequences(mode) => navigation(Some(13), '~', false, mods, event, mode),
        "f3" => navigation(None, 'R', true, mods, event, mode),
        "f4" => navigation(None, 'S', true, mods, event, mode),
        "f5" => navigation(Some(15), '~', false, mods, event, mode),
        "f6" => navigation(Some(17), '~', false, mods, event, mode),
        "f7" => navigation(Some(18), '~', false, mods, event, mode),
        "f8" => navigation(Some(19), '~', false, mods, event, mode),
        "f9" => navigation(Some(20), '~', false, mods, event, mode),
        "f10" => navigation(Some(21), '~', false, mods, event, mode),
        "f11" => navigation(Some(23), '~', false, mods, event, mode),
        "f12" => navigation(Some(24), '~', false, mods, event, mode),
        _ => return None,
    };
    Some(sequence)
}

fn navigation(
    base: Option<u16>,
    terminator: char,
    ss3_unmodified: bool,
    mods: Mods,
    event: KeyEventKind,
    mode: InputModes,
) -> String {
    let report_events = mode.keyboard.contains(KeyboardMode::REPORT_EVENT_TYPES);
    if event == KeyEventKind::Release && !report_events {
        return String::new();
    }

    let event_code = report_events.then(|| event.code()).flatten();
    let details = mods.any() || event_code.is_some();
    if ss3_unmodified && !kitty_sequences(mode) && !details {
        return format!("{ESC}O{terminator}");
    }

    let mut sequence = String::from(CSI);
    match base {
        Some(base) => sequence.push_str(&base.to_string()),
        None if details => sequence.push('1'),
        None => {}
    }
    if details {
        sequence.push(';');
        sequence.push_str(&mods.param().to_string());
        if let Some(event_code) = event_code {
            sequence.push(':');
            sequence.push_str(&event_code.to_string());
        }
    }
    sequence.push(terminator);
    sequence
}

#[derive(Clone, Copy)]
enum ControlKey {
    Enter,
    Escape,
    Tab,
    Backspace,
}

impl ControlKey {
    fn codepoint(self) -> u32 {
        match self {
            ControlKey::Enter => 13,
            ControlKey::Escape => 27,
            ControlKey::Tab => 9,
            ControlKey::Backspace => 127,
        }
    }

    fn is_safety_key(self) -> bool {
        !matches!(self, ControlKey::Escape)
    }
}

fn control_key(key: ControlKey, mods: Mods, event: KeyEventKind, mode: InputModes) -> String {
    let report_events = mode.keyboard.contains(KeyboardMode::REPORT_EVENT_TYPES);
    let report_all = mode.keyboard.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC);
    if event == KeyEventKind::Release && !report_events {
        return String::new();
    }

    let disambiguate = mode.keyboard.contains(KeyboardMode::DISAMBIGUATE_ESC_CODES);
    if matches!(key, ControlKey::Escape)
        && event == KeyEventKind::Press
        && !mods.any()
        && !disambiguate
        && !report_all
    {
        return ESC.to_string();
    }

    let legacy_mode = !kitty_sequences(mode);
    if legacy_mode && !mods.has_kitty_only_modifier() {
        return if event == KeyEventKind::Release {
            String::new()
        } else {
            legacy_control_key(key, mods)
        };
    }

    let safety_path = key.is_safety_key() && !report_all && !mods.any();
    if safety_path {
        return if event == KeyEventKind::Release {
            String::new()
        } else {
            legacy_control_key(key, mods)
        };
    }

    kitty_u(key.codepoint().to_string(), mods, event, mode, None)
}

fn legacy_control_key(key: ControlKey, mods: Mods) -> String {
    let mut sequence = String::new();
    match key {
        ControlKey::Enter => {
            if mods.alt {
                sequence.push_str(ESC);
            }
            sequence.push('\r');
        }
        ControlKey::Escape => {
            if mods.alt {
                sequence.push_str(ESC);
            }
            sequence.push_str(ESC);
        }
        ControlKey::Tab if mods.shift => {
            if mods.alt {
                sequence.push_str(ESC);
            }
            sequence.push_str("\u{1b}[Z");
        }
        ControlKey::Tab => {
            if mods.alt {
                sequence.push_str(ESC);
            }
            sequence.push('\t');
        }
        ControlKey::Backspace => {
            if mods.alt {
                sequence.push_str(ESC);
            }
            sequence.push(if mods.ctrl { '\u{8}' } else { '\u{7f}' });
        }
    }
    sequence
}

fn legacy_character(ch: char, mods: Mods) -> Option<String> {
    if mods.has_kitty_only_modifier() {
        return None;
    }

    let (base, mut text) = key_chars(ch, mods);
    if mods.ctrl && mods.shift && (mods.alt || base != ' ') {
        return None;
    }
    if mods.ctrl {
        if !text.is_ascii() {
            return None;
        }
        text = legacy_ctrl(text);
    }

    let mut sequence = String::new();
    if mods.alt {
        sequence.push_str(ESC);
    }
    sequence.push(text);
    Some(sequence)
}

fn key_chars(ch: char, mods: Mods) -> (char, char) {
    let base = if ch.is_ascii_alphabetic() {
        ch.to_ascii_lowercase()
    } else if mods.shift {
        unshift_ascii(ch).unwrap_or(ch)
    } else {
        ch
    };
    let text = if mods.shift {
        shift_ascii(base).unwrap_or(base)
    } else {
        ch
    };
    (base, text)
}

fn shift_ascii(ch: char) -> Option<char> {
    Some(match ch {
        'a'..='z' => ch.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        '`' => '~',
        _ => return None,
    })
}

fn unshift_ascii(ch: char) -> Option<char> {
    Some(match ch {
        'A'..='Z' => ch.to_ascii_lowercase(),
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => return None,
    })
}

fn legacy_ctrl(ch: char) -> char {
    match ch {
        ' ' | '2' | '@' => '\0',
        'a'..='z' => char::from_u32(ch as u32 - 'a' as u32 + 1).unwrap_or(ch),
        'A'..='Z' => char::from_u32(ch as u32 - 'A' as u32 + 1).unwrap_or(ch),
        '3' | '[' => '\u{1b}',
        '4' | '\\' => '\u{1c}',
        '5' | ']' => '\u{1d}',
        '6' | '^' | '~' => '\u{1e}',
        '7' | '/' | '_' => '\u{1f}',
        '8' | '?' => '\u{7f}',
        _ => ch,
    }
}

fn associated_text(ch: char, mods: Mods) -> Option<String> {
    let (_, mut text) = key_chars(ch, mods);
    if mods.ctrl {
        text = legacy_ctrl(text);
    }
    if is_control(text) {
        None
    } else {
        Some(text.to_string())
    }
}

fn is_control(ch: char) -> bool {
    let codepoint = ch as u32;
    codepoint < 0x20 || (0x7f..=0x9f).contains(&codepoint)
}

fn literal_key(ch: char) -> (char, Mods) {
    match unshift_ascii(ch) {
        Some(key) => (
            key,
            Mods {
                shift: true,
                ..Mods::default()
            },
        ),
        None => (ch, Mods::default()),
    }
}

fn character(ch: char, mods: Mods, event: KeyEventKind, mode: InputModes) -> String {
    let report_events = mode.keyboard.contains(KeyboardMode::REPORT_EVENT_TYPES);
    if event == KeyEventKind::Release && !report_events {
        return String::new();
    }

    let legacy = legacy_character(ch, mods);
    // Text-producing keys need report-all mode before repeat/release events
    // can be represented separately from their UTF-8 text.
    let produces_text = !mods.disambiguates_character();
    let escape_encoded = mode.keyboard.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC)
        || (report_events && event != KeyEventKind::Press && !produces_text)
        || (mode.keyboard.contains(KeyboardMode::DISAMBIGUATE_ESC_CODES)
            && mods.disambiguates_character())
        || legacy.is_none();
    if event == KeyEventKind::Release && !escape_encoded {
        return String::new();
    }
    if !escape_encoded {
        return legacy.unwrap_or_default();
    }

    let (base, text) = key_chars(ch, mods);
    let payload = if mode.keyboard.contains(KeyboardMode::REPORT_ALTERNATE_KEYS)
        && mods.shift
        && text != base
    {
        format!("{}:{}", base as u32, text as u32)
    } else {
        (base as u32).to_string()
    };
    let associated = associated_text(ch, mods);
    kitty_u(payload, mods, event, mode, associated.as_deref())
}

fn kitty_u(
    payload: String,
    mods: Mods,
    event: KeyEventKind,
    mode: InputModes,
    associated_text: Option<&str>,
) -> String {
    let event_code = mode
        .keyboard
        .contains(KeyboardMode::REPORT_EVENT_TYPES)
        .then(|| event.code())
        .flatten();
    let associated_text = associated_text.filter(|text| {
        mode.keyboard.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT)
            && event != KeyEventKind::Release
            && text.chars().all(|ch| !is_control(ch))
    });

    let mut sequence = format!("{CSI}{payload}");
    if mods.any() || event_code.is_some() || associated_text.is_some() {
        sequence.push(';');
        sequence.push_str(&mods.param().to_string());
    }
    if let Some(event_code) = event_code {
        sequence.push(':');
        sequence.push_str(&event_code.to_string());
    }
    if let Some(text) = associated_text {
        sequence.push(';');
        let mut codepoints = text.chars().map(u32::from);
        if let Some(codepoint) = codepoints.next() {
            sequence.push_str(&codepoint.to_string());
        }
        for codepoint in codepoints {
            sequence.push(':');
            sequence.push_str(&codepoint.to_string());
        }
    }
    sequence.push('u');
    sequence
}

fn action_events(action: KeyAction) -> &'static [KeyEventKind] {
    match action {
        KeyAction::Press => &[KeyEventKind::Press, KeyEventKind::Release],
        KeyAction::Down => &[KeyEventKind::Press],
        KeyAction::Repeat => &[KeyEventKind::Repeat],
        KeyAction::Up => &[KeyEventKind::Release],
    }
}

/// One key event, as the encoder sees it before any escape sequence exists.
///
/// This is the hand-off point for [`crate::terminal::emu::Emulator::encode_key`]:
/// a backend with its own encoder is given this rather than a token, so it
/// never has to know tui-test's spelling of a key name.
#[derive(Debug, Clone)]
pub struct KeyPress {
    /// The key's name. Either one of the named keys, lowercased (`"up"`,
    /// `"f5"`, `"enter"`), or the single *unshifted* character the key sits
    /// on, so `!` arrives as `"1"` with `mods.shift`.
    ///
    /// Owned rather than borrowed: a key name is a handful of bytes, and the
    /// ghostty backend has to move it to the thread that owns its terminal.
    pub key: CompactString,
    pub mods: Mods,
    pub event: KeyEventKind,
    /// The text this key produces, for the keys that produce any.
    ///
    /// Carried rather than derived because deriving it needs a keyboard
    /// layout: `Shift+1` is `!` on a US layout and `"` on a UK one. tui-test
    /// works in the US layout its token vocabulary already assumes, and a
    /// backend encoder is handed the answer rather than asked to guess.
    pub text: Option<CompactString>,
}

/// Split a token into the key events an action produces.
///
/// Exposed so a caller routing to a backend encoder can hand it the same
/// events the shared encoder would have encoded, rather than re-parsing the
/// token itself and drifting from this one.
pub fn token_to_presses(token: &str, action: KeyAction) -> anyhow::Result<Vec<KeyPress>> {
    if token.is_empty() {
        return Ok(Vec::new());
    }
    let parsed = parse_token(token)?;
    // A multi-character token that is not a named key is a literal string,
    // typed one character at a time; the backend encoder is handed each
    // character as its own press.
    let is_named = named(
        parsed.key,
        parsed.mods,
        KeyEventKind::Press,
        InputModes::default(),
    )
    .is_some();

    // Mirrors how `token_to_seq_for_action_with_mode` classifies the same
    // token, so both encoders are handed the same events. A named key keeps
    // its name; a character is reduced to the key it sits on plus the text it
    // produces, which is what an encoder needs to know.
    let keys: Vec<(CompactString, Mods, Option<CompactString>)> = if is_named {
        let text = (parsed.key.eq_ignore_ascii_case("space")).then(|| " ".into());
        vec![(parsed.key.to_ascii_lowercase().into(), parsed.mods, text)]
    } else if parsed.key.chars().count() == 1 {
        let ch = parsed.key.chars().next().expect("one character");
        let (key, mods) = if parsed.mods.any() {
            (ch, parsed.mods)
        } else {
            literal_key(ch)
        };
        vec![(
            key.to_compact_string(),
            mods,
            Some(produced_text(key, mods)),
        )]
    } else if parsed.mods.any() {
        anyhow::bail!("invalid key: '{}'", parsed.key);
    } else {
        parsed
            .key
            .chars()
            .map(|ch| {
                let (key, mods) = literal_key(ch);
                (
                    key.to_compact_string(),
                    mods,
                    Some(produced_text(key, mods)),
                )
            })
            .collect()
    };

    let mut out = Vec::new();
    for (key, mods, text) in keys {
        for &event in action_events(action) {
            out.push(KeyPress {
                key: key.clone(),
                mods,
                event,
                text: text.clone(),
            });
        }
    }
    Ok(out)
}

/// The text an unshifted key plus its modifiers actually produces.
fn produced_text(key: char, mods: Mods) -> CompactString {
    match mods.shift.then(|| shift_ascii(key)).flatten() {
        Some(shifted) => shifted.to_compact_string(),
        None => key.to_compact_string(),
    }
}

/// Translate one key token and action using the active terminal keyboard mode.
pub fn token_to_seq_for_action_with_mode(
    token: &str,
    action: KeyAction,
    mode: impl Into<InputModes>,
) -> anyhow::Result<String> {
    let mode = mode.into();
    if token.is_empty() {
        return Ok(String::new());
    }

    let parsed = parse_token(token)?;
    let events = action_events(action);
    let (&first_event, remaining_events) = events
        .split_first()
        .expect("every key action has at least one event");
    if let Some(sequence) = named(parsed.key, parsed.mods, first_event, mode) {
        let mut out = String::new();
        out.push_str(&sequence);
        for &event in remaining_events {
            out.push_str(
                &named(parsed.key, parsed.mods, event, mode)
                    .expect("named key classification does not depend on event type"),
            );
        }
        return Ok(out);
    }

    let mut chars = parsed.key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        let (key, mods) = if parsed.mods.any() {
            (ch, parsed.mods)
        } else {
            literal_key(ch)
        };
        let mut out = String::new();
        for &event in events {
            out.push_str(&character(key, mods, event, mode));
        }
        return Ok(out);
    }

    if parsed.mods.any() {
        anyhow::bail!("invalid key: '{}'", parsed.key);
    }
    let mut out = String::new();
    for ch in parsed.key.chars() {
        let (key, mods) = literal_key(ch);
        for &event in events {
            out.push_str(&character(key, mods, event, mode));
        }
    }
    Ok(out)
}

/// Translate one key-down event using the active terminal keyboard mode.
pub fn token_to_seq_with_mode(token: &str, mode: impl Into<InputModes>) -> anyhow::Result<String> {
    token_to_seq_for_action_with_mode(token, KeyAction::Down, mode)
}

/// Translate key tokens and an action using the active terminal keyboard mode.
pub fn tokens_to_seq_for_action_with_mode(
    tokens: &[String],
    action: KeyAction,
    mode: impl Into<InputModes>,
) -> anyhow::Result<String> {
    let mode = mode.into();
    let mut out = String::new();
    for token in tokens {
        out.push_str(&token_to_seq_for_action_with_mode(token, action, mode)?);
    }
    Ok(out)
}

/// Translate key-down tokens using the active terminal keyboard mode.
pub fn tokens_to_seq_with_mode(
    tokens: &[String],
    mode: impl Into<InputModes>,
) -> anyhow::Result<String> {
    tokens_to_seq_for_action_with_mode(tokens, KeyAction::Down, mode)
}

/// Translate a single key-down token using legacy terminal input encoding.
pub fn token_to_seq(token: &str) -> anyhow::Result<String> {
    token_to_seq_with_mode(token, KeyboardMode::empty())
}

/// Translate key-down tokens using legacy terminal input encoding.
pub fn tokens_to_seq(tokens: &[String]) -> anyhow::Result<String> {
    tokens_to_seq_with_mode(tokens, KeyboardMode::empty())
}

#[cfg(test)]
mod decckm_tests {
    use super::*;

    fn app(token: &str) -> String {
        token_to_seq_with_mode(
            token,
            InputModes {
                keyboard: KeyboardMode::empty(),
                cursor_key_application: true,
            },
        )
        .unwrap()
    }

    /// `CSI ?1h` swaps the cursor keys onto `SS3`. readline, vim, and less all
    /// set it, so a session that kept sending `CSI A` was sending something no
    /// real terminal sends once the child had asked.
    #[test]
    fn application_cursor_keys_use_ss3() {
        for (token, expected) in [
            ("Up", "\x1bOA"),
            ("Down", "\x1bOB"),
            ("Right", "\x1bOC"),
            ("Left", "\x1bOD"),
            ("Home", "\x1bOH"),
            ("End", "\x1bOF"),
        ] {
            assert_eq!(app(token), expected, "{token} in application mode");
        }
    }

    /// Without the mode the cursor keys keep the `CSI` form, so this changes
    /// nothing for a child that never asked.
    #[test]
    fn normal_cursor_keys_are_unchanged() {
        for (token, expected) in [
            ("Up", "\x1b[A"),
            ("Down", "\x1b[B"),
            ("Right", "\x1b[C"),
            ("Left", "\x1b[D"),
            ("Home", "\x1b[H"),
            ("End", "\x1b[F"),
        ] {
            assert_eq!(token_to_seq(token).unwrap(), expected, "{token} normally");
        }
    }

    /// A modified cursor key carries its modifier parameter, which `SS3` has
    /// nowhere to put, so it stays `CSI` in application mode too.
    #[test]
    fn a_modified_cursor_key_stays_csi() {
        let modes = InputModes {
            keyboard: KeyboardMode::empty(),
            cursor_key_application: true,
        };
        assert_eq!(
            token_to_seq_with_mode("Ctrl+Up", modes).unwrap(),
            "\x1b[1;5A"
        );
        assert_eq!(
            token_to_seq_with_mode("Shift+Home", modes).unwrap(),
            "\x1b[1;2H"
        );
    }

    /// The Kitty protocol replaces the legacy encoding outright, and its
    /// cursor keys are `CSI` with a parameter, so `DECCKM` does not apply.
    #[test]
    fn kitty_encoding_outranks_application_mode() {
        let modes = InputModes {
            keyboard: KeyboardMode::DISAMBIGUATE_ESC_CODES,
            cursor_key_application: true,
        };
        assert_eq!(token_to_seq_with_mode("Up", modes).unwrap(), "\x1b[A");
    }

    /// Keys that are not cursor keys are untouched, including the function
    /// keys that have their own unrelated `SS3` form.
    #[test]
    fn application_mode_reaches_only_the_cursor_keys() {
        assert_eq!(app("PageUp"), "\x1b[5~");
        assert_eq!(app("Delete"), "\x1b[3~");
        assert_eq!(app("F1"), "\x1bOP", "F1 is SS3 either way");
        assert_eq!(app("F5"), "\x1b[15~");
        assert_eq!(app("a"), "a");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_seq(token: &str) -> anyhow::Result<String> {
        token_to_seq(token)
    }

    fn action_seq(
        token: &str,
        action: KeyAction,
        mode: impl Into<InputModes>,
    ) -> anyhow::Result<String> {
        token_to_seq_for_action_with_mode(token, action, mode)
    }

    #[test]
    fn named_keys() {
        assert_eq!(default_seq("Enter").unwrap(), "\r");
        assert_eq!(default_seq("Escape").unwrap(), "\u{1b}");
        assert_eq!(default_seq("Up").unwrap(), "\u{1b}[A");
        assert_eq!(default_seq("F5").unwrap(), "\u{1b}[15~");
    }

    #[test]
    fn ctrl_combos() {
        assert_eq!(default_seq("Ctrl+C").unwrap(), "\u{3}");
        assert_eq!(default_seq("Ctrl+a").unwrap(), "\u{1}");
    }

    #[test]
    fn preserves_aliases_and_distinguishes_alt_from_meta() {
        assert_eq!(default_seq("Control+a").unwrap(), "\u{1}");
        assert_eq!(default_seq("Option+a").unwrap(), "\u{1b}a");
        assert_eq!(default_seq("Meta+a").unwrap(), "\u{1b}[97;33u");
        assert_eq!(default_seq("Return").unwrap(), "\r");
        assert_eq!(default_seq("Esc").unwrap(), "\u{1b}");
        assert_eq!(default_seq("hello").unwrap(), "hello");
    }

    #[test]
    fn literal_tokens_follow_the_active_keyboard_mode() {
        let report_all = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(
            token_to_seq_with_mode("hello", report_all).unwrap(),
            "\u{1b}[104u\u{1b}[101u\u{1b}[108u\u{1b}[108u\u{1b}[111u"
        );
        assert_eq!(
            token_to_seq_with_mode("A", report_all).unwrap(),
            "\u{1b}[97;2u"
        );
        assert_eq!(
            token_to_seq_with_mode("!", report_all).unwrap(),
            "\u{1b}[49;2u"
        );
        assert_eq!(
            token_to_seq_with_mode(":", report_all).unwrap(),
            "\u{1b}[59;2u"
        );

        assert_eq!(default_seq("A").unwrap(), "A");
        assert_eq!(default_seq("!").unwrap(), "!");
        assert_eq!(default_seq(":").unwrap(), ":");
    }

    #[test]
    fn rejects_unsupported_lock_modifiers() {
        for token in ["CapsLock+a", "Caps_Lock+a", "NumLock+a", "Num_Lock+a"] {
            assert!(default_seq(token).is_err(), "{token}");
        }
    }

    #[test]
    fn literals() {
        assert_eq!(default_seq(":").unwrap(), ":");
        assert_eq!(default_seq("w").unwrap(), "w");
    }

    #[test]
    fn sequence() {
        let toks: Vec<String> = ["Escape", ":", "w", "q", "Enter"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(tokens_to_seq(&toks).unwrap(), "\u{1b}:wq\r");
    }

    #[test]
    fn legacy_c0_modifier_table_matches_kitty() {
        for (token, expected) in [
            ("Ctrl+Enter", "\r"),
            ("Alt+Enter", "\u{1b}\r"),
            ("Shift+Tab", "\u{1b}[Z"),
            ("Ctrl+Backspace", "\u{8}"),
            ("Alt+Escape", "\u{1b}\u{1b}"),
            ("Ctrl+Shift+Space", "\0"),
        ] {
            assert_eq!(default_seq(token).unwrap(), expected, "{token}");
            assert_eq!(
                action_seq(token, KeyAction::Repeat, KeyboardMode::empty()).unwrap(),
                expected
            );
            assert_eq!(
                action_seq(token, KeyAction::Up, KeyboardMode::empty()).unwrap(),
                ""
            );
        }
    }

    #[test]
    fn c0_safety_keys_stay_legacy_until_report_all() {
        for mode in [
            KeyboardMode::empty(),
            KeyboardMode::DISAMBIGUATE_ESC_CODES,
            KeyboardMode::REPORT_EVENT_TYPES,
        ] {
            for (key, legacy) in [("Enter", "\r"), ("Tab", "\t"), ("Backspace", "\u{7f}")] {
                assert_eq!(token_to_seq_with_mode(key, mode).unwrap(), legacy);
                assert_eq!(action_seq(key, KeyAction::Repeat, mode).unwrap(), legacy);
                assert_eq!(action_seq(key, KeyAction::Up, mode).unwrap(), "");
            }
        }

        let report_all = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
        for (key, codepoint) in [("Enter", 13), ("Tab", 9), ("Backspace", 127)] {
            let expected = format!("{CSI}{codepoint}u");
            assert_eq!(token_to_seq_with_mode(key, report_all).unwrap(), expected);
            assert_eq!(
                action_seq(key, KeyAction::Repeat, report_all).unwrap(),
                expected
            );
            assert_eq!(action_seq(key, KeyAction::Up, report_all).unwrap(), "");
        }

        let disambiguate = KeyboardMode::DISAMBIGUATE_ESC_CODES;
        for (token, expected) in [
            ("Ctrl+Enter", "\u{1b}[13;5u"),
            ("Alt+Enter", "\u{1b}[13;3u"),
            ("Shift+Tab", "\u{1b}[9;2u"),
            ("Ctrl+Backspace", "\u{1b}[127;5u"),
            ("Alt+Escape", "\u{1b}[27;3u"),
            ("Ctrl+Shift+Space", "\u{1b}[32;6u"),
        ] {
            assert_eq!(
                token_to_seq_with_mode(token, disambiguate).unwrap(),
                expected,
                "{token}"
            );
        }

        let enhanced_events = KeyboardMode::REPORT_EVENT_TYPES;
        assert_eq!(
            token_to_seq_with_mode("Ctrl+Enter", enhanced_events).unwrap(),
            "\u{1b}[13;5u"
        );
        assert_eq!(
            action_seq("Ctrl+Enter", KeyAction::Up, enhanced_events).unwrap(),
            "\u{1b}[13;5:3u"
        );
        assert_eq!(
            action_seq("Ctrl+Enter", KeyAction::Repeat, enhanced_events).unwrap(),
            "\u{1b}[13;5:2u"
        );
    }

    #[test]
    fn kitty_disambiguates_ambiguous_keys() {
        let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(
            token_to_seq_with_mode("Ctrl+i", mode).unwrap(),
            "\u{1b}[105;5u"
        );
        assert_eq!(token_to_seq_with_mode("Tab", mode).unwrap(), "\t");
        assert_eq!(token_to_seq_with_mode("Enter", mode).unwrap(), "\r");
        assert_eq!(token_to_seq_with_mode("Backspace", mode).unwrap(), "\u{7f}");
        assert_eq!(
            token_to_seq_with_mode("Escape", mode).unwrap(),
            "\u{1b}[27u"
        );
        assert_eq!(token_to_seq_with_mode("a", mode).unwrap(), "a");
        assert_eq!(token_to_seq_with_mode("Shift+a", mode).unwrap(), "A");
    }

    #[test]
    fn kitty_reports_all_keys_with_alternates_and_text() {
        let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC
            | KeyboardMode::REPORT_ALTERNATE_KEYS
            | KeyboardMode::REPORT_ASSOCIATED_TEXT;
        assert_eq!(
            token_to_seq_with_mode("Shift+a", mode).unwrap(),
            "\u{1b}[97:65;2;65u"
        );
        assert_eq!(
            token_to_seq_with_mode("a", mode).unwrap(),
            "\u{1b}[97;1;97u"
        );
        assert_eq!(
            token_to_seq_with_mode("Ctrl+c", mode).unwrap(),
            "\u{1b}[99;5u"
        );
        assert_eq!(
            token_to_seq_with_mode("Shift+1", mode).unwrap(),
            "\u{1b}[49:33;2;33u"
        );
    }

    fn assert_events(
        mode: impl Into<InputModes>,
        key: &str,
        press: &str,
        repeat: &str,
        release: &str,
    ) {
        let mode = mode.into();
        assert_eq!(token_to_seq_with_mode(key, mode).unwrap(), press, "{key}");
        assert_eq!(
            action_seq(key, KeyAction::Repeat, mode).unwrap(),
            repeat,
            "repeat {key}"
        );
        assert_eq!(
            action_seq(key, KeyAction::Up, mode).unwrap(),
            release,
            "release {key}"
        );
    }

    #[test]
    fn explicit_events_use_csi_u_when_event_reporting_is_enabled() {
        let events = KeyboardMode::REPORT_EVENT_TYPES;
        assert_events(events, "a", "a", "a", "");
        assert_events(
            events,
            "Ctrl+a",
            "\u{1}",
            "\u{1b}[97;5:2u",
            "\u{1b}[97;5:3u",
        );
        assert_events(events, "Up", "\u{1b}[A", "\u{1b}[1;1:2A", "\u{1b}[1;1:3A");
        assert_events(events, "Enter", "\r", "\r", "");

        let disambiguated_events = events | KeyboardMode::DISAMBIGUATE_ESC_CODES;
        assert_events(disambiguated_events, "a", "a", "a", "");
        assert_events(
            disambiguated_events,
            "Ctrl+a",
            "\u{1b}[97;5u",
            "\u{1b}[97;5:2u",
            "\u{1b}[97;5:3u",
        );
        assert_events(
            disambiguated_events,
            "Up",
            "\u{1b}[A",
            "\u{1b}[1;1:2A",
            "\u{1b}[1;1:3A",
        );
        assert_events(disambiguated_events, "Enter", "\r", "\r", "");

        let all_events = events | KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
        assert_events(
            all_events,
            "a",
            "\u{1b}[97u",
            "\u{1b}[97;1:2u",
            "\u{1b}[97;1:3u",
        );
        assert_events(
            all_events,
            "Ctrl+a",
            "\u{1b}[97;5u",
            "\u{1b}[97;5:2u",
            "\u{1b}[97;5:3u",
        );
        assert_events(
            all_events,
            "Up",
            "\u{1b}[A",
            "\u{1b}[1;1:2A",
            "\u{1b}[1;1:3A",
        );
        assert_events(
            all_events,
            "Enter",
            "\u{1b}[13u",
            "\u{1b}[13;1:2u",
            "\u{1b}[13;1:3u",
        );
    }

    #[test]
    fn escape_disambiguation_is_independent_from_event_reporting() {
        assert_events(KeyboardMode::empty(), "Escape", ESC, ESC, "");
        assert_events(
            KeyboardMode::DISAMBIGUATE_ESC_CODES,
            "Escape",
            "\u{1b}[27u",
            "\u{1b}[27u",
            "",
        );
        assert_events(
            KeyboardMode::REPORT_EVENT_TYPES,
            "Escape",
            ESC,
            "\u{1b}[27;1:2u",
            "\u{1b}[27;1:3u",
        );
        assert_events(
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_EVENT_TYPES,
            "Escape",
            "\u{1b}[27u",
            "\u{1b}[27;1:2u",
            "\u{1b}[27;1:3u",
        );
        assert_events(
            KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
            "Escape",
            "\u{1b}[27u",
            "\u{1b}[27u",
            "",
        );
    }

    #[test]
    fn unrepresentable_legacy_modifiers_fall_back_to_csi_u() {
        assert_eq!(default_seq("Super+a").unwrap(), "\u{1b}[97;9u");
        assert_eq!(default_seq("Hyper+a").unwrap(), "\u{1b}[97;17u");
        assert_eq!(default_seq("Meta+a").unwrap(), "\u{1b}[97;33u");
        assert_eq!(default_seq("Ctrl+Shift+a").unwrap(), "\u{1b}[97;6u");
        assert_eq!(default_seq("Ctrl+Shift+1").unwrap(), "\u{1b}[49;6u");

        assert_eq!(default_seq("Alt+Shift+a").unwrap(), "\u{1b}A");
        assert_eq!(default_seq("Ctrl+Alt+a").unwrap(), "\u{1b}\u{1}");
    }

    #[test]
    fn shift_and_ctrl_use_defined_ascii_key_mappings() {
        assert_eq!(default_seq("Shift+1").unwrap(), "!");
        assert_eq!(default_seq("Shift+=").unwrap(), "+");
        assert_eq!(default_seq("Shift+!").unwrap(), "!");
        assert_eq!(default_seq("Ctrl+2").unwrap(), "\0");
        assert_eq!(default_seq("Ctrl+3").unwrap(), "\u{1b}");
        assert_eq!(default_seq("Ctrl+8").unwrap(), "\u{7f}");
        assert_eq!(default_seq("Ctrl+/").unwrap(), "\u{1f}");
    }

    #[test]
    fn legacy_repeat_is_another_press_and_release_is_silent() {
        assert_eq!(
            action_seq("Ctrl+C", KeyAction::Repeat, KeyboardMode::empty()).unwrap(),
            "\u{3}"
        );
        assert_eq!(
            action_seq("Ctrl+C", KeyAction::Up, KeyboardMode::empty()).unwrap(),
            ""
        );
    }

    #[test]
    fn press_reports_release_only_when_requested_and_representable() {
        assert_eq!(
            action_seq("Up", KeyAction::Press, KeyboardMode::empty()).unwrap(),
            "\u{1b}[A"
        );

        let events = KeyboardMode::REPORT_EVENT_TYPES;
        assert_eq!(
            action_seq("Up", KeyAction::Press, events).unwrap(),
            "\u{1b}[A\u{1b}[1;1:3A"
        );
        assert_eq!(action_seq("a", KeyAction::Press, events).unwrap(), "a");

        let all_events = events | KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(
            action_seq("a", KeyAction::Press, all_events).unwrap(),
            "\u{1b}[97u\u{1b}[97;1:3u"
        );
        assert_eq!(
            action_seq("ab", KeyAction::Press, all_events).unwrap(),
            "\u{1b}[97u\u{1b}[97;1:3u\u{1b}[98u\u{1b}[98;1:3u"
        );
    }

    #[test]
    fn kitty_uses_function_key_encoding() {
        let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(token_to_seq_with_mode("F1", mode).unwrap(), "\u{1b}[P");
        assert_eq!(token_to_seq_with_mode("F3", mode).unwrap(), "\u{1b}[13~");
    }

    #[test]
    fn rejects_event_prefixes_in_key_tokens() {
        let error = default_seq("Release+a").unwrap_err();
        assert!(error.to_string().contains("unknown modifier: release"));
    }
}
