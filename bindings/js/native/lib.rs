#![deny(clippy::all)]

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use napi::bindgen_prelude::{spawn_blocking, Buffer, Either};
use napi::{Error, Result, Status};
use napi_derive::napi;
use shell_use::shell::Shell as CoreShell;
use shell_use::{
    global_registry, Cell as CoreCell, CellColor, Cursor as CoreCursor,
    EffectiveTimeouts as CoreEffectiveTimeouts, ErrorKind, MouseAction,
    OpenOptions as CoreOpenOptions, OpenResult as CoreOpenResult, Operation, OperationResult,
    RunOptions as CoreRunOptions, ScreenshotResult as CoreScreenshotResult, SessionHandle,
    ShellUseError, Size as CoreSize, SnapshotResult as CoreSnapshotResult, State as CoreState,
    Timeouts as CoreTimeouts,
};

const ERROR_PREFIX: &str = "__shell_use_native_error__:";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[napi(string_enum = "lowercase")]
pub enum Shell {
    Bash,
    Powershell,
    Pwsh,
    Cmd,
    Fish,
    Zsh,
    Xonsh,
    Elvish,
    Nushell,
}

impl From<Shell> for CoreShell {
    fn from(value: Shell) -> Self {
        match value {
            Shell::Bash => Self::Bash,
            Shell::Powershell => Self::Powershell,
            Shell::Pwsh => Self::Pwsh,
            Shell::Cmd => Self::Cmd,
            Shell::Fish => Self::Fish,
            Shell::Zsh => Self::Zsh,
            Shell::Xonsh => Self::Xonsh,
            Shell::Elvish => Self::Elvish,
            Shell::Nushell => Self::Nushell,
        }
    }
}

#[napi(object)]
pub struct Timeouts {
    pub text: Option<f64>,
    pub idle: Option<f64>,
    pub command: Option<f64>,
    pub exit: Option<f64>,
    pub ready: Option<f64>,
}

#[napi(object)]
pub struct OpenOptions {
    pub shell: Option<Shell>,
    pub cols: Option<f64>,
    pub rows: Option<f64>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub wait_ready: Option<bool>,
    pub timeouts: Option<Timeouts>,
}

#[napi(object)]
pub struct RunOptions {
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cols: Option<f64>,
    pub rows: Option<f64>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub wait_ready: Option<bool>,
    pub timeouts: Option<Timeouts>,
}

#[napi(object, use_nullable = true)]
pub struct OpenResult {
    #[napi(js_name = "shell_pid")]
    pub shell_pid: Option<u32>,
    pub session: String,
    pub ready: bool,
    pub recording: String,
}

impl From<CoreOpenResult> for OpenResult {
    fn from(value: CoreOpenResult) -> Self {
        Self {
            shell_pid: value.shell_pid,
            session: value.session,
            ready: value.ready,
            recording: value.recording,
        }
    }
}

#[napi(object)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
}

