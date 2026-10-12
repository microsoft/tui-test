#![deny(clippy::all)]

mod convert;
mod types;

use self::types::*;
use crate::session::{self, SessionInput};
use convert::*;
use tui_test::api::{self, KeyAction, MouseAction, Operation, OperationResult};
use tui_test::runtime::global_registry;

use ::napi::bindgen_prelude::spawn_blocking;
use ::napi::{Error, Result, Status};
use napi_derive::napi;

const ERROR_PREFIX: &str = "__tui_test_native_error__:";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

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

fn native_error(error: api::TuiTestError) -> Error {
    let mut envelope = serde_json::json!({
        "kind": error.kind.as_str(), "message": error.message,
    });
    if let Some(details) = error.details {
        envelope["details"] = match serde_json::to_value(details) {
            Ok(value) => value,
            Err(error) => return internal(format!("failed to serialize native details: {error}")),
        };
    }
    if let Some(artifact) = error.artifact {
        envelope["artifact"] = match serde_json::to_value(artifact) {
            Ok(value) => value,
            Err(error) => return internal(format!("failed to serialize native artifact: {error}")),
        };
    }
    Error::new(Status::GenericFailure, format!("{ERROR_PREFIX}{envelope}"))
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

async fn execute<T: Send + 'static>(
    context: SessionInput,
    operation_name: &'static str,
    operation: impl FnOnce() -> Result<Operation> + Send + 'static,
    convert: impl FnOnce(OperationResult) -> Result<T> + Send + 'static,
) -> Result<T> {
    blocking(move || {
        let (name, context) = context.validate().map_err(native_error)?;
        convert(
            session::execute(name, context, operation_name, operation()?).map_err(native_error)?,
        )
    })
    .await
}

#[napi]
pub struct NativeSession {
    name: String,
    context: SessionInput,
    recording_directory: Option<String>,
}

impl NativeSession {
    async fn call<T: Send + 'static>(
        &self,
        operation_name: &'static str,
        operation: impl FnOnce() -> Result<Operation> + Send + 'static,
        convert: impl FnOnce(OperationResult) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        execute(self.context.clone(), operation_name, operation, convert).await
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
        let mut context = SessionInput {
            name: name.clone(),
            ..Default::default()
        };
        if let Some(options) = artifacts {
            apply_artifact(&mut context, options);
        }
        if let Some(options) = trace {
            apply_trace(&mut context, options);
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
        let mut context = self.context.clone();
        open_options_context(&mut context, &options)?;
        let directory = self.recording_directory.clone();
        execute(
            context,
            "open",
            move || Ok(Operation::Open(open_options(options, directory)?)),
            open_result,
        )
        .await
    }

    #[napi]
    pub async fn run(&self, options: RunOptions) -> Result<OpenResult> {
        let mut context = self.context.clone();
        run_options_context(&mut context, &options)?;
        let directory = self.recording_directory.clone();
        execute(
            context,
            "run",
            move || Ok(Operation::Run(run_options(options, directory)?)),
            open_result,
        )
        .await
    }

    #[napi]
    pub async fn restart(&self, graceful_timeout_ms: f64) -> Result<OpenResult> {
        self.call(
            "restart",
            move || {
                Ok(Operation::Restart {
                    graceful_timeout_ms: integer(
                        graceful_timeout_ms,
                        "gracefulTimeoutMs",
                        u64::MAX,
                    )?,
                })
            },
            open_result,
        )
        .await
    }

    #[napi]
    pub async fn close(&self, failed: Option<bool>) -> Result<()> {
        let context = self.context.clone();
        blocking(move || {
            let (name, context) = context.validate().map_err(native_error)?;
            unit(session::close(name, context, failed).map_err(native_error)?)
        })
        .await
    }

