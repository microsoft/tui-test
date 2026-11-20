use std::path::Path;
use std::time::Duration;

use super::{
    Command, Error, ErrorKind, Key, Located, Location, ModifiedKey, Modifier, ScrollDirection,
    Setting, Tape, Theme, WaitScope,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    String,
    Number,
    Regex,
    Json,
    Symbol(char),
    Comment,
    End,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    location: Location,
}

impl Token {
    fn error(&self, kind: ErrorKind, message: impl Into<String>) -> Error {
        Error::new(&self.location, kind, message)
    }

    fn is_string(&self) -> bool {
        self.kind == Kind::String || (self.kind == Kind::Word && !reserved(&self.text))
    }
}

// Keep comments as tokens: they end a Type/Copy string group, unlike newlines.
fn lex(path: &Path, text: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    let mut location = Location::start(path);
    while let Some(ch) = chars.next() {
        let start = location.clone();
        advance(&mut location, ch);
        if matches!(ch, ' ' | '\t' | '\r' | '\n') {
            continue;
        }
        let mut value = String::new();
        let kind = match ch {
            '#' => {
                while let Some(&next) = chars.peek() {
                    if matches!(next, '\r' | '\n') {
                        break;
                    }
                    chars.next();
                    advance(&mut location, next);
                }
                Kind::Comment
            }
            '\'' | '"' | '`' | '/' => {
                let regex = ch == '/';
                let mut slashes = 0;
                let mut closed = false;
                for next in chars.by_ref() {
                    advance(&mut location, next);
                    if next == '\0' {
                        return Err(Error::new(
                            &start,
                            ErrorKind::InvalidValue,
                            "NUL is not allowed in quoted values or regular expressions",
                        ));
                    }
                    if matches!(next, '\r' | '\n') {
                        break;
                    }
                    if next == ch && (!regex || slashes % 2 == 0) {
                        closed = true;
                        break;
                    }
                    value.push(next);
                    slashes = if next == '\\' { slashes + 1 } else { 0 };
                }
                if !closed {
                    return Err(Error::new(
                        &start,
                        ErrorKind::Syntax,
                        if regex {
                            "unterminated /regex/"
                        } else {
                            "unterminated quoted string; quotes are literal, not shell escapes"
                        },
                    ));
                }
                if regex {
                    Kind::Regex
                } else {
                    Kind::String
                }
            }
            '{' => {
                value.push(ch);
                let mut closed = false;
                for next in chars.by_ref() {
                    advance(&mut location, next);
                    value.push(next);
                    if next == '}' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(Error::new(
                        &start,
                        ErrorKind::Syntax,
                        "unterminated Theme object",
                    ));
                }
                Kind::Json
            }
            ch if ch.is_ascii_digit()
                || (ch == '.' && chars.peek().is_some_and(char::is_ascii_digit)) =>
            {
                value.push(ch);
                while let Some(&next) = chars.peek() {
                    if !next.is_ascii_digit() && next != '.' {
                        break;
                    }
                    value.push(next);
                    chars.next();
                    advance(&mut location, next);
                }
                Kind::Number
            }
            ch if ch.is_ascii_alphabetic() || ch == '.' => {
                value.push(ch);
                while let Some(&next) = chars.peek() {
                    if !next.is_ascii_alphanumeric() && !matches!(next, '.' | '-' | '_' | '/' | '%')
                    {
                        break;
                    }
                    value.push(next);
                    chars.next();
                    advance(&mut location, next);
                }
                Kind::Word
            }
            '@' | '+' | '-' | '=' | '[' | ']' | '^' | '\\' | '%' => {
                value.push(ch);
                Kind::Symbol(ch)
            }
            _ => {
                return Err(Error::new(
                    &start,
                    ErrorKind::Syntax,
                    format!("unexpected character {ch:?}; quote literal text and paths"),
                ));
            }
        };
        tokens.push(Token {
            kind,
            text: value,
            location: start,
        });
    }
    tokens.push(Token {
        kind: Kind::End,
        text: String::new(),
        location,
    });
    Ok(tokens)
}

