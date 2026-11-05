//! Parse and validate a limited VHS tape dialect.
//!
//! [`parse`] is pure. [`load`] and [`resolve_sources`] only read source files.
//! [`compile`] produces an owned, immutable [`Plan`] without a child or output writes.

mod compiler;
mod parser;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

pub use compiler::{compile, Action, Plan, Settings, Shell, Warning, WarningKind};
pub use parser::parse;

/// The grammar reference, not a claim of full VHS compatibility.
pub const VHS_REVISION: &str = "24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub included_from: Option<Box<Location>>,
}

impl Location {
    fn start(path: &Path) -> Self {
        Self {
            path: path.into(),
            line: 1,
            column: 1,
            included_from: None,
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.path.display(), self.line, self.column)?;
        if let Some(parent) = &self.included_from {
            write!(f, " (included from {parent})")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Located<T> {
    pub location: Location,
    pub value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorKind {
    Syntax,
    InvalidValue,
    Unsupported,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    pub location: Location,
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    fn new(location: &Location, kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            location: location.clone(),
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tape {
    origin: Location,
    commands: Vec<Located<Command>>,
}

impl Tape {
    pub fn commands(&self) -> &[Located<Command>] {
        &self.commands
    }

    pub fn origin(&self) -> &Location {
        &self.origin
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Key {
    Backspace,
    Delete,
    Insert,
    Enter,
    Escape,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ModifiedKey {
    Named(Key),
    Character(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WaitScope {
    Line,
    Screen,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Theme {
    Name(String),
    Object(serde_json::Map<String, serde_json::Value>),
}

/// All 23 setting names in the pinned grammar have a typed representation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Setting {
    Shell(String),
    FontFamily(String),
    FontSize(f32),
    Framerate(u8),
    PlaybackSpeed(f64),
    TypingSpeed(Duration),
    Rows(u16),
    Columns(u16),
    Padding(u32),
    CursorBlink(bool),
    Height(u32),
    Width(u32),
    LetterSpacing(f64),
    LineHeight(f64),
    Theme(Theme),
    LoopOffset(f64),
    MarginFill(String),
    Margin(u32),
    WindowBar(String),
    WindowBarSize(u32),
    BorderRadius(u32),
    WaitTimeout(Duration),
    WaitPattern(String),
}

impl Setting {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Shell(_) => "Shell",
            Self::FontFamily(_) => "FontFamily",
            Self::FontSize(_) => "FontSize",
            Self::Framerate(_) => "Framerate",
            Self::PlaybackSpeed(_) => "PlaybackSpeed",
            Self::TypingSpeed(_) => "TypingSpeed",
            Self::Rows(_) => "Rows",
            Self::Columns(_) => "Columns",
            Self::Padding(_) => "Padding",
            Self::CursorBlink(_) => "CursorBlink",
            Self::Height(_) => "Height",
            Self::Width(_) => "Width",
            Self::LetterSpacing(_) => "LetterSpacing",
            Self::LineHeight(_) => "LineHeight",
            Self::Theme(_) => "Theme",
            Self::LoopOffset(_) => "LoopOffset",
            Self::MarginFill(_) => "MarginFill",
            Self::Margin(_) => "Margin",
            Self::WindowBar(_) => "WindowBar",
            Self::WindowBarSize(_) => "WindowBarSize",
            Self::BorderRadius(_) => "BorderRadius",
            Self::WaitTimeout(_) => "WaitTimeout",
            Self::WaitPattern(_) => "WaitPattern",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Command {
    Output(PathBuf),
    Set(Setting),
    Type {
        text: String,
        delay: Option<Duration>,
    },
    Key {
        key: Key,
        delay: Option<Duration>,
        count: u64,
    },
    ModifiedKeys {
        modifiers: Vec<Modifier>,
        keys: Vec<ModifiedKey>,
    },
    Scroll {
        direction: ScrollDirection,
        delay: Option<Duration>,
        count: u64,
    },
    Sleep(Duration),
    Hide,
    Show,
    Env {
        key: String,
        value: String,
    },
    Require(String),
    Source(PathBuf),
    Wait {
        scope: WaitScope,
        timeout: Option<Duration>,
        pattern: Option<String>,
    },
    Screenshot(PathBuf),
    Copy(String),
    Paste,
}

/// Read a root tape. Relative root and Source paths use `invocation_cwd`.
pub fn load(path: impl AsRef<Path>, invocation_cwd: impl AsRef<Path>) -> Result<Tape, Error> {
    let path = invocation_cwd.as_ref().join(path);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Error::new(
            &Location::start(&path),
            ErrorKind::Source,
            error.to_string(),
        )
    })?;
    resolve_sources(&parse(path, &text)?, invocation_cwd)
}

/// Expand one level of Source commands, preserving each import location.
///
/// Imported Output commands are discarded. Nested imports are errors.
/// This function does not change the process directory or environment.
pub fn resolve_sources(tape: &Tape, invocation_cwd: impl AsRef<Path>) -> Result<Tape, Error> {
    let mut commands = Vec::new();
    for command in &tape.commands {
        let Command::Source(path) = &command.value else {
            commands.push(command.clone());
            continue;
        };
        let path = invocation_cwd.as_ref().join(path);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            Error::new(
                &command.location,
                ErrorKind::Source,
                format!("cannot read Source {}: {error}", path.display()),
            )
        })?;
        let mut imported = parse(&path, &text).map_err(|mut error| {
            error.location.included_from = Some(Box::new(command.location.clone()));
            error
        })?;
        if imported.commands.is_empty() {
            return Err(Error::new(
                &command.location,
                ErrorKind::Source,
                format!("Source {} contains no commands", path.display()),
            ));
        }
        for item in &mut imported.commands {
            item.location.included_from = Some(Box::new(command.location.clone()));
            match item.value {
                Command::Source(_) => {
                    return Err(Error::new(
                        &item.location,
                        ErrorKind::Source,
                        "nested Source is not supported",
                    ));
                }
                Command::Output(_) => {}
                _ => commands.push(item.clone()),
            }
        }
    }
    Ok(Tape {
        origin: tape.origin.clone(),
        commands,
    })
}

#[cfg(test)]
mod tests;