impl From<CoreCursor> for Cursor {
    fn from(value: CoreCursor) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[napi(object)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl From<CoreSize> for Size {
    fn from(value: CoreSize) -> Self {
        Self {
            cols: value.cols,
            rows: value.rows,
        }
    }
}

#[napi(object)]
pub struct EffectiveTimeouts {
    pub text: f64,
    pub idle: f64,
    pub command: f64,
    pub exit: f64,
    pub ready: f64,
}

impl From<CoreEffectiveTimeouts> for EffectiveTimeouts {
    fn from(value: CoreEffectiveTimeouts) -> Self {
        Self {
            text: value.text as f64,
            idle: value.idle as f64,
            command: value.command as f64,
            exit: value.exit as f64,
            ready: value.ready as f64,
        }
    }
}

#[napi(object, use_nullable = true)]
pub struct State {
    #[napi(js_name = "session_shell")]
    pub session_shell: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub cursor: Cursor,
    pub cwd: Option<String>,
    #[napi(js_name = "last_command")]
    pub last_command: Option<String>,
    #[napi(js_name = "last_exit")]
    pub last_exit: Option<i32>,
    pub exited: Option<i32>,
    pub ready: bool,
    #[napi(js_name = "bell_count")]
    pub bell_count: f64,
    pub timeouts: EffectiveTimeouts,
    pub text: String,
}

impl From<CoreState> for State {
    fn from(value: CoreState) -> Self {
        Self {
            session_shell: value.session_shell,
            cols: value.cols,
            rows: value.rows,
            cursor: value.cursor.into(),
            cwd: value.cwd,
            last_command: value.last_command,
            last_exit: value.last_exit,
            exited: value.exited,
            ready: value.ready,
            bell_count: value.bell_count as f64,
            timeouts: value.timeouts.into(),
            text: value.text,
        }
    }
}

#[napi(string_enum = "lowercase")]
pub enum UnderlineStyle {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

fn underline_style(value: String) -> std::result::Result<UnderlineStyle, ShellUseError> {
    match value.as_str() {
        "none" => Ok(UnderlineStyle::None),
        "single" => Ok(UnderlineStyle::Single),
        "double" => Ok(UnderlineStyle::Double),
        "curly" => Ok(UnderlineStyle::Curly),
        "dotted" => Ok(UnderlineStyle::Dotted),
        "dashed" => Ok(UnderlineStyle::Dashed),
        _ => Err(ShellUseError::internal(format!(
            "terminal returned unknown underline style '{value}'"
        ))),
    }
}

#[napi]
pub type Color = Either<u32, String>;

fn color(value: CellColor) -> Color {
    match value {
        CellColor::Default => Either::B("default".to_string()),
        CellColor::Indexed(index) => Either::A(u32::from(index)),
        CellColor::Rgb(red, green, blue) => Either::B(format!("#{red:02x}{green:02x}{blue:02x}")),
    }
}

#[napi(object)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub r#char: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strike: bool,
    pub blink: bool,
    pub underline: bool,
    #[napi(js_name = "underline_style")]
    pub underline_style: UnderlineStyle,
    #[napi(js_name = "underline_color")]
    pub underline_color: Color,
}

impl TryFrom<CoreCell> for Cell {
    type Error = ShellUseError;

    fn try_from(value: CoreCell) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            x: value.x,
            y: value.y,
            r#char: value.char,
            fg: color(value.fg),
            bg: color(value.bg),
            bold: value.bold,
            dim: value.dim,
            italic: value.italic,
            inverse: value.inverse,
            invisible: value.invisible,
            strike: value.strike,
            blink: value.blink,
            underline: value.underline,
            underline_style: underline_style(value.underline_style)?,
            underline_color: color(value.underline_color),
        })
    }
}

#[napi(object)]
/// Private native-owned packed screen snapshot.
///
/// `utf8` decodes to exactly `rows` newline-delimited logical rows. Trailing
/// spaces and blank rows are retained. UTF-8 byte offsets are not terminal cell
/// offsets when rows contain Unicode graphemes.
pub struct PackedScreen {
    #[napi(readonly)]
    /// Logical column count.
    pub cols: u16,
    #[napi(readonly)]
    /// Number of logical rows encoded in `utf8`.
    pub rows: u16,
    #[napi(readonly, ts_type = "Uint8Array")]
    /// Detached native-owned UTF-8 bytes. Treat this private snapshot as immutable.
    pub utf8: Buffer,
}

#[napi(object)]
pub struct MouseClickOptions {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub on_text: Option<String>,
    pub button: Option<f64>,
    pub clicks: Option<f64>,
}

#[napi(object)]
pub struct WaitTextOptions {
    pub regex: Option<bool>,
    pub full: Option<bool>,
    pub not: Option<bool>,
    pub timeout_ms: Option<f64>,
}

#[napi(object)]
pub struct ExpectTextOptions {
    pub regex: Option<bool>,
    pub full: Option<bool>,
    pub strict: Option<bool>,
    pub not: Option<bool>,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub timeout_ms: Option<f64>,
}

#[napi(object)]
pub struct SnapshotOptions {
    pub update: Option<bool>,
    pub include_colors: Option<bool>,
    pub cwd: Option<String>,
}

#[napi(object)]
pub struct ScreenshotOptions {
    pub full: Option<bool>,
    pub path: Option<String>,
}

