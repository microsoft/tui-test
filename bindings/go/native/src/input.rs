use crate::types::*;
use tui_test::api::*;
use tui_test::locator_query::LocatorExpression;
use tui_test::profile::{Profile, Rgb};
use tui_test::shell::Shell;
use tui_test::{
    DiagnosticRetentionOptions, ExecutionContext, FailureArtifactMode, FailureArtifactOptions,
    TraceMode, TraceOptions,
};

type Result<T> = std::result::Result<T, TuiTestError>;

impl TuiOptionalU64 {
    pub(crate) fn option(self) -> Option<u64> {
        self.present.then_some(self.value)
    }
}
impl TuiOptionalF64 {
    pub(crate) fn option(self) -> Option<f64> {
        self.present.then_some(self.value)
    }
}
impl TuiOptionalBool {
    pub(crate) fn option(self) -> Option<bool> {
        self.present.then_some(self.value)
    }
}

// Every exported entrypoint is unsafe: the caller must provide valid readable
// buffers for the duration of the call. Null/length and UTF-8 checks catch
// representable usage mistakes, but cannot prove foreign pointer validity.
pub(crate) unsafe fn slice<'a, T>(data: *const T, len: usize) -> Result<&'a [T]> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() || len > isize::MAX as usize / std::mem::size_of::<T>() {
        return Err(TuiTestError::usage(
            "invalid native array pointer or length",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}
impl TuiString {
    pub(crate) unsafe fn optional(self) -> Result<Option<String>> {
        if self.data.is_null() {
            if self.len != 0 {
                return Err(TuiTestError::usage(
                    "null native string with nonzero length",
                ));
            }
            return Ok(None);
        }
        let bytes = unsafe { slice(self.data, self.len)? };
        std::str::from_utf8(bytes)
            .map(|s| Some(s.to_owned()))
            .map_err(|_| TuiTestError::usage("native string must be UTF-8"))
    }
    pub(crate) unsafe fn required(self) -> Result<String> {
        unsafe { self.optional()? }
            .ok_or_else(|| TuiTestError::usage("required native string is absent"))
    }
}
pub(crate) unsafe fn strings(data: *const TuiString, len: usize) -> Result<Vec<String>> {
    unsafe { slice(data, len)? }
        .iter()
        .map(|s| unsafe { s.required() })
        .collect()
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionInput {
    name: String,
    #[serde(default)]
    artifact_directory: Option<String>,
    #[serde(default)]
    artifact_mode: Option<String>,
    #[serde(default)]
    artifact_include_recording: bool,
    #[serde(default)]
    trace_mode: Option<String>,
    #[serde(default)]
    trace_directory: Option<String>,
    #[serde(default)]
    screen_history_limit: Option<u16>,
}

pub(crate) unsafe fn session(value: TuiString) -> Result<(String, ExecutionContext)> {
    let json = unsafe { value.required()? };
    let value: SessionInput = serde_json::from_str(&json)
        .map_err(|error| TuiTestError::usage(format!("invalid native session context: {error}")))?;
    let artifact = match value.artifact_mode {
        None => None,
        Some(mode) => {
            let mode = match mode.as_str() {
                "all" => FailureArtifactMode::All,
                "html" => FailureArtifactMode::Html,
                "text" => FailureArtifactMode::Text,
                "none" => FailureArtifactMode::None,
                other => {
                    return Err(TuiTestError::usage(format!(
                        "unknown failure artifact mode {other:?}; expected all, html, text, or none"
                    )))
                }
            };
            let options = FailureArtifactOptions {
                directory: value.artifact_directory.unwrap_or_default().into(),
                mode,
                include_recording: value.artifact_include_recording,
            };
            options.validate().map_err(TuiTestError::usage)?;
            Some(options)
        }
    };
    let trace = match value.trace_mode {
        None => None,
        Some(mode) => {
            let mode = match mode.as_str() {
                "off" => TraceMode::Off,
                "on-failure" => TraceMode::OnFailure,
                "on" => TraceMode::On,
                other => {
                    return Err(TuiTestError::usage(format!(
                        "unknown trace mode {other:?}; expected off, on-failure, or on"
                    )))
                }
            };
            let options = TraceOptions {
                mode,
                directory: value
                    .trace_directory
                    .map(Into::into)
                    .unwrap_or_else(|| TraceOptions::default().directory),
            };
            options.validate().map_err(TuiTestError::usage)?;
            Some(options)
        }
    };
    let mut retention = DiagnosticRetentionOptions::default();
    if let Some(limit) = value.screen_history_limit {
        retention.screen_history_limit = limit;
    }
    retention.validate().map_err(TuiTestError::usage)?;
    Ok((
        value.name,
        ExecutionContext {
            artifact,
            trace,
            retention,
            ..ExecutionContext::default()
        },
    ))
}
unsafe fn pairs(data: *const TuiPair, len: usize) -> Result<Vec<(String, String)>> {
    unsafe { slice(data, len)? }
        .iter()
        .map(|p| Ok((unsafe { p.key.required()? }, unsafe { p.value.required()? })))
        .collect()
}
pub(crate) fn u16_option(value: TuiOptionalU64, name: &str) -> Result<Option<u16>> {
    value
        .option()
        .map(|v| {
            u16::try_from(v)
                .map_err(|_| TuiTestError::usage(format!("{name} must be between 0 and 65535")))
        })
        .transpose()
}
pub(crate) unsafe fn open(value: TuiOpenOptions) -> Result<OpenOptions> {
    let backend = unsafe { value.backend.optional()? }
        .map(|s| s.parse())
        .transpose()
        .map_err(TuiTestError::usage)?
        .unwrap_or_default();
    let shell = unsafe { value.shell.optional()? }
        .map(|s| match s.as_str() {
            "bash" => Ok(Shell::Bash),
            "powershell" => Ok(Shell::Powershell),
            "pwsh" => Ok(Shell::Pwsh),
            "cmd" => Ok(Shell::Cmd),
            "fish" => Ok(Shell::Fish),
            "zsh" => Ok(Shell::Zsh),
            "xonsh" => Ok(Shell::Xonsh),
            "elvish" => Ok(Shell::Elvish),
            "nushell" => Ok(Shell::Nushell),
            _ => Err(TuiTestError::usage(format!("unknown shell {s:?}"))),
        })
        .transpose()?;
    let mut profile = Profile::default();
    if let Some(scrollback) = value.scrollback.option() {
        profile.scrollback = usize::try_from(scrollback)
            .map_err(|_| TuiTestError::usage("scrollback exceeds addressable size"))?;
    }
    for (name, color) in unsafe { pairs(value.colors, value.colors_len)? } {
        let color = Rgb::parse(&color).map_err(TuiTestError::usage)?;
        if !profile.colors.set_named(&name, color) {
            return Err(TuiTestError::usage(format!(
                "unknown profile color {name:?}"
            )));
        }
    }
    let mode = match unsafe { value.recording_mode.optional()? }
        .as_deref()
        .unwrap_or("disabled")
    {
        "always" => AutomaticRecordingMode::Always,
        "disabled" => AutomaticRecordingMode::Disabled,
        "on-failure" => AutomaticRecordingMode::OnFailure,
        other => {
            return Err(TuiTestError::usage(format!(
                "unknown automatic recording mode {other:?}"
            )))
        }
    };
    Ok(OpenOptions {
        backend,
        shell,
        profile,
        style: Default::default(),
        cols: u16_option(value.cols, "cols")?.unwrap_or(80),
        rows: u16_option(value.rows, "rows")?.unwrap_or(30),
        cwd: unsafe { value.cwd.optional()? },
        env: unsafe { pairs(value.env, value.env_len)? },
        wait_ready: value.wait_ready.option(),
        restart: value.restart,
        timeouts: Timeouts {
            text: value.timeouts.text.option(),
            idle: value.timeouts.idle.option(),
            command: value.timeouts.command.option(),
            exit: value.timeouts.exit.option(),
            ready: value.timeouts.ready.option(),
        },
        recording: AutomaticRecording {
            mode,
            directory: unsafe { value.recording_directory.optional()? }.map(Into::into),
        },
    })
}
pub(crate) fn mouse(value: TuiMouseOptions) -> Result<MouseOptions> {
    let button = match value.button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => return Err(TuiTestError::usage("unknown mouse button")),
    };
    Ok(MouseOptions {
        button,
        alt: value.alt,
        ctrl: value.ctrl,
        shift: value.shift,
    })
}
pub(crate) unsafe fn query(value: TuiQuery) -> Result<LocatorQuery> {
    let json = unsafe { value.json.required()? };
    serde_json::from_str::<LocatorExpression>(&json)
        .map_err(|error| TuiTestError::usage(format!("invalid locator expression: {error}")))?
        .into_query()
}

pub(crate) unsafe fn capture_background(
    background: TuiString,
    transparent: bool,
) -> Result<Option<CaptureBackground>> {
    let background = unsafe { background.optional()? };
    if background.is_some() && transparent {
        return Err(TuiTestError::usage(
            "background and transparent options conflict",
        ));
    }
    if transparent {
        return Ok(Some(CaptureBackground::Transparent));
    }
    background
        .map(|value| CaptureBackground::parse(&value))
        .transpose()
}
pub(crate) unsafe fn recording(value: TuiRecordingOptions) -> Result<Operation> {
    let format = unsafe { value.format.optional()? }
        .map(|s| match s.as_str() {
            "apng" => Ok(RecordingFormat::Apng),
            "gif" => Ok(RecordingFormat::Gif),
            "mp4" => Ok(RecordingFormat::Mp4),
            "cast" => Ok(RecordingFormat::Cast),
            _ => Err(TuiTestError::usage("unknown recording format")),
        })
        .transpose()?;
    let fps = value
        .fps
        .option()
        .map(|v| u8::try_from(v).map_err(|_| TuiTestError::usage("fps must be between 0 and 255")))
        .transpose()?;
    Ok(Operation::StartRecording {
        path: unsafe { value.path.required()? },
        format,
        fps,
        speed: value.speed.option(),
        idle_time_limit: value.idle_time_limit.option(),
        zoom: value.zoom.option(),
        background: unsafe { capture_background(value.background, value.transparent)? },
    })
}
