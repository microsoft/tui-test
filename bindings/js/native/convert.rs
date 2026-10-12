use super::{abi::*, internal, types::*, usage, MAX_SAFE_INTEGER};
use napi::bindgen_prelude::Either;
use napi::Result;

pub fn borrowed(value: &str) -> TuiString {
    TuiString {
        data: value.as_ptr(),
        len: value.len(),
    }
}

pub fn optional_borrowed(value: Option<&str>) -> TuiString {
    value.map(borrowed).unwrap_or_default()
}

pub fn array<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    }
}

pub fn bytes<'a>(value: TuiString) -> &'a [u8] {
    array(value.data, value.len)
}
pub fn text(value: TuiString) -> String {
    String::from_utf8_lossy(bytes(value)).into_owned()
}
pub fn optional_text(value: TuiString) -> Option<String> {
    (!value.data.is_null()).then(|| text(value))
}

pub fn optional_bool(value: Option<bool>) -> TuiOptionalBool {
    TuiOptionalBool {
        present: value.is_some(),
        value: value.unwrap_or_default(),
    }
}
pub fn optional_float(value: Option<f64>) -> TuiOptionalF64 {
    TuiOptionalF64 {
        present: value.is_some(),
        value: value.unwrap_or_default(),
    }
}
pub fn integer(value: f64, name: &str, max: u64) -> Result<u64> {
    let max = max.min(MAX_SAFE_INTEGER);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > max as f64 {
        return Err(usage(format!(
            "{name} must be an integer between 0 and {max}"
        )));
    }
    Ok(value as u64)
}
pub fn optional_integer(value: Option<f64>, name: &str, max: u64) -> Result<TuiOptionalU64> {
    Ok(TuiOptionalU64 {
        present: value.is_some(),
        value: value
            .map(|value| integer(value, name, max))
            .transpose()?
            .unwrap_or_default(),
    })
}
pub fn u16_value(value: f64, name: &str) -> Result<u16> {
    Ok(integer(value, name, u16::MAX.into())? as u16)
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
pub fn wait_options(timeout: Option<f64>, regex: bool, not: bool) -> Result<TuiWaitOptions> {
    Ok(TuiWaitOptions {
        timeout_ms: optional_integer(timeout, "timeoutMs", u64::MAX)?,
        regex,
        not,
    })
}
pub fn mouse_options(value: f64) -> Result<TuiMouseOptions> {
    let code = u8_value(value, "button")?;
    if code & !31 != 0 || code & 3 == 3 {
        return Err(usage(format!("invalid mouse button code {code}")));
    }
    Ok(TuiMouseOptions {
        button: (code & 3).into(),
        shift: code & 4 != 0,
        alt: code & 8 != 0,
        ctrl: code & 16 != 0,
    })
}
pub fn session_context(context: &serde_json::Value, limit: Option<f64>) -> Result<String> {
    let mut context = context.clone();
    if let Some(limit) = limit {
        context["screen_history_limit"] = u16_value(limit, "screenHistoryLimit")?.into();
    }
    Ok(context.to_string())
}

pub struct OpenInput {
    pub options: TuiOpenOptions,
    _value: OpenOptions,
    _env: Vec<TuiPair>,
    _colors: Vec<TuiPair>,
    _directory: Option<String>,
}

impl OpenInput {
    pub fn new(value: OpenOptions, directory: Option<String>) -> Result<Self> {
        let pairs = |pairs: &Option<Vec<(String, String)>>| -> Vec<TuiPair> {
            pairs
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|(key, value)| TuiPair {
                    key: borrowed(key),
                    value: borrowed(value),
                })
                .collect()
        };
        let env = pairs(&value.env);
        let colors = pairs(&value.profile_colors);
        let timeouts = value.timeouts.as_ref();
        let options = TuiOpenOptions {
            backend: optional_borrowed(value.backend.as_ref().map(|value| match value {
                Backend::Alacritty => "alacritty",
                Backend::Ghostty => "ghostty",
                Backend::Rio => "rio",
                Backend::Xtermjs => "xtermjs",
            })),
            shell: optional_borrowed(value.shell.as_ref().map(|value| match value {
                Shell::Bash => "bash",
                Shell::Powershell => "powershell",
                Shell::Pwsh => "pwsh",
                Shell::Cmd => "cmd",
                Shell::Fish => "fish",
                Shell::Zsh => "zsh",
                Shell::Xonsh => "xonsh",
                Shell::Elvish => "elvish",
                Shell::Nushell => "nushell",
            })),
            cols: optional_integer(value.cols, "cols", u16::MAX.into())?,
            rows: optional_integer(value.rows, "rows", u16::MAX.into())?,
            cwd: optional_borrowed(value.cwd.as_deref()),
            env: env.as_ptr(),
            env_len: env.len(),
            wait_ready: optional_bool(value.wait_ready),
            restart: value.restart.unwrap_or(false),
            scrollback: optional_integer(
                value.profile_scrollback,
                "profile.scrollback",
                usize::MAX as u64,
            )?,
            colors: colors.as_ptr(),
            colors_len: colors.len(),
            timeouts: TuiTimeouts {
                text: optional_integer(
                    timeouts.and_then(|value| value.text),
                    "timeouts.text",
                    u64::MAX,
                )?,
                idle: optional_integer(
                    timeouts.and_then(|value| value.idle),
                    "timeouts.idle",
                    u64::MAX,
                )?,
                command: optional_integer(
                    timeouts.and_then(|value| value.command),
                    "timeouts.command",
                    u64::MAX,
                )?,
                exit: optional_integer(
                    timeouts.and_then(|value| value.exit),
                    "timeouts.exit",
                    u64::MAX,
                )?,
                ready: optional_integer(
                    timeouts.and_then(|value| value.ready),
                    "timeouts.ready",
                    u64::MAX,
                )?,
            },
            recording_directory: optional_borrowed(directory.as_deref()),
            ..Default::default()
        };
        Ok(Self {
            options,
            _value: value,
            _env: env,
            _colors: colors,
            _directory: directory,
        })
    }
}

