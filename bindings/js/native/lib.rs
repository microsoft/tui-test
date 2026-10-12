#![deny(clippy::all)]

mod abi;
mod convert;
mod types;

use abi::*;
use convert::*;
use types::*;

use napi::bindgen_prelude::spawn_blocking;
use napi::{Error, Result, Status};
use napi_derive::napi;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

const ERROR_PREFIX: &str = "__tui_test_native_error__:";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

struct Library {
    functions: Functions,
    // Keep the library loaded until all native calls and result guards finish.
    _library: libloading::Library,
}

static LIBRARY: OnceLock<Arc<Library>> = OnceLock::new();
static LIBRARY_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

#[napi]
pub fn configure_library(path: String) -> Result<()> {
    *LIBRARY_PATH
        .lock()
        .map_err(|_| internal("library path lock poisoned"))? = Some(path.into());
    Ok(())
}

fn library() -> Result<Arc<Library>> {
    if let Some(library) = LIBRARY.get() {
        return Ok(library.clone());
    }
    let configured = LIBRARY_PATH
        .lock()
        .map_err(|_| internal("library path lock poisoned"))?;
    if let Some(library) = LIBRARY.get() {
        return Ok(library.clone());
    }
    let path = std::env::var_os("TUI_TEST_NATIVE_LIBRARY")
        .map(PathBuf::from)
        .or_else(|| configured.clone())
        .ok_or_else(|| internal("shared native library path is not configured"))?;
    let loaded = unsafe { libloading::Library::new(&path) }.map_err(|error| {
        internal(format!(
            "failed to load shared native library {}: {error}",
            path.display()
        ))
    })?;
    let version = unsafe { loaded.get::<unsafe extern "C" fn() -> u32>(b"tui_abi_version\0") }
        .map_err(|error| internal(format!("failed to read native ABI version: {error}")))?;
    let actual = unsafe { version() };
    if actual != ABI_VERSION {
        return Err(internal(format!(
            "native ABI version mismatch: expected {ABI_VERSION}, got {actual}"
        )));
    }
    let functions = unsafe { Functions::load(&loaded) }
        .map_err(|error| internal(format!("failed to load native ABI symbols: {error}")))?;
    let loaded = Arc::new(Library {
        functions,
        _library: loaded,
    });
    let _ = LIBRARY.set(loaded.clone());
    Ok(loaded)
}

fn error(kind: &str, message: impl Into<String>) -> Error {
    Error::new(
        Status::GenericFailure,
        format!(
            "{ERROR_PREFIX}{}",
            serde_json::json!({
                "kind": kind, "message": message.into(),
            })
        ),
    )
}

fn internal(message: impl Into<String>) -> Error {
    error("internal", message)
}
fn usage(message: impl Into<String>) -> Error {
    error("usage", message)
}

fn boundary<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            Err(internal(format!("native binding panicked: {message}")))
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    spawn_blocking(move || boundary(work))
        .await
        .map_err(|error| internal(format!("native worker failed: {error}")))?
}

struct ResultGuard<'a> {
    pointer: *mut TuiResult,
    library: &'a Library,
}

impl Drop for ResultGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.library.functions.tui_result_free)(self.pointer) };
    }
}

fn result<T>(
    library: &Library,
    pointer: *mut TuiResult,
    convert: impl FnOnce(&TuiResult) -> Result<T>,
) -> Result<T> {
    if pointer.is_null() {
        return Err(internal("native operation returned a null result"));
    }
    let guard = ResultGuard { pointer, library };
    let value = unsafe { &*guard.pointer };
    if value.error_kind != 0 {
        let kind = match value.error_kind {
            1 => "assertion",
            2 => "usage",
            3 => "no_session",
            _ => "internal",
        };
        let mut envelope = serde_json::json!({"kind": kind, "message": text(value.error_message)});
        for (key, value) in [
            ("details", value.error_details),
            ("artifact", value.error_artifact),
        ] {
            if let Some(value) = optional_text(value) {
                envelope[key] = serde_json::from_str(&value)
                    .map_err(|error| internal(format!("invalid native {key}: {error}")))?;
            }
        }
        return Err(Error::new(
            Status::GenericFailure,
            format!("{ERROR_PREFIX}{envelope}"),
        ));
    }
    convert(value)
}