#[napi(string_enum = "lowercase")]
pub enum SnapshotResult {
    Passed,
    Written,
    Updated,
}

impl From<CoreSnapshotResult> for SnapshotResult {
    fn from(value: CoreSnapshotResult) -> Self {
        match value {
            CoreSnapshotResult::Passed => Self::Passed,
            CoreSnapshotResult::Written => Self::Written,
            CoreSnapshotResult::Updated => Self::Updated,
        }
    }
}

fn native_error(error: ShellUseError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("{ERROR_PREFIX}{}\n{}", error.kind.as_str(), error.message),
    )
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn ffi_boundary<T>(work: impl FnOnce() -> std::result::Result<T, ShellUseError>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(native_error(error)),
        Err(payload) => Err(native_error(ShellUseError::internal(format!(
            "native binding panicked: {}",
            panic_message(payload.as_ref())
        )))),
    }
}

async fn blocking<T>(
    context: &'static str,
    work: impl FnOnce() -> std::result::Result<T, ShellUseError> + Send + 'static,
) -> Result<T>
where
    T: Send + 'static,
{
    spawn_blocking(move || ffi_boundary(work))
        .await
        .map_err(|error| {
            native_error(ShellUseError::internal(format!(
                "{context} worker failed: {error}"
            )))
        })?
}

fn timeout(value: Option<f64>, name: &str) -> std::result::Result<Option<u64>, ShellUseError> {
    value
        .map(|value| integer(value, name, u64::MAX))
        .transpose()
}

fn integer(value: f64, name: &str, max: u64) -> std::result::Result<u64, ShellUseError> {
    let max = max.min(MAX_SAFE_INTEGER);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > max as f64 {
        return Err(ShellUseError::usage(format!(
            "{name} must be an integer between 0 and {max}"
        )));
    }
    Ok(value as u64)
}

fn u16_value(value: f64, name: &str) -> std::result::Result<u16, ShellUseError> {
    Ok(integer(value, name, u64::from(u16::MAX))? as u16)
}

fn u8_value(value: f64, name: &str) -> std::result::Result<u8, ShellUseError> {
    Ok(integer(value, name, u64::from(u8::MAX))? as u8)
}

fn i32_value(value: f64, name: &str) -> std::result::Result<i32, ShellUseError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < f64::from(i32::MIN)
        || value > f64::from(i32::MAX)
    {
        return Err(ShellUseError::usage(format!(
            "{name} must be an integer between {} and {}",
            i32::MIN,
            i32::MAX
        )));
    }
    Ok(value as i32)
}

fn core_timeouts(value: Option<Timeouts>) -> std::result::Result<CoreTimeouts, ShellUseError> {
    let Some(value) = value else {
        return Ok(CoreTimeouts::default());
    };
    Ok(CoreTimeouts {
        text: timeout(value.text, "timeouts.text")?,
        idle: timeout(value.idle, "timeouts.idle")?,
        command: timeout(value.command, "timeouts.command")?,
        exit: timeout(value.exit, "timeouts.exit")?,
        ready: timeout(value.ready, "timeouts.ready")?,
    })
}

fn open_options(value: Option<OpenOptions>) -> std::result::Result<CoreOpenOptions, ShellUseError> {
    let Some(value) = value else {
        return Ok(CoreOpenOptions::default());
    };
    Ok(CoreOpenOptions {
        shell: value.shell.map(Into::into),
        cols: match value.cols {
            Some(cols) => u16_value(cols, "cols")?,
            None => shell_use::config::DEFAULT_COLS,
        },
        rows: match value.rows {
            Some(rows) => u16_value(rows, "rows")?,
            None => shell_use::config::DEFAULT_ROWS,
        },
        cwd: value.cwd,
        env: value.env.unwrap_or_default(),
        wait_ready: value.wait_ready,
        timeouts: core_timeouts(value.timeouts)?,
    })
}