fn advance(location: &mut Location, ch: char) {
    if ch == '\n' {
        location.line += 1;
        location.column = 1;
    } else {
        location.column += 1;
    }
}

fn reserved(word: &str) -> bool {
    key(word).is_some()
        || modifier(word).is_some()
        || matches!(
            word,
            "Set"
                | "Sleep"
                | "Type"
                | "Hide"
                | "Show"
                | "Output"
                | "Require"
                | "Source"
                | "Wait"
                | "Screenshot"
                | "Copy"
                | "Paste"
                | "Env"
                | "ScrollUp"
                | "ScrollDown"
                | "End"
                | "Shell"
                | "FontFamily"
                | "FontSize"
                | "Framerate"
                | "PlaybackSpeed"
                | "TypingSpeed"
                | "Rows"
                | "Columns"
                | "Padding"
                | "CursorBlink"
                | "Height"
                | "Width"
                | "LetterSpacing"
                | "LineHeight"
                | "Theme"
                | "LoopOffset"
                | "MarginFill"
                | "Margin"
                | "WindowBar"
                | "WindowBarSize"
                | "BorderRadius"
                | "WaitTimeout"
                | "WaitPattern"
                | "true"
                | "false"
                | "ms"
                | "s"
                | "m"
                | "px"
                | "em"
        )
}

fn key(word: &str) -> Option<Key> {
    Some(match word {
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Space" => Key::Space,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        _ => return None,
    })
}

fn modifier(word: &str) -> Option<Modifier> {
    match word {
        "Ctrl" => Some(Modifier::Ctrl),
        "Alt" => Some(Modifier::Alt),
        "Shift" => Some(Modifier::Shift),
        _ => None,
    }
}