fn unit(_: &TuiResult) -> Result<()> {
    Ok(())
}
fn string_result(value: &TuiResult) -> Result<String> {
    Ok(text(value.text))
}
fn optional_string_result(value: &TuiResult) -> Result<Option<String>> {
    Ok(optional_text(value.text))
}

#[napi]
pub struct NativeSession {
    name: String,
    context: serde_json::Value,
    recording_directory: Option<String>,
}

impl NativeSession {
    async fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Library, TuiString) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let context = self.context.to_string();
        blocking(move || {
            let library = library()?;
            work(&library, borrowed(&context))
        })
        .await
    }
}

#[napi]
impl NativeSession {
    #[napi(constructor)]
    pub fn new(
        name: String,
        recording: Option<AutomaticRecordingOptions>,
        artifacts: Option<FailureArtifactOptions>,
        trace: Option<TraceOptions>,
    ) -> Result<Self> {
        let mut context = serde_json::json!({"name": name});
        if let Some(options) = artifacts {
            context["artifact_directory"] = options.directory.into();
            context["artifact_mode"] = options.mode.unwrap_or_else(|| "all".into()).into();
            context["artifact_include_recording"] =
                options.include_recording.unwrap_or(false).into();
        }
        if let Some(options) = trace {
            context["trace_mode"] = options.mode.unwrap_or_else(|| "off".into()).into();
            context["trace_directory"] = options.directory.into();
        }
        Ok(Self {
            name,
            context,
            recording_directory: recording.and_then(|options| options.directory),
        })
    }

