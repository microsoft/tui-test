use crate::types::*;
use tui_test::api::*;
use tui_test::profile::{Profile, Rgb};
use tui_test::shell::Shell;

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
        .unwrap_or("always")
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
fn occurrence(kind: u32, index: usize) -> Result<MatchOccurrence> {
    match kind {
        0 => Ok(MatchOccurrence::Any),
        1 => Ok(MatchOccurrence::Unique),
        2 => Ok(MatchOccurrence::First),
        3 => Ok(MatchOccurrence::Last),
        4 => Ok(MatchOccurrence::Nth(index)),
        _ => Err(TuiTestError::usage("unknown match occurrence")),
    }
}
unsafe fn anchor(value: TuiAnchor) -> Result<Option<TextAnchor>> {
    unsafe { value.text.optional()? }
        .map(|text| {
            Ok(TextAnchor {
                text,
                regex: value.regex,
                occurrence: occurrence(value.occurrence, value.index)?,
            })
        })
        .transpose()
}
unsafe fn style(value: TuiTextStyle) -> Result<TextStyle> {
    Ok(TextStyle {
        foreground: unsafe { value.foreground.optional()? },
        background: unsafe { value.background.optional()? },
        bold: value.bold.option(),
        dim: value.dim.option(),
        italic: value.italic.option(),
        underline_style: unsafe { value.underline_style.optional()? },
        underline_color: unsafe { value.underline_color.optional()? },
        inverse: value.inverse.option(),
        hidden: value.hidden.option(),
        strikethrough: value.strikethrough.option(),
        blink: value.blink.option(),
    })
}
pub(crate) unsafe fn query(value: TuiQuery, strict: bool) -> Result<LocatorQuery> {
    let mut parent = None;
    for stage in unsafe { slice(value.stages, value.len)? } {
        let style = unsafe { style(stage.style)? };
        let selector = match stage.kind {
            0 => LocatorSelector::Text(TextSelector {
                text: unsafe { stage.text.required()? },
                regex: stage.regex,
                full: stage.full,
                whitespace: match stage.whitespace {
                    0 => WhitespaceMode::Exact,
                    1 => WhitespaceMode::Normalize,
                    _ => return Err(TuiTestError::usage("unknown whitespace mode")),
                },
                scope: TextScope {
                    after: unsafe { anchor(stage.after)? },
                    before: unsafe { anchor(stage.before)? },
                },
            }),
            1 => LocatorSelector::Style(StyleSelector {
                style: style.clone(),
                full: stage.full,
            }),
            _ => return Err(TuiTestError::usage("unknown locator selector kind")),
        };
        parent = Some(Box::new(LocatorQuery {
            selector,
            occurrence: occurrence(stage.occurrence, stage.index)?,
            within: parent,
            direction: match stage.direction {
                0 => LocatorDirection::Within,
                1 => LocatorDirection::After,
                2 => LocatorDirection::Before,
                _ => return Err(TuiTestError::usage("unknown locator direction")),
            },
            style,
        }));
    }
    let mut query =
        *parent.ok_or_else(|| TuiTestError::usage("locator requires at least one stage"))?;
    if strict && query.occurrence == MatchOccurrence::Any {
        query.occurrence = MatchOccurrence::Unique;
    }
    Ok(query)
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
    })
}