fn run_options(value: RunOptions) -> std::result::Result<CoreRunOptions, ShellUseError> {
    if value.program.is_empty() {
        return Err(ShellUseError::usage("program must not be empty"));
    }
    Ok(CoreRunOptions {
        program: value.program,
        args: value.args.unwrap_or_default(),
        cols: match value.cols {
            Some(cols) => u16_value(cols, "cols")?,
            None => shell_use::config::DEFAULT_COLS,
        },
        rows: match value.rows {
            Some(rows) => u16_value(rows, "rows")?,
            None => shell_use::config::DEFAULT_ROWS,
        },
        cwd: value.cwd,
        env: value.env.unwrap_or_default(),
        wait_ready: value.wait_ready,
        timeouts: core_timeouts(value.timeouts)?,
    })
}

fn unexpected(operation: &str) -> ShellUseError {
    ShellUseError::internal(format!("{operation} returned an unexpected result type"))
}

async fn execute<T>(
    handle: SessionHandle,
    operation_name: &'static str,
    operation: Operation,
    convert: impl FnOnce(OperationResult) -> std::result::Result<T, ShellUseError> + Send + 'static,
) -> Result<T>
where
    T: Send + 'static,
{
    blocking(operation_name, move || {
        let result = handle.execute(operation)?;
        convert(result)
    })
    .await
}

#[napi]
pub struct NativeSession {
    handle: SessionHandle,
}

#[napi]
impl NativeSession {
    #[napi(constructor)]
    pub fn new(name: String) -> Self {
        Self {
            handle: global_registry().session(name),
        }
    }

    #[napi]
    pub fn name(&self) -> String {
        self.handle.name().to_string()
    }

    #[napi]
    pub async fn open(&self, options: Option<OpenOptions>) -> Result<OpenResult> {
        let handle = self.handle.clone();
        blocking("open", move || {
            let result = handle.execute(Operation::Open(open_options(options)?))?;
            match result {
                OperationResult::Open(value) => Ok(value.into()),
                _ => Err(unexpected("open")),
            }
        })
        .await
    }

    #[napi]
    pub async fn run(&self, options: RunOptions) -> Result<OpenResult> {
        let handle = self.handle.clone();
        blocking("run", move || {
            let result = handle.execute(Operation::Run(run_options(options)?))?;
            match result {
                OperationResult::Open(value) => Ok(value.into()),
                _ => Err(unexpected("run")),
            }
        })
        .await
    }

