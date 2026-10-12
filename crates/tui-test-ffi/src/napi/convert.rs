use super::{internal, native_error, types::*, usage, MAX_SAFE_INTEGER};
use napi::bindgen_prelude::Either;
use napi::Result;
use tui_test::api::{self, OperationResult};
use tui_test::profile::{Profile, Rgb};
use tui_test::terminal::backend::Backend as TerminalBackend;

pub fn integer(value: f64, name: &str, max: u64) -> Result<u64> {
    let max = max.min(MAX_SAFE_INTEGER);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > max as f64 {
        return Err(usage(format!(
            "{name} must be an integer between 0 and {max}"
        )));
    }
    Ok(value as u64)
}

pub fn optional_integer(value: Option<f64>, name: &str, max: u64) -> Result<Option<u64>> {
    value.map(|value| integer(value, name, max)).transpose()
}

pub fn u16_value(value: f64, name: &str) -> Result<u16> {
    Ok(integer(value, name, u16::MAX.into())? as u16)
}

pub fn optional_u16(value: Option<f64>, name: &str) -> Result<Option<u16>> {
    value.map(|value| u16_value(value, name)).transpose()
}

pub fn u8_value(value: f64, name: &str) -> Result<u8> {
    Ok(integer(value, name, u8::MAX.into())? as u8)
}

pub fn i32_value(value: f64, name: &str) -> Result<i32> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i32::MIN as f64
        || value > i32::MAX as f64
    {
        return Err(usage(format!(
            "{name} must be an integer between {} and {}",
            i32::MIN,
            i32::MAX
        )));
    }
    Ok(value as i32)
}

pub fn mouse_options(value: f64) -> Result<api::MouseOptions> {
    let code = u8_value(value, "button")?;
    if code & !31 != 0 || code & 3 == 3 {
        return Err(usage(format!("invalid mouse button code {code}")));
    }
    Ok(api::MouseOptions {
        button: match code & 3 {
            0 => api::MouseButton::Left,
            1 => api::MouseButton::Middle,
            _ => api::MouseButton::Right,
        },
        shift: code & 4 != 0,
        alt: code & 8 != 0,
        ctrl: code & 16 != 0,
    })
}

pub fn open_options(value: OpenOptions, directory: Option<String>) -> Result<api::OpenOptions> {
    let mut profile = Profile::default();
    if let Some(scrollback) = optional_integer(
        value.profile_scrollback,
        "profile.scrollback",
        usize::MAX as u64,
    )? {
        profile.scrollback = scrollback as usize;
    }
    for (name, color) in value.profile_colors.unwrap_or_default() {
        let color = Rgb::parse(&color).map_err(usage)?;
        if !profile.colors.set_named(&name, color) {
            return Err(usage(format!("unknown profile color {name:?}")));
        }
    }
    let timeouts = value.timeouts.unwrap_or_default();
    Ok(api::OpenOptions {
        backend: match value.backend {
            None => Default::default(),
            Some(Backend::Alacritty) => TerminalBackend::Alacritty,
            Some(Backend::Ghostty) => TerminalBackend::Ghostty,
            Some(Backend::Rio) => TerminalBackend::Rio,
            Some(Backend::Xtermjs) => TerminalBackend::Xtermjs,
        },
        shell: value.shell.map(|shell| match shell {
            Shell::Bash => tui_test::shell::Shell::Bash,
            Shell::Powershell => tui_test::shell::Shell::Powershell,
            Shell::Pwsh => tui_test::shell::Shell::Pwsh,
            Shell::Cmd => tui_test::shell::Shell::Cmd,
            Shell::Fish => tui_test::shell::Shell::Fish,
            Shell::Zsh => tui_test::shell::Shell::Zsh,
            Shell::Xonsh => tui_test::shell::Shell::Xonsh,
            Shell::Elvish => tui_test::shell::Shell::Elvish,
            Shell::Nushell => tui_test::shell::Shell::Nushell,
        }),
        profile,
        cols: u16_value(value.cols.unwrap_or(80.0), "cols")?,
        rows: u16_value(value.rows.unwrap_or(30.0), "rows")?,
        cwd: value.cwd,
        env: value.env.unwrap_or_default(),
        wait_ready: value.wait_ready,
        restart: value.restart.unwrap_or(false),
        timeouts: api::Timeouts {
            text: optional_integer(timeouts.text, "timeouts.text", u64::MAX)?,
            idle: optional_integer(timeouts.idle, "timeouts.idle", u64::MAX)?,
            command: optional_integer(timeouts.command, "timeouts.command", u64::MAX)?,
            exit: optional_integer(timeouts.exit, "timeouts.exit", u64::MAX)?,
            ready: optional_integer(timeouts.ready, "timeouts.ready", u64::MAX)?,
        },
        recording: api::AutomaticRecording {
            mode: api::AutomaticRecordingMode::Disabled,
            directory: directory.map(Into::into),
        },
    })
}