/// Parse a complete tape without filesystem access or process side effects.
///
/// Newlines are whitespace, not statement separators. Quotes are literal.
/// Source commands remain unresolved until [`super::resolve_sources`].
pub fn parse(path: impl AsRef<Path>, text: &str) -> Result<Tape, Error> {
    let mut parser = Parser {
        tokens: lex(path.as_ref(), text)?,
        position: 0,
    };
    let mut commands = Vec::new();
    while parser.peek().kind != Kind::End {
        if parser.peek().kind == Kind::Comment {
            parser.take();
            continue;
        }
        let token = parser.take();
        if token.kind != Kind::Word {
            return Err(token.error(ErrorKind::Syntax, "expected a command name"));
        }
        let value = parser.command(&token)?;
        commands.push(Located {
            location: token.location,
            value,
        });
    }
    Ok(Tape {
        origin: Location::start(path.as_ref()),
        commands,
    })
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn take(&mut self) -> Token {
        let token = self.peek().clone();
        if token.kind != Kind::End {
            self.position += 1;
        }
        token
    }

    fn symbol(&mut self, symbol: char) -> bool {
        if self.peek().kind == Kind::Symbol(symbol) {
            self.take();
            true
        } else {
            false
        }
    }

    fn string(&mut self, usage: &str) -> Result<Token, Error> {
        if !self.peek().is_string() {
            return Err(self.peek().error(ErrorKind::Syntax, usage));
        }
        Ok(self.take())
    }

    fn strings(&mut self, usage: &str) -> Result<String, Error> {
        let mut value = self.string(usage)?.text;
        while self.peek().is_string() {
            value.push(' ');
            value.push_str(&self.take().text);
        }
        Ok(value)
    }

    fn number(&mut self, usage: &str) -> Result<Token, Error> {
        let negative = self.symbol('-');
        if !matches!(self.peek().kind, Kind::Number | Kind::String) {
            return Err(self.peek().error(ErrorKind::InvalidValue, usage));
        }
        let mut token = self.take();
        if negative {
            token.text.insert(0, '-');
        }
        Ok(token)
    }

    fn unsigned<T: std::str::FromStr>(&mut self, usage: &str) -> Result<T, Error> {
        let token = self.number(usage)?;
        if !token.text.bytes().all(|ch| ch.is_ascii_digit()) || token.text.is_empty() {
            return Err(token.error(ErrorKind::InvalidValue, usage));
        }
        token
            .text
            .parse()
            .map_err(|_| token.error(ErrorKind::InvalidValue, usage))
    }

    fn float(&mut self, usage: &str, positive: bool) -> Result<f64, Error> {
        let token = self.number(usage)?;
        let value: f64 = token
            .text
            .parse()
            .map_err(|_| token.error(ErrorKind::InvalidValue, usage))?;
        if !value.is_finite() || (positive && value <= 0.0) {
            return Err(token.error(ErrorKind::InvalidValue, usage));
        }
        Ok(value)
    }

    fn time(&mut self, usage: &str, typing_setting: bool) -> Result<Duration, Error> {
        let token = if typing_setting {
            // The upstream setting accepts a quoted number, then appends a unit.
            self.number(usage)?
        } else {
            if self.peek().kind != Kind::Number {
                return Err(self.peek().error(ErrorKind::InvalidValue, usage));
            }
            self.take()
        };
        let unit = if self.peek().kind == Kind::Word
            && matches!(self.peek().text.as_str(), "ms" | "s" | "m")
        {
            if typing_setting && self.peek().text == "m" {
                return Err(self.peek().error(
                    ErrorKind::InvalidValue,
                    "Set TypingSpeed accepts bare seconds, ms, or s; use 60s instead of 1m",
                ));
            }
            self.take().text
        } else {
            "s".into()
        };
        duration(&format!("{}{unit}", token.text))
            .ok_or_else(|| token.error(ErrorKind::InvalidValue, usage))
    }

    fn speed(&mut self) -> Result<Option<Duration>, Error> {
        if self.symbol('@') {
            self.time(
                "expected a nonnegative duration after @ (seconds, ms, s, or m)",
                false,
            )
            .map(Some)
        } else {
            Ok(None)
        }
    }

    fn repeat(&mut self) -> Result<u64, Error> {
        if self.peek().kind == Kind::Number || self.peek().kind == Kind::Symbol('-') {
            self.unsigned("repeat count must be a nonnegative integer within u64")
        } else {
            Ok(1)
        }
    }

    fn command(&mut self, token: &Token) -> Result<Command, Error> {
        if let Some(key) = key(&token.text) {
            return Ok(Command::Key {
                key,
                delay: self.speed()?,
                count: self.repeat()?,
            });
        }
        if let Some(modifier) = modifier(&token.text) {
            return self.modified(modifier);
        }
        Ok(match token.text.as_str() {
            "Set" => Command::Set(self.setting()?),
            "Output" => {
                let path = self.string("Output expects one path; quote absolute paths")?;
                nonempty(&path, "Output path")?;
                let extension = Path::new(&path.text).extension();
                if extension.is_none() && !path.text.ends_with('/') {
                    return Err(path.error(
                        ErrorKind::InvalidValue,
                        "Output expects a file extension or a folder with a trailing slash",
                    ));
                }
                Command::Output(path.text.into())
            }
            "Type" => Command::Type {
                delay: self.speed()?,
                text: self.strings("Type expects literal text, optionally after @duration")?,
            },
            "ScrollUp" | "ScrollDown" => Command::Scroll {
                direction: if token.text == "ScrollUp" {
                    ScrollDirection::Up
                } else {
                    ScrollDirection::Down
                },
                delay: self.speed()?,
                count: self.repeat()?,
            },
            "Sleep" => Command::Sleep(self.time(
                "Sleep expects a nonnegative duration (seconds, ms, s, or m)",
                false,
            )?),
            "Hide" => Command::Hide,
            "Show" => Command::Show,
            "Env" => {
                let name = self.take();
                if !matches!(name.kind, Kind::Word | Kind::String)
                    || name.text.is_empty()
                    || name.text.contains(['=', '\0'])
                {
                    return Err(name.error(
                        ErrorKind::InvalidValue,
                        "Env expects a nonempty key without = or NUL",
                    ));
                }
                let value = self.string("Env expects a key and one literal string value")?;
                if value.text.contains('\0') {
                    return Err(
                        value.error(ErrorKind::InvalidValue, "Env value must not contain NUL")
                    );
                }
                Command::Env {
                    key: name.text,
                    value: value.text,
                }
            }
            "Require" => {
                let name = self.string("Require expects one executable name")?;
                nonempty(&name, "Require executable")?;
                Command::Require(name.text)
            }
            "Source" | "Screenshot" => {
                let path = self.string(&format!("{} expects one path", token.text))?;
                nonempty(&path, &format!("{} path", token.text))?;
                let extension = if token.text == "Source" {
                    "tape"
                } else {
                    "png"
                };
                if Path::new(&path.text).extension().and_then(|s| s.to_str()) != Some(extension) {
                    return Err(path.error(
                        ErrorKind::InvalidValue,
                        format!("{} expects a .{extension} file", token.text),
                    ));
                }
                if token.text == "Source" {
                    Command::Source(path.text.into())
                } else {
                    Command::Screenshot(path.text.into())
                }
            }
            "Copy" => Command::Copy(self.strings("Copy expects literal text")?),
            "Paste" => Command::Paste,
            "Wait" => {
                let scope = if self.symbol('+') {
                    let scope = self.take();
                    match scope.text.as_str() {
                        "Line" if scope.is_string() => WaitScope::Line,
                        "Screen" if scope.is_string() => WaitScope::Screen,
                        _ => {
                            return Err(
                                scope.error(ErrorKind::Syntax, "Wait+ expects Line or Screen")
                            )
                        }
                    }
                } else {
                    WaitScope::Line
                };
                let timeout = self.speed()?;
                if timeout.is_some_and(|time| time.is_zero()) {
                    return Err(
                        token.error(ErrorKind::InvalidValue, "Wait timeout must be positive")
                    );
                }
                let pattern = if self.peek().kind == Kind::Regex {
                    Some(self.pattern()?)
                } else {
                    None
                };
                Command::Wait {
                    scope,
                    timeout,
                    pattern,
                }
            }
            _ => {
                return Err(token.error(
                    ErrorKind::Syntax,
                    format!("unknown or unsupported syntax for command {}", token.text),
                ))
            }
        })
    }

    fn modified(&mut self, first: Modifier) -> Result<Command, Error> {
        let mut modifiers = vec![first];
        let mut keys = Vec::new();
        while self.symbol('+') {
            let token = self.take();
            if token.kind == Kind::Word {
                if let Some(next) = modifier(&token.text) {
                    if first != Modifier::Ctrl || next == Modifier::Ctrl || !keys.is_empty() {
                        return Err(token.error(
                            ErrorKind::Syntax,
                            "Ctrl modifier chains require Alt/Shift before all keys",
                        ));
                    }
                    modifiers.push(next);
                    continue;
                }
            }
            let named = (token.kind == Kind::Word)
                .then(|| key(&token.text))
                .flatten();
            let allowed = match first {
                Modifier::Ctrl => matches!(
                    named,
                    Some(
                        Key::Enter
                            | Key::Space
                            | Key::Backspace
                            | Key::Left
                            | Key::Right
                            | Key::Up
                            | Key::Down
                    )
                ),
                Modifier::Alt | Modifier::Shift => matches!(named, Some(Key::Enter | Key::Tab)),
            };
            if allowed {
                keys.push(ModifiedKey::Named(named.expect("allowed named key")));
            } else if (token.is_string()
                || matches!(token.kind, Kind::Symbol('[' | ']'))
                || (first == Modifier::Ctrl
                    && matches!(token.kind, Kind::Symbol('-' | '@' | '^' | '\\'))))
                && token.text.chars().count() == 1
                && !token.text.contains('\0')
            {
                keys.push(ModifiedKey::Character(token.text.chars().next().unwrap()));
            } else {
                return Err(token.error(
                    ErrorKind::InvalidValue,
                    "modifier expects one supported key or one quoted character",
                ));
            }
            if first != Modifier::Ctrl {
                break;
            }
        }
        if keys.is_empty() {
            return Err(self.peek().error(
                ErrorKind::Syntax,
                "modifier expects +key; Ctrl may have +Alt/+Shift before keys",
            ));
        }
        Ok(Command::ModifiedKeys { modifiers, keys })
    }

    fn pattern(&mut self) -> Result<String, Error> {
        let token = self.take();
        if token.kind != Kind::Regex && !token.is_string() {
            return Err(token.error(ErrorKind::Syntax, "expected /regex/ or a quoted pattern"));
        }
        regex::Regex::new(&token.text).map_err(|error| {
            token.error(
                ErrorKind::InvalidValue,
                format!("invalid regular expression: {error}"),
            )
        })?;
        Ok(token.text)
    }

    fn setting(&mut self) -> Result<Setting, Error> {
        let name = self.take();
        if name.kind != Kind::Word {
            return Err(name.error(ErrorKind::Syntax, "Set expects an unquoted setting name"));
        }
        Ok(match name.text.as_str() {
            "Shell" | "FontFamily" => {
                let value = self.string(&format!("Set {} expects one string", name.text))?;
                nonempty(&value, &name.text)?;
                if name.text == "Shell" {
                    Setting::Shell(value.text)
                } else {
                    Setting::FontFamily(value.text)
                }
            }
            "FontSize" => {
                let value = self.number("FontSize must be finite and positive within f32")?;
                let size: f32 = value.text.parse().map_err(|_| {
                    value.error(
                        ErrorKind::InvalidValue,
                        "FontSize must be finite and positive within f32",
                    )
                })?;
                if !size.is_finite() || size <= 0.0 {
                    return Err(value.error(
                        ErrorKind::InvalidValue,
                        "FontSize must be finite and positive within f32",
                    ));
                }
                Setting::FontSize(size)
            }
            "PlaybackSpeed" => Setting::PlaybackSpeed(
                self.float("PlaybackSpeed must be finite and positive", true)?,
            ),
            "Framerate" => {
                let value = self.unsigned::<u8>("Framerate must be an integer from 1 to 255")?;
                if value == 0 {
                    return Err(name.error(
                        ErrorKind::InvalidValue,
                        "Framerate must be an integer from 1 to 255",
                    ));
                }
                Setting::Framerate(value)
            }
            "Rows" | "Columns" => {
                let value =
                    self.unsigned::<u16>("Rows and Columns must be integers from 1 to 65535")?;
                if value == 0 || (cfg!(windows) && value > i16::MAX as u16) {
                    return Err(name.error(ErrorKind::InvalidValue, "terminal size must be positive and within core limits (32767 on Windows, 65535 elsewhere)"));
                }
                if name.text == "Rows" {
                    Setting::Rows(value)
                } else {
                    Setting::Columns(value)
                }
            }
            "TypingSpeed" => Setting::TypingSpeed(self.time(
                "TypingSpeed expects a nonnegative duration within i64 nanoseconds",
                true,
            )?),
            "WaitTimeout" => {
                let value = self.time("WaitTimeout expects a positive duration", false)?;
                if value.is_zero() {
                    return Err(name.error(ErrorKind::InvalidValue, "WaitTimeout must be positive"));
                }
                Setting::WaitTimeout(value)
            }
            "CursorBlink" => {
                let value = self.take();
                match (value.kind, value.text.as_str()) {
                    (Kind::Word, "true") => Setting::CursorBlink(true),
                    (Kind::Word, "false") => Setting::CursorBlink(false),
                    _ => {
                        return Err(value.error(
                            ErrorKind::InvalidValue,
                            "CursorBlink expects true or false without quotes",
                        ))
                    }
                }
            }
            "Padding" => {
                Setting::Padding(self.unsigned("Padding must be a nonnegative u32 integer")?)
            }
            "Height" => Setting::Height(self.unsigned("Height must be a nonnegative u32 integer")?),
            "Width" => Setting::Width(self.unsigned("Width must be a nonnegative u32 integer")?),
            "Margin" => Setting::Margin(self.unsigned("Margin must be a nonnegative u32 integer")?),
            "WindowBarSize" => Setting::WindowBarSize(
                self.unsigned("WindowBarSize must be a nonnegative u32 integer")?,
            ),
            "BorderRadius" => Setting::BorderRadius(
                self.unsigned("BorderRadius must be a nonnegative u32 integer")?,
            ),
            "LetterSpacing" => {
                Setting::LetterSpacing(self.float("LetterSpacing must be finite", false)?)
            }
            "LineHeight" => {
                Setting::LineHeight(self.float("LineHeight must be finite and positive", true)?)
            }
            "LoopOffset" => {
                let value = self.float("LoopOffset must be a percentage from 0 to 100", false)?;
                if !(0.0..=100.0).contains(&value) {
                    return Err(name.error(
                        ErrorKind::InvalidValue,
                        "LoopOffset must be a percentage from 0 to 100",
                    ));
                }
                self.symbol('%');
                Setting::LoopOffset(value)
            }
            "MarginFill" => {
                let value = self.string("MarginFill expects a color or image path")?;
                nonempty(&value, "MarginFill")?;
                if value.text.starts_with('#')
                    && (value.text.len() != 7
                        || !value.text[1..].bytes().all(|c| c.is_ascii_hexdigit()))
                {
                    return Err(value.error(
                        ErrorKind::InvalidValue,
                        "MarginFill hex colors require #RRGGBB",
                    ));
                }
                Setting::MarginFill(value.text)
            }
            "WindowBar" => {
                let value = self.string("WindowBar expects a quoted style or an empty string")?;
                if !matches!(
                    value.text.as_str(),
                    "" | "Colorful" | "ColorfulRight" | "Rings" | "RingsRight"
                ) {
                    return Err(value.error(ErrorKind::InvalidValue, "WindowBar expects Colorful, ColorfulRight, Rings, RingsRight, or an empty string"));
                }
                Setting::WindowBar(value.text)
            }
            "Theme" => {
                if self.peek().kind == Kind::Json {
                    let value = self.take();
                    let object = serde_json::from_str(&value.text).map_err(|error| {
                        value.error(
                            ErrorKind::InvalidValue,
                            format!("Theme expects a JSON object: {error}"),
                        )
                    })?;
                    Setting::Theme(Theme::Object(object))
                } else {
                    let value = self.string("Theme expects a name or JSON object")?;
                    nonempty(&value, "Theme")?;
                    Setting::Theme(Theme::Name(value.text))
                }
            }
            "WaitPattern" => Setting::WaitPattern(self.pattern()?),
            _ => {
                return Err(name.error(ErrorKind::Syntax, format!("unknown setting {}", name.text)))
            }
        })
    }
}

