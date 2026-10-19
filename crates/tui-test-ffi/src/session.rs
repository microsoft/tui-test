use crate::{finish_trace_and_close, Result};
use tui_test::api::*;
use tui_test::runtime::global_registry;
use tui_test::{
    DiagnosticRetentionOptions, ExecutionContext, FailureArtifactMode, FailureArtifactOptions,
    TraceMode, TraceOptions,
};

include!("session_generated.rs");

fn artifact_mode(mode: &str) -> Result<FailureArtifactMode> {
    serde_json::from_value(mode.into()).map_err(|_| {
        TuiTestError::usage(format!(
            "unknown failure artifact mode {mode:?}; expected all, html, text, or none"
        ))
    })
}
fn trace_mode(mode: &str) -> Result<TraceMode> {
    serde_json::from_value(mode.into()).map_err(|_| {
        TuiTestError::usage(format!(
            "unknown trace mode {mode:?}; expected off, on-failure, or on"
        ))
    })
}

pub(crate) fn execute(
    name: String,
    context: ExecutionContext,
    operation_name: &'static str,
    operation: Operation,
) -> Result<OperationResult> {
    global_registry()
        .session(name)
        .execute_with_context(operation, context.with_operation(operation_name))
}

pub(crate) fn close(
    name: String,
    context: ExecutionContext,
    failed: Option<bool>,
) -> Result<OperationResult> {
    let handle = global_registry().session(name);
    if let Some(failed) = failed {
        return finish_trace_and_close(
            || {
                handle.execute_with_context(
                    Operation::FinishTrace { failed },
                    context.clone().with_operation("trace.finish"),
                )
            },
            || {
                handle
                    .execute_with_context(Operation::Close, context.clone().with_operation("close"))
            },
        );
    }
    handle.execute_with_context(Operation::Close, context.with_operation("close"))
}

pub(crate) fn recording(name: &str) -> Result<OperationResult> {
    global_registry()
        .recording(name)
        .map(OperationResult::Recording)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                TuiTestError::new(
                    ErrorKind::NoSession,
                    format!("no recording for session '{name}'"),
                )
            } else {
                TuiTestError::internal(format!(
                    "failed to read recording for session '{name}': {error}"
                ))
            }
        })
}

pub(crate) fn capture_background(
    background: Option<String>,
    transparent: bool,
) -> Result<Option<CaptureBackground>> {
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
