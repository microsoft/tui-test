#![deny(clippy::all)]

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use napi::bindgen_prelude::{spawn_blocking, Buffer, Either};
use napi::{Error, Result, Status};
use napi_derive::napi;
use tui_test::profile::{Profile as CoreProfile, Rgb};
use tui_test::shell::Shell as CoreShell;
use tui_test::{
    global_registry, AutomaticRecording as CoreAutomaticRecording,
    AutomaticRecordingMode as CoreAutomaticRecordingMode, Backend as CoreBackend,
    BellEvent as CoreBellEvent, CaptureBackground, Cell as CoreCell, CellColor, ClipboardPattern,
    Cursor as CoreCursor, EffectiveTimeouts as CoreEffectiveTimeouts, ErrorKind, KeyAction,
    LocatorDirection as CoreLocatorDirection, LocatorQuery as CoreLocatorQuery,
    LocatorSelector as CoreLocatorSelector, MatchOccurrence as CoreMatchOccurrence, MouseAction,
    MouseOptions as CoreMouseOptions, OpenOptions as CoreOpenOptions, OpenResult as CoreOpenResult,
    Operation, OperationResult, RecordingFormat as CoreRecordingFormat,
    RunOptions as CoreRunOptions, ScreenshotResult as CoreScreenshotResult, SessionHandle,
    Size as CoreSize, SnapshotResult as CoreSnapshotResult, State as CoreState,
    StyleSelector as CoreStyleSelector, TextMatch as CoreTextMatch,
    TextSelector as CoreTextSelector, TextStyle as CoreTextStyle, Timeouts as CoreTimeouts,
    TuiTestError, WhitespaceMode as CoreWhitespaceMode,
};

const ERROR_PREFIX: &str = "__tui_test_native_error__:";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[napi(string_enum = "lowercase")]
pub enum Backend {
    Alacritty,
    Ghostty,
    Rio,
    Xtermjs,
}

impl From<Backend> for CoreBackend {
    fn from(value: Backend) -> Self {
        match value {
            Backend::Alacritty => Self::Alacritty,
            Backend::Ghostty => Self::Ghostty,
            Backend::Rio => Self::Rio,
            Backend::Xtermjs => Self::Xtermjs,
        }
    }
}

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

#[napi(string_enum = "lowercase")]
pub enum RecordingFormat {
    Apng,
    Gif,
    Mp4,
    Cast,
}