pub fn query(expression: LocatorExpression) -> Result<String> {
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
    Ok(
        serde_json::json!({"nodes": nodes, "root": integer(expression.root, "locator root", 255)?})
            .to_string(),
    )
}

pub fn open_result(value: &TuiResult) -> Result<OpenResult> {
    Ok(OpenResult {
        shell_pid: value
            .open
            .shell_pid
            .present
            .then_some(value.open.shell_pid.value as u32),
        session: text(value.open.session),
        ready: value.open.ready,
        recording: text(value.open.recording),
    })
}
pub fn cursor(value: TuiCursor) -> Cursor {
    Cursor {
        x: value.x,
        y: value.y,
        visible: value.visible,
        shape: text(value.shape),
        color: text(value.color),
    }
}
pub fn state(value: &TuiState) -> State {
    State {
        session_shell: optional_text(value.session_shell),
        cols: value.cols,
        rows: value.rows,
        cursor: cursor(value.cursor),
        title: optional_text(value.title),
        cwd: optional_text(value.cwd),
        last_command: optional_text(value.last_command),
        last_exit: value.last_exit.present.then_some(value.last_exit.value),
        exit_signal: optional_text(value.exit_signal),
        exited: value.exited.present.then_some(value.exited.value),
        ready: value.ready,
        bell_count: value.bell_count as f64,
        modes: array(value.modes, value.modes_len)
            .iter()
            .map(|value| (text(value.name), value.enabled))
            .collect(),
        mouse_mode: text(value.mouse_mode),
        colors: TerminalColors {
            foreground: text(value.colors.foreground),
            background: text(value.colors.background),
            cursor: text(value.colors.cursor),
            palette: array(value.colors.palette, value.colors.palette_len)
                .iter()
                .map(|value| (text(value.key), text(value.value)))
                .collect(),
        },
        timeouts: EffectiveTimeouts {
            text: value.timeouts.text.value as f64,
            idle: value.timeouts.idle.value as f64,
            command: value.timeouts.command.value as f64,
            exit: value.timeouts.exit.value as f64,
            ready: value.timeouts.ready.value as f64,
        },
        text: text(value.text),
    }
}
pub fn color(value: TuiColor) -> Result<Color> {
    match value.kind {
        0 => Ok(Either::B("default".into())),
        1 => Ok(Either::A(value.index.into())),
        2 => Ok(Either::B(format!(
            "#{:02x}{:02x}{:02x}",
            value.red, value.green, value.blue
        ))),
        _ => Err(internal("terminal returned unknown color kind")),
    }
}
pub fn cell(value: &TuiCell) -> Result<Cell> {
    Ok(Cell {
        x: value.x,
        y: value.y,
        r#char: text(value.character),
        fg: color(value.fg)?,
        bg: color(value.bg)?,
        bold: value.bold,
        dim: value.dim,
        italic: value.italic,
        inverse: value.inverse,
        invisible: value.invisible,
        strike: value.strike,
        blink: value.blink,
        underline: value.underline,
        underline_style: match text(value.underline_style).as_str() {
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
        underline_color: color(value.underline_color)?,
        link: text(value.link),
        link_id: text(value.link_id),
    })
}
pub fn matches(value: &TuiResult) -> Result<Vec<TextMatch>> {
    Ok(array(value.matches, value.matches_len)
        .iter()
        .map(|value| TextMatch {
            text: text(value.text),
            start: TextPosition {
                row: value.start.row,
                column: value.start.column,
            },
            end: TextPosition {
                row: value.end.row,
                column: value.end.column,
            },
            spans: array(value.spans, value.spans_len)
                .iter()
                .map(|value| TextSpan {
                    row: value.row,
                    start: value.start,
                    end: value.end,
                })
                .collect(),
        })
        .collect())
}
