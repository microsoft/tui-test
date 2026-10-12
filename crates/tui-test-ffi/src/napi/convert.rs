use super::{internal, native_error, types::*, usage, MAX_SAFE_INTEGER};
use napi::bindgen_prelude::Either;
use napi::Result;
use tui_test::api::{self, OperationResult};
use tui_test::profile::{Profile, Rgb};

include!("convert_generated.rs");

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
    for (name, color) in value.profile_colors.as_deref().unwrap_or_default() {
        let color = Rgb::parse(color).map_err(usage)?;
        if !profile.colors.set_named(name, color) {
            return Err(usage(format!("unknown profile color {name:?}")));
        }
    }
    open_input(value, profile, directory)
}

fn occurrence(
    value: Option<String>,
    nth: Option<f64>,
    index: usize,
) -> Result<api::MatchOccurrence> {
    let occurrence = if let Some(nth) = nth {
        if let Some(value) = value.as_deref().filter(|value| *value != "nth") {
            return Err(usage(format!(
                "nodes[{index}].nth cannot be used with occurrence '{value}'"
            )));
        }
        serde_json::json!({"nth": integer(nth, &format!("nodes[{index}].nth"), usize::MAX as u64)?})
    } else {
        match value.as_deref().unwrap_or("any") {
            "any" | "unique" | "first" | "last" => value.unwrap_or_else(|| "any".into()).into(),
            "nth" => return Err(usage(format!("nodes[{index}].nth requires an nth index"))),
            value => return Err(usage(format!("invalid occurrence '{value}'"))),
        }
    };
    serde_json::from_value(occurrence)
        .map_err(|error| usage(format!("invalid locator expression: {error}")))
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
    Ok(open_value(value))
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

pub fn matches(value: OperationResult) -> Result<Vec<TextMatch>> {
    let OperationResult::Matches(value) = value else {
        return Err(internal("native operation returned an unexpected result"));
    };
    Ok(value.into_iter().map(text_match).collect())
}
