use crate::{finish_trace_and_close, Result};
use tui_test::api::*;
use tui_test::runtime::global_registry;
use tui_test::{
    DiagnosticRetentionOptions, ExecutionContext, FailureArtifactMode, FailureArtifactOptions,
    TraceMode, TraceOptions,
};

#[derive(Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionInput {
    pub name: String,
    pub artifact_directory: Option<String>,
    pub artifact_mode: Option<String>,
    #[serde(default)]
    pub artifact_include_recording: bool,
    pub trace_mode: Option<String>,
    pub trace_directory: Option<String>,
    pub screen_history_limit: Option<u16>,
}

impl SessionInput {
    pub fn validate(self) -> Result<(String, ExecutionContext)> {
        let artifact = match self.artifact_mode {
            None => None,
            Some(mode) => {
                let mode = match mode.as_str() {
                    "all" => FailureArtifactMode::All,
                    "html" => FailureArtifactMode::Html,
                    "text" => FailureArtifactMode::Text,
                    "none" => FailureArtifactMode::None,
                    other => {
                        return Err(TuiTestError::usage(format!(
                            "unknown failure artifact mode {other:?}; expected all, html, text, or none"
                        )))
                    }
                };
                let options = FailureArtifactOptions {
                    directory: self.artifact_directory.unwrap_or_default().into(),
                    mode,
                    include_recording: self.artifact_include_recording,
                };
                options.validate().map_err(TuiTestError::usage)?;
                Some(options)
            }
        };
        let trace = match self.trace_mode {
            None => None,
            Some(mode) => {
                let mode = match mode.as_str() {
                    "off" => TraceMode::Off,
                    "on-failure" => TraceMode::OnFailure,
                    "on" => TraceMode::On,
                    other => {
                        return Err(TuiTestError::usage(format!(
                            "unknown trace mode {other:?}; expected off, on-failure, or on"
                        )))
                    }
                };
                let options = TraceOptions {
                    mode,
                    directory: self
                        .trace_directory
                        .map(Into::into)
                        .unwrap_or_else(|| TraceOptions::default().directory),
                };
                options.validate().map_err(TuiTestError::usage)?;
                Some(options)
            }
        };
        let mut retention = DiagnosticRetentionOptions::default();
        if let Some(limit) = self.screen_history_limit {
            retention.screen_history_limit = limit;
        }
        retention.validate().map_err(TuiTestError::usage)?;
        Ok((
            self.name,
            ExecutionContext {
                artifact,
                trace,
                retention,
                ..ExecutionContext::default()
            },
        ))
    }
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