    #[napi]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[napi]
    pub async fn open(&self, options: Option<OpenOptions>) -> Result<OpenResult> {
        let options = options.unwrap_or_default();
        let context = session_context(&self.context, options.screen_history_limit)?;
        let directory = self.recording_directory.clone();
        blocking(move || {
            let library = library()?;
            let input = OpenInput::new(options, directory)?;
            result(
                &library,
                unsafe { (library.functions.tui_open_ptr)(borrowed(&context), &input.options) },
                open_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn run(&self, options: RunOptions) -> Result<OpenResult> {
        let context = session_context(&self.context, options.screen_history_limit)?;
        let directory = self.recording_directory.clone();
        blocking(move || {
            let library = library()?;
            let args = options.args.unwrap_or_default();
            let borrowed_args: Vec<_> = args.iter().map(|value| borrowed(value)).collect();
            let input = OpenInput::new(
                OpenOptions {
                    backend: options.backend,
                    cols: options.cols,
                    rows: options.rows,
                    cwd: options.cwd,
                    env: options.env,
                    wait_ready: options.wait_ready,
                    restart: options.restart,
                    profile_scrollback: options.profile_scrollback,
                    profile_colors: options.profile_colors,
                    timeouts: options.timeouts,
                    ..Default::default()
                },
                directory,
            )?;
            result(
                &library,
                unsafe {
                    (library.functions.tui_run_ptr)(
                        borrowed(&context),
                        &input.options,
                        borrowed(&options.program),
                        borrowed_args.as_ptr(),
                        borrowed_args.len(),
                    )
                },
                open_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn restart(&self, graceful_timeout_ms: f64) -> Result<OpenResult> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_restart)(
                        session,
                        integer(graceful_timeout_ms, "gracefulTimeoutMs", u64::MAX)?,
                    )
                },
                open_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn close(&self, failed: Option<bool>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_close)(session, optional_bool(failed)) },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn state(&self) -> Result<State> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_state)(session) },
                |value| Ok(state(&value.state)),
            )
        })
        .await
    }

    #[napi]
    pub async fn text(&self, full: Option<bool>) -> Result<String> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_text)(session, full.unwrap_or(false)) },
                string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn packed_screen(&self, full: Option<bool>) -> Result<PackedScreen> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_packed_screen)(session, full.unwrap_or(false)) },
                |value| {
                    Ok(PackedScreen {
                        cols: value.size.cols,
                        rows: value.size.rows,
                        utf8: bytes(value.text).to_vec().into(),
                    })
                },
            )
        })
        .await
    }

    #[napi]
    pub async fn cells(&self, x: f64, y: f64, w: Option<f64>, h: Option<f64>) -> Result<Vec<Cell>> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_cells)(
                        session,
                        u16_value(x, "x")?,
                        u16_value(y, "y")?,
                        u16_value(w.unwrap_or(1.0), "w")?,
                        u16_value(h.unwrap_or(1.0), "h")?,
                    )
                },
                |value| {
                    array(value.cells, value.cells_len)
                        .iter()
                        .map(cell)
                        .collect()
                },
            )
        })
        .await
    }

    #[napi]
    pub async fn find_locator(
        &self,
        expression: LocatorExpression,
        require_one: Option<bool>,
    ) -> Result<Vec<TextMatch>> {
        self.call(move |library, session| {
            let query = query(expression)?;
            result(
                library,
                unsafe {
                    (library.functions.tui_find_locator)(
                        session,
                        TuiQuery {
                            json: borrowed(&query),
                        },
                        require_one.unwrap_or(false),
                    )
                },
                matches,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_locator(
        &self,
        expression: LocatorExpression,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        self.call(move |library, session| {
            let query = query(expression)?;
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_locator)(
                        session,
                        TuiQuery {
                            json: borrowed(&query),
                        },
                        wait_options(timeout_ms, false, not.unwrap_or(false))?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn expect_locator(
        &self,
        expression: LocatorExpression,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        self.call(move |library, session| {
            let query = query(expression)?;
            result(
                library,
                unsafe {
                    (library.functions.tui_expect_locator)(
                        session,
                        TuiQuery {
                            json: borrowed(&query),
                        },
                        wait_options(timeout_ms, false, not.unwrap_or(false))?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn click_locator(
        &self,
        expression: LocatorExpression,
        button: Option<f64>,
        clicks: Option<f64>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        self.call(move |library, session| {
            let query = query(expression)?;
            result(
                library,
                unsafe {
                    (library.functions.tui_click_locator)(
                        session,
                        TuiQuery {
                            json: borrowed(&query),
                        },
                        mouse_options(button.unwrap_or(0.0))?,
                        u8_value(clicks.unwrap_or(1.0), "clicks")?,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn highlight_locator(
        &self,
        expression: LocatorExpression,
        timeout_ms: Option<f64>,
    ) -> Result<Vec<TextMatch>> {
        self.call(move |library, session| {
            let query = query(expression)?;
            result(
                library,
                unsafe {
                    (library.functions.tui_highlight_locator)(
                        session,
                        TuiQuery {
                            json: borrowed(&query),
                        },
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                matches,
            )
        })
        .await
    }

    #[napi]
    pub async fn panic_probe(&self) -> Result<()> {
        blocking(|| panic!("intentional native panic probe")).await
    }

    #[napi]
    pub async fn get_command(&self) -> Result<Option<String>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_command)(session) },
                optional_string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn get_output(&self) -> Result<Option<String>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_output)(session) },
                optional_string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn get_exit_code(&self) -> Result<Option<i32>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_exit_code)(session) },
                |value| Ok(value.exit_code.present.then_some(value.exit_code.value)),
            )
        })
        .await
    }

    #[napi]
    pub async fn get_cwd(&self) -> Result<Option<String>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_cwd)(session) },
                optional_string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn get_title(&self) -> Result<Option<String>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_title)(session) },
                optional_string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn get_clipboard(&self) -> Result<String> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_clipboard)(session) },
                string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn get_cursor(&self) -> Result<Cursor> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_cursor)(session) },
                |value| Ok(cursor(value.cursor)),
            )
        })
        .await
    }

    #[napi]
    pub async fn get_size(&self) -> Result<Size> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_size)(session) },
                |value| {
                    Ok(Size {
                        cols: value.size.cols,
                        rows: value.size.rows,
                    })
                },
            )
        })
        .await
    }

    #[napi]
    pub async fn get_bell_count(&self) -> Result<f64> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_bell_count)(session) },
                |value| Ok(value.number as f64),
            )
        })
        .await
    }

    #[napi]
    pub async fn get_bell_events(&self) -> Result<Vec<BellEvent>> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_get_bell_events)(session) },
                |value| {
                    Ok(array(value.bells, value.bells_len)
                        .iter()
                        .map(|value| BellEvent {
                            sequence: value.sequence as f64,
                            elapsed_ms: value.elapsed_ms as f64,
                        })
                        .collect())
                },
            )
        })
        .await
    }

    #[napi]
    pub async fn write(&self, data: String) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_write)(session, borrowed(&data)) },
                unit,
            )
        })
        .await
    }

    #[napi(js_name = "type")]
    pub async fn type_text(&self, text: String) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_type)(session, borrowed(&text)) },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn submit(&self, data: Option<String>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_submit)(session, optional_borrowed(data.as_deref()))
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn press(&self, keys: Vec<String>) -> Result<()> {
        self.keys(keys, 0).await
    }

    #[napi]
    pub async fn key_down(&self, keys: Vec<String>) -> Result<()> {
        self.keys(keys, 1).await
    }

    #[napi]
    pub async fn repeat(&self, keys: Vec<String>) -> Result<()> {
        self.keys(keys, 2).await
    }

    #[napi]
    pub async fn key_up(&self, keys: Vec<String>) -> Result<()> {
        self.keys(keys, 3).await
    }

    #[napi]
    pub async fn mouse_click(&self, options: Option<MouseClickOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_click)(
                        session,
                        optional_integer(options.x, "x", u16::MAX.into())?,
                        optional_integer(options.y, "y", u16::MAX.into())?,
                        optional_borrowed(options.on_text.as_deref()),
                        mouse_options(options.button.unwrap_or(0.0))?,
                        u8_value(options.clicks.unwrap_or(1.0), "clicks")?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn mouse_move(&self, x: f64, y: f64) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_move)(
                        session,
                        u16_value(x, "x")?,
                        u16_value(y, "y")?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn mouse_down(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_down)(
                        session,
                        u16_value(x, "x")?,
                        u16_value(y, "y")?,
                        mouse_options(button.unwrap_or(0.0))?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn mouse_up(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_up)(
                        session,
                        u16_value(x, "x")?,
                        u16_value(y, "y")?,
                        mouse_options(button.unwrap_or(0.0))?,
                    )
                },
                unit,
            )
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
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_drag)(
                        session,
                        u16_value(x1, "x1")?,
                        u16_value(y1, "y1")?,
                        u16_value(x2, "x2")?,
                        u16_value(y2, "y2")?,
                        mouse_options(button.unwrap_or(0.0))?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn mouse_scroll(&self, direction: String, amount: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_mouse_scroll)(
                        session,
                        borrowed(&direction),
                        u16_value(amount.unwrap_or(3.0), "amount")?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn resize(&self, cols: f64, rows: f64) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_resize)(
                        session,
                        u16_value(cols, "cols")?,
                        u16_value(rows, "rows")?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn signal(&self, name: String) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe { (library.functions.tui_signal)(session, borrowed(&name)) },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_title)(
                        session,
                        borrowed(&text),
                        wait_options(
                            options.timeout_ms,
                            options.regex.unwrap_or(false),
                            options.not.unwrap_or(false),
                        )?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn expect_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_expect_title)(
                        session,
                        borrowed(&text),
                        wait_options(
                            options.timeout_ms,
                            options.regex.unwrap_or(false),
                            options.not.unwrap_or(false),
                        )?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_clipboard(
        &self,
        text: Option<String>,
        options: Option<ClipboardWaitOptions>,
    ) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_clipboard)(
                        session,
                        optional_borrowed(text.as_deref()),
                        wait_options(options.timeout_ms, options.regex.unwrap_or(false), false)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_idle(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_idle)(
                        session,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_command(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_command)(
                        session,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_exit(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_exit)(
                        session,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_ready(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_ready)(
                        session,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn wait_bell(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_wait_bell)(
                        session,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn expect_exit_code(&self, code: f64, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_expect_exit_code)(
                        session,
                        i32_value(code, "code")?,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn expect_output(&self, text: String, regex: Option<bool>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_expect_output)(
                        session,
                        borrowed(&text),
                        regex.unwrap_or(false),
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn expect_bell_count(&self, count: f64, timeout_ms: Option<f64>) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_expect_bell_count)(
                        session,
                        integer(count, "count", u64::MAX)?,
                        optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn snapshot(
        &self,
        name: String,
        options: Option<SnapshotOptions>,
    ) -> Result<SnapshotResult> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_snapshot)(
                        session,
                        borrowed(&name),
                        options.update.unwrap_or(false),
                        options.include_style.unwrap_or(false),
                        options.include_title.unwrap_or(false),
                        optional_borrowed(options.cwd.as_deref()),
                    )
                },
                |value| match value.snapshot {
                    0 => Ok(SnapshotResult::Passed),
                    1 => Ok(SnapshotResult::Written),
                    2 => Ok(SnapshotResult::Updated),
                    _ => Err(internal(
                        "native operation returned an unknown snapshot result",
                    )),
                },
            )
        })
        .await
    }

    #[napi]
    pub async fn screenshot(&self, options: Option<ScreenshotOptions>) -> Result<String> {
        let options = options.unwrap_or_default();
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_screenshot)(
                        session,
                        options.full.unwrap_or(false),
                        optional_borrowed(options.path.as_deref()),
                        optional_float(options.zoom),
                        optional_borrowed(options.background.as_deref()),
                        options.transparent.unwrap_or(false),
                    )
                },
                string_result,
            )
        })
        .await
    }

    #[napi]
    pub async fn start_recording(&self, options: RecordingOptions) -> Result<()> {
        self.call(move |library, session| {
            result(
                library,
                unsafe {
                    (library.functions.tui_start_recording)(
                        session,
                        TuiRecordingOptions {
                            path: borrowed(&options.path),
                            format: optional_borrowed(options.format.as_ref().map(|format| {
                                match format {
                                    RecordingFormat::Apng => "apng",
                                    RecordingFormat::Gif => "gif",
                                    RecordingFormat::Mp4 => "mp4",
                                    RecordingFormat::Cast => "cast",
                                }
                            })),
                            fps: optional_integer(options.fps, "fps", u8::MAX.into())?,
                            speed: optional_float(options.speed),
                            idle_time_limit: optional_float(options.idle_time_limit),
                            zoom: optional_float(options.zoom),
                            background: optional_borrowed(options.background.as_deref()),
                            transparent: options.transparent.unwrap_or(false),
                        },
                    )
                },
                unit,
            )
        })
        .await
    }

    #[napi]
    pub async fn stop_recording(&self) -> Result<String> {
        self.call(|library, session| {
            result(
                library,
                unsafe { (library.functions.tui_stop_recording)(session) },
                string_result,
            )
        })
        .await
    }
}

