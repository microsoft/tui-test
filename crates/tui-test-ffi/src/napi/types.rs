use ::napi::bindgen_prelude::{
    Buffer, Either, FromNapiValue, JsObjectValue, Object, ValidateNapiValue,
};
use napi::{Error, Result, Status};
use napi_derive::napi;
use std::collections::HashMap;

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum Backend {
    Alacritty,
    Ghostty,
    Rio,
    Xtermjs,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum RecordingFormat {
    Apng,
    Gif,
    Mp4,
    Cast,
}

#[derive(Default)]
#[napi(object)]
pub struct Timeouts {
    pub text: Option<f64>,
    pub idle: Option<f64>,
    pub command: Option<f64>,
    pub exit: Option<f64>,
    pub ready: Option<f64>,
}

#[derive(Default)]
#[napi(object)]
pub struct AutomaticRecordingOptions {
    pub directory: Option<String>,
}

#[derive(Default)]
#[napi(object)]
pub struct TraceOptions {
    pub mode: Option<String>,
    pub directory: Option<String>,
}

#[derive(Default)]
#[napi(object)]
pub struct FailureArtifactOptions {
    pub directory: String,
    pub mode: Option<String>,
    pub include_recording: Option<bool>,
}

#[derive(Default)]
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
    pub screen_history_limit: Option<f64>,
}

#[derive(Default)]
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
    pub screen_history_limit: Option<f64>,
}

#[napi(object, use_nullable = true)]
pub struct OpenResult {
    #[napi(js_name = "shell_pid")]
    pub shell_pid: Option<u32>,
    pub session: String,
    pub ready: bool,
    pub recording: String,
}

#[derive(Default)]
#[napi(object)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub shape: String,
    pub color: String,
}

#[derive(Default)]
#[napi(object)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Default)]
#[napi(object)]
pub struct EffectiveTimeouts {
    pub text: f64,
    pub idle: f64,
    pub command: f64,
    pub exit: f64,
    pub ready: f64,
}

#[derive(Default)]
#[napi(object)]
pub struct BellEvent {
    pub sequence: f64,
    #[napi(js_name = "elapsed_ms")]
    pub elapsed_ms: f64,
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
    #[napi(js_name = "exit_signal")]
    pub exit_signal: Option<String>,
    pub exited: Option<i32>,
    pub ready: bool,
    #[napi(js_name = "bell_count")]
    pub bell_count: f64,
    pub modes: HashMap<String, bool>,
    #[napi(js_name = "mouse_mode")]
    pub mouse_mode: String,
    pub colors: TerminalColors,
    pub timeouts: EffectiveTimeouts,
    pub text: String,
}

