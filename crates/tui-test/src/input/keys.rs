//! Key-name and key-event to terminal input sequence mapping.

use crate::terminal::emu::KeyboardMode;

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

#[derive(Default, Clone, Copy)]
struct Mods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
    hyper: bool,
    meta: bool,
    caps_lock: bool,
    num_lock: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.ctrl
            || self.alt
            || self.shift
            || self.super_key
            || self.hyper
            || self.meta
            || self.caps_lock
            || self.num_lock
    }

    fn disambiguates_character(self) -> bool {
        self.ctrl || self.alt || self.super_key || self.hyper || self.meta
    }

    /// Kitty and xterm modifier parameters are one plus the modifier bitfield.
    fn param(self) -> u16 {
        1 + self.shift as u16
            + 2 * self.alt as u16
            + 4 * self.ctrl as u16
            + 8 * self.super_key as u16
            + 16 * self.hyper as u16
            + 32 * self.meta as u16
            + 64 * self.caps_lock as u16
            + 128 * self.num_lock as u16
    }

    fn legacy_alt(self) -> bool {
        self.alt || self.meta
    }
}

struct ParsedToken<'a> {
    key: &'a str,
    mods: Mods,
    event: KeyEventKind,
}

fn parse_token(token: &str) -> anyhow::Result<ParsedToken<'_>> {
    if !token.contains('+') || token.len() == 1 {
        return Ok(ParsedToken {
            key: token,
            mods: Mods::default(),
            event: KeyEventKind::Press,
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
    let mut event = None;
    for prefix in prefixes {
        match prefix.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "option" | "meta" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "command" | "cmd" | "win" | "windows" => mods.super_key = true,
            "hyper" => mods.hyper = true,
            "kittymeta" | "kitty_meta" => mods.meta = true,
            "capslock" | "caps_lock" => mods.caps_lock = true,
            "numlock" | "num_lock" => mods.num_lock = true,
            "press" => set_event(&mut event, KeyEventKind::Press)?,
            "repeat" => set_event(&mut event, KeyEventKind::Repeat)?,
            "release" => set_event(&mut event, KeyEventKind::Release)?,
            other => anyhow::bail!("unknown modifier or event type: {other}"),
        }
    }

    Ok(ParsedToken {
        key,
        mods,
        event: event.unwrap_or_default(),
    })
}

fn set_event(current: &mut Option<KeyEventKind>, event: KeyEventKind) -> anyhow::Result<()> {
    if current.replace(event).is_some() {
        anyhow::bail!("multiple key event types");
    }
    Ok(())
}

fn kitty_sequences(mode: KeyboardMode) -> bool {
    mode.intersects(
        KeyboardMode::DISAMBIGUATE_ESC_CODES
            | KeyboardMode::REPORT_EVENT_TYPES
            | KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
    )
}

fn named(key: &str, mods: Mods, event: KeyEventKind, mode: KeyboardMode) -> Option<String> {
    let sequence = match key.to_ascii_lowercase().as_str() {
        "home" => navigation(None, 'H', false, mods, event, mode),
        "end" => navigation(None, 'F', false, mods, event, mode),
        "up" => navigation(None, 'A', false, mods, event, mode),
        "down" => navigation(None, 'B', false, mods, event, mode),
        "right" => navigation(None, 'C', false, mods, event, mode),
        "left" => navigation(None, 'D', false, mods, event, mode),
        "pageup" => navigation(Some(5), '~', false, mods, event, mode),
        "pagedown" => navigation(Some(6), '~', false, mods, event, mode),
        "insert" => navigation(Some(2), '~', false, mods, event, mode),
        "delete" => navigation(Some(3), '~', false, mods, event, mode),
        "backspace" => control_key(127, "\u{7f}", false, true, mods, event, mode),
        "tab" => control_key(9, "\t", false, true, mods, event, mode),
        "enter" | "return" => control_key(13, "\r", false, true, mods, event, mode),
        "space" => control_key(32, " ", true, false, mods, event, mode),
        "escape" | "esc" => control_key(27, ESC, false, false, mods, event, mode),
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
    mode: KeyboardMode,
) -> String {
    let report_events = mode.contains(KeyboardMode::REPORT_EVENT_TYPES);
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

fn control_key(
    codepoint: u32,
    legacy: &str,
    textual: bool,
    suppress_release_without_all: bool,
    mods: Mods,
    event: KeyEventKind,
    mode: KeyboardMode,
) -> String {
    let report_events = mode.contains(KeyboardMode::REPORT_EVENT_TYPES);
    let report_all = mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC);
    if event == KeyEventKind::Release && !report_events {
        return String::new();
    }
    if event == KeyEventKind::Release && suppress_release_without_all && !report_all {
        return String::new();
    }

    let disambiguate = mode.contains(KeyboardMode::DISAMBIGUATE_ESC_CODES);
    let encode = report_all
        || (!textual && kitty_sequences(mode))
        || (textual && disambiguate && mods.disambiguates_character())
        || (textual && event == KeyEventKind::Release && report_events);
    if encode {
        let associated = (textual && !mods.ctrl).then_some(legacy);
        return kitty_u(codepoint.to_string(), mods, event, mode, associated);
    }

    if mods.any() {
        format!("{CSI}27;{};{codepoint}~", mods.param())
    } else {
        legacy.to_string()
    }
}

fn char_combo(ch: char, mods: Mods) -> String {
    let mut ch = shifted_char(ch, mods);
    if mods.ctrl {
        let code = (ch.to_ascii_uppercase() as u32) & 0xff;
        if (0x40..=0x5f).contains(&code) {
            ch = char::from_u32(code - 0x40).unwrap_or(ch);
        }
    }

    if mods.legacy_alt() {
        format!("{ESC}{ch}")
    } else {
        ch.to_string()
    }
}

fn shifted_char(ch: char, mods: Mods) -> char {
    if mods.shift && ch.is_ascii_alphabetic() {
        ch.to_ascii_uppercase()
    } else {
        ch
    }
}

fn unshifted_char(ch: char) -> char {
    if ch.is_ascii_alphabetic() {
        ch.to_ascii_lowercase()
    } else {
        ch
    }
}

fn associated_text(ch: char, mods: Mods) -> Option<String> {
    let text = char_combo(
        ch,
        Mods {
            alt: false,
            meta: false,
            ..mods
        },
    );
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || is_control(ch) {
        None
    } else {
        Some(ch.to_string())
    }
}

fn is_control(ch: char) -> bool {
    let codepoint = ch as u32;
    codepoint < 0x20 || (0x7f..=0x9f).contains(&codepoint)
}

fn character(ch: char, mods: Mods, event: KeyEventKind, mode: KeyboardMode) -> String {
    let report_events = mode.contains(KeyboardMode::REPORT_EVENT_TYPES);
    if event == KeyEventKind::Release && !report_events {
        return String::new();
    }

    let encode = mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC)
        || (mode.contains(KeyboardMode::DISAMBIGUATE_ESC_CODES) && mods.disambiguates_character())
        || (event == KeyEventKind::Release && report_events);
    if !encode {
        return char_combo(ch, mods);
    }

    let base = unshifted_char(ch);
    let shifted = shifted_char(base, mods);
    let payload =
        if mode.contains(KeyboardMode::REPORT_ALTERNATE_KEYS) && mods.shift && shifted != base {
            format!("{}:{}", base as u32, shifted as u32)
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
    mode: KeyboardMode,
    associated_text: Option<&str>,
) -> String {
    let event_code = mode
        .contains(KeyboardMode::REPORT_EVENT_TYPES)
        .then(|| event.code())
        .flatten();
    let associated_text = associated_text.filter(|text| {
        mode.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT)
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

/// Translate a single `press` token such as `Enter`, `Ctrl+C`, or `Release+a`.
pub fn token_to_seq_with_mode(token: &str, mode: KeyboardMode) -> anyhow::Result<String> {
    if token.is_empty() {
        return Ok(String::new());
    }

    let parsed = parse_token(token)?;
    if let Some(sequence) = named(parsed.key, parsed.mods, parsed.event, mode) {
        return Ok(sequence);
    }

    let mut chars = parsed.key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Ok(character(ch, parsed.mods, parsed.event, mode));
    }

    if parsed.mods.any() || parsed.event != KeyEventKind::Press {
        anyhow::bail!("invalid key: '{}'", parsed.key);
    }
    Ok(parsed.key.to_string())
}

/// Translate a single `press` token using legacy terminal input encoding.
pub fn token_to_seq(token: &str) -> anyhow::Result<String> {
    token_to_seq_with_mode(token, KeyboardMode::empty())
}

/// Translate `press` tokens using the active terminal keyboard mode.
pub fn tokens_to_seq_with_mode(tokens: &[String], mode: KeyboardMode) -> anyhow::Result<String> {
    let mut out = String::new();
    for token in tokens {
        out.push_str(&token_to_seq_with_mode(token, mode)?);
    }
    Ok(out)
}

/// Translate `press` tokens using legacy terminal input encoding.
pub fn tokens_to_seq(tokens: &[String]) -> anyhow::Result<String> {
    tokens_to_seq_with_mode(tokens, KeyboardMode::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys() {
        assert_eq!(token_to_seq("Enter").unwrap(), "\r");
        assert_eq!(token_to_seq("Escape").unwrap(), "\u{1b}");
        assert_eq!(token_to_seq("Up").unwrap(), "\u{1b}[A");
        assert_eq!(token_to_seq("F5").unwrap(), "\u{1b}[15~");
    }

    #[test]
    fn ctrl_combos() {
        assert_eq!(token_to_seq("Ctrl+C").unwrap(), "\u{3}");
        assert_eq!(token_to_seq("Control+a").unwrap(), "\u{1}");
        assert_eq!(token_to_seq("Meta+a").unwrap(), "\u{1b}a");
        assert_eq!(token_to_seq("Meta+Up").unwrap(), "\u{1b}[1;3A");
    }

    #[test]
    fn literals() {
        assert_eq!(token_to_seq(":").unwrap(), ":");
        assert_eq!(token_to_seq("w").unwrap(), "w");
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
    fn kitty_disambiguates_control_keys() {
        let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(
            token_to_seq_with_mode("Ctrl+i", mode).unwrap(),
            "\u{1b}[105;5u"
        );
        assert_eq!(token_to_seq_with_mode("Tab", mode).unwrap(), "\u{1b}[9u");
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
    }

    #[test]
    fn kitty_reports_repeat_and_release_events() {
        let events = KeyboardMode::REPORT_EVENT_TYPES;
        assert_eq!(
            token_to_seq_with_mode("Repeat+Up", events).unwrap(),
            "\u{1b}[1;1:2A"
        );
        assert_eq!(
            token_to_seq_with_mode("Release+a", events).unwrap(),
            "\u{1b}[97;1:3u"
        );
        assert_eq!(token_to_seq_with_mode("Repeat+a", events).unwrap(), "a");
        assert_eq!(token_to_seq_with_mode("Release+Enter", events).unwrap(), "");

        let all_events = events | KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(
            token_to_seq_with_mode("Repeat+a", all_events).unwrap(),
            "\u{1b}[97;1:2u"
        );
        assert_eq!(
            token_to_seq_with_mode("Release+Enter", all_events).unwrap(),
            "\u{1b}[13;1:3u"
        );
    }

    #[test]
    fn legacy_repeat_is_another_press_and_release_is_silent() {
        assert_eq!(token_to_seq("Repeat+Ctrl+C").unwrap(), "\u{3}");
        assert_eq!(token_to_seq("Release+Ctrl+C").unwrap(), "");
    }

    #[test]
    fn kitty_uses_function_key_encoding() {
        let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(token_to_seq_with_mode("F1", mode).unwrap(), "\u{1b}[P");
        assert_eq!(token_to_seq_with_mode("F3", mode).unwrap(), "\u{1b}[13~");
    }

    #[test]
    fn rejects_conflicting_event_types() {
        let error = token_to_seq("Repeat+Release+a").unwrap_err();
        assert!(error.to_string().contains("multiple key event types"));
    }
}