impl NativeSession {
    async fn keys(&self, keys: Vec<String>, action: u32) -> Result<()> {
        self.call(move |library, session| {
            let borrowed_keys: Vec<_> = keys.iter().map(|value| borrowed(value)).collect();
            result(
                library,
                unsafe {
                    (library.functions.tui_key)(
                        session,
                        borrowed_keys.as_ptr(),
                        borrowed_keys.len(),
                        action,
                    )
                },
                unit,
            )
        })
        .await
    }
}

#[napi]
pub async fn sessions() -> Result<Vec<String>> {
    blocking(|| {
        let library = library()?;
        result(
            &library,
            unsafe { (library.functions.tui_sessions)() },
            |value| {
                Ok(array(value.strings, value.strings_len)
                    .iter()
                    .map(|value| text(*value))
                    .collect())
            },
        )
    })
    .await
}

#[napi]
pub async fn close_all() -> Result<()> {
    blocking(close_all_sync).await
}

#[napi]
pub fn close_all_sync() -> Result<()> {
    boundary(|| {
        let library = library()?;
        result(
            &library,
            unsafe { (library.functions.tui_close_all)() },
            unit,
        )
    })
}

#[napi]
pub async fn recording(name: String) -> Result<String> {
    blocking(move || {
        let library = library()?;
        result(
            &library,
            unsafe { (library.functions.tui_recording)(borrowed(&name)) },
            string_result,
        )
    })
    .await
}