impl From<RecordingFormat> for CoreRecordingFormat {
    fn from(value: RecordingFormat) -> Self {
        match value {
            RecordingFormat::Apng => Self::Apng,
            RecordingFormat::Gif => Self::Gif,
            RecordingFormat::Mp4 => Self::Mp4,
            RecordingFormat::Cast => Self::Cast,
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
pub struct AutomaticRecordingOptions {
    pub mode: Option<String>,
    pub directory: Option<String>,
}

#[napi(object)]
pub struct OpenOptions {
    pub backend: Option<Backend>,
    pub shell: Option<Shell>,
    pub cols: Option<f64>,
    pub rows: Option<f64>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub wait_ready: Option<bool>,
    pub restart: Option<bool>,
    pub profile_scrollback: Option<f64>,
    pub profile_colors: Option<Vec<(String, String)>>,
    pub timeouts: Option<Timeouts>,
}

#[napi(object)]
pub struct RunOptions {
    pub backend: Option<Backend>,
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cols: Option<f64>,
    pub rows: Option<f64>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub wait_ready: Option<bool>,
    pub restart: Option<bool>,
    pub profile_scrollback: Option<f64>,
    pub profile_colors: Option<Vec<(String, String)>>,
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

#[napi(object)]
pub struct BellEvent {
    pub sequence: f64,
    #[napi(js_name = "elapsed_ms")]
    pub elapsed_ms: f64,
}

impl From<CoreBellEvent> for BellEvent {
    fn from(value: CoreBellEvent) -> Self {
        Self {
            sequence: value.sequence as f64,
            elapsed_ms: value.elapsed_ms as f64,
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
    pub title: Option<String>,
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
            title: value.title,
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

fn underline_style(value: String) -> std::result::Result<UnderlineStyle, TuiTestError> {
    match value.as_str() {
        "none" => Ok(UnderlineStyle::None),
        "single" => Ok(UnderlineStyle::Single),
        "double" => Ok(UnderlineStyle::Double),
        "curly" => Ok(UnderlineStyle::Curly),
        "dotted" => Ok(UnderlineStyle::Dotted),
        "dashed" => Ok(UnderlineStyle::Dashed),
        _ => Err(TuiTestError::internal(format!(
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
    type Error = TuiTestError;

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
pub struct TextPosition {
    pub row: u32,
    pub column: u16,
}

#[napi(object)]
pub struct TextSpan {
    pub row: u32,
    pub start: u16,
    pub end: u16,
}

#[napi(object)]
pub struct TextMatch {
    pub text: String,
    pub start: TextPosition,
    pub end: TextPosition,
    pub spans: Vec<TextSpan>,
}

impl From<CoreTextMatch> for TextMatch {
    fn from(value: CoreTextMatch) -> Self {
        Self {
            text: value.text,
            start: TextPosition {
                row: value.start.row,
                column: value.start.column,
            },
            end: TextPosition {
                row: value.end.row,
                column: value.end.column,
            },
            spans: value
                .spans
                .into_iter()
                .map(|span| TextSpan {
                    row: span.row,
                    start: span.start,
                    end: span.end,
                })
                .collect(),
        }
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

#[napi(string_enum = "lowercase")]
pub enum LocatorStageKind {
    Text,
    Style,
}

#[napi(string_enum = "lowercase")]
pub enum LocatorStageDirection {
    Within,
    After,
    Before,
}

#[derive(Default)]
#[napi(object)]
pub struct LocatorStyle {
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

#[napi(object)]
pub struct LocatorStage {
    pub kind: LocatorStageKind,
    pub direction: Option<LocatorStageDirection>,
    pub text: Option<String>,
    pub regex: Option<bool>,
    pub full: Option<bool>,
    pub whitespace: Option<String>,
    pub occurrence: Option<String>,
    pub nth: Option<f64>,
    pub style: Option<LocatorStyle>,
}

#[napi(object)]
pub struct TitleOptions {
    pub regex: Option<bool>,
    pub not: Option<bool>,
    pub timeout_ms: Option<f64>,
}

#[napi(object)]
pub struct ClipboardWaitOptions {
    pub regex: Option<bool>,
    pub timeout_ms: Option<f64>,
}

#[napi(object)]
pub struct SnapshotOptions {
    pub update: Option<bool>,
    pub include_colors: Option<bool>,
    pub include_title: Option<bool>,
    pub cwd: Option<String>,
}

#[napi(object)]
pub struct ScreenshotOptions {
    pub full: Option<bool>,
    pub path: Option<String>,
    pub zoom: Option<f64>,
    pub background: Option<String>,
    pub transparent: Option<bool>,
}

#[napi(object)]
pub struct RecordingOptions {
    pub path: String,
    pub format: Option<RecordingFormat>,
    pub fps: Option<f64>,
    pub speed: Option<f64>,
    pub idle_time_limit: Option<f64>,
    pub zoom: Option<f64>,
    pub background: Option<String>,
    pub transparent: Option<bool>,
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

fn native_error(error: TuiTestError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("{ERROR_PREFIX}{}\n{}", error.kind.as_str(), error.message),
    )
}

fn capture_background(
    background: Option<String>,
    transparent: bool,
) -> Result<Option<CaptureBackground>> {
    if background.is_some() && transparent {
        return Err(native_error(TuiTestError::usage(
            "background and transparent options conflict",
        )));
    }
    if transparent {
        return Ok(Some(CaptureBackground::Transparent));
    }
    background
        .map(|value| CaptureBackground::parse(&value).map_err(native_error))
        .transpose()
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

fn ffi_boundary<T>(work: impl FnOnce() -> std::result::Result<T, TuiTestError>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(native_error(error)),
        Err(payload) => Err(native_error(TuiTestError::internal(format!(
            "native binding panicked: {}",
            panic_message(payload.as_ref())
        )))),
    }
}

async fn blocking<T>(
    context: &'static str,
    work: impl FnOnce() -> std::result::Result<T, TuiTestError> + Send + 'static,
) -> Result<T>
where
    T: Send + 'static,
{
    spawn_blocking(move || ffi_boundary(work))
        .await
        .map_err(|error| {
            native_error(TuiTestError::internal(format!(
                "{context} worker failed: {error}"
            )))
        })?
}

fn timeout(value: Option<f64>, name: &str) -> std::result::Result<Option<u64>, TuiTestError> {
    value
        .map(|value| integer(value, name, u64::MAX))
        .transpose()
}

fn integer(value: f64, name: &str, max: u64) -> std::result::Result<u64, TuiTestError> {
    let max = max.min(MAX_SAFE_INTEGER);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > max as f64 {
        return Err(TuiTestError::usage(format!(
            "{name} must be an integer between 0 and {max}"
        )));
    }
    Ok(value as u64)
}

fn u16_value(value: f64, name: &str) -> std::result::Result<u16, TuiTestError> {
    Ok(integer(value, name, u64::from(u16::MAX))? as u16)
}

fn u8_value(value: f64, name: &str) -> std::result::Result<u8, TuiTestError> {
    Ok(integer(value, name, u64::from(u8::MAX))? as u8)
}

fn mouse_options(value: f64) -> std::result::Result<CoreMouseOptions, TuiTestError> {
    let code = u8_value(value, "button")?;
    CoreMouseOptions::from_sgr_code(code)
        .ok_or_else(|| TuiTestError::usage(format!("invalid mouse button code {code}")))
}

fn core_occurrence(
    value: Option<String>,
    nth: Option<f64>,
    default: CoreMatchOccurrence,
    name: &str,
) -> std::result::Result<CoreMatchOccurrence, TuiTestError> {
    if let Some(index) = nth {
        if let Some(value) = value.as_deref().filter(|value| *value != "nth") {
            return Err(TuiTestError::usage(format!(
                "{name} cannot be used with occurrence '{value}'"
            )));
        }
        return Ok(CoreMatchOccurrence::Nth(
            integer(index, name, usize::MAX as u64)? as usize,
        ));
    }
    match value.as_deref() {
        None => Ok(default),
        Some("any") => Ok(CoreMatchOccurrence::Any),
        Some("unique") => Ok(CoreMatchOccurrence::Unique),
        Some("first") => Ok(CoreMatchOccurrence::First),
        Some("last") => Ok(CoreMatchOccurrence::Last),
        Some("nth") => Err(TuiTestError::usage(format!("{name} requires an nth index"))),
        Some(value) => Err(TuiTestError::usage(format!(
            "{name} must be any, unique, first, last, or nth (got '{value}')"
        ))),
    }
}

fn core_style(style: LocatorStyle) -> CoreTextStyle {
    CoreTextStyle {
        foreground: style.foreground,
        background: style.background,
        bold: style.bold,
        dim: style.dim,
        italic: style.italic,
        underline_style: style.underline_style,
        underline_color: style.underline_color,
        inverse: style.inverse,
        hidden: style.hidden,
        strikethrough: style.strikethrough,
        blink: style.blink,
    }
}

fn core_query(stages: Vec<LocatorStage>) -> std::result::Result<CoreLocatorQuery, TuiTestError> {
    let mut parent = None;
    for (index, stage) in stages.into_iter().enumerate() {
        let occurrence = core_occurrence(
            stage.occurrence,
            stage.nth,
            CoreMatchOccurrence::Any,
            &format!("stages[{index}].nth"),
        )?;
        let selector = match stage.kind {
            LocatorStageKind::Text => {
                if stage.style.is_some() {
                    return Err(TuiTestError::usage(
                        "text locator stages do not accept style parameters",
                    ));
                }
                let whitespace = match stage.whitespace.as_deref() {
                    None | Some("exact") => CoreWhitespaceMode::Exact,
                    Some("normalize") => CoreWhitespaceMode::Normalize,
                    Some(value) => {
                        return Err(TuiTestError::usage(format!(
                            "whitespace must be exact or normalize (got '{value}')"
                        )))
                    }
                };
                CoreLocatorSelector::Text(CoreTextSelector {
                    text: stage
                        .text
                        .ok_or_else(|| TuiTestError::usage("text locator stage requires text"))?,
                    regex: stage.regex.unwrap_or(false),
                    full: stage.full.unwrap_or(false),
                    whitespace,
                    scope: Default::default(),
                })
            }
            LocatorStageKind::Style => {
                if stage.text.is_some()
                    || stage.regex.unwrap_or(false)
                    || stage.whitespace.is_some()
                {
                    return Err(TuiTestError::usage(
                        "style locator stages do not accept text parameters",
                    ));
                }
                CoreLocatorSelector::Style(CoreStyleSelector {
                    style: core_style(stage.style.ok_or_else(|| {
                        TuiTestError::usage("style locator stage requires style")
                    })?),
                    full: stage.full.unwrap_or(false),
                })
            }
        };
        parent = Some(CoreLocatorQuery {
            selector,
            occurrence,
            within: parent.map(Box::new),
            direction: match stage.direction {
                None | Some(LocatorStageDirection::Within) => CoreLocatorDirection::Within,
                Some(LocatorStageDirection::After) => CoreLocatorDirection::After,
                Some(LocatorStageDirection::Before) => CoreLocatorDirection::Before,
            },
            style: CoreTextStyle::default(),
        });
    }
    parent.ok_or_else(|| TuiTestError::usage("locator requires at least one stage"))
}

fn i32_value(value: f64, name: &str) -> std::result::Result<i32, TuiTestError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < f64::from(i32::MIN)
        || value > f64::from(i32::MAX)
    {
        return Err(TuiTestError::usage(format!(
            "{name} must be an integer between {} and {}",
            i32::MIN,
            i32::MAX
        )));
    }
    Ok(value as i32)
}

fn core_timeouts(value: Option<Timeouts>) -> std::result::Result<CoreTimeouts, TuiTestError> {
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

fn core_recording(
    value: Option<AutomaticRecordingOptions>,
) -> std::result::Result<CoreAutomaticRecording, TuiTestError> {
    let Some(value) = value else {
        return Ok(CoreAutomaticRecording::default());
    };
    let mode = match value.mode.as_deref().unwrap_or("always") {
        "disabled" => CoreAutomaticRecordingMode::Disabled,
        "on-failure" => CoreAutomaticRecordingMode::OnFailure,
        "always" => CoreAutomaticRecordingMode::Always,
        other => {
            return Err(TuiTestError::usage(format!(
            "unknown automatic recording mode {other:?}; expected disabled, on-failure, or always"
        )))
        }
    };
    Ok(CoreAutomaticRecording {
        mode,
        directory: value.directory.map(Into::into),
    })
}

fn core_profile(
    scrollback: Option<f64>,
    colors: &[(String, String)],
) -> std::result::Result<CoreProfile, TuiTestError> {
    let mut profile = CoreProfile::default();
    if let Some(scrollback) = scrollback {
        profile.scrollback = integer(scrollback, "profile.scrollback", usize::MAX as u64)? as usize;
    }
    for (name, value) in colors {
        let value = Rgb::parse(value)
            .map_err(|error| TuiTestError::usage(format!("profile.colors.{name}: {error}")))?;
        if !profile.colors.set_named(name, value) {
            return Err(TuiTestError::usage(format!(
                "unknown profile color {name:?}"
            )));
        }
    }
    Ok(profile)
}

fn open_options(
    value: Option<OpenOptions>,
    recording: CoreAutomaticRecording,
) -> std::result::Result<CoreOpenOptions, TuiTestError> {
    let Some(value) = value else {
        return Ok(CoreOpenOptions {
            recording,
            ..CoreOpenOptions::default()
        });
    };
    let profile = core_profile(
        value.profile_scrollback,
        value.profile_colors.as_deref().unwrap_or_default(),
    )?;
    Ok(CoreOpenOptions {
        backend: value.backend.map(Into::into).unwrap_or_default(),
        profile,
        shell: value.shell.map(Into::into),
        cols: match value.cols {
            Some(cols) => u16_value(cols, "cols")?,
            None => tui_test::config::DEFAULT_COLS,
        },
        rows: match value.rows {
            Some(rows) => u16_value(rows, "rows")?,
            None => tui_test::config::DEFAULT_ROWS,
        },
        cwd: value.cwd,
        env: value.env.unwrap_or_default(),
        wait_ready: value.wait_ready,
        restart: value.restart.unwrap_or(false),
        timeouts: core_timeouts(value.timeouts)?,
        recording,
    })
}

fn run_options(
    value: RunOptions,
    recording: CoreAutomaticRecording,
) -> std::result::Result<CoreRunOptions, TuiTestError> {
    if value.program.is_empty() {
        return Err(TuiTestError::usage("program must not be empty"));
    }
    let profile = core_profile(
        value.profile_scrollback,
        value.profile_colors.as_deref().unwrap_or_default(),
    )?;
    Ok(CoreRunOptions {
        backend: value.backend.map(Into::into).unwrap_or_default(),
        profile,
        program: value.program,
        args: value.args.unwrap_or_default(),
        cols: match value.cols {
            Some(cols) => u16_value(cols, "cols")?,
            None => tui_test::config::DEFAULT_COLS,
        },
        rows: match value.rows {
            Some(rows) => u16_value(rows, "rows")?,
            None => tui_test::config::DEFAULT_ROWS,
        },
        cwd: value.cwd,
        env: value.env.unwrap_or_default(),
        wait_ready: value.wait_ready,
        restart: value.restart.unwrap_or(false),
        timeouts: core_timeouts(value.timeouts)?,
        recording,
    })
}

fn unexpected(operation: &str) -> TuiTestError {
    TuiTestError::internal(format!("{operation} returned an unexpected result type"))
}

async fn execute<T>(
    handle: SessionHandle,
    operation_name: &'static str,
    operation: Operation,
    convert: impl FnOnce(OperationResult) -> std::result::Result<T, TuiTestError> + Send + 'static,
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
    recording: CoreAutomaticRecording,
}

#[napi]
impl NativeSession {
    #[napi(constructor)]
    pub fn new(name: String, recording: Option<AutomaticRecordingOptions>) -> Result<Self> {
        Ok(Self {
            handle: global_registry().session(name),
            recording: core_recording(recording).map_err(native_error)?,
        })
    }

    #[napi]
    pub fn name(&self) -> String {
        self.handle.name().to_string()
    }

    #[napi]
    pub async fn open(&self, options: Option<OpenOptions>) -> Result<OpenResult> {
        let handle = self.handle.clone();
        let recording = self.recording.clone();
        blocking("open", move || {
            let result = handle.execute(Operation::Open(open_options(options, recording)?))?;
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
        let recording = self.recording.clone();
        blocking("run", move || {
            let result = handle.execute(Operation::Run(run_options(options, recording)?))?;
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
    pub async fn find_locator(&self, stages: Vec<LocatorStage>) -> Result<Vec<TextMatch>> {
        let handle = self.handle.clone();
        blocking("findLocator", move || {
            let query = core_query(stages)?;
            match handle.execute(Operation::FindLocator { query })? {
                OperationResult::Matches(matches) => {
                    Ok(matches.into_iter().map(TextMatch::from).collect())
                }
                _ => Err(unexpected("findLocator")),
            }
        })
        .await
    }

    #[napi]
    pub async fn wait_locator(
        &self,
        stages: Vec<LocatorStage>,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        let handle = self.handle.clone();
        blocking("waitLocator", move || {
            let query = core_query(stages)?;
            match handle.execute(Operation::WaitLocator {
                query,
                not: not.unwrap_or(false),
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("waitLocator")),
            }
        })
        .await
    }

    #[napi]
    pub async fn click_locator(
        &self,
        stages: Vec<LocatorStage>,
        button: Option<f64>,
        clicks: Option<f64>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        let handle = self.handle.clone();
        blocking("clickLocator", move || {
            let query = core_query(stages)?;
            match handle.execute(Operation::ClickLocator {
                query,
                options: mouse_options(button.unwrap_or(0.0))?,
                clicks: u8_value(clicks.unwrap_or(1.0), "clicks")?,
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("clickLocator")),
            }
        })
        .await
    }

    #[napi]
    pub async fn highlight_locator(
        &self,
        stages: Vec<LocatorStage>,
        timeout_ms: Option<f64>,
    ) -> Result<Vec<TextMatch>> {
        let handle = self.handle.clone();
        blocking("highlightLocator", move || {
            let query = core_query(stages)?;
            match handle.execute(Operation::HighlightLocator {
                query,
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            })? {
                OperationResult::Matches(matches) => {
                    Ok(matches.into_iter().map(TextMatch::from).collect())
                }
                _ => Err(unexpected("highlightLocator")),
            }
        })
        .await
    }

    #[napi]
    pub async fn expect_locator(
        &self,
        stages: Vec<LocatorStage>,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        let handle = self.handle.clone();
        blocking("expectLocator", move || {
            let query = core_query(stages)?;
            match handle.execute(Operation::WaitLocator {
                query,
                not: not.unwrap_or(false),
                timeout_ms: timeout(timeout_ms, "timeoutMs")?,
            })? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("expectLocator")),
            }
        })
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
    pub async fn get_bell_events(&self) -> Result<Vec<BellEvent>> {
        execute(
            self.handle.clone(),
            "getBellEvents",
            Operation::GetBellEvents,
            |result| match result {
                OperationResult::BellEvents(events) => {
                    Ok(events.into_iter().map(Into::into).collect())
                }
                _ => Err(unexpected("getBellEvents")),
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
        self.unit(
            "press",
            Operation::Key {
                keys,
                action: KeyAction::Press,
            },
        )
        .await
    }

    #[napi]
    pub async fn key_down(&self, keys: Vec<String>) -> Result<()> {
        self.unit(
            "keydown",
            Operation::Key {
                keys,
                action: KeyAction::Down,
            },
        )
        .await
    }

    #[napi]
    pub async fn repeat(&self, keys: Vec<String>) -> Result<()> {
        self.unit(
            "repeat",
            Operation::Key {
                keys,
                action: KeyAction::Repeat,
            },
        )
        .await
    }

    #[napi]
    pub async fn key_up(&self, keys: Vec<String>) -> Result<()> {
        self.unit(
            "keyup",
            Operation::Key {
                keys,
                action: KeyAction::Up,
            },
        )
        .await
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
                options: mouse_options(options.button.unwrap_or(0.0))?,
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
                options: mouse_options(button.unwrap_or(0.0))?,
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
                options: mouse_options(button.unwrap_or(0.0))?,
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
                options: mouse_options(button.unwrap_or(0.0))?,
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
    pub async fn get_title(&self) -> Result<Option<String>> {
        execute(
            self.handle.clone(),
            "getTitle",
            Operation::GetTitle,
            |result| match result {
                OperationResult::Title(value) => Ok(value),
                _ => Err(unexpected("getTitle")),
            },
        )
        .await
    }

    #[napi]
    pub async fn get_clipboard(&self) -> Result<String> {
        execute(
            self.handle.clone(),
            "getClipboard",
            Operation::GetClipboard,
            |result| match result {
                OperationResult::Clipboard(value) => Ok(value),
                _ => Err(unexpected("getClipboard")),
            },
        )
        .await
    }

    #[napi]
    pub async fn wait_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or(TitleOptions {
            regex: None,
            not: None,
            timeout_ms: None,
        });
        let handle = self.handle.clone();
        blocking("waitTitle", move || {
            let operation = Operation::WaitTitle {
                text,
                regex: options.regex.unwrap_or(false),
                timeout_ms: timeout(options.timeout_ms, "timeoutMs")?,
                not: options.not.unwrap_or(false),
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("waitTitle")),
            }
        })
        .await
    }

    #[napi]
    pub async fn wait_clipboard(
        &self,
        text: Option<String>,
        options: Option<ClipboardWaitOptions>,
    ) -> Result<()> {
        let options = options.unwrap_or(ClipboardWaitOptions {
            regex: None,
            timeout_ms: None,
        });
        let handle = self.handle.clone();
        blocking("waitClipboard", move || {
            let regex = options.regex.unwrap_or(false);
            let timeout_ms = timeout(options.timeout_ms, "timeoutMs")?;
            let operation = match text {
                Some(text) => {
                    let pattern = if regex {
                        ClipboardPattern::regex(&text).map_err(|error| {
                            TuiTestError::usage(format!("invalid regex: {error}"))
                        })?
                    } else {
                        text.into()
                    };
                    Operation::WaitClipboardMatch {
                        pattern,
                        timeout_ms,
                    }
                }
                None if regex => return Err(TuiTestError::usage("clipboard regex requires text")),
                None => Operation::WaitClipboard { timeout_ms },
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("waitClipboard")),
            }
        })
        .await
    }

    #[napi]
    pub async fn expect_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or(TitleOptions {
            regex: None,
            not: None,
            timeout_ms: None,
        });
        let handle = self.handle.clone();
        blocking("expectTitle", move || {
            let operation = Operation::ExpectTitle {
                text,
                regex: options.regex.unwrap_or(false),
                not: options.not.unwrap_or(false),
                timeout_ms: timeout(options.timeout_ms, "timeoutMs")?,
            };
            match handle.execute(operation)? {
                OperationResult::Unit => Ok(()),
                _ => Err(unexpected("expectTitle")),
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
            include_title: None,
            cwd: None,
        });
        execute(
            self.handle.clone(),
            "snapshot",
            Operation::Snapshot {
                name,
                update: options.update.unwrap_or(false),
                include_colors: options.include_colors.unwrap_or(false),
                include_title: options.include_title.unwrap_or(false),
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
            zoom: None,
            background: None,
            transparent: None,
        });
        let background =
            capture_background(options.background, options.transparent.unwrap_or(false))?;
        execute(
            self.handle.clone(),
            "screenshot",
            Operation::Screenshot {
                full: options.full.unwrap_or(false),
                path: options.path,
                zoom: options.zoom,
                background,
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
    pub async fn start_recording(&self, options: RecordingOptions) -> Result<()> {
        let fps = options
            .fps
            .map(|value| u8_value(value, "fps"))
            .transpose()
            .map_err(native_error)?;
        let background =
            capture_background(options.background, options.transparent.unwrap_or(false))?;
        self.unit(
            "startRecording",
            Operation::StartRecording {
                path: options.path,
                format: options.format.map(Into::into),
                fps,
                speed: options.speed,
                idle_time_limit: options.idle_time_limit,
                zoom: options.zoom,
                background,
            },
        )
        .await
    }

    #[napi]
    pub async fn stop_recording(&self) -> Result<String> {
        execute(
            self.handle.clone(),
            "stopRecording",
            Operation::StopRecording,
            |result| match result {
                OperationResult::Recording(path) => Ok(path),
                _ => Err(unexpected("stopRecording")),
            },
        )
        .await
    }

    #[napi]
    pub async fn panic_probe(&self) -> Result<()> {
        blocking("panicProbe", || -> std::result::Result<(), TuiTestError> {
            panic!("intentional native panic probe")
        })
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
                TuiTestError::new(
                    ErrorKind::NoSession,
                    format!("no recording for session '{name}'"),
                )
            } else {
                TuiTestError::internal(format!(
                    "failed to read the recording for session '{name}': {error}"
                ))
            }
        })
    })
    .await
}
