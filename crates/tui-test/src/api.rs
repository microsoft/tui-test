use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::shell::Shell;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutomaticRecordingMode {
    Disabled,
    OnFailure,
    #[default]
    Always,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutomaticRecording {
    pub mode: AutomaticRecordingMode,
    pub directory: Option<PathBuf>,
}

impl AutomaticRecording {
    pub fn validate(&self) -> Result<(), TuiTestError> {
        if self
            .directory
            .as_ref()
            .is_some_and(|directory| directory.as_os_str().is_empty())
        {
            return Err(TuiTestError::usage(
                "automatic recording directory must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timeouts {
    pub text: Option<u64>,
    pub idle: Option<u64>,
    pub command: Option<u64>,
    pub exit: Option<u64>,
    pub ready: Option<u64>,
}

impl Timeouts {
    pub fn get(&self, class: crate::config::TimeoutClass) -> Option<u64> {
        use crate::config::TimeoutClass::*;
        match class {
            Text => self.text,
            Idle => self.idle,
            Command => self.command,
            Exit => self.exit,
            Ready => self.ready,
        }
    }

    /// Apply higher-precedence timeout values over these defaults.
    pub fn with_overrides(self, overrides: Self) -> Self {
        Self {
            text: overrides.text.or(self.text),
            idle: overrides.idle.or(self.idle),
            command: overrides.command.or(self.command),
            exit: overrides.exit.or(self.exit),
            ready: overrides.ready.or(self.ready),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenOptions {
    pub backend: crate::terminal::backend::Backend,
    pub shell: Option<Shell>,
    /// Terminal settings, already resolved from the config file by the
    /// client. The daemon never reads that file: it is long-lived and shared,
    /// so it has no single working directory to resolve a project-local config
    /// against.
    pub profile: crate::profile::Profile,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub wait_ready: Option<bool>,
    pub restart: bool,
    pub timeouts: Timeouts,
    pub recording: AutomaticRecording,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            backend: crate::terminal::backend::Backend::default(),
            shell: None,
            profile: crate::profile::Profile::default(),
            cols: crate::config::DEFAULT_COLS,
            rows: crate::config::DEFAULT_ROWS,
            cwd: None,
            env: Vec::new(),
            wait_ready: None,
            restart: false,
            timeouts: Timeouts::default(),
            recording: AutomaticRecording::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub backend: crate::terminal::backend::Backend,
    pub program: String,
    pub args: Vec<String>,
    /// Terminal settings, already resolved from the config file by the
    /// client. The daemon never reads that file: it is long-lived and shared,
    /// so it has no single working directory to resolve a project-local config
    /// against.
    pub profile: crate::profile::Profile,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub wait_ready: Option<bool>,
    pub restart: bool,
    pub timeouts: Timeouts,
    pub recording: AutomaticRecording,
}

/// Clipboard text or regex.
#[derive(Debug, Clone)]
pub enum ClipboardPattern {
    Text(String),
    Regex(regex::Regex),
}

impl ClipboardPattern {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn regex(pattern: &str) -> Result<Self, regex::Error> {
        regex::Regex::new(pattern).map(Self::Regex)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Regex(regex) => regex.as_str(),
        }
    }

    pub(crate) fn matches(&self, value: &str) -> bool {
        match self {
            Self::Text(text) => value.contains(text),
            Self::Regex(regex) => regex.is_match(value),
        }
    }
}

impl From<String> for ClipboardPattern {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for ClipboardPattern {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<regex::Regex> for ClipboardPattern {
    fn from(regex: regex::Regex) -> Self {
        Self::Regex(regex)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyAction {
    #[default]
    Press,
    Down,
    Repeat,
    Up,
}

/// A terminal mouse button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    #[default]
    Left,
    Middle,
    Right,
}

/// Button and modifier state for a mouse action.
///
/// The default is an unmodified left button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MouseOptions {
    pub button: MouseButton,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl MouseOptions {
    pub const fn new(button: MouseButton) -> Self {
        Self {
            button,
            alt: false,
            ctrl: false,
            shift: false,
        }
    }

    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Return the SGR mouse button code used by terminal protocols.
    pub const fn sgr_code(self) -> u8 {
        let button = match self.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        };
        button + 4 * self.shift as u8 + 8 * self.alt as u8 + 16 * self.ctrl as u8
    }

    /// Decode an SGR mouse button code used by protocol adapters.
    pub const fn from_sgr_code(code: u8) -> Option<Self> {
        if code & !0b1_1111 != 0 {
            return None;
        }
        let button = match code & 0b11 {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => return None,
        };
        Some(Self {
            button,
            shift: code & 4 != 0,
            alt: code & 8 != 0,
            ctrl: code & 16 != 0,
        })
    }
}

impl From<MouseButton> for MouseOptions {
    fn from(button: MouseButton) -> Self {
        Self::new(button)
    }
}

mod mouse_options_code {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::MouseOptions;

    pub fn serialize<S>(options: &MouseOptions, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(options.sgr_code())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MouseOptions, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = u8::deserialize(deserializer)?;
        MouseOptions::from_sgr_code(code).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid SGR mouse button code {code}"))
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhitespaceMode {
    #[default]
    Exact,
    Normalize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchOccurrence {
    Any,
    #[default]
    Unique,
    First,
    Last,
    Nth(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextAnchor {
    pub text: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub occurrence: MatchOccurrence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextScope {
    pub after: Option<TextAnchor>,
    pub before: Option<TextAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSelector {
    pub text: String,
    pub regex: bool,
    pub full: bool,
    pub whitespace: WhitespaceMode,
    pub scope: TextScope,
}

impl Default for TextSelector {
    fn default() -> Self {
        Self {
            text: String::new(),
            regex: false,
            full: false,
            whitespace: WhitespaceMode::Exact,
            scope: TextScope::default(),
        }
    }
}

impl TextSelector {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

impl From<&str> for TextSelector {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for TextSelector {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline_style: Option<String>,
    pub underline_color: Option<String>,
    pub inverse: Option<bool>,
    pub hidden: Option<bool>,
    pub strikethrough: Option<bool>,
    pub blink: Option<bool>,
}

impl TextStyle {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
/// Select contiguous per-row runs whose cells match every requested style.
pub struct StyleSelector {
    pub style: TextStyle,
    pub full: bool,
}

impl From<TextStyle> for StyleSelector {
    fn from(style: TextStyle) -> Self {
        Self {
            style,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "selector", rename_all = "snake_case")]
pub enum LocatorSelector {
    Text(TextSelector),
    Style(StyleSelector),
}

impl LocatorSelector {
    pub fn full(&self) -> bool {
        match self {
            Self::Text(selector) => selector.full,
            Self::Style(selector) => selector.full,
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::Text(selector) => selector.text.clone(),
            Self::Style(_) => "style".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorDirection {
    /// Search inside each selected parent match.
    #[default]
    Within,
    /// Search after each parent, stopping at the next selected parent.
    After,
    /// Search before each parent, starting after the previous selected parent.
    Before,
}

fn default_locator_occurrence() -> MatchOccurrence {
    MatchOccurrence::Any
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A lazy locator stage and the parent query that defines its search region.
pub struct LocatorQuery {
    pub selector: LocatorSelector,
    #[serde(default = "default_locator_occurrence")]
    pub occurrence: MatchOccurrence,
    #[serde(default)]
    pub within: Option<Box<LocatorQuery>>,
    #[serde(default)]
    pub direction: LocatorDirection,
    #[serde(default)]
    pub style: TextStyle,
}

impl LocatorQuery {
    pub fn text(selector: impl Into<TextSelector>) -> Self {
        Self {
            selector: LocatorSelector::Text(selector.into()),
            occurrence: MatchOccurrence::Any,
            within: None,
            direction: LocatorDirection::Within,
            style: TextStyle::default(),
        }
    }

    pub fn style(selector: impl Into<StyleSelector>) -> Self {
        Self {
            selector: LocatorSelector::Style(selector.into()),
            occurrence: MatchOccurrence::Any,
            within: None,
            direction: LocatorDirection::Within,
            style: TextStyle::default(),
        }
    }

    pub fn uses_full_grid(&self) -> bool {
        self.selector.full()
            || self
                .within
                .as_deref()
                .is_some_and(LocatorQuery::uses_full_grid)
    }
}

#[derive(Debug, Clone)]
pub enum Operation {
    Open(OpenOptions),
    Run(RunOptions),
    Close,
    State,
    Text {
        full: bool,
    },
    PackedScreen {
        full: bool,
    },
    Cells {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
    },
    GetCommand,
    GetOutput,
    GetExitCode,
    GetCwd,
    GetCursor,
    GetSize,
    GetTitle,
    GetClipboard,
    GetBellCount,
    GetBellEvents,
    Write {
        data: String,
    },
    Submit {
        data: Option<String>,
    },
    Key {
        keys: Vec<String>,
        action: KeyAction,
    },
    Mouse {
        action: MouseAction,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Signal {
        name: String,
    },
    WaitTitle {
        text: String,
        regex: bool,
        timeout_ms: Option<u64>,
        not: bool,
    },
    WaitClipboard {
        timeout_ms: Option<u64>,
    },
    WaitClipboardMatch {
        pattern: ClipboardPattern,
        timeout_ms: Option<u64>,
    },
    WaitIdle {
        timeout_ms: Option<u64>,
    },
    WaitCommand {
        timeout_ms: Option<u64>,
    },
    WaitExit {
        timeout_ms: Option<u64>,
    },
    WaitReady {
        timeout_ms: Option<u64>,
    },
    WaitBell {
        timeout_ms: Option<u64>,
    },
    FindLocator {
        query: LocatorQuery,
    },
    WaitLocator {
        query: LocatorQuery,
        not: bool,
        timeout_ms: Option<u64>,
    },
    ClickLocator {
        query: LocatorQuery,
        options: MouseOptions,
        clicks: u8,
        timeout_ms: Option<u64>,
    },
    HighlightLocator {
        query: LocatorQuery,
        timeout_ms: Option<u64>,
    },
    ExpectTitle {
        text: String,
        regex: bool,
        not: bool,
        timeout_ms: Option<u64>,
    },
    ExpectExitCode {
        code: i32,
        timeout_ms: Option<u64>,
    },
    ExpectOutput {
        text: String,
        regex: bool,
    },
    ExpectBellCount {
        count: u64,
        timeout_ms: Option<u64>,
    },
    Snapshot {
        name: String,
        update: bool,
        include_colors: bool,
        include_title: bool,
        cwd: Option<String>,
    },
    Screenshot {
        full: bool,
        path: Option<String>,
        zoom: Option<f64>,
    },
    StartRecording {
        path: String,
        format: Option<RecordingFormat>,
        fps: Option<u8>,
        speed: Option<f64>,
        idle_time_limit: Option<f64>,
        zoom: Option<f64>,
    },
    StopRecording,
}

impl Operation {
    /// Wait for clipboard text or a regex.
    pub fn wait_clipboard_match(
        pattern: impl Into<ClipboardPattern>,
        timeout_ms: Option<u64>,
    ) -> Self {
        Self::WaitClipboardMatch {
            pattern: pattern.into(),
            timeout_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub enum OperationResult {
    Unit,
    Open(OpenResult),
    State(State),
    Text(String),
    PackedScreen(PackedScreen),
    Cells(Vec<Cell>),
    Matches(Vec<TextMatch>),
    Command(Option<String>),
    Output(Option<String>),
    ExitCode(Option<i32>),
    Cwd(Option<String>),
    Title(Option<String>),
    Clipboard(String),
    Cursor(Cursor),
    Size(Size),
    BellCount(u64),
    BellEvents(Vec<BellEvent>),
    Snapshot(SnapshotResult),
    Screenshot(ScreenshotResult),
    Recording(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Assertion,
    Usage,
    NoSession,
    Internal,
}

impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Assertion => 1,
            ErrorKind::Usage => 2,
            ErrorKind::NoSession => 3,
            ErrorKind::Internal => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Assertion => "assertion",
            ErrorKind::Usage => "usage",
            ErrorKind::NoSession => "no_session",
            ErrorKind::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TuiTestError {
    pub kind: ErrorKind,
    pub message: String,
}

impl TuiTestError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn assertion(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Assertion, message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message)
    }

    pub fn no_session() -> Self {
        Self::new(
            ErrorKind::NoSession,
            "no active session; run `tui-test open` (or `tui-test run <program>`) first",
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for TuiTestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TuiTestError {}

#[derive(Debug, Clone, Serialize)]
pub struct OpenResult {
    pub shell_pid: Option<u32>,
    pub session: String,
    pub ready: bool,
    pub recording: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BellEvent {
    pub sequence: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextPosition {
    pub row: u32,
    pub column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextSpan {
    pub row: u32,
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextMatch {
    pub text: String,
    pub start: TextPosition,
    /// Exclusive end position.
    pub end: TextPosition,
    /// Per-row column ranges with exclusive ends.
    pub spans: Vec<TextSpan>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectiveTimeouts {
    pub text: u64,
    pub idle: u64,
    pub command: u64,
    pub exit: u64,
    pub ready: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub session_shell: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub cursor: Cursor,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub last_command: Option<String>,
    pub last_exit: Option<i32>,
    pub exited: Option<i32>,
    pub ready: bool,
    pub bell_count: u64,
    pub timeouts: EffectiveTimeouts,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Serialize for CellColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            CellColor::Default => serializer.serialize_str("default"),
            CellColor::Indexed(index) => serializer.serialize_u8(*index),
            CellColor::Rgb(r, g, b) => serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub char: String,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strike: bool,
    pub blink: bool,
    pub underline: bool,
    pub underline_style: String,
    pub underline_color: CellColor,
}

#[derive(Debug, Clone)]
pub struct PackedScreen {
    /// Logical terminal dimensions for the newline-delimited UTF-8 snapshot.
    /// Rows retain trailing spaces and blank lines; byte offsets are not cell
    /// offsets because Unicode graphemes may occupy multiple bytes.
    pub cols: u16,
    pub rows: u16,
    pub utf8: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotResult {
    Passed,
    Written,
    Updated,
}

#[derive(Debug, Clone)]
pub enum ScreenshotResult {
    Path(String),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingFormat {
    Apng,
    Gif,
    Mp4,
    Cast,
}

impl RecordingFormat {
    pub fn infer(path: &str) -> Option<Self> {
        let extension = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_ascii_lowercase();
        match extension.as_str() {
            "png" | "apng" => Some(Self::Apng),
            "gif" => Some(Self::Gif),
            "mp4" => Some(Self::Mp4),
            "cast" => Some(Self::Cast),
            _ => None,
        }
    }
}

pub(crate) fn resolve_zoom(zoom: Option<f64>) -> Result<f64, TuiTestError> {
    let zoom = zoom.unwrap_or(1.0);
    if !zoom.is_finite() || zoom <= 0.0 {
        return Err(TuiTestError::usage(
            "zoom must be finite and greater than zero",
        ));
    }
    if zoom > f64::from(f32::MAX) / 2.0 {
        return Err(TuiTestError::usage("zoom is too large"));
    }
    Ok(zoom)
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub session: String,
    pub shell_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exited: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeouts: Option<EffectiveTimeouts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MouseAction {
    Click {
        x: Option<u16>,
        y: Option<u16>,
        on_text: Option<String>,
        #[serde(default, rename = "button", with = "mouse_options_code")]
        options: MouseOptions,
        clicks: u8,
    },
    Move {
        x: u16,
        y: u16,
    },
    Down {
        x: u16,
        y: u16,
        #[serde(default, rename = "button", with = "mouse_options_code")]
        options: MouseOptions,
    },
    Up {
        x: u16,
        y: u16,
        #[serde(default, rename = "button", with = "mouse_options_code")]
        options: MouseOptions,
    },
    Drag {
        x1: u16,
        y1: u16,
        x2: u16,
        y2: u16,
        #[serde(default, rename = "button", with = "mouse_options_code")]
        options: MouseOptions,
    },
    Scroll {
        direction: String,
        amount: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_patterns_infer_matching_from_the_rust_type() {
        let literal: ClipboardPattern = "ready".into();
        assert!(literal.matches("prefix-ready-suffix"));

        let regex: ClipboardPattern = regex::Regex::new(r"^build-[0-9]+$").unwrap().into();
        assert!(regex.matches("build-123"));
        assert!(!regex.matches("prefix-build-123"));

        assert!(matches!(
            Operation::wait_clipboard_match("ready", Some(5_000)),
            Operation::WaitClipboardMatch { .. }
        ));
    }

    #[test]
    fn recording_format_is_inferred_from_supported_extensions() {
        assert_eq!(
            RecordingFormat::infer("demo.png"),
            Some(RecordingFormat::Apng)
        );
        assert_eq!(
            RecordingFormat::infer("demo.APNG"),
            Some(RecordingFormat::Apng)
        );
        assert_eq!(
            RecordingFormat::infer("demo.gif"),
            Some(RecordingFormat::Gif)
        );
        assert_eq!(
            RecordingFormat::infer("demo.MP4"),
            Some(RecordingFormat::Mp4)
        );
        assert_eq!(
            RecordingFormat::infer("demo.cast"),
            Some(RecordingFormat::Cast)
        );
        assert_eq!(RecordingFormat::infer("demo.webm"), None);
    }

    #[test]
    fn mouse_options_are_semantic_and_wire_compatible() {
        let options = MouseOptions::new(MouseButton::Middle)
            .with_ctrl()
            .with_shift();
        assert_eq!(options.sgr_code(), 21);
        assert_eq!(MouseOptions::from_sgr_code(21), Some(options));
        assert_eq!(MouseOptions::from_sgr_code(3), None);
        assert_eq!(MouseOptions::from_sgr_code(32), None);
        assert_eq!(serde_json::to_value(options).unwrap()["button"], "middle");

        let action = MouseAction::Down {
            x: 4,
            y: 7,
            options,
        };
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["button"], 21);
        assert_eq!(
            serde_json::from_value::<MouseAction>(value).unwrap(),
            action
        );

        let defaulted: MouseAction = serde_json::from_str(r#"{"op":"down","x":4,"y":7}"#).unwrap();
        assert_eq!(
            defaulted,
            MouseAction::Down {
                x: 4,
                y: 7,
                options: MouseOptions::default(),
            }
        );
    }

    #[test]
    fn zoom_defaults_to_one_and_rejects_invalid_values() {
        assert_eq!(resolve_zoom(None).unwrap(), 1.0);
        assert_eq!(resolve_zoom(Some(0.5)).unwrap(), 0.5);
        for zoom in [0.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(resolve_zoom(Some(zoom)).is_err());
        }
    }

    #[test]
    fn nested_selectors_use_full_grid_when_any_stage_requests_it() {
        let mut parent = TextSelector::new("parent");
        parent.full = true;
        let child = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("child")),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text(parent))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        assert!(child.uses_full_grid());

        let mut full_child = TextSelector::new("child");
        full_child.full = true;
        let query = LocatorQuery {
            selector: LocatorSelector::Text(full_child),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text("parent"))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        assert!(query.uses_full_grid());
    }
}