fn nonempty(token: &Token, name: &str) -> Result<(), Error> {
    if token.text.is_empty() || token.text.contains('\0') {
        Err(token.error(
            ErrorKind::InvalidValue,
            format!("{name} must not be empty or contain NUL"),
        ))
    } else {
        Ok(())
    }
}

// Decimal arithmetic keeps large durations exact. Go durations also stop at
// i64 nanoseconds and discard fractions smaller than one nanosecond.
fn duration(value: &str) -> Option<Duration> {
    let (number, unit) = if let Some(value) = value.strip_suffix("ms") {
        (value, 1_000_000_u128)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1_000_000_000)
    } else {
        (value.strip_suffix('m')?, 60_000_000_000)
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if number.is_empty()
        || number == "."
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    let whole = if whole.is_empty() {
        0
    } else {
        whole.parse::<u128>().ok()?
    };
    // Scale from the least significant digit so every digit contributes without
    // an unbounded numerator. The carry stays smaller than one unit.
    let fraction = fraction.bytes().rev().fold(0_u128, |carry, digit| {
        (u128::from(digit - b'0') * unit + carry) / 10
    });
    let nanos = whole.checked_mul(unit)?.checked_add(fraction)?;
    if nanos > i64::MAX as u128 {
        return None;
    }
    Some(Duration::from_nanos(u64::try_from(nanos).ok()?))
}