pub fn query(expression: LocatorExpression) -> Result<api::LocatorQuery> {
    let mut nodes = Vec::with_capacity(expression.nodes.len());
    for (index, node) in expression.nodes.into_iter().enumerate() {
        let occurrence = if let Some(nth) = node.nth {
            if let Some(value) = node.occurrence.as_deref().filter(|value| *value != "nth") {
                return Err(usage(format!(
                    "nodes[{index}].nth cannot be used with occurrence '{value}'"
                )));
            }
            serde_json::json!({"nth": integer(nth, &format!("nodes[{index}].nth"), usize::MAX as u64)?})
        } else {
            match node.occurrence.as_deref().unwrap_or("any") {
                "any" | "unique" | "first" | "last" => {
                    node.occurrence.unwrap_or_else(|| "any".into()).into()
                }
                "nth" => return Err(usage(format!("nodes[{index}].nth requires an nth index"))),
                value => return Err(usage(format!("invalid occurrence '{value}'"))),
            }
        };
        let reference = |value: Option<f64>| {
            value
                .map(|value| integer(value, "locator reference", 255))
                .transpose()
        };
        nodes.push(serde_json::json!({
            "kind": node.kind, "direction": node.direction, "text": node.text, "regex": node.regex,
            "full": node.full, "whitespace": node.whitespace, "occurrence": occurrence, "style": node.style,
            "link": node.link, "within": reference(node.within)?, "left": reference(node.left)?,
            "right": reference(node.right)?, "input": reference(node.input)?,
            "has": reference(node.has)?, "has_not": reference(node.has_not)?,
        }));
    }
    serde_json::from_value::<tui_test::locator_query::LocatorExpression>(
        serde_json::json!({"nodes": nodes, "root": integer(expression.root, "locator root", 255)?}),
    )
    .map_err(|error| usage(format!("invalid locator expression: {error}")))?
    .into_query()
    .map_err(native_error)
}

pub fn unit(value: OperationResult) -> Result<()> {
    match value {
        OperationResult::Unit => Ok(()),
        _ => Err(internal("native operation returned an unexpected result")),
    }
}

pub fn string_result(value: OperationResult) -> Result<String> {
    match value {
        OperationResult::Text(value)
        | OperationResult::Clipboard(value)
        | OperationResult::Recording(value)
        | OperationResult::Screenshot(
            api::ScreenshotResult::Path(value) | api::ScreenshotResult::Text(value),
        ) => Ok(value),
        _ => Err(internal("native operation returned an unexpected result")),
    }
}

pub fn optional_string_result(value: OperationResult) -> Result<Option<String>> {
    match value {
        OperationResult::Command(value)
        | OperationResult::Output(value)
        | OperationResult::Cwd(value)
        | OperationResult::Title(value) => Ok(value),
        _ => Err(internal("native operation returned an unexpected result")),
    }
}

pub fn open_result(value: OperationResult) -> Result<OpenResult> {
    let OperationResult::Open(value) = value else {
        return Err(internal("native operation returned an unexpected result"));
    };
    Ok(OpenResult {
        shell_pid: value.shell_pid,
        session: value.session,
        ready: value.ready,
        recording: value.recording,
    })
}

pub fn cursor(value: api::Cursor) -> Cursor {
    Cursor {
        x: value.x,
        y: value.y,
        visible: value.visible,
        shape: value.shape,
        color: value.color,
    }
}

pub fn state(value: api::State) -> State {
    State {
        session_shell: value.session_shell,
        cols: value.cols,
        rows: value.rows,
        cursor: cursor(value.cursor),
        title: value.title,
        cwd: value.cwd,
        last_command: value.last_command,
        last_exit: value.last_exit,
        exit_signal: value.exit_signal,
        exited: value.exited,
        ready: value.ready,
        bell_count: value.bell_count as f64,
        modes: value.modes.into_iter().collect(),
        mouse_mode: value.mouse_mode,
        colors: TerminalColors {
            foreground: value.colors.foreground,
            background: value.colors.background,
            cursor: value.colors.cursor,
            palette: value
                .colors
                .palette
                .into_iter()
                .map(|(index, color)| (index.to_string(), color))
                .collect(),
        },
        timeouts: EffectiveTimeouts {
            text: value.timeouts.text as f64,
            idle: value.timeouts.idle as f64,
            command: value.timeouts.command as f64,
            exit: value.timeouts.exit as f64,
            ready: value.timeouts.ready as f64,
        },
        text: value.text,
    }
}

pub fn color(value: api::CellColor) -> Color {
    match value {
        api::CellColor::Default => Either::B("default".into()),
        api::CellColor::Indexed(index) => Either::A(index.into()),
        api::CellColor::Rgb(red, green, blue) => {
            Either::B(format!("#{red:02x}{green:02x}{blue:02x}"))
        }
    }
}

pub fn cell(value: api::Cell) -> Result<Cell> {
    Ok(Cell {
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
        underline_style: match value.underline_style.as_str() {
            "none" => UnderlineStyle::None,
            "single" => UnderlineStyle::Single,
            "double" => UnderlineStyle::Double,
            "curly" => UnderlineStyle::Curly,
            "dotted" => UnderlineStyle::Dotted,
            "dashed" => UnderlineStyle::Dashed,
            value => {
                return Err(internal(format!(
                    "terminal returned unknown underline style '{value}'"
                )))
            }
        },
        underline_color: color(value.underline_color),
        link: value.link,
        link_id: value.link_id,
    })
}

pub fn matches(value: OperationResult) -> Result<Vec<TextMatch>> {
    let OperationResult::Matches(value) = value else {
        return Err(internal("native operation returned an unexpected result"));
    };
    Ok(value
        .into_iter()
        .map(|value| TextMatch {
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
                .map(|value| TextSpan {
                    row: value.row,
                    start: value.start,
                    end: value.end,
                })
                .collect(),
        })
        .collect())
}
