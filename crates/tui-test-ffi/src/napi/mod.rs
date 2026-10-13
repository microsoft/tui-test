#![deny(clippy::all)]

mod bindings_generated;
mod convert;
mod types;

use self::types::*;
use crate::session::{self, SessionInput};
use convert::*;
use tui_test::api::{self, Operation, OperationResult};
use tui_test::runtime::global_registry;

use ::napi::bindgen_prelude::spawn_blocking;
use ::napi::{Error, Result, Status};

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

struct SessionRuntime {
    name: String,
    context: SessionInput,
    recording_directory: Option<String>,
}

impl SessionRuntime {
    fn new(
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

    fn name(&self) -> String {
        self.name.clone()
    }

    async fn open(&self, options: Option<OpenOptions>) -> Result<OpenResult> {
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

    async fn run(&self, options: RunOptions) -> Result<OpenResult> {
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

    async fn close(&self, failed: Option<bool>) -> Result<()> {
        let context = self.context.clone();
        blocking(move || {
            let (name, context) = context.validate().map_err(native_error)?;
            unit(session::close(name, context, failed).map_err(native_error)?)
        })
        .await
    }

    async fn call<T: Send + 'static>(
        &self,
        operation_name: &'static str,
        operation: impl FnOnce() -> Result<Operation> + Send + 'static,
        convert: impl FnOnce(OperationResult) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        execute(self.context.clone(), operation_name, operation, convert).await
    }
}

async fn sessions() -> Result<Vec<String>> {
    blocking(|| Ok(global_registry().sessions())).await
}

fn close_all_sync() -> Result<()> {
    boundary(|| {
        global_registry().close_all();
        Ok(())
    })
}

async fn recording(name: String) -> Result<String> {
    blocking(move || string_result(session::recording(&name).map_err(native_error)?)).await
}

fn wait_clipboard_operation(
    text: Option<String>,
    options: ClipboardWaitOptions,
) -> Result<Operation> {
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
        None if options.regex.unwrap_or(false) => Err(usage("clipboard regex requires text")),
        None => Ok(Operation::WaitClipboard { timeout_ms }),
    }
}
