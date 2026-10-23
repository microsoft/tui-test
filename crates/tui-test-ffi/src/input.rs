use crate::session::SessionInput;
use crate::types::*;
use tui_test::api::*;
use tui_test::locator_query::LocatorExpression;
use tui_test::profile::{Profile, Rgb};
use tui_test::shell::Shell;
use tui_test::ExecutionContext;

type Result<T> = std::result::Result<T, TuiTestError>;

include!("input_generated.rs");

unsafe fn style_padding(
    value: TuiPadding,
    default: tui_test::render::style::Padding,
) -> Result<tui_test::render::style::Padding> {
    use tui_test::render::style::{Padding, PaddingSides};
    let sides = unsafe { style_padding_sides(value.sides)? };
    if let Some(uniform) = value.uniform.option() {
        if sides != PaddingSides::default() {
            return Err(TuiTestError::usage(
                "padding cannot combine uniform and individual sides",
            ));
        }
        return Ok(Padding::Uniform(
            u32::try_from(uniform).map_err(|_| TuiTestError::usage("padding exceeds u32"))?,
        ));
    }
    Ok(if sides == PaddingSides::default() {
        default
    } else {
        Padding::Sides(sides)
    })
}

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
pub(crate) unsafe fn session(value: TuiString) -> Result<(String, ExecutionContext)> {
    let json = unsafe { value.required()? };
    let value: SessionInput = serde_json::from_str(&json)
        .map_err(|error| TuiTestError::usage(format!("invalid native session context: {error}")))?;
    value.validate()
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
        .map(|s| {
            serde_json::from_value(s.clone().into())
                .map_err(|_| TuiTestError::usage(format!("unknown shell {s:?}")))
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
    let mode = unsafe { value.recording_mode.optional()? }.unwrap_or_else(|| "disabled".into());
    let mode = serde_json::from_value(mode.clone().into())
        .map_err(|_| TuiTestError::usage(format!("unknown automatic recording mode {mode:?}")))?;
    unsafe { open_input(value, backend, shell, profile, mode) }
}
pub(crate) fn mouse(value: TuiMouseOptions) -> Result<MouseOptions> {
    let button = match value.button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => return Err(TuiTestError::usage("unknown mouse button")),
    };
    Ok(mouse_input(value, button))
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
    crate::session::capture_background(unsafe { background.optional()? }, transparent)
}
pub(crate) unsafe fn recording(value: TuiRecordingOptions) -> Result<Operation> {
    let format = unsafe { value.format.optional()? }
        .map(|s| {
            serde_json::from_value(s.into())
                .map_err(|_| TuiTestError::usage("unknown recording format"))
        })
        .transpose()?;
    let fps = value
        .fps
        .option()
        .map(|v| u8::try_from(v).map_err(|_| TuiTestError::usage("fps must be between 0 and 255")))
        .transpose()?;
    unsafe { recording_input(value, format, fps) }
}

pub(crate) unsafe fn expect_colors(value: TuiColorExpectation) -> Result<Operation> {
    let palette = unsafe { pairs(value.palette, value.palette_len)? }
        .into_iter()
        .map(|(index, color)| {
            Ok((
                index
                    .parse::<u8>()
                    .map_err(|_| TuiTestError::usage("palette index must be between 0 and 255"))?,
                color,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Operation::ExpectColors {
        foreground: unsafe { value.foreground.optional()? },
        background: unsafe { value.background.optional()? },
        cursor: unsafe { value.cursor.optional()? },
        palette,
        timeout_ms: value.timeout_ms.option(),
    })
}