    #[napi]
    pub async fn close(&self) -> Result<()> {
        execute(
            self.handle.clone(),
            "close",
            Operation::Close,
            |result| match result {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("close")),
            },
        )
        .await
    }

    #[napi]
    pub async fn state(&self) -> Result<State> {
        execute(
            self.handle.clone(),
            "state",
            Operation::State,
            |result| match result {
                OperationResult::State(value) => Ok(value.into()),
                _ => Err(unexpected("state")),
            },
        )
        .await
    }

    #[napi]
    pub async fn text(&self, full: Option<bool>) -> Result<String> {
        execute(
            self.handle.clone(),
            "text",
            Operation::Text {
                full: full.unwrap_or(false),
            },
            |result| match result {
                OperationResult::Text(value) => Ok(value),
                _ => Err(unexpected("text")),
            },
        )
        .await
    }

    #[napi]
    pub async fn packed_screen(&self, full: Option<bool>) -> Result<PackedScreen> {
        execute(
            self.handle.clone(),
            "packedScreen",
            Operation::PackedScreen {
                full: full.unwrap_or(false),
            },
            |result| match result {
                OperationResult::PackedScreen(value) => Ok(PackedScreen {
                    cols: value.cols,
                    rows: value.rows,
                    utf8: value.utf8.into(),
                }),
                _ => Err(unexpected("packedScreen")),
            },
        )
        .await
    }

    #[napi]
    pub async fn cells(&self, x: f64, y: f64, w: Option<f64>, h: Option<f64>) -> Result<Vec<Cell>> {
        let handle = self.handle.clone();
        blocking("cells", move || {
            let operation = Operation::Cells {
                x: u16_value(x, "x")?,
                y: u16_value(y, "y")?,
                w: u16_value(w.unwrap_or(1.0), "w")?,
                h: u16_value(h.unwrap_or(1.0), "h")?,
            };
            match handle.execute(operation)? {
                OperationResult::Cells(values) => values.into_iter().map(Cell::try_from).collect(),
                _ => Err(unexpected("cells")),
            }
        })
        .await
    }

    #[napi]
    pub async fn get_command(&self) -> Result<Option<String>> {
        execute(
            self.handle.clone(),
            "getCommand",
            Operation::GetCommand,
            |result| match result {
                OperationResult::Command(value) => Ok(value),
                _ => Err(unexpected("getCommand")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_output(&self) -> Result<Option<String>> {
        execute(
            self.handle.clone(),
            "getOutput",
            Operation::GetOutput,
            |result| match result {
                OperationResult::Output(value) => Ok(value),
                _ => Err(unexpected("getOutput")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_exit_code(&self) -> Result<Option<i32>> {
        execute(
            self.handle.clone(),
            "getExitCode",
            Operation::GetExitCode,
            |result| match result {
                OperationResult::ExitCode(value) => Ok(value),
                _ => Err(unexpected("getExitCode")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_cwd(&self) -> Result<Option<String>> {
        execute(
            self.handle.clone(),
            "getCwd",
            Operation::GetCwd,
            |result| match result {
                OperationResult::Cwd(value) => Ok(value),
                _ => Err(unexpected("getCwd")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_cursor(&self) -> Result<Cursor> {
        execute(
            self.handle.clone(),
            "getCursor",
            Operation::GetCursor,
            |result| match result {
                OperationResult::Cursor(value) => Ok(value.into()),
                _ => Err(unexpected("getCursor")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_size(&self) -> Result<Size> {
        execute(
            self.handle.clone(),
            "getSize",
            Operation::GetSize,
            |result| match result {
                OperationResult::Size(value) => Ok(value.into()),
                _ => Err(unexpected("getSize")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_bell_count(&self) -> Result<f64> {
        execute(
            self.handle.clone(),
            "getBellCount",
            Operation::GetBellCount,
            |result| match result {
                OperationResult::BellCount(value) => Ok(value as f64),
                _ => Err(unexpected("getBellCount")),
            },
        )
        .await
    }

    #[napi]
    pub async fn write(&self, data: String) -> Result<()> {
        self.unit("write", Operation::Write { data }).await
    }

    #[napi(js_name = "type")]
    pub async fn type_text(&self, text: String) -> Result<()> {
        self.unit("type", Operation::Write { data: text }).await
    }

    #[napi]
    pub async fn submit(&self, data: Option<String>) -> Result<()> {
        self.unit("submit", Operation::Submit { data }).await
    }

    #[napi]
    pub async fn press(&self, keys: Vec<String>) -> Result<()> {
        self.unit("press", Operation::Press { keys }).await
    }

    #[napi]
    pub async fn mouse_click(&self, options: Option<MouseClickOptions>) -> Result<()> {
        let options = options.unwrap_or(MouseClickOptions {
            x: None,
            y: None,
            on_text: None,
            button: None,
            clicks: None,
        });
        let handle = self.handle.clone();
        blocking("mouseClick", move || {
            let action = MouseAction::Click {
                x: options.x.map(|value| u16_value(value, "x")).transpose()?,
                y: options.y.map(|value| u16_value(value, "y")).transpose()?,
                on_text: options.on_text,
                button: u8_value(options.button.unwrap_or(0.0), "button")?,
                clicks: u8_value(options.clicks.unwrap_or(1.0), "clicks")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseClick")),
            }
        })
        .await
    }

    #[napi]
    pub async fn mouse_move(&self, x: f64, y: f64) -> Result<()> {
        let handle = self.handle.clone();
        blocking("mouseMove", move || {
            let action = MouseAction::Move {
                x: u16_value(x, "x")?,
                y: u16_value(y, "y")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseMove")),
            }
        })
        .await
    }

    #[napi]
    pub async fn mouse_down(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        let handle = self.handle.clone();
        blocking("mouseDown", move || {
            let action = MouseAction::Down {
                x: u16_value(x, "x")?,
                y: u16_value(y, "y")?,
                button: u8_value(button.unwrap_or(0.0), "button")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseDown")),
            }
        })
        .await
    }

    #[napi]
    pub async fn mouse_up(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        let handle = self.handle.clone();
        blocking("mouseUp", move || {
            let action = MouseAction::Up {
                x: u16_value(x, "x")?,
                y: u16_value(y, "y")?,
                button: u8_value(button.unwrap_or(0.0), "button")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseUp")),
            }
        })
        .await
    }

    #[napi]
    pub async fn mouse_drag(
        &self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        button: Option<f64>,
    ) -> Result<()> {
        let handle = self.handle.clone();
        blocking("mouseDrag", move || {
            let action = MouseAction::Drag {
                x1: u16_value(x1, "x1")?,
                y1: u16_value(y1, "y1")?,
                x2: u16_value(x2, "x2")?,
                y2: u16_value(y2, "y2")?,
                button: u8_value(button.unwrap_or(0.0), "button")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseDrag")),
            }
        })
        .await
    }

    #[napi]
    pub async fn mouse_scroll(&self, direction: String, amount: Option<f64>) -> Result<()> {
        let handle = self.handle.clone();
        blocking("mouseScroll", move || {
            let action = MouseAction::Scroll {
                direction,
                amount: u16_value(amount.unwrap_or(3.0), "amount")?,
            };
            match handle.execute(Operation::Mouse { action })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("mouseScroll")),
            }
        })
        .await
    }

    #[napi]
    pub async fn resize(&self, cols: f64, rows: f64) -> Result<()> {
        let handle = self.handle.clone();
        blocking("resize", move || {
            let operation = Operation::Resize {
                cols: u16_value(cols, "cols")?,
                rows: u16_value(rows, "rows")?,
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("resize")),
            }
        })
        .await
    }

    #[napi]
    pub async fn signal(&self, name: String) -> Result<()> {
        self.unit("signal", Operation::Signal { name }).await
    }

    #[napi]
    pub async fn wait_text(&self, text: String, options: Option<WaitTextOptions>) -> Result<()> {
        let options = options.unwrap_or(WaitTextOptions {
            regex: None,
            full: None,
            not: None,
            timeout_ms: None,
        });
        let handle = self.handle.clone();
        blocking("waitText", move || {
            let operation = Operation::WaitText {
                text,
                regex: options.regex.unwrap_or(false),
                full: options.full.unwrap_or(false),
                timeout_ms: timeout(options.timeout_ms, "timeoutMs")?,
                not: options.not.unwrap_or(false),
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("waitText")),
            }
        })
        .await
    }

    #[napi]
    pub async fn wait_idle(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.timeout_unit("waitIdle", timeout_ms, |timeout_ms| Operation::WaitIdle {
            timeout_ms,
        })
        .await
    }

    #[napi]
    pub async fn wait_command(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.timeout_unit("waitCommand", timeout_ms, |timeout_ms| {
            Operation::WaitCommand { timeout_ms }
        })
        .await
    }

    #[napi]
    pub async fn wait_exit(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.timeout_unit("waitExit", timeout_ms, |timeout_ms| Operation::WaitExit {
            timeout_ms,
        })
        .await
    }

    #[napi]
    pub async fn wait_ready(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.timeout_unit("waitReady", timeout_ms, |timeout_ms| Operation::WaitReady {
            timeout_ms,
        })
        .await
    }

    #[napi]
    pub async fn wait_bell(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.timeout_unit("waitBell", timeout_ms, |timeout_ms| Operation::WaitBell {
            timeout_ms,
        })
        .await
    }

    #[napi]
    pub async fn expect_text(
        &self,
        text: String,
        options: Option<ExpectTextOptions>,
    ) -> Result<()> {
        let options = options.unwrap_or(ExpectTextOptions {
            regex: None,
            full: None,
            strict: None,
            not: None,
            fg: None,
            bg: None,
            timeout_ms: None,
        });
        let handle = self.handle.clone();
        blocking("expectText", move || {
            let operation = Operation::ExpectText {
                text,
                regex: options.regex.unwrap_or(false),
                full: options.full.unwrap_or(false),
                strict: options.strict.unwrap_or(true),
                not: options.not.unwrap_or(false),
                fg: options.fg,
                bg: options.bg,
                timeout_ms: timeout(options.timeout_ms, "timeoutMs")?,
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("expectText")),
            }
        })
        .await
    }

    #[napi]
    pub async fn expect_exit_code(&self, code: f64, timeout_ms: Option<f64>) -> Result<()> {
        let handle = self.handle.clone();
        blocking("expectExitCode", move || {
            let operation = Operation::ExpectExitCode {
                code: i32_value(code, "code")?,
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("expectExitCode")),
            }
        })
        .await
    }

    #[napi]
    pub async fn expect_output(&self, text: String, regex: Option<bool>) -> Result<()> {
        self.unit(
            "expectOutput",
            Operation::ExpectOutput {
                text,
                regex: regex.unwrap_or(false),
            },
        )
        .await
    }

    #[napi]
    pub async fn expect_bell_count(&self, count: f64, timeout_ms: Option<f64>) -> Result<()> {
        let handle = self.handle.clone();
        blocking("expectBellCount", move || {
            let operation = Operation::ExpectBellCount {
                count: integer(count, "count", u64::MAX)?,
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("expectBellCount")),
            }
        })
        .await
    }

    #[napi]
    pub async fn snapshot(
        &self,
        name: String,
        options: Option<SnapshotOptions>,
    ) -> Result<SnapshotResult> {
        let options = options.unwrap_or(SnapshotOptions {
            update: None,
            include_colors: None,
            cwd: None,
        });
        execute(
            self.handle.clone(),
            "snapshot",
            Operation::Snapshot {
                name,
                update: options.update.unwrap_or(false),
                include_colors: options.include_colors.unwrap_or(false),
                cwd: options.cwd,
            },
            |result| match result {
                OperationResult::Snapshot(value) => Ok(value.into()),
                _ => Err(unexpected("snapshot")),
            },
        )
        .await
    }

    #[napi]
    pub async fn screenshot(&self, options: Option<ScreenshotOptions>) -> Result<String> {
        let options = options.unwrap_or(ScreenshotOptions {
            full: None,
            path: None,
        });
        execute(
            self.handle.clone(),
            "screenshot",
            Operation::Screenshot {
                full: options.full.unwrap_or(false),
                path: options.path,
            },
            |result| match result {
                OperationResult::Screenshot(CoreScreenshotResult::Path(value))
                | OperationResult::Screenshot(CoreScreenshotResult::Text(value)) => Ok(value),
                _ => Err(unexpected("screenshot")),
            },
        )
        .await
    }

    #[napi]
    pub async fn panic_probe(&self) -> Result<()> {
        blocking(
            "panicProbe",
            || -> std::result::Result<(), ShellUseError> {
                panic!("intentional native panic probe")
            },
        )
        .await
    }
}

impl NativeSession {
    async fn unit(&self, operation_name: &'static str, operation: Operation) -> Result<()> {
        execute(
            self.handle.clone(),
            operation_name,
            operation,
            move |result| match result {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected(operation_name)),
            },
        )
        .await
    }

    async fn timeout_unit(
        &self,
        operation_name: &'static str,
        timeout_ms: Option<f64>,
        operation: impl FnOnce(Option<u64>) -> Operation + Send + 'static,
    ) -> Result<()> {
        let handle = self.handle.clone();
        blocking(operation_name, move || {
            let operation = operation(timeout(timeout_ms, "timeoutMs")?);
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected(operation_name)),
            }
        })
        .await
    }
}

#[napi]
pub async fn sessions() -> Result<Vec<String>> {
    blocking("sessions", || Ok(global_registry().sessions())).await
}

#[napi]
pub async fn close_all() -> Result<()> {
    blocking("closeAll", || {
        global_registry().close_all();
        Ok(())
    })
    .await
}

#[napi]
pub fn close_all_sync() -> Result<()> {
    ffi_boundary(|| {
        global_registry().close_all();
        Ok(())
    })
}

#[napi]
pub async fn recording(name: String) -> Result<String> {
    blocking("recording", move || {
        global_registry().recording(&name).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ShellUseError::new(
                    ErrorKind::NoSession,
                    format!("no recording for session '{name}'"),
                )
            } else {
                ShellUseError::internal(format!(
                    "failed to read the recording for session '{name}': {error}"
                ))
            }
        })
    })
    .await
}