/// The colors the terminal is painting with.
#[derive(Default)]
#[napi(object)]
pub struct TerminalColors {
    /// The default foreground (`OSC 10`).
    pub foreground: String,
    /// The default background (`OSC 11`).
    pub background: String,
    /// The cursor color (`OSC 12`).
    pub cursor: String,
    /// Palette entries a program overrode (`OSC 4`), keyed by index.
    ///
    /// Only entries that differ from the profile are listed.
    pub palette: HashMap<String, String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum UnderlineStyle {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[napi]
pub type Color = Either<u32, String>;

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
    /// The OSC 8 URI this cell links to, empty when it links nowhere.
    pub link: String,
    /// The link's `id=` parameter, empty when the sequence carried none.
    ///
    /// Identifies a link across a wrap rather than describing where it points:
    /// a program that wraps its own links tags each run with a shared `id=`.
    ///
    /// Backend-dependent. Ghostty reports a link's URI and nothing else, so
    /// this is always empty there and no locator matches on it.
    #[napi(js_name = "link_id")]
    pub link_id: String,
}

#[derive(Default)]
#[napi(object)]
pub struct TextPosition {
    pub row: u32,
    pub column: u16,
}

#[derive(Default)]
#[napi(object)]
pub struct TextSpan {
    pub row: u32,
    pub start: u16,
    pub end: u16,
}

#[derive(Default)]
#[napi(object)]
pub struct TextMatch {
    pub text: String,
    pub start: TextPosition,
    pub end: TextPosition,
    pub spans: Vec<TextSpan>,
}

#[derive(Default)]
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

#[derive(Default)]
#[napi(object)]
pub struct MouseClickOptions {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub on_text: Option<String>,
    pub button: Option<f64>,
    pub clicks: Option<f64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum LocatorNodeKind {
    Text,
    Style,
    Link,
    And,
    Or,
    Filter,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum LocatorStageDirection {
    Within,
    After,
    Before,
}

#[derive(Default, serde::Serialize)]
#[napi(object, object_from_js = false)]
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

#[napi(object, object_from_js = false)]
pub struct LocatorNode {
    pub kind: LocatorNodeKind,
    pub direction: Option<LocatorStageDirection>,
    pub text: Option<String>,
    pub regex: Option<bool>,
    pub full: Option<bool>,
    pub whitespace: Option<String>,
    pub occurrence: Option<String>,
    pub nth: Option<f64>,
    pub style: Option<LocatorStyle>,
    pub link: Option<String>,
    pub within: Option<f64>,
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub input: Option<f64>,
    pub has: Option<f64>,
    pub has_not: Option<f64>,
}

#[napi(object, object_from_js = false)]
pub struct LocatorExpression {
    pub nodes: Vec<LocatorNode>,
    pub root: f64,
}

// Reject unknown fields before conversion so misspelled predicates cannot
// silently broaden a locator.
macro_rules! strict_locator_input {
    ($name:ident { $($field:ident => $key:literal),* $(,)? }) => {
        impl ValidateNapiValue for $name {}
        impl FromNapiValue for $name {
            unsafe fn from_napi_value(env: napi::sys::napi_env, value: napi::sys::napi_value) -> Result<Self> {
                unsafe { Object::validate(env, value)?; }
                let object = unsafe { Object::from_napi_value(env, value)? };
                for key in Object::keys(&object)? {
                    if ![$($key),*].contains(&key.as_str()) {
                        return Err(Error::new(Status::InvalidArg, format!("unknown locator field {key:?}")));
                    }
                }
                Ok(Self { $($field: object.get_named_property($key)?),* })
            }
        }
    };
}

strict_locator_input!(LocatorStyle {
    foreground => "foreground", background => "background", bold => "bold",
    dim => "dim", italic => "italic", underline_style => "underlineStyle",
    underline_color => "underlineColor", inverse => "inverse", hidden => "hidden",
    strikethrough => "strikethrough", blink => "blink",
});
strict_locator_input!(LocatorNode {
    kind => "kind", direction => "direction", text => "text", regex => "regex",
    full => "full", whitespace => "whitespace", occurrence => "occurrence",
    nth => "nth", style => "style", link => "link", within => "within",
    left => "left", right => "right", input => "input", has => "has", has_not => "hasNot",
});
strict_locator_input!(LocatorExpression { nodes => "nodes", root => "root" });

#[derive(Default)]
#[napi(object)]
pub struct TitleOptions {
    pub regex: Option<bool>,
    pub not: Option<bool>,
    pub timeout_ms: Option<f64>,
}

#[derive(Default)]
#[napi(object)]
pub struct ClipboardWaitOptions {
    pub regex: Option<bool>,
    pub timeout_ms: Option<f64>,
}

#[derive(Default)]
#[napi(object)]
pub struct SnapshotOptions {
    pub update: Option<bool>,
    pub include_style: Option<bool>,
    pub include_title: Option<bool>,
    pub cwd: Option<String>,
}

#[derive(Default)]
#[napi(object)]
pub struct ScreenshotOptions {
    pub full: Option<bool>,
    pub path: Option<String>,
    pub zoom: Option<f64>,
    pub background: Option<String>,
    pub transparent: Option<bool>,
}

#[derive(Default)]
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

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[napi(string_enum = "lowercase")]
pub enum SnapshotResult {
    Passed,
    Written,
    Updated,
}