    #[napi]
    pub async fn state(&self) -> Result<State> {
        self.call(
            "state",
            || Ok(Operation::State),
            |value| match value {
                OperationResult::State(value) => Ok(state(*value)),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }

    #[napi]
    pub async fn text(&self, full: Option<bool>) -> Result<String> {
        self.call(
            "text",
            move || {
                Ok(Operation::Text {
                    full: full.unwrap_or(false),
                })
            },
            string_result,
        )
        .await
    }

    #[napi]
    pub async fn packed_screen(&self, full: Option<bool>) -> Result<PackedScreen> {
        self.call(
            "packedScreen",
            move || {
                Ok(Operation::PackedScreen {
                    full: full.unwrap_or(false),
                })
            },
            |value| match value {
                OperationResult::PackedScreen(value) => Ok(packed_screen(value)),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }

    #[napi]
    pub async fn cells(&self, x: f64, y: f64, w: Option<f64>, h: Option<f64>) -> Result<Vec<Cell>> {
        self.call(
            "cells",
            move || {
                Ok(Operation::Cells {
                    x: u16_value(x, "x")?,
                    y: u16_value(y, "y")?,
                    w: u16_value(w.unwrap_or(1.0), "w")?,
                    h: u16_value(h.unwrap_or(1.0), "h")?,
                })
            },
            |value| match value {
                OperationResult::Cells(value) => value.into_iter().map(cell).collect(),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }

    #[napi]
    pub async fn find_locator(
        &self,
        expression: LocatorExpression,
        require_one: Option<bool>,
    ) -> Result<Vec<TextMatch>> {
        let require_one = require_one.unwrap_or(false);
        self.call(
            if require_one {
                "locator.location"
            } else {
                "locator.locations"
            },
            move || {
                let query = query(expression)?;
                Ok(if require_one {
                    Operation::ResolveLocator { query }
                } else {
                    Operation::FindLocator { query }
                })
            },
            matches,
        )
        .await
    }

    #[napi]
    pub async fn wait_locator(
        &self,
        expression: LocatorExpression,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        self.call(
            "locator.wait",
            move || {
                Ok(Operation::WaitLocator {
                    query: query(expression)?,
                    not: not.unwrap_or(false),
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }

    #[napi]
    pub async fn expect_locator(
        &self,
        expression: LocatorExpression,
        not: Option<bool>,
        timeout_ms: Option<f64>,
    ) -> Result<()> {
        self.call(
            "locator.expect",
            move || {
                Ok(Operation::WaitLocator {
                    query: query(expression)?,
                    not: not.unwrap_or(false),
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
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
        self.call(
            "locator.click",
            move || {
                Ok(Operation::ClickLocator {
                    query: query(expression)?,
                    options: mouse_options(button.unwrap_or(0.0))?,
                    clicks: u8_value(clicks.unwrap_or(1.0), "clicks")?,
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }

    #[napi]
    pub async fn highlight_locator(
        &self,
        expression: LocatorExpression,
        timeout_ms: Option<f64>,
    ) -> Result<Vec<TextMatch>> {
        self.call(
            "locator.highlight",
            move || {
                Ok(Operation::HighlightLocator {
                    query: query(expression)?,
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            matches,
        )
        .await
    }

    #[napi]
    pub async fn panic_probe(&self) -> Result<()> {
        blocking(|| panic!("intentional native panic probe")).await
    }

    #[napi]
    pub async fn get_command(&self) -> Result<Option<String>> {
        self.call(
            "getCommand",
            || Ok(Operation::GetCommand),
            optional_string_result,
        )
        .await
    }
    #[napi]
    pub async fn get_output(&self) -> Result<Option<String>> {
        self.call(
            "getOutput",
            || Ok(Operation::GetOutput),
            optional_string_result,
        )
        .await
    }
    #[napi]
    pub async fn get_exit_code(&self) -> Result<Option<i32>> {
        self.call(
            "getExitCode",
            || Ok(Operation::GetExitCode),
            |value| match value {
                OperationResult::ExitCode(value) => Ok(value),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }
    #[napi]
    pub async fn get_cwd(&self) -> Result<Option<String>> {
        self.call("getCwd", || Ok(Operation::GetCwd), optional_string_result)
            .await
    }
    #[napi]
    pub async fn get_title(&self) -> Result<Option<String>> {
        self.call(
            "getTitle",
            || Ok(Operation::GetTitle),
            optional_string_result,
        )
        .await
    }
    #[napi]
    pub async fn get_clipboard(&self) -> Result<String> {
        self.call(
            "getClipboard",
            || Ok(Operation::GetClipboard),
            string_result,
        )
        .await
    }
    #[napi]
    pub async fn get_cursor(&self) -> Result<Cursor> {
        self.call(
            "getCursor",
            || Ok(Operation::GetCursor),
            |value| match value {
                OperationResult::Cursor(value) => Ok(cursor(value)),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }
    #[napi]
    pub async fn get_size(&self) -> Result<Size> {
        self.call(
            "getSize",
            || Ok(Operation::GetSize),
            |value| match value {
                OperationResult::Size(value) => Ok(size(value)),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }
    #[napi]
    pub async fn get_bell_count(&self) -> Result<f64> {
        self.call(
            "getBellCount",
            || Ok(Operation::GetBellCount),
            |value| match value {
                OperationResult::BellCount(value) => Ok(value as f64),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }
    #[napi]
    pub async fn get_bell_events(&self) -> Result<Vec<BellEvent>> {
        self.call(
            "getBellEvents",
            || Ok(Operation::GetBellEvents),
            |value| match value {
                OperationResult::BellEvents(value) => {
                    Ok(value.into_iter().map(bell_event).collect())
                }
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }

    #[napi]
    pub async fn write(&self, data: String) -> Result<()> {
        self.call("write", move || Ok(Operation::Write { data }), unit)
            .await
    }
    #[napi(js_name = "type")]
    pub async fn type_text(&self, text: String) -> Result<()> {
        self.call("type", move || Ok(Operation::Write { data: text }), unit)
            .await
    }
    #[napi]
    pub async fn submit(&self, data: Option<String>) -> Result<()> {
        self.call("submit", move || Ok(Operation::Submit { data }), unit)
            .await
    }
    #[napi]
    pub async fn press(&self, keys: Vec<String>) -> Result<()> {
        self.call(
            "press",
            move || {
                Ok(Operation::Key {
                    keys,
                    action: KeyAction::Press,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn key_down(&self, keys: Vec<String>) -> Result<()> {
        self.call(
            "keyDown",
            move || {
                Ok(Operation::Key {
                    keys,
                    action: KeyAction::Down,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn repeat(&self, keys: Vec<String>) -> Result<()> {
        self.call(
            "repeat",
            move || {
                Ok(Operation::Key {
                    keys,
                    action: KeyAction::Repeat,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn key_up(&self, keys: Vec<String>) -> Result<()> {
        self.call(
            "keyUp",
            move || {
                Ok(Operation::Key {
                    keys,
                    action: KeyAction::Up,
                })
            },
            unit,
        )
        .await
    }

    #[napi]
    pub async fn mouse_click(&self, options: Option<MouseClickOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(
            "mouse.click",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Click {
                        x: optional_u16(options.x, "x")?,
                        y: optional_u16(options.y, "y")?,
                        on_text: options.on_text,
                        options: mouse_options(options.button.unwrap_or(0.0))?,
                        clicks: u8_value(options.clicks.unwrap_or(1.0), "clicks")?,
                    },
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn mouse_move(&self, x: f64, y: f64) -> Result<()> {
        self.call(
            "mouse.move",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Move {
                        x: u16_value(x, "x")?,
                        y: u16_value(y, "y")?,
                    },
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn mouse_down(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        self.call(
            "mouse.down",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Down {
                        x: u16_value(x, "x")?,
                        y: u16_value(y, "y")?,
                        options: mouse_options(button.unwrap_or(0.0))?,
                    },
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn mouse_up(&self, x: f64, y: f64, button: Option<f64>) -> Result<()> {
        self.call(
            "mouse.up",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Up {
                        x: u16_value(x, "x")?,
                        y: u16_value(y, "y")?,
                        options: mouse_options(button.unwrap_or(0.0))?,
                    },
                })
            },
            unit,
        )
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
        self.call(
            "mouse.drag",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Drag {
                        x1: u16_value(x1, "x1")?,
                        y1: u16_value(y1, "y1")?,
                        x2: u16_value(x2, "x2")?,
                        y2: u16_value(y2, "y2")?,
                        options: mouse_options(button.unwrap_or(0.0))?,
                    },
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn mouse_scroll(&self, direction: String, amount: Option<f64>) -> Result<()> {
        self.call(
            "mouse.scroll",
            move || {
                Ok(Operation::Mouse {
                    action: MouseAction::Scroll {
                        direction,
                        amount: u16_value(amount.unwrap_or(3.0), "amount")?,
                    },
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn resize(&self, cols: f64, rows: f64) -> Result<()> {
        self.call(
            "resize",
            move || {
                Ok(Operation::Resize {
                    cols: u16_value(cols, "cols")?,
                    rows: u16_value(rows, "rows")?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn signal(&self, name: String) -> Result<()> {
        self.call("signal", move || Ok(Operation::Signal { name }), unit)
            .await
    }

    #[napi]
    pub async fn wait_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(
            "waitTitle",
            move || wait_title_operation(options, text),
            unit,
        )
        .await
    }
    #[napi]
    pub async fn expect_title(&self, text: String, options: Option<TitleOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(
            "expectTitle",
            move || expect_title_operation(options, text),
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_clipboard(
        &self,
        text: Option<String>,
        options: Option<ClipboardWaitOptions>,
    ) -> Result<()> {
        let options = options.unwrap_or_default();
        self.call(
            "waitClipboard",
            move || {
                let timeout_ms = optional_integer(options.timeout_ms, "timeoutMs", u64::MAX)?;
                match text {
                    Some(text) => Ok(Operation::WaitClipboardMatch {
                        pattern: if options.regex.unwrap_or(false) {
                            api::ClipboardPattern::regex(&text)
                                .map_err(|error| usage(format!("invalid regex: {error}")))?
                        } else {
                            text.into()
                        },
                        timeout_ms,
                    }),
                    None if options.regex.unwrap_or(false) => {
                        Err(usage("clipboard regex requires text"))
                    }
                    None => Ok(Operation::WaitClipboard { timeout_ms }),
                }
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_idle(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "waitIdle",
            move || {
                Ok(Operation::WaitIdle {
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_command(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "waitCommand",
            move || {
                Ok(Operation::WaitCommand {
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_exit(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "waitExit",
            move || {
                Ok(Operation::WaitExit {
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_ready(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "waitReady",
            move || {
                Ok(Operation::WaitReady {
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn wait_bell(&self, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "waitBell",
            move || {
                Ok(Operation::WaitBell {
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn expect_exit_code(&self, code: f64, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "expectExitCode",
            move || {
                Ok(Operation::ExpectExitCode {
                    code: i32_value(code, "code")?,
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn expect_output(&self, text: String, regex: Option<bool>) -> Result<()> {
        self.call(
            "expectOutput",
            move || {
                Ok(Operation::ExpectOutput {
                    text,
                    regex: regex.unwrap_or(false),
                })
            },
            unit,
        )
        .await
    }
    #[napi]
    pub async fn expect_bell_count(&self, count: f64, timeout_ms: Option<f64>) -> Result<()> {
        self.call(
            "expectBellCount",
            move || {
                Ok(Operation::ExpectBellCount {
                    count: integer(count, "count", u64::MAX)?,
                    timeout_ms: optional_integer(timeout_ms, "timeoutMs", u64::MAX)?,
                })
            },
            unit,
        )
        .await
    }

    #[napi]
    pub async fn snapshot(
        &self,
        name: String,
        options: Option<SnapshotOptions>,
    ) -> Result<SnapshotResult> {
        let options = options.unwrap_or_default();
        self.call(
            "expectSnapshot",
            move || snapshot_operation(options, name),
            |value| match value {
                OperationResult::Snapshot(value) => Ok(value.into()),
                _ => Err(internal("native operation returned an unexpected result")),
            },
        )
        .await
    }
    #[napi]
    pub async fn screenshot(&self, options: Option<ScreenshotOptions>) -> Result<String> {
        let options = options.unwrap_or_default();
        self.call(
            "screenshot",
            move || screenshot_operation(options),
            string_result,
        )
        .await
    }
    #[napi]
    pub async fn start_recording(&self, options: RecordingOptions) -> Result<()> {
        self.call(
            "startRecording",
            move || start_recording_operation(options),
            unit,
        )
        .await
    }
    #[napi]
    pub async fn stop_recording(&self) -> Result<String> {
        self.call(
            "stopRecording",
            || Ok(Operation::StopRecording),
            string_result,
        )
        .await
    }
}

#[napi]
pub async fn sessions() -> Result<Vec<String>> {
    blocking(|| Ok(global_registry().sessions())).await
}

#[napi]
pub async fn close_all() -> Result<()> {
    blocking(close_all_sync).await
}

#[napi]
pub fn close_all_sync() -> Result<()> {
    boundary(|| {
        global_registry().close_all();
        Ok(())
    })
}

#[napi]
pub async fn recording(name: String) -> Result<String> {
    blocking(move || string_result(session::recording(&name).map_err(native_error)?)).await
}
