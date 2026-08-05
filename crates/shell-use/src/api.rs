use std::fmt;

use serde::{Deserialize, Serialize};

use crate::shell::Shell;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
}

#[derive(Debug, Clone)]
pub struct OpenOptions {
    pub shell: Option<Shell>,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub wait_ready: Option<bool>,
    pub timeouts: Timeouts,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            shell: None,
            cols: crate::config::DEFAULT_COLS,
            rows: crate::config::DEFAULT_ROWS,
            cwd: None,
            env: Vec::new(),
            wait_ready: None,
            timeouts: Timeouts::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub program: String,
    pub args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub wait_ready: Option<bool>,
    pub timeouts: Timeouts,
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
    GetBellCount,
    Write {
        data: String,
    },
    Submit {
        data: Option<String>,
    },
    Press {
        keys: Vec<String>,
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
    WaitText {
        text: String,
        regex: bool,
        full: bool,
        timeout_ms: Option<u64>,
        not: bool,
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
    ExpectText {
        text: String,
        regex: bool,
        full: bool,
        strict: bool,
        not: bool,
        fg: Option<String>,
        bg: Option<String>,
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
        cwd: Option<String>,
    },
    Screenshot {
        full: bool,
        path: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum OperationResult {
    Unit,
    Open(OpenResult),
    State(State),
    Text(String),
    PackedScreen(PackedScreen),
    Cells(Vec<Cell>),
    Command(Option<String>),
    Output(Option<String>),
    ExitCode(Option<i32>),
    Cwd(Option<String>),
    Cursor(Cursor),
    Size(Size),
    BellCount(u64),
    Snapshot(SnapshotResult),
    Screenshot(ScreenshotResult),
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
pub struct ShellUseError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ShellUseError {
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
            "no active session; run `shell-use open` (or `shell-use run <program>`) first",
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for ShellUseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ShellUseError {}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MouseAction {
    Click {
        x: Option<u16>,
        y: Option<u16>,
        on_text: Option<String>,
        button: u8,
        clicks: u8,
    },
    Move {
        x: u16,
        y: u16,
    },
    Down {
        x: u16,
        y: u16,
        button: u8,
    },
    Up {
        x: u16,
        y: u16,
        button: u8,
    },
    Drag {
        x1: u16,
        y1: u16,
        x2: u16,
        y2: u16,
        button: u8,
    },
    Scroll {
        direction: String,
        amount: u16,
    },
}
