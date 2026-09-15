//! Reusable in-process terminal engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::api::{
    AutomaticRecording, AutomaticRecordingMode, Cell, CellColor, ClipboardPattern, Cursor,
    EffectiveTimeouts, ErrorKind, LocatorQuery, LocatorSelector, OpenOptions, OpenResult,
    Operation, OperationResult, PackedScreen, RunOptions, RuntimeStatus, ScreenshotResult, Size,
    SnapshotResult, TextAnchor, TextMatch, TextSelector, TextStyle, TuiTestError,
};
use crate::assert::color::{self, Expected};
use crate::assert::snapshot::{self, SnapshotStatus};
use crate::config::{self, POLL_DELAY_MS};
use crate::diagnostics::strings::{
    base_error_message, capture_error_message, diagnostic_hints, diagnostic_operation_name,
    format_timeout, locator_failure_message, operation_timeout, safe_operation_summary,
    timeout_message, title_timeout_message_from_actual, truncate_diagnostic_value,
};
use crate::diagnostics::{
    allocate_artifact_directory, allocate_trace_directory, elapsed_ms, failure_reason,
    recording_temp_path, write_failure_artifact, ArtifactInputs, CellMismatch, CellStyleEvaluation,
    ExecutionContext, FailureArtifactRef, FailureArtifactStatus, FailureObservation, FailureReason,
    FailureReport, InputDetails, LocatorFailureReason, OperationEvent, OperationExpectation,
    OperationHistory, PreparedRecording, ProcessDiagnostics, RecordingDiagnostics, RecordingStatus,
    RuntimeDiagnostics, TraceMode, TraceOptions, TraceOutcome, RECORDING_COPY_LIMIT,
};
use crate::diagnostics::{comparison_failure, merge_failure_details};
use crate::input::{keys, mouse};
use crate::logger::Logger;
use crate::session::{
    capture_visual_state, try_capture_visual_state, Session as TerminalSession, TermState,
    TextHighlight,
};
use crate::terminal::cell::{rows_to_strings, Attrs, Color, EmuCell};
use crate::terminal::emu::{
    ClipboardType, CursorShape, Emulator, KeyboardMode, MouseMode, TerminalMode,
};
use crate::terminal::locator::{self, Pattern};

pub struct Engine {
    name: String,
    operations: Mutex<()>,
    session: Mutex<Option<TerminalSession>>,
    spawn_spec: Mutex<Option<SpawnSpec>>,
    live: Arc<Mutex<Option<LiveTarget>>>,
    interrupt: Mutex<Option<InterruptTarget>>,
    logger: Arc<Logger>,
    default_recording_path: PathBuf,
    recording: Mutex<RecordingState>,
    trace: Mutex<TraceState>,
    operation_history: Mutex<OperationHistory>,
}

#[derive(Clone)]
struct SpawnSpec {
    command: SpawnCommand,
    resolved_cwd: Option<PathBuf>,
    retention: crate::diagnostics::DiagnosticRetentionOptions,
    trace: Option<TraceOptions>,
}

#[derive(Clone)]
enum SpawnCommand {
    Open(OpenOptions),
    Run(RunOptions),
}

impl SpawnSpec {
    fn restart(mut self) -> Self {
        match &mut self.command {
            SpawnCommand::Open(options) => options.restart = true,
            SpawnCommand::Run(options) => options.restart = true,
        }
        self
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        match &mut self.command {
            SpawnCommand::Open(options) => {
                options.cols = cols;
                options.rows = rows;
            }
            SpawnCommand::Run(options) => {
                options.cols = cols;
                options.rows = rows;
            }
        }
    }
}

#[derive(Clone)]
struct RecordingState {
    path: Option<PathBuf>,
    mode: AutomaticRecordingMode,
    failed: bool,
}

#[derive(Default)]
struct TraceState {
    options: TraceOptions,
    artifact: Option<FailureArtifactRef>,
    context: std::collections::BTreeMap<String, String>,
    owned_directories: Vec<PathBuf>,
}

#[derive(Clone)]
struct InterruptTarget {
    pty: Arc<Mutex<crate::terminal::pty::Pty>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

struct LiveTarget {
    state: Arc<Mutex<TermState>>,
    pty: Arc<Mutex<crate::terminal::pty::Pty>>,
    shell: Option<&'static str>,
}

#[derive(Clone)]
struct OperationMetadata {
    name: String,
    timeout_ms: Option<u64>,
    started_at: Instant,
    started_ms: u64,
    screen_before: u64,
    safe_summary: String,
    is_assertion: bool,
    expectation: Option<OperationExpectation>,
    input: Option<InputDetails>,
}

pub struct LiveFrame {
    pub grid: Vec<Vec<EmuCell>>,
    pub cursor: (u16, u16),
    pub size: (u16, u16),
    pub keyboard_mode: KeyboardMode,
    pub cursor_key_application: bool,
    pub bracketed_paste: bool,
    pub mouse_mode: MouseMode,
    pub exited: Option<i32>,
    pub shell: Option<&'static str>,
}

/// One-line operation description for the verbose log. Open and Run redact env
/// values (they may contain secrets) and report only the variable count.
fn operation_summary(operation: &Operation) -> String {
    match operation {
        Operation::Open(options) => format!(
            "Open {{ backend: {}, shell: {:?}, scrollback: {}, {}x{}, cwd: {:?}, wait_ready: {:?}, restart: {}, timeouts: {:?}, env: <{} vars> }}",
            options.backend.as_str(),
            options.shell,
            options.profile.scrollback,
            options.cols,
            options.rows,
            options.cwd,
            options.wait_ready,
            options.restart,
            options.timeouts,
            options.env.len()
        ),
        Operation::Run(options) => format!(
            "Run {{ backend: {}, program: {:?}, args: {:?}, scrollback: {}, {}x{}, cwd: {:?}, wait_ready: {:?}, restart: {}, timeouts: {:?}, env: <{} vars> }}",
            options.backend.as_str(),
            options.program,
            options.args,
            options.profile.scrollback,
            options.cols,
            options.rows,
            options.cwd,
            options.wait_ready,
            options.restart,
            options.timeouts,
            options.env.len()
        ),
        other => format!("{other:?}"),
    }
}

impl Engine {
    pub fn new(name: String, logger: Arc<Logger>, recording_path: PathBuf) -> Self {
        Self {
            name,
            operations: Mutex::new(()),
            session: Mutex::new(None),
            spawn_spec: Mutex::new(None),
            live: Arc::new(Mutex::new(None)),
            interrupt: Mutex::new(None),
            logger,
            default_recording_path: recording_path.clone(),
            recording: Mutex::new(RecordingState {
                path: None,
                mode: AutomaticRecordingMode::Disabled,
                failed: false,
            }),
            trace: Mutex::new(TraceState::default()),
            operation_history: Mutex::new(OperationHistory::new()),
        }
    }

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, TuiTestError> {
        self.execute_with_context(operation, ExecutionContext::default())
    }

    pub fn execute_with_context(
        &self,
        operation: Operation,
        context: ExecutionContext,
    ) -> Result<OperationResult, TuiTestError> {
        if let Some(artifact) = &context.artifact {
            artifact.validate().map_err(TuiTestError::usage)?;
        }
        if let Some(trace) = &context.trace {
            trace.validate().map_err(TuiTestError::usage)?;
        }
        let _operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.logger.enabled() {
            self.logger
                .event(&format!("operation {}", operation_summary(&operation)));
        }
        let name = context
            .operation_name
            .clone()
            .unwrap_or_else(|| diagnostic_operation_name(&operation).to_string());
        let screen_before = self.capture_current_screen_sequence(true, false);
        let canonical_name = diagnostic_operation_name(&operation);
        let is_assertion = canonical_name.starts_with("expect.")
            || canonical_name.starts_with("wait.")
            || matches!(canonical_name, "locator.wait" | "locator.location");
        let started_ms = self.current_session_elapsed_ms();
        let mut metadata = OperationMetadata {
            name: name.clone(),
            timeout_ms: operation_timeout(&operation),
            started_at: Instant::now(),
            started_ms,
            screen_before,
            safe_summary: safe_operation_summary(&operation),
            is_assertion,
            expectation: OperationExpectation::capture(&operation),
            input: InputDetails::capture(&operation),
        };
        let pending = self
            .operation_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin(
                name,
                started_ms,
                screen_before,
                metadata.safe_summary.clone(),
                is_assertion,
                metadata.expectation.clone(),
            );
        let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute_inner(operation, &context, &mut metadata)
        }))
        .unwrap_or_else(|payload| {
            Err(TuiTestError::internal(format!(
                "native terminal operation panicked: {}",
                panic_message(payload.as_ref())
            )))
        });
        let failed = result
            .as_ref()
            .is_err_and(|error| matches!(error.kind, ErrorKind::Assertion | ErrorKind::Internal));
        if failed {
            self.recording
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .failed = true;
        }
        if let Err(error) = &mut result {
            self.prepare_failure_observation(error);
        }
        let pin_checkpoint = is_assertion
            || (metadata.input.is_some()
                && self
                    .trace
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .options
                    .mode
                    != TraceMode::Off);
        let screen_at_return = result
            .as_ref()
            .err()
            .and_then(|error| error.observation.as_deref())
            .map_or_else(
                || self.capture_current_screen_sequence(true, pin_checkpoint),
                |observation| observation.screen_sequence,
            );
        let result_name = match &result {
            Ok(_) => "ok",
            Err(error) => error.kind.as_str(),
        };
        self.operation_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish(
                pending,
                result
                    .as_ref()
                    .err()
                    .and_then(|error| error.observation.as_deref())
                    .map_or_else(
                        || self.current_session_elapsed_ms(),
                        |observation| observation.captured_ms,
                    ),
                screen_at_return,
                result_name,
                metadata.input.clone(),
            );
        if let Err(error) = &mut result {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.finalize_failure(error, &context, &metadata);
            }));
            error.observation = None;
            error.report = None;
        }
        result
    }

    fn execute_inner(
        &self,
        operation: Operation,
        context: &ExecutionContext,
        metadata: &mut OperationMetadata,
    ) -> Result<OperationResult, TuiTestError> {
        match operation {
            Operation::FinishTrace { failed } => {
                self.finish_trace(context, Some(failed))?;
                Ok(OperationResult::Unit)
            }
            Operation::Open(options) => self
                .open(options, context, metadata)
                .map(OperationResult::Open),
            Operation::Run(options) => self
                .run(options, context, metadata)
                .map(OperationResult::Open),
            Operation::Restart {
                graceful_timeout_ms,
            } => self
                .restart(graceful_timeout_ms, context, metadata)
                .map(OperationResult::Open),
            Operation::Close => {
                let trace_result = self.finish_trace(context, None);
                *self
                    .live
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                *self
                    .interrupt
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                if let Some(session) = self.lock_session().take() {
                    session.kill();
                    drop(session);
                }
                *self
                    .spawn_spec
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                self.cleanup_recording();
                trace_result?;
                Ok(OperationResult::Unit)
            }
            Operation::Resize { cols, rows } => {
                let result = self.with_session(|session| {
                    dispatch(
                        session,
                        Operation::Resize { cols, rows },
                        &mut metadata.input,
                    )
                });
                if result.is_ok() {
                    if let Some(spec) = self
                        .spawn_spec
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_mut()
                    {
                        spec.resize(cols, rows);
                    }
                }
                result
            }
            other => self.with_session(|session| dispatch(session, other, &mut metadata.input)),
        }
    }

    fn open(
        &self,
        options: OpenOptions,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        self.spawn(
            SpawnSpec {
                command: SpawnCommand::Open(options),
                resolved_cwd: None,
                retention: context.retention,
                trace: context.trace.clone(),
            },
            context,
            metadata,
        )
    }

    fn run(
        &self,
        options: RunOptions,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        self.spawn(
            SpawnSpec {
                command: SpawnCommand::Run(options),
                resolved_cwd: None,
                retention: context.retention,
                trace: context.trace.clone(),
            },
            context,
            metadata,
        )
    }

    fn restart(
        &self,
        graceful_timeout_ms: u64,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        let spec = self
            .spawn_spec
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(TuiTestError::no_restart_metadata)?;

        if let Some(session) = self.lock_session().as_ref() {
            if session.is_alive()? {
                if let Err(error) = session
                    .pty
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .signal("INT")
                {
                    self.logger
                        .event(&format!("restart interrupt failed error={error}"));
                }
                let start = Instant::now();
                let timeout = Duration::from_millis(graceful_timeout_ms);
                while session.is_alive()? && start.elapsed() < timeout {
                    std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
                }
            }
        }

        let mut context = context.clone();
        if context.trace.is_none() {
            context.trace = spec.trace.clone();
        }
        self.spawn(spec.restart(), &context, metadata)
    }

    fn spawn(
        &self,
        mut spec: SpawnSpec,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        let diagnostics = spec.retention;
        let (
            shell,
            program,
            backend,
            profile,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            restart,
            timeouts,
            mut recording,
        ) = match &spec.command {
            SpawnCommand::Open(options) => (
                options.shell,
                None,
                options.backend,
                options.profile,
                options.cols,
                options.rows,
                options.cwd.clone(),
                options.env.clone(),
                options.wait_ready,
                options.restart,
                options.timeouts,
                options.recording.clone(),
            ),
            SpawnCommand::Run(options) => {
                let mut program = Vec::with_capacity(options.args.len() + 1);
                program.push(options.program.clone());
                program.extend(options.args.clone());
                (
                    None,
                    Some(program),
                    options.backend,
                    options.profile,
                    options.cols,
                    options.rows,
                    options.cwd.clone(),
                    options.env.clone(),
                    options.wait_ready,
                    options.restart,
                    options.timeouts,
                    options.recording.clone(),
                )
            }
        };
        recording.validate()?;
        let mut trace_options = context.trace.clone().unwrap_or_default();
        trace_options.directory =
            std::path::absolute(&trace_options.directory).map_err(|error| {
                TuiTestError::internal(format!("failed to resolve trace directory: {error}"))
            })?;
        spec.trace = context.trace.as_ref().map(|_| trace_options.clone());
        if context.trace.is_some() {
            recording.mode = match trace_options.mode {
                TraceMode::Off => AutomaticRecordingMode::Disabled,
                TraceMode::On => AutomaticRecordingMode::Always,
                TraceMode::OnFailure => AutomaticRecordingMode::OnFailure,
            };
        } else if context
            .artifact
            .as_ref()
            .is_some_and(|artifact| artifact.include_recording)
            && recording.mode == AutomaticRecordingMode::Disabled
        {
            recording.mode = AutomaticRecordingMode::OnFailure;
        }
        match &mut spec.command {
            SpawnCommand::Open(options) => options.recording = recording.clone(),
            SpawnCommand::Run(options) => options.recording = recording.clone(),
        }
        diagnostics.validate().map_err(TuiTestError::usage)?;
        let mut current = self.lock_session();
        if let Some(previous) = current.as_ref() {
            if !restart && previous.is_alive()? {
                return Ok(OpenResult {
                    shell_pid: previous.pid(),
                    session: self.name.clone(),
                    ready: previous.is_ready(),
                    recording: self.recording_path_string(),
                });
            }
        }
        let cwd = match &spec.resolved_cwd {
            Some(cwd) => cwd.clone(),
            None => std::path::absolute(cwd.as_deref().unwrap_or(".")).map_err(|error| {
                TuiTestError::internal(format!("failed to resolve session cwd: {error}"))
            })?,
        };
        spec.resolved_cwd = Some(cwd.clone());
        let recording_required = recording.directory.is_some();
        let recording_path = self.resolve_recording_path(&recording)?;

        *self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .interrupt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        if let Some(previous) = current.take() {
            self.finish_trace_with_session(&previous, context, None)?;
            previous.kill();
            drop(previous);
        }
        drop(current);
        self.operation_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reset_session();
        self.discard_recording();
        *self
            .trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = TraceState {
            options: trace_options,
            artifact: None,
            context: context.sanitized_context(),
            owned_directories: Vec::new(),
        };
        *self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = RecordingState {
            path: None,
            mode: recording.mode,
            failed: false,
        };
        let session = TerminalSession::open(
            shell,
            program.clone(),
            backend,
            profile,
            cols,
            rows,
            Some(cwd),
            env,
            timeouts,
            diagnostics,
            self.logger.clone(),
            recording_path.clone(),
            recording_required,
        )
        .map_err(|error| TuiTestError::internal(format!("failed to open session: {error}")))?;
        self.recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .path = session
            .automatic_recording_enabled()
            .then_some(recording_path)
            .flatten();

        let shell_pid = session.pid();
        let ready_timeout = open_ready_timeout(&session);
        let ready = if wait_ready.unwrap_or(program.is_none()) {
            await_ready(&session, ready_timeout)
        } else {
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tracker
                .is_ready()
        };
        if wait_ready == Some(true) && !ready {
            let message = format!(
                "open: the session started but reported no prompt within \
                 {ready_timeout}ms; pass --no-wait-ready if it has no shell \
                 integration"
            );
            let mut error = TuiTestError::assertion(message);
            error.observation = Some(Box::new(capture_failure_observation(&session)));
            let metadata = OperationMetadata {
                started_ms: 0,
                screen_before: 0,
                ..metadata.clone()
            };
            self.finalize_failure_with_session(
                &mut error,
                context,
                &metadata,
                Some(&session),
                true,
            );
            session.kill();
            return Err(error);
        }
        let live = LiveTarget {
            state: session.state.clone(),
            pty: session.pty.clone(),
            shell: session.shell.map(|value| value.as_str()),
        };
        *self
            .interrupt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(InterruptTarget {
            pty: session.pty.clone(),
            cancelled: session.cancelled.clone(),
        });
        *self.lock_session() = Some(session);
        *self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(live);
        *self
            .spawn_spec
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(spec);
        Ok(OpenResult {
            shell_pid,
            session: self.name.clone(),
            ready,
            recording: self.recording_path_string(),
        })
    }

    fn with_session<F>(&self, operation: F) -> Result<OperationResult, TuiTestError>
    where
        F: FnOnce(&mut TerminalSession) -> Result<OperationResult, TuiTestError>,
    {
        let mut guard = self.lock_session();
        let session = guard.as_mut().ok_or_else(TuiTestError::no_session)?;
        // The emulator is fed on the reader thread, where there is nobody to
        // return an error to, so a backend that failed to parse records it and
        // the next operation reports it. Checked before the operation runs:
        // once the grid has stopped tracking the bytes, every answer read out
        // of it is a guess, and a wrong answer is worse than a failure.
        if let Some(fault) = session.fault() {
            return Err(
                TuiTestError::internal(fault.clone()).with_report(FailureReport::new(
                    "terminal.operation",
                    None,
                    FailureReason::EmulatorFault,
                    fault,
                )),
            );
        }
        operation(session)
    }

    fn capture_current_screen_sequence(&self, force: bool, pin: bool) -> u64 {
        let mut guard = self.lock_session();
        let Some(session) = guard.as_mut() else {
            return 0;
        };
        let mut state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sequence = try_capture_visual_state(&mut state, force).unwrap_or(0);
        if pin && sequence != 0 {
            state.screen_history.pin_current();
        }
        sequence
    }

    fn current_session_elapsed_ms(&self) -> u64 {
        let guard = self.lock_session();
        guard.as_ref().map_or(0, |session| {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            elapsed_ms(state.started_at)
        })
    }

    fn prepare_failure_observation(&self, error: &mut TuiTestError) {
        if error.observation.is_some()
            || !matches!(error.kind, ErrorKind::Assertion | ErrorKind::Internal)
        {
            return;
        }
        let guard = self.lock_session();
        if let Some(session) = guard.as_ref() {
            error.observation = safe_capture_failure_observation(session).map(Box::new);
        }
    }

    fn finalize_failure(
        &self,
        error: &mut TuiTestError,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) {
        if !matches!(error.kind, ErrorKind::Assertion | ErrorKind::Internal) {
            return;
        }
        if error.details.is_some() {
            return;
        }
        let guard = self.lock_session();
        self.finalize_failure_with_session(error, context, metadata, guard.as_ref(), false);
    }

    fn finalize_failure_with_session(
        &self,
        error: &mut TuiTestError,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
        session: Option<&TerminalSession>,
        include_pending_operation: bool,
    ) {
        if !matches!(error.kind, ErrorKind::Assertion | ErrorKind::Internal) {
            return;
        }
        if error.details.is_some() {
            return;
        }
        let captured = if error.observation.is_none() {
            session.and_then(safe_capture_failure_observation)
        } else {
            None
        };
        let observation = error.observation.as_deref().or(captured.as_ref());
        let (summary, summary_truncated) =
            truncate_diagnostic_value(base_error_message(&error.message), 64 * 1024);
        let mut details = FailureReport::new(
            metadata.name.clone(),
            metadata.timeout_ms,
            failure_reason(error, observation),
            summary,
        );
        details.truncated = summary_truncated;
        details.operation.elapsed_ms = metadata.started_at.elapsed().as_millis() as u64;
        details.operation.started_screen_sequence = metadata.screen_before;
        details.operation.failed_screen_sequence = observation
            .as_ref()
            .map_or(0, |value| value.screen_sequence);
        if let Some(existing) = error.report.take() {
            merge_failure_details(&mut details, *existing);
        }
        details.truncated |= details
            .locator
            .as_ref()
            .is_some_and(|locator| locator.stages_truncated);
        let trace = self
            .trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .options
            .clone();
        let trace_artifact = (trace.mode != TraceMode::Off).then(|| trace.artifact_options());
        if !trace_artifact
            .as_ref()
            .or(context.artifact.as_ref())
            .is_some_and(|options| options.mode != crate::diagnostics::FailureArtifactMode::None)
        {
            let failure = details.failure_details();
            error.message = failure.summary.clone();
            error.details = Some(Box::new(failure));
            return;
        }
        details.context = context.sanitized_context();
        details.recent_operations = self
            .operation_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot();
        if include_pending_operation {
            details.recent_operations.push(OperationEvent {
                sequence: details
                    .recent_operations
                    .last()
                    .map_or(1, |event| event.sequence.saturating_add(1)),
                name: metadata.name.clone(),
                started_ms: metadata.started_ms,
                ended_ms: metadata
                    .started_ms
                    .saturating_add(metadata.started_at.elapsed().as_millis() as u64),
                result: error.kind.as_str().to_string(),
                screen_before: metadata.screen_before,
                screen_at_return: observation
                    .as_ref()
                    .map_or(metadata.screen_before, |value| value.screen_sequence),
                safe_summary: metadata.safe_summary.clone(),
                is_assertion: metadata.is_assertion,
                expectation: metadata.expectation.clone(),
                input: metadata.input.clone(),
            });
        }

        details.truncated |= details.recent_operations.iter().any(|event| {
            matches!(
                event.expectation,
                Some(OperationExpectation::Unavailable { .. })
            ) || event
                .input
                .as_ref()
                .is_some_and(InputDetails::is_unavailable)
        });
        if let Some(observation) = &observation {
            details.terminal = Some(observation.terminal());
            details.process = Some(observation.process.clone());
            details.runtime = Some(RuntimeDiagnostics {
                session_name: Some(self.name.clone()),
                ..observation.runtime.clone()
            });
            details.recording = Some(self.recording_diagnostics(observation));
        }
        details.hints = diagnostic_hints(&details);

        details.finish_signature();

        if trace_artifact.is_some() {
            details.outcome = Some(TraceOutcome::Failed);
        }
        if let (Some(options), Some(observation)) = (
            trace_artifact.as_ref().or(context.artifact.as_ref()),
            &observation,
        ) {
            if options.mode != crate::diagnostics::FailureArtifactMode::None {
                let allocated = if trace_artifact.is_some() {
                    allocate_trace_directory(&options.directory)
                } else {
                    allocate_artifact_directory(&options.directory)
                };
                let mut owned_directory = None;
                let artifact = match allocated {
                    Ok(directory) => {
                        if trace_artifact.is_some() {
                            owned_directory = Some(directory.clone());
                        }
                        let prepared_recording = if options.include_recording {
                            session.and_then(|session| {
                                self.prepare_recording_artifact(
                                    session,
                                    observation,
                                    &directory,
                                    &mut details,
                                )
                            })
                        } else {
                            None
                        };
                        write_failure_artifact(
                            options,
                            ArtifactInputs {
                                details: &mut details,
                                observation,
                                recording: prepared_recording,
                            },
                            directory,
                        )
                    }
                    Err(error) => FailureArtifactRef {
                        status: FailureArtifactStatus::Failed,
                        directory: options.directory.to_string_lossy().into_owned(),
                        manifest: None,
                        report: None,
                        report_html: None,
                        timeline: None,
                        screen_text: None,
                        screen_svg: None,
                        recording: None,
                        errors: vec![format!(
                            "failed to allocate failure artifact directory: {error}"
                        )],
                    },
                };
                if trace_artifact.is_some() {
                    let mut trace = self
                        .trace
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    trace.artifact = Some(artifact.clone());
                    trace.owned_directories.extend(owned_directory);
                }
                error.artifact = Some(Box::new(artifact));
            }
        }
        let failure = details.failure_details();
        error.message = failure.summary.clone();
        error.details = Some(Box::new(failure));
    }

    fn finish_trace(
        &self,
        context: &ExecutionContext,
        failed: Option<bool>,
    ) -> Result<(), TuiTestError> {
        let session = self.lock_session();
        if let Some(session) = session.as_ref() {
            self.finish_trace_with_session(session, context, failed)?;
        }
        Ok(())
    }

    fn finish_trace_with_session(
        &self,
        session: &TerminalSession,
        context: &ExecutionContext,
        failed: Option<bool>,
    ) -> Result<(), TuiTestError> {
        let previous_failed = self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .failed;
        if failed.is_some_and(|failed| failed != previous_failed) {
            let mut trace = self
                .trace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while let Some(directory) = trace.owned_directories.last() {
                std::fs::remove_dir_all(directory).map_err(|error| {
                    TuiTestError::internal(format!(
                        "failed to discard superseded trace {}: {error}",
                        directory.display()
                    ))
                })?;
                trace.owned_directories.pop();
            }
            trace.artifact = None;
        }
        let failed = failed.unwrap_or(previous_failed);
        self.recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .failed = failed;
        let options = {
            let trace = self
                .trace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if trace.options.mode == TraceMode::Off || trace.artifact.is_some() {
                return Ok(());
            }
            trace.options.clone()
        };
        if options.mode == TraceMode::OnFailure && !failed {
            return Ok(());
        }
        let observation = capture_failure_observation(session);
        let mut details = FailureReport::new(
            "test",
            None,
            if failed {
                FailureReason::TestFailed
            } else {
                FailureReason::Completed
            },
            if failed {
                "Test failed outside a terminal assertion."
            } else {
                "Session completed successfully."
            },
        );
        details.outcome = Some(if failed {
            TraceOutcome::Failed
        } else {
            TraceOutcome::Passed
        });
        details.operation.failed_screen_sequence = observation.screen_sequence;
        details.operation.elapsed_ms = observation.captured_ms;
        details.terminal = Some(observation.terminal());
        details.runtime = Some(RuntimeDiagnostics {
            session_name: Some(self.name.clone()),
            ..observation.runtime.clone()
        });
        details.process = Some(observation.process.clone());
        details.recording = Some(self.recording_diagnostics(&observation));
        details.context = self
            .trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .context
            .clone();
        details.context.extend(context.sanitized_context());
        details.recent_operations = self
            .operation_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot();
        details.truncated = details.recent_operations.iter().any(|event| {
            matches!(
                event.expectation,
                Some(OperationExpectation::Unavailable { .. })
            ) || event
                .input
                .as_ref()
                .is_some_and(InputDetails::is_unavailable)
        });
        details.finish_signature();
        let directory = allocate_trace_directory(&options.directory).map_err(|error| {
            TuiTestError::internal(format!("failed to allocate trace directory: {error}"))
        })?;
        let recording =
            self.prepare_recording_artifact(session, &observation, &directory, &mut details);
        let artifact = write_failure_artifact(
            &options.artifact_options(),
            ArtifactInputs {
                details: &mut details,
                observation: &observation,
                recording,
            },
            directory.clone(),
        );
        {
            let mut trace = self
                .trace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            trace.artifact = Some(artifact.clone());
            trace.owned_directories.push(directory);
        }
        if artifact.status != FailureArtifactStatus::Written {
            let mut error = TuiTestError::internal(format!(
                "trace could not be fully written at {}: {}",
                artifact.directory,
                if artifact.errors.is_empty() {
                    "evidence was omitted; see the manifest for details".to_string()
                } else {
                    artifact.errors.join("; ")
                }
            ));
            error.artifact = Some(Box::new(artifact));
            return Err(error);
        }
        Ok(())
    }

    fn prepare_recording_artifact(
        &self,
        session: &TerminalSession,
        observation: &FailureObservation,
        directory: &std::path::Path,
        details: &mut FailureReport,
    ) -> Option<PreparedRecording> {
        if details
            .recording
            .as_ref()
            .is_some_and(|recording| recording.status == RecordingStatus::Disabled)
        {
            return None;
        }
        let state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if details.outcome.is_none()
            && (state.visual_revision != observation.output_revision
                || state.screen_dirty
                || state.screen_history.current_sequence() != observation.screen_sequence)
        {
            if let Some(recording) = details.recording.as_mut() {
                recording.status = RecordingStatus::Omitted;
                recording.reason = Some(
                    "terminal output advanced after the pinned failure observation".to_string(),
                );
            }
            return None;
        }
        let temporary_path = recording_temp_path(directory);
        let result =
            session.snapshot_automatic_recording(temporary_path.clone(), RECORDING_COPY_LIMIT);
        drop(state);
        match result {
            Ok(snapshot) => {
                if let Some(recording) = details.recording.as_mut() {
                    recording.status = RecordingStatus::Live;
                    recording.last_committed_ms = snapshot.last_committed_ms;
                    recording.path = None;
                    recording.bytes = Some(snapshot.bytes);
                    recording.reason = None;
                    recording.ephemeral = false;
                }
                Some(PreparedRecording {
                    temporary_path,
                    bytes: snapshot.bytes,
                    sha256: snapshot.sha256,
                })
            }
            Err(error) => {
                let message = capture_error_message(&error);
                if let Some(recording) = details.recording.as_mut() {
                    recording.status = if message.contains("maximum byte limit") {
                        RecordingStatus::Omitted
                    } else {
                        RecordingStatus::Failed
                    };
                    recording.reason = Some(message);
                }
                None
            }
        }
    }

    fn recording_diagnostics(&self, observation: &FailureObservation) -> RecordingDiagnostics {
        let recording = self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (status, reason) = match (&recording.mode, &recording.path) {
            (AutomaticRecordingMode::Disabled, _) => {
                (RecordingStatus::Disabled, Some("disabled".to_string()))
            }
            (_, Some(_)) => (RecordingStatus::Live, None),
            _ => (
                RecordingStatus::Unavailable,
                Some("automatic recording could not be created".to_string()),
            ),
        };
        RecordingDiagnostics {
            mode: recording.mode,
            status,
            failure_offset_ms: observation.captured_ms,
            last_committed_ms: None,
            path: None,
            bytes: None,
            reason,
            ephemeral: false,
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        let guard = self.lock_session();
        match guard.as_ref() {
            Some(session) => {
                let state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                RuntimeStatus {
                    session: self.name.clone(),
                    shell_pid: session.pid(),
                    cols: Some(session.cols),
                    rows: Some(session.rows),
                    shell: session.shell.map(|value| value.as_str().to_string()),
                    exited: state.exited,
                    timeouts: Some(effective_timeouts(session)),
                }
            }
            None => RuntimeStatus {
                session: self.name.clone(),
                shell_pid: None,
                cols: None,
                rows: None,
                shell: None,
                exited: None,
                timeouts: None,
            },
        }
    }

    pub fn frame(&self) -> Option<LiveFrame> {
        let live = self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        live.as_ref().map(|target| {
            let state = target
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            LiveFrame {
                grid: highlighted_rows(&state, false),
                cursor: state.emu.cursor(),
                size: state.emu.size(),
                keyboard_mode: state.emu.keyboard_mode(),
                cursor_key_application: state.emu.cursor_key_application(),
                bracketed_paste: state.emu.mode(TerminalMode::BracketedPaste),
                mouse_mode: state.mouse_mode.relayable(),
                exited: state.exited,
                shell: target.shell,
            }
        })
    }

    pub fn monitor_mouse_size(&self) -> Option<(u16, u16)> {
        let live = self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        live.as_ref().and_then(|target| {
            let state = target
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.mouse_mode.relayable() != MouseMode::None).then(|| state.emu.size())
        })
    }

    /// Write viewer keystrokes straight to the pty: untracked and unlogged, so
    /// a human watching cannot disturb what the agent is waiting on.
    pub fn write_monitor_input_raw(&self, data: &[u8]) -> Result<(), TuiTestError> {
        let Some((state, pty)) = ({
            let live = self
                .live
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            live.as_ref()
                .map(|target| (Arc::clone(&target.state), Arc::clone(&target.pty)))
        }) else {
            return Ok(());
        };
        let exited = || {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .exited
                .is_some()
        };
        if exited() {
            return Ok(());
        }
        // A child that exits mid-write is a normal race, not a failure.
        let written = pty
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write(data);
        match written {
            Err(error)
                if !matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::NotConnected
                ) && !exited() =>
            {
                Err(TuiTestError::internal(error.to_string()))
            }
            _ => Ok(()),
        }
    }

    pub fn log_event(&self, message: &str) {
        self.logger.event(message);
    }

    pub fn interrupt(&self) {
        let target = self
            .interrupt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(target) = target {
            target
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            target
                .pty
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .kill();
        }
    }

    pub fn is_open(&self) -> bool {
        self.lock_session().is_some()
    }

    pub fn recording_path(&self) -> Option<PathBuf> {
        self.recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .path
            .clone()
    }

    pub(crate) fn retained_recording_path(&self) -> Option<PathBuf> {
        let recording = self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retain = match recording.mode {
            AutomaticRecordingMode::Disabled => false,
            AutomaticRecordingMode::OnFailure => recording.failed,
            AutomaticRecordingMode::Always => true,
        };
        retain
            .then(|| recording.path.clone())
            .flatten()
            .filter(|path| path.is_file())
    }

    pub fn flush_recording(&self) -> Result<(), TuiTestError> {
        let _operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.recording_path().is_none() {
            return Err(TuiTestError::usage("automatic recording is disabled"));
        }
        let guard = self.lock_session();
        if let Some(session) = guard.as_ref() {
            return session.flush_recording();
        }
        if self.recording_path().is_some_and(|path| path.is_file()) {
            Ok(())
        } else {
            Err(TuiTestError::no_session())
        }
    }

    fn resolve_recording_path(
        &self,
        recording: &AutomaticRecording,
    ) -> Result<Option<PathBuf>, TuiTestError> {
        if recording.mode == AutomaticRecordingMode::Disabled {
            return Ok(None);
        }
        let Some(directory) = &recording.directory else {
            return Ok(Some(self.default_recording_path.clone()));
        };
        if directory.as_os_str().is_empty() {
            return Err(TuiTestError::usage(
                "automatic recording directory must not be empty",
            ));
        }
        let directory = if directory.is_absolute() {
            directory.clone()
        } else {
            std::env::current_dir()
                .map_err(|error| {
                    TuiTestError::internal(format!(
                        "failed to resolve automatic recording directory: {error}"
                    ))
                })?
                .join(directory)
        };
        let name = self
            .default_recording_path
            .file_name()
            .ok_or_else(|| TuiTestError::internal("automatic recording path has no file name"))?;
        Ok(Some(directory.join(name)))
    }

    fn recording_path_string(&self) -> String {
        self.recording_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn cleanup_recording(&self) {
        let recording = self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keep = match recording.mode {
            AutomaticRecordingMode::Disabled => false,
            AutomaticRecordingMode::OnFailure => recording.failed,
            AutomaticRecordingMode::Always => true,
        };
        if !keep {
            if let Some(path) = &recording.path {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn discard_recording(&self) {
        if let Some(path) = self
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .path
            .as_ref()
        {
            let _ = std::fs::remove_file(path);
        }
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<TerminalSession>> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Err(error) = self.finish_trace(
            &ExecutionContext::default(),
            std::thread::panicking().then_some(true),
        ) {
            eprintln!("failed to finish terminal trace: {error}");
        }
        if let Ok(session) = self.session.get_mut() {
            if let Some(session) = session.take() {
                session.kill();
                drop(session);
            }
        }
        self.cleanup_recording();
    }
}

fn capture_failure_observation(session: &TerminalSession) -> FailureObservation {
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    capture_failure_observation_locked(session, &mut state)
}

fn safe_capture_failure_observation(session: &TerminalSession) -> Option<FailureObservation> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture_failure_observation(session)
    })) {
        Ok(observation) => Some(observation),
        Err(payload) => {
            let message = format!(
                "terminal diagnostic capture panicked: {}",
                panic_message(payload.as_ref())
            );
            let mut state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.diagnostic_error.is_none() {
                state.diagnostic_error = Some(message);
            }
            None
        }
    }
}

fn capture_failure_observation_locked(
    session: &TerminalSession,
    state: &mut TermState,
) -> FailureObservation {
    let screen_sequence = capture_visual_state(state, true);
    state.screen_history.pin_current();
    let snapshot = svg_snapshot_from(state.emu.as_ref(), false);
    let captured_ms = elapsed_ms(state.started_at);
    let last_visual_change_ms = state.last_visual_change_ms;
    let cancelled = session.cancelled.load(std::sync::atomic::Ordering::Acquire);
    let process_state = if cancelled {
        "cancelled"
    } else if state.exited.is_some() {
        "exited"
    } else if state.exit_error.is_some() {
        "unknown"
    } else {
        "running"
    };
    let process = ProcessDiagnostics {
        pid: session.child_pid,
        state: process_state.to_string(),
        exit_code: state.exited,
        status_error: state.exit_error.clone(),
        cancelled,
        ready: state.tracker.is_ready(),
        command_running: state.tracker.executing(),
        last_command_exit: state.tracker.last_exit(),
    };
    let runtime = RuntimeDiagnostics {
        session_name: None,
        shell: session.shell.map(|shell| shell.as_str().to_string()),
        timeouts: Some(effective_timeouts(session)),
        tui_test_version: env!("CARGO_PKG_VERSION").to_string(),
        backend: session.backend.as_str().to_string(),
        target_os: std::env::consts::OS.to_string(),
        target_arch: std::env::consts::ARCH.to_string(),
    };
    FailureObservation {
        rows: snapshot.rows,
        cols: snapshot.cols,
        title: snapshot.title,
        cursor: snapshot.cursor,
        cursor_position: state.emu.cursor(),
        cursor_visible: state.emu.cursor_visible(),
        cursor_shape: state.emu.cursor_shape(),
        render_state: snapshot.render_state,
        screen_sequence,
        output_revision: state.visual_revision,
        captured_ms,
        last_visual_change_ms,
        history: state.screen_history.clone(),
        process,
        runtime,
    }
}

fn open_ready_timeout(session: &TerminalSession) -> u64 {
    session
        .timeouts
        .get(config::TimeoutClass::Ready)
        .or_else(|| config::TimeoutClass::Ready.env_ms())
        .unwrap_or(config::OPEN_READY_CAP_MS)
}

fn await_ready(session: &TerminalSession, timeout_ms: u64) -> bool {
    let start = Instant::now();
    let cap = Duration::from_millis(timeout_ms);
    loop {
        if session.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return false;
        }
        {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.tracker.is_ready() {
                return true;
            }
            if state.exited.is_some() {
                return false;
            }
        }
        if start.elapsed() >= cap {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn viewable(session: &TerminalSession) -> Vec<Vec<EmuCell>> {
    session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .emu
        .viewable_rows()
}

fn grid(session: &TerminalSession, full: bool) -> Vec<Vec<EmuCell>> {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if full {
        state.emu.full_rows()
    } else {
        state.emu.viewable_rows()
    }
}

fn highlighted_rows(state: &TermState, full: bool) -> Vec<Vec<EmuCell>> {
    let mut rows = if full {
        state.emu.full_rows()
    } else {
        state.emu.viewable_rows()
    };
    apply_highlight(&mut rows, state.highlight.as_ref(), full);
    rows
}

fn apply_highlight(rows: &mut [Vec<EmuCell>], highlight: Option<&TextHighlight>, full: bool) {
    let Some(highlight) = highlight else {
        return;
    };
    let row_offset = if full { 0 } else { highlight.viewport_offset };
    for &(x, absolute_y) in &highlight.cells {
        let Some(y) = absolute_y.checked_sub(row_offset) else {
            continue;
        };
        if let Some(cell) = rows.get_mut(y).and_then(|row| row.get_mut(x)) {
            cell.attrs.toggle(Attrs::INVERSE);
        }
    }
}

fn text_of(rows: &[Vec<EmuCell>]) -> String {
    rows_to_strings(rows)
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn dispatch(
    session: &mut TerminalSession,
    operation: Operation,
    input: &mut Option<InputDetails>,
) -> Result<OperationResult, TuiTestError> {
    match operation {
        Operation::State => Ok(OperationResult::State(Box::new(state(session)))),
        Operation::Text { full } => Ok(OperationResult::Text(text_of(&grid(session, full)))),
        Operation::PackedScreen { full } => {
            Ok(OperationResult::PackedScreen(packed_screen(session, full)))
        }
        Operation::Cells { x, y, w, h } => Ok(OperationResult::Cells(cells(session, x, y, w, h))),
        Operation::GetCommand => Ok(OperationResult::Command(
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tracker
                .last_command()
                .map(str::to_string),
        )),
        Operation::GetOutput => Ok(OperationResult::Output(
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tracker
                .last_output()
                .map(str::to_string),
        )),
        Operation::GetExitCode => Ok(OperationResult::ExitCode(
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tracker
                .last_exit(),
        )),
        Operation::GetCwd => Ok(OperationResult::Cwd(
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tracker
                .cwd()
                .map(str::to_string),
        )),
        Operation::GetTitle => Ok(OperationResult::Title(title_of(session))),
        Operation::GetClipboard => Ok(OperationResult::Clipboard(get_clipboard(session)?)),
        Operation::GetCursor => {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(OperationResult::Cursor(cursor_model(state.emu.as_ref())))
        }
        Operation::GetColors => {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(OperationResult::Colors(colors_of(
                state.emu.as_ref(),
                &state.profile,
            )))
        }
        Operation::ExpectColors {
            foreground,
            background,
            cursor,
            palette,
            timeout_ms,
        } => {
            expect_colors(
                session,
                foreground.as_deref(),
                background.as_deref(),
                cursor.as_deref(),
                &palette,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::GetModes => {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(OperationResult::Modes(modes_of(state.emu.as_ref())))
        }
        Operation::ExpectMode {
            mode,
            enabled,
            timeout_ms,
        } => {
            expect_mode(
                session,
                &mode,
                enabled,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::ExpectCursor {
            visible,
            shape,
            x,
            y,
            timeout_ms,
        } => {
            expect_cursor(
                session,
                visible,
                shape.as_deref(),
                x,
                y,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::GetSize => {
            let (cols, rows) = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .emu
                .size();
            Ok(OperationResult::Size(Size { cols, rows }))
        }
        Operation::GetBellCount => Ok(OperationResult::BellCount(session.bells.count())),
        Operation::GetBellEvents => {
            Ok(OperationResult::BellEvents(session.bells.snapshot().events))
        }
        Operation::Write { data } => {
            write_input(session, data.as_bytes(), input, None)?;
            Ok(OperationResult::Unit)
        }
        Operation::Submit { data } => {
            let mut bytes = data.unwrap_or_default().into_bytes();
            let enter = session
                .shell
                .map(|shell| shell.return_char())
                .unwrap_or("\r");
            bytes.extend_from_slice(enter.as_bytes());
            write_input(session, &bytes, input, None)?;
            Ok(OperationResult::Unit)
        }
        Operation::Key { keys, action } => {
            key_action(session, keys, action, input)?;
            Ok(OperationResult::Unit)
        }
        Operation::Mouse { action } => {
            mouse_action(session, action, input)?;
            Ok(OperationResult::Unit)
        }
        Operation::Resize { cols, rows } => {
            act(session.resize(cols, rows))?;
            Ok(OperationResult::Unit)
        }
        Operation::Signal { name } => {
            act(session
                .pty
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .signal(&name))?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitTitle {
            text,
            regex,
            timeout_ms,
            not,
        } => {
            wait_title(
                session,
                &text,
                regex,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
                not,
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitClipboard { timeout_ms } => {
            wait_clipboard_change(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitClipboardMatch {
            pattern,
            timeout_ms,
        } => {
            wait_clipboard_match(
                session,
                &pattern,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitIdle { timeout_ms } => {
            wait_idle(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Idle)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitCommand { timeout_ms } => {
            wait_command(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Command)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitExit { timeout_ms } => {
            wait_exit(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Exit)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitReady { timeout_ms } => {
            wait_ready(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Ready)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::WaitBell { timeout_ms } => {
            wait_bell(
                session,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::FindLocator { query } => Ok(OperationResult::Matches(find_locator(
            session, &query, false,
        )?)),
        Operation::ResolveLocator { query } => Ok(OperationResult::Matches(find_locator(
            session, &query, true,
        )?)),
        Operation::WaitLocator {
            query,
            not,
            timeout_ms,
        } => {
            wait_locator(
                session,
                &query,
                not,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::ClickLocator {
            query,
            options,
            clicks,
            timeout_ms,
        } => {
            click_locator(
                session,
                &query,
                options,
                clicks,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
                input,
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::HighlightLocator { query, timeout_ms } => {
            Ok(OperationResult::Matches(highlight_locator(
                session,
                &query,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?))
        }
        Operation::ExpectTitle {
            text,
            regex,
            not,
            timeout_ms,
        } => {
            expect_title(
                session,
                &text,
                regex,
                not,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::ExpectExitCode { code, timeout_ms } => {
            expect_exit_code(
                session,
                code,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Command)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::ExpectOutput { text, regex } => {
            expect_output(session, &text, regex)?;
            Ok(OperationResult::Unit)
        }
        Operation::ExpectBellCount { count, timeout_ms } => {
            expect_bell_count(
                session,
                count,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
        }
        Operation::Snapshot {
            name,
            update,
            include_style,
            include_title,
            cwd,
        } => Ok(OperationResult::Snapshot(do_snapshot(
            session,
            &name,
            update,
            include_style,
            include_title,
            cwd,
        )?)),
        Operation::Screenshot {
            full,
            path,
            zoom,
            background,
        } => Ok(OperationResult::Screenshot(screenshot(
            session, full, path, zoom, background,
        )?)),
        Operation::StartRecording {
            path,
            format,
            fps,
            speed,
            idle_time_limit,
            zoom,
            background,
        } => {
            session.start_recording(crate::session::ManualRecordingOptions {
                path,
                format,
                fps,
                speed,
                idle_time_limit,
                zoom,
                background,
            })?;
            Ok(OperationResult::Unit)
        }
        Operation::StopRecording => Ok(OperationResult::Recording(session.stop_recording()?)),
        Operation::Open(_)
        | Operation::Run(_)
        | Operation::Restart { .. }
        | Operation::Close
        | Operation::FinishTrace { .. } => {
            Err(TuiTestError::internal("unsupported nested operation"))
        }
    }
}

fn act(result: anyhow::Result<()>) -> Result<(), TuiTestError> {
    result.map_err(|error| TuiTestError::internal(error.to_string()))
}

fn state(session: &TerminalSession) -> crate::api::State {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (cols, rows) = state.emu.size();
    let bells = session.bells.snapshot();
    crate::api::State {
        session_shell: session.shell.map(|value| value.as_str().to_string()),
        cols,
        rows,
        cursor: cursor_model(state.emu.as_ref()),
        title: state.emu.title(),
        cwd: state.tracker.cwd().map(str::to_string),
        last_command: state.tracker.last_command().map(str::to_string),
        last_exit: state.tracker.last_exit(),
        exited: state.exited,
        ready: state.tracker.is_ready(),
        bell_count: bells.count,
        modes: crate::terminal::emu::TerminalMode::ALL
            .into_iter()
            .map(|mode| (mode.name().to_string(), state.emu.mode(mode)))
            .collect(),
        mouse_mode: state.mouse_mode.mode().name().to_string(),
        colors: colors_of(state.emu.as_ref(), &state.profile),
        timeouts: effective_timeouts(session),
        text: text_of(&state.emu.viewable_rows()),
    }
}

fn effective_timeouts(session: &TerminalSession) -> EffectiveTimeouts {
    use config::TimeoutClass::*;
    EffectiveTimeouts {
        text: session.timeout_for(Text),
        idle: session.timeout_for(Idle),
        command: session.timeout_for(Command),
        exit: session.timeout_for(Exit),
        ready: session.timeout_for(Ready),
    }
}

fn packed_screen(session: &TerminalSession, full: bool) -> PackedScreen {
    let rows = grid(session, full);
    PackedScreen {
        cols: session.cols,
        rows: rows.len().min(u16::MAX as usize) as u16,
        utf8: rows_to_strings(&rows).join("\n").into_bytes(),
    }
}

fn cells(session: &TerminalSession, x: u16, y: u16, w: u16, h: u16) -> Vec<Cell> {
    let rows = viewable(session);
    let mut out = Vec::new();
    for row in y..y.saturating_add(h.max(1)) {
        for col in x..x.saturating_add(w.max(1)) {
            if let Some(cell) = rows
                .get(row as usize)
                .and_then(|line| line.get(col as usize))
            {
                out.push(cell_model(col, row, cell));
            }
        }
    }
    out
}

fn cell_model(x: u16, y: u16, cell: &EmuCell) -> Cell {
    Cell {
        x,
        y,
        char: cell.ch.to_string(),
        fg: cell_color(cell.fg),
        bg: cell_color(cell.bg),
        bold: cell.has(Attrs::BOLD),
        dim: cell.has(Attrs::DIM),
        italic: cell.has(Attrs::ITALIC),
        inverse: cell.has(Attrs::INVERSE),
        invisible: cell.has(Attrs::INVISIBLE),
        strike: cell.has(Attrs::STRIKE),
        blink: cell.has(Attrs::BLINK),
        underline: cell.underline.is_underlined(),
        underline_style: cell.underline.name().to_string(),
        underline_color: cell_color(cell.underline_color),
        link: cell.uri().unwrap_or_default().to_string(),
        link_id: cell
            .hyperlink
            .as_ref()
            .and_then(|link| link.id.as_deref())
            .unwrap_or_default()
            .to_string(),
    }
}

/// Resolve a mode name, so an unknown one is a usage error naming the set
/// rather than a silent `false`.
fn parse_mode(name: &str) -> Result<TerminalMode, TuiTestError> {
    TerminalMode::ALL
        .into_iter()
        .find(|mode| mode.name() == name)
        .ok_or_else(|| {
            let known = TerminalMode::ALL
                .iter()
                .map(|mode| mode.name())
                .collect::<Vec<_>>()
                .join(", ");
            TuiTestError::usage(format!(
                "unknown terminal mode '{name}'; expected one of: {known}"
            ))
        })
}

/// The terminal's colors, resolved through the profile so a slot nothing
/// has overridden still has an answer.
///
/// The three defaults (`OSC 10/11/12`) are always reported. Palette entries
/// (`OSC 4`) are reported only where they differ from the profile, so the
/// answer names what a program changed instead of all 256 slots.
fn colors_of(emu: &dyn Emulator, profile: &crate::profile::Profile) -> crate::api::TerminalColors {
    let colors = emu.colors();
    crate::api::TerminalColors {
        foreground: colors.foreground.to_hex(),
        background: colors.background.to_hex(),
        cursor: colors.cursor.to_hex(),
        palette: colors
            .palette
            .iter()
            .enumerate()
            .filter_map(|(index, now)| {
                let index = index as u8;
                (*now != profile.colors.rgb(index)).then(|| (index, now.to_hex()))
            })
            .collect(),
    }
}

/// Resolve a color a caller named into a concrete value.
///
/// Takes the same spellings `--fg` and `--bg` do, minus `default`: a terminal
/// color is what `default` would resolve *to*, so there is nothing for it to
/// refer to. An ANSI index resolves through the session's own palette, so
/// `--background 0` means the black this profile paints rather than a fixed
/// one.
fn resolve_expected_color(
    spec: &str,
    emu: &dyn Emulator,
) -> Result<crate::profile::Rgb, TuiTestError> {
    use crate::assert::color::Expected;
    use crate::profile::ColorSlot;
    // Not the parse error itself: it offers `default` as a spelling, which the
    // next arm rejects, so a typo would be answered with advice that fails.
    let invalid = || {
        TuiTestError::usage(format!(
            "terminal color must be ansi256 (0-255), hex (#rrggbb), or rgb (r,g,b) (got: {spec:?})"
        ))
    };
    match Expected::parse(spec).map_err(|_| invalid())? {
        Expected::Default => Err(TuiTestError::usage(
            "'default' has no meaning for a terminal color; name a hex value or an ANSI index"
                .to_string(),
        )),
        Expected::Ansi256(index) => Ok(emu.color(ColorSlot::Indexed(index))),
        Expected::Hex(r, g, b) | Expected::Rgb(r, g, b) => Ok(crate::profile::Rgb::new(r, g, b)),
    }
}

/// Wait for the terminal's colors to match every slot the caller named.
///
/// The three defaults (`OSC 10/11/12`) and any number of palette entries
/// (`OSC 4`) are matched together, so a program that recolors several slots
/// at once is asserted as a single state rather than a race between polls.
fn expect_colors(
    session: &TerminalSession,
    foreground: Option<&str>,
    background: Option<&str>,
    cursor: Option<&str>,
    palette: &[(u8, String)],
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    use crate::profile::ColorSlot;
    if foreground.is_none() && background.is_none() && cursor.is_none() && palette.is_empty() {
        return Err(TuiTestError::usage(
            "expect colors needs at least one of --foreground, --background, --cursor, or --palette",
        ));
    }
    // Resolve once, so a malformed color is a usage error rather than a wait
    // that can never succeed, and an indexed color is read against the
    // palette in force now rather than re-resolved on every poll.
    let (wanted, wanted_palette) = {
        let state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let resolve = |spec: Option<&str>| -> Result<Option<crate::profile::Rgb>, TuiTestError> {
            spec.map(|spec| resolve_expected_color(spec, state.emu.as_ref()))
                .transpose()
        };
        let defaults = [resolve(foreground)?, resolve(background)?, resolve(cursor)?];
        let entries = palette
            .iter()
            .map(|(index, spec)| Ok((*index, resolve_expected_color(spec, state.emu.as_ref())?)))
            .collect::<Result<Vec<_>, TuiTestError>>()?;
        (defaults, entries)
    };

    let mut matched = false;
    let mut last = None;
    poll_until(
        || {
            let (actual, actual_palette) = {
                let state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let defaults = [
                    state.emu.color(ColorSlot::Foreground),
                    state.emu.color(ColorSlot::Background),
                    state.emu.color(ColorSlot::Cursor),
                ];
                let entries = wanted_palette
                    .iter()
                    .map(|(index, _)| (*index, state.emu.color(ColorSlot::Indexed(*index))))
                    .collect::<Vec<_>>();
                (defaults, entries)
            };
            matched = wanted
                .iter()
                .zip(actual)
                .all(|(expected, actual)| expected.is_none_or(|expected| expected == actual))
                && wanted_palette
                    .iter()
                    .zip(&actual_palette)
                    .all(|((_, expected), (_, actual))| expected == actual);
            last = Some((actual, actual_palette));
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        return Ok(());
    }
    let ([fg, bg, cur], entries) = last.expect("the colors are read at least once");
    // Only the slots that were asked about: naming a foreground the caller
    // never mentioned invites reading it as the thing that failed.
    let mut parts = Vec::new();
    for (label, wanted, actual) in [
        ("foreground", wanted[0], fg),
        ("background", wanted[1], bg),
        ("cursor", wanted[2], cur),
    ] {
        if let Some(wanted) = wanted {
            parts.push(format!(
                "{label} {} (wanted {})",
                actual.to_hex(),
                wanted.to_hex()
            ));
        }
    }
    for ((index, wanted), (_, actual)) in wanted_palette.iter().zip(&entries) {
        parts.push(format!(
            "palette {index} {} (wanted {})",
            actual.to_hex(),
            wanted.to_hex()
        ));
    }
    Err(TuiTestError::assertion(format!(
        "colors did not match within {timeout_ms}ms; {}",
        parts.join(", ")
    )))
}

fn modes_of(emu: &dyn Emulator) -> std::collections::BTreeMap<String, bool> {
    TerminalMode::ALL
        .into_iter()
        .map(|mode| (mode.name().to_string(), emu.mode(mode)))
        .collect()
}

fn expect_mode(
    session: &TerminalSession,
    name: &str,
    enabled: bool,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let mode = parse_mode(name)?;
    let reached = |session: &TerminalSession| {
        session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .emu
            .mode(mode)
            == enabled
    };
    let mut matched = false;
    poll_until(
        || {
            matched = reached(session);
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        return Ok(());
    }
    Err(TuiTestError::assertion(format!(
        "{} did not turn {} within {timeout_ms}ms",
        mode.name(),
        if enabled { "on" } else { "off" }
    )))
}

fn expect_cursor(
    session: &TerminalSession,
    visible: Option<bool>,
    shape: Option<&str>,
    x: Option<u16>,
    y: Option<u16>,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    if let Some(shape) = shape {
        if CursorShape::parse(shape).is_none() {
            return Err(TuiTestError::usage(format!(
                "unknown cursor shape '{shape}'; expected block, underline, or bar"
            )));
        }
    }
    let mut last = None;
    let mut matched = false;
    poll_until(
        || {
            let cursor = {
                let state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                cursor_model(state.emu.as_ref())
            };
            matched = visible.is_none_or(|want| want == cursor.visible)
                && shape.is_none_or(|want| want == cursor.shape)
                && x.is_none_or(|want| want == cursor.x)
                && y.is_none_or(|want| want == cursor.y);
            last = Some(cursor);
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        return Ok(());
    }
    let cursor = last.expect("the cursor is read at least once");
    Err(TuiTestError::assertion(format!(
        "cursor did not match within {timeout_ms}ms; it is at {},{}, {}, shape {}",
        cursor.x,
        cursor.y,
        if cursor.visible { "visible" } else { "hidden" },
        cursor.shape
    )))
}

/// The cursor as a caller sees it: where it is, and how it is drawn.
fn cursor_model(emu: &dyn Emulator) -> Cursor {
    let (x, y) = emu.cursor();
    Cursor {
        x,
        y,
        visible: emu.cursor_visible(),
        shape: emu.cursor_shape().name().to_string(),
        color: emu.color(crate::profile::ColorSlot::Cursor).to_hex(),
    }
}

pub(crate) fn cell_color(color: Option<Color>) -> CellColor {
    match color {
        None => CellColor::Default,
        Some(Color::Rgb(r, g, b)) => CellColor::Rgb(r, g, b),
        Some(color) => CellColor::Indexed(color.to_index()),
    }
}

fn write_input(
    session: &TerminalSession,
    bytes: &[u8],
    input: &mut Option<InputDetails>,
    position: Option<(u16, u16)>,
) -> Result<(), TuiTestError> {
    act(session.write(bytes))?;
    if let Some(input) = input {
        input.record_sent(bytes, position);
    }
    Ok(())
}

fn key_action(
    session: &TerminalSession,
    tokens: Vec<String>,
    action: crate::api::KeyAction,
    input: &mut Option<InputDetails>,
) -> Result<(), TuiTestError> {
    // A backend with its own key encoder is preferred, per token, because it
    // reads terminal state the shared encoder does not model: ghostty's
    // applies keypad application mode, `modifyOtherKeys`, and the alt-escape
    // prefix. A backend that has no encoder, or cannot express a particular
    // event, answers `None` and that token falls back.
    let mut sequence: Vec<u8> = Vec::new();
    {
        let state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let modes = keys::InputModes {
            keyboard: state.emu.keyboard_mode(),
            cursor_key_application: state.emu.cursor_key_application(),
        };
        for token in &tokens {
            let presses = keys::token_to_presses(token, action)
                .map_err(|error| TuiTestError::usage(error.to_string()))?;
            let encoded: Option<Vec<Vec<u8>>> = presses
                .iter()
                .map(|press| state.emu.encode_key(press))
                .collect();
            match encoded {
                // All or nothing per token: a token half-encoded by the
                // backend and half by the fallback would interleave two
                // encodings of the same keypress.
                Some(parts) => sequence.extend(parts.concat()),
                None => {
                    let text = keys::token_to_seq_for_action_with_mode(token, action, modes)
                        .map_err(|error| TuiTestError::usage(error.to_string()))?;
                    sequence.extend_from_slice(text.as_bytes());
                }
            }
        }
    }
    if sequence.is_empty() {
        if let Some(input) = input {
            input.record_sent(&[], None);
        }
        Ok(())
    } else {
        write_input(session, &sequence, input, None)
    }
}

fn mouse_action(
    session: &TerminalSession,
    action: crate::api::MouseAction,
    input: &mut Option<InputDetails>,
) -> Result<(), TuiTestError> {
    let position;
    let sequence = match action {
        crate::api::MouseAction::Click {
            x,
            y,
            on_text,
            options,
            clicks,
        } => {
            let (x, y) = if let Some(text) = on_text {
                locate_center(session, &text).ok_or_else(|| {
                    TuiTestError::assertion(format!("text not found on screen: {text}"))
                })?
            } else {
                (x.unwrap_or(0), y.unwrap_or(0))
            };
            position = (x, y);
            let mut out = String::new();
            for _ in 0..clicks.max(1) {
                out.push_str(&mouse::click(x, y, options));
            }
            out
        }
        crate::api::MouseAction::Move { x, y } => {
            position = (x, y);
            mouse::motion(x, y)
        }
        crate::api::MouseAction::Down { x, y, options } => {
            position = (x, y);
            mouse::down(x, y, options)
        }
        crate::api::MouseAction::Up { x, y, options } => {
            position = (x, y);
            mouse::up(x, y, options)
        }
        crate::api::MouseAction::Drag {
            x1,
            y1,
            x2,
            y2,
            options,
        } => {
            position = (x2, y2);
            format!(
                "{}{}{}",
                mouse::down(x1, y1, options),
                mouse::drag_motion(x2, y2, options),
                mouse::up(x2, y2, options)
            )
        }
        crate::api::MouseAction::Scroll { direction, amount } => {
            position = (0, 0);
            let up = direction.eq_ignore_ascii_case("up");
            (0..amount.max(1))
                .map(|_| mouse::scroll(0, 0, up))
                .collect()
        }
    };
    write_input(session, sequence.as_bytes(), input, Some(position))
}

fn locate_center(session: &TerminalSession, text: &str) -> Option<(u16, u16)> {
    let mut query = LocatorQuery::text(text);
    query.occurrence = crate::api::MatchOccurrence::First;
    let evaluated = evaluate_locator(session, &query, false).ok()?;
    matched_center(evaluated.evaluation.matches.first()?)
        .and_then(|(x, y)| Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
}

fn poll_until<F: FnMut() -> bool>(mut predicate: F, timeout_ms: u64) -> bool {
    let start = Instant::now();
    loop {
        if predicate() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(timeout_ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn session_stopped(session: &TerminalSession) -> bool {
    session.cancelled.load(std::sync::atomic::Ordering::Acquire)
        || session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exited
            .is_some()
}

/// The window title the terminal is currently reporting.
fn title_of(session: &TerminalSession) -> Option<String> {
    session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .emu
        .title()
}

fn clipboard_error(error: anyhow::Error) -> TuiTestError {
    TuiTestError::internal(error.to_string())
}

fn get_clipboard(session: &TerminalSession) -> Result<String, TuiTestError> {
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let value = state
        .emu
        .clipboard(ClipboardType::Clipboard)
        .map_err(clipboard_error)?;
    state.observed_clipboard_revision = state
        .emu
        .clipboard_revision(ClipboardType::Clipboard)
        .map_err(clipboard_error)?;
    Ok(value)
}

fn wait_clipboard_match(
    session: &TerminalSession,
    pattern: &ClipboardPattern,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let mut matched = false;
    let mut read_error = None;
    poll_until(
        || {
            let mut state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let value = state
                .emu
                .clipboard(ClipboardType::Clipboard)
                .map_err(clipboard_error);
            let revision = state
                .emu
                .clipboard_revision(ClipboardType::Clipboard)
                .map_err(clipboard_error);
            match (value, revision) {
                (Ok(value), Ok(revision)) if pattern.matches(&value) => {
                    state.observed_clipboard_revision = revision;
                    matched = true;
                }
                (Err(error), _) | (_, Err(error)) => read_error = Some(error),
                _ => {}
            }
            drop(state);
            matched || read_error.is_some() || session_stopped(session)
        },
        timeout_ms,
    );
    if let Some(error) = read_error {
        Err(error)
    } else if matched {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before the clipboard matched '{}'",
            pattern.as_str()
        )))
    } else {
        Err(TuiTestError::assertion(format!(
            "wait clipboard: timed out after {} waiting for '{}'",
            format_timeout(timeout_ms),
            pattern.as_str()
        )))
    }
}

fn wait_clipboard_change(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    let baseline = {
        let mut state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = state
            .emu
            .clipboard_revision(ClipboardType::Clipboard)
            .map_err(clipboard_error)?;
        if current != state.observed_clipboard_revision {
            state.observed_clipboard_revision = current;
            return Ok(());
        }
        current
    };
    let mut changed = false;
    let mut read_error = None;
    poll_until(
        || {
            let mut state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match state
                .emu
                .clipboard_revision(ClipboardType::Clipboard)
                .map_err(clipboard_error)
            {
                Ok(current) if current != baseline => {
                    state.observed_clipboard_revision = current;
                    changed = true;
                }
                Ok(_) => {}
                Err(error) => read_error = Some(error),
            }
            drop(state);
            changed || read_error.is_some() || session_stopped(session)
        },
        timeout_ms,
    );
    if let Some(error) = read_error {
        Err(error)
    } else if changed {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(
            "session exited before the clipboard changed",
        ))
    } else {
        Err(TuiTestError::assertion(format!(
            "wait clipboard: timed out after {} without a change",
            format_timeout(timeout_ms)
        )))
    }
}

/// Whether the title matches now. An unset title matches nothing, so `--not`
/// on a session that never set one succeeds.
fn title_matches(session: &TerminalSession, pattern: &Pattern) -> bool {
    title_of(session).is_some_and(|title| pattern.matches(&title))
}

fn wait_title(
    session: &TerminalSession,
    text: &str,
    regex: bool,
    timeout_ms: u64,
    not: bool,
) -> Result<(), TuiTestError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| TuiTestError::usage(format!("invalid regex: {error}")))?;
    let mut matched = false;
    poll_until(
        || {
            matched = title_matches(session, &pattern) != not;
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before the title '{}' became {}",
            pattern.describe(),
            if not { "hidden" } else { "visible" }
        )))
    } else {
        let expected = pattern.describe();
        // Share immutable history until a mismatch needs public diagnostics. The
        // compared grid and its history stay pinned even if output advances during I/O.
        let observation = capture_failure_observation(session);
        let actual = observation.title.clone();
        let message =
            title_timeout_message_from_actual(actual.as_deref(), &expected, timeout_ms, not);
        let mut error = comparison_failure(
            "wait.title",
            Some(timeout_ms),
            FailureReason::TimedOut,
            message,
            "title",
            Some(expected),
            actual,
        );
        error.observation = Some(Box::new(observation));
        Err(error)
    }
}

fn expect_title(
    session: &TerminalSession,
    text: &str,
    regex: bool,
    not: bool,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| TuiTestError::usage(format!("invalid regex: {error}")))?;
    let mut matched = false;
    poll_until(
        || {
            matched = title_matches(session, &pattern) != not;
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before the title '{}' became {}",
            pattern.describe(),
            if not { "hidden" } else { "visible" }
        )))
    } else {
        let expected = pattern.describe();
        let observation = capture_failure_observation(session);
        let actual = observation.title.clone();
        let message =
            title_timeout_message_from_actual(actual.as_deref(), &expected, timeout_ms, not);
        let mut error = comparison_failure(
            "expect.title",
            Some(timeout_ms),
            FailureReason::TimedOut,
            message,
            "title",
            Some(expected),
            actual,
        );
        error.observation = Some(Box::new(observation));
        Err(error)
    }
}

fn wait_idle(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    let quiet = Duration::from_millis(250);
    if poll_until(
        || {
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last_change
                .elapsed()
                >= quiet
                || session.cancelled.load(std::sync::atomic::Ordering::Acquire)
        },
        timeout_ms,
    ) {
        Ok(())
    } else {
        Err(TuiTestError::assertion(
            "wait idle: screen kept changing until timeout",
        ))
    }
}

fn awaiting_command_start(state: &TermState) -> bool {
    state
        .awaiting_start
        .is_some_and(|seen| state.tracker.started_count() == seen)
}

fn command_settled(session: &TerminalSession, baseline: u64) -> bool {
    const QUIET: Duration = Duration::from_millis(300);
    if session.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return true;
    }
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.exited.is_some() {
        return true;
    }
    let tracker = &state.tracker;
    if !tracker.started() {
        return state.last_change.elapsed() >= QUIET;
    }
    if awaiting_command_start(&state) {
        return false;
    }
    tracker.finished_count() > baseline || !tracker.executing()
}

fn wait_command(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    let baseline = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .finished_count();
    if poll_until(|| command_settled(session, baseline), timeout_ms) {
        Ok(())
    } else {
        Err(TuiTestError::assertion(format!(
            "wait command: timed out after {timeout_ms}ms; {}",
            stall_reason(session)
        )))
    }
}

fn stall_reason(session: &TerminalSession) -> String {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if awaiting_command_start(&state) {
        "the shell never started a command for the input that was sent, so there \
         is nothing to wait for (was the line submitted?)"
            .to_string()
    } else {
        "the command was still running".to_string()
    }
}

fn wait_exit(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    let start = Instant::now();
    loop {
        let (exited, exit_error) = {
            let state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.exited.is_some(), state.exit_error.clone())
        };
        if exited || session.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        if let Some(error) = exit_error {
            return Err(TuiTestError::internal(format!(
                "wait exit: failed to query process status: {error}"
            )));
        }
        if start.elapsed() >= Duration::from_millis(timeout_ms) {
            return Err(TuiTestError::assertion(
                "wait exit: session still running at timeout",
            ));
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn wait_ready(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    if await_ready(session, timeout_ms) {
        Ok(())
    } else {
        Err(TuiTestError::assertion(
            "wait ready: no prompt was reported within timeout",
        ))
    }
}

fn wait_bell(session: &TerminalSession, timeout_ms: u64) -> Result<(), TuiTestError> {
    let baseline = session.bells.sequence();
    let mut rang = false;
    poll_until(
        || {
            rang = session.bells.sequence() != baseline;
            rang || session_stopped(session)
        },
        timeout_ms,
    );
    if rang {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(
            "session exited before a bell was received",
        ))
    } else {
        Err(TuiTestError::assertion(format!(
            "wait bell: timed out after {timeout_ms}ms without receiving a bell"
        )))
    }
}

fn validate_locator_query(query: &LocatorQuery) -> Result<(), TuiTestError> {
    validate_locator_node(query, 0, &mut 0)
}

fn validate_locator_node(
    query: &LocatorQuery,
    depth: usize,
    count: &mut usize,
) -> Result<(), TuiTestError> {
    *count += 1;
    if depth >= 64 || *count > 4096 {
        return Err(TuiTestError::usage(
            "locator expression exceeds the size or depth limit",
        ));
    }
    if query.within.is_none() && query.direction != crate::api::LocatorDirection::Within {
        return Err(TuiTestError::usage(
            "locator direction requires a preceding locator",
        ));
    }
    match &query.selector {
        LocatorSelector::Text(selector) => validate_selector(selector)?,
        LocatorSelector::Style(selector) => {
            if selector.style.is_empty() {
                return Err(TuiTestError::usage(
                    "getByStyle requires at least one style property",
                ));
            }
            validate_style(&selector.style)?;
        }
        LocatorSelector::Link(_) => {}
        LocatorSelector::And { .. }
        | LocatorSelector::Or { .. }
        | LocatorSelector::Filter { .. } => {
            if query.within.is_some() || !query.style.is_empty() {
                return Err(TuiTestError::usage(
                    "composition nodes do not accept scope or style fields",
                ));
            }
            if let LocatorSelector::Filter {
                has: None,
                has_not: None,
                ..
            } = &query.selector
            {
                return Err(TuiTestError::usage("filter requires has or hasNot"));
            }
            for child in query.selector.children() {
                validate_locator_node(child, depth + 1, count)?;
            }
        }
    }
    if let Some(parent) = query.within.as_deref() {
        validate_locator_node(parent, depth + 1, count)?;
    }
    validate_style(&query.style)?;
    Ok(())
}

struct EvaluatedLocator {
    evaluation: locator::LocatorEvaluation,
    screen_sequence: u64,
    visible_rows: usize,
}

fn evaluate_locator_in_state_with_requirement(
    state: &mut TermState,
    query: &LocatorQuery,
    require_one: bool,
) -> anyhow::Result<EvaluatedLocator> {
    let screen_sequence = capture_visual_state(state, true);
    let visible_rows = state.emu.viewable_rows();
    let visible_len = visible_rows.len();
    let full = query.uses_full_grid();
    let rows = if full {
        state.emu.full_rows()
    } else {
        visible_rows
    };
    let mut evaluation = locator::evaluate_query(
        &rows,
        query,
        require_one,
        &mut |cell, style, x, y, budget| {
            evaluate_cell_style(cell, style, state.emu.as_ref(), x, y, budget)
        },
    )?;
    if full {
        evaluation.diagnostics.viewport_origin_y = rows
            .len()
            .saturating_sub(visible_len)
            .min(u32::MAX as usize) as u32;
    }
    Ok(EvaluatedLocator {
        evaluation,
        screen_sequence,
        visible_rows: visible_len,
    })
}

fn evaluate_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    require_one: bool,
) -> Result<EvaluatedLocator, TuiTestError> {
    validate_locator_query(query)?;
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    evaluate_locator_in_state_with_requirement(&mut state, query, require_one)
        .map_err(|error| TuiTestError::assertion(error.to_string()))
}

fn evaluate_locator_with_observation(
    session: &TerminalSession,
    query: &LocatorQuery,
    require_one: bool,
) -> Result<(EvaluatedLocator, FailureObservation), TuiTestError> {
    validate_locator_query(query)?;
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let evaluated = evaluate_locator_in_state_with_requirement(&mut state, query, require_one)
        .map_err(|error| TuiTestError::assertion(error.to_string()))?;
    let observation = capture_failure_observation_locked(session, &mut state);
    Ok((evaluated, observation))
}

fn find_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    require_one: bool,
) -> Result<Vec<TextMatch>, TuiTestError> {
    validate_locator_query(query)?;
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let evaluated = evaluate_locator_in_state_with_requirement(&mut state, query, require_one)
        .map_err(|error| TuiTestError::assertion(error.to_string()))?;
    let failure = evaluated.evaluation.diagnostics.failure_reason;
    if matches!(
        failure,
        Some(LocatorFailureReason::Ambiguous | LocatorFailureReason::AnchorAmbiguous)
    ) || (require_one && evaluated.evaluation.matches.len() != 1)
    {
        let observation = capture_failure_observation_locked(session, &mut state);
        drop(state);
        let message = locator_failure_message(query, &evaluated.evaluation.diagnostics);
        return Err(locator_failure_error(
            if require_one {
                "locator.location"
            } else {
                "locator.find"
            },
            None,
            message,
            evaluated,
            Vec::new(),
            false,
            Some(observation),
        ));
    }
    drop(state);
    Ok(evaluated
        .evaluation
        .matches
        .into_iter()
        .map(|matched| matched.value)
        .collect())
}

fn wait_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    not: bool,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    validate_locator_query(query)?;
    let description = query.selector.description();
    let started = Instant::now();
    let mut transitions = Vec::new();
    let mut last_signature = None;
    loop {
        let evaluated = evaluate_locator(session, query, false)?;
        let ambiguous = matches!(
            evaluated.evaluation.diagnostics.failure_reason,
            Some(LocatorFailureReason::Ambiguous | LocatorFailureReason::AnchorAmbiguous)
        );
        let visible = !evaluated.evaluation.matches.is_empty() && !ambiguous;
        let matched = !ambiguous && visible != not;
        push_evaluation_transition(
            &mut transitions,
            &mut last_signature,
            &evaluated,
            if ambiguous {
                "ambiguous"
            } else if visible {
                "matched"
            } else {
                "no_match"
            },
            started.elapsed().as_millis() as u64,
        );
        if matched {
            return Ok(());
        }
        if session_stopped(session) || started.elapsed() >= Duration::from_millis(timeout_ms) {
            let (final_evaluated, observation) =
                evaluate_locator_with_observation(session, query, false)?;
            let final_ambiguous = matches!(
                final_evaluated.evaluation.diagnostics.failure_reason,
                Some(LocatorFailureReason::Ambiguous | LocatorFailureReason::AnchorAmbiguous)
            );
            let final_visible = !final_evaluated.evaluation.matches.is_empty() && !final_ambiguous;
            if !final_ambiguous && final_visible != not {
                return Ok(());
            }
            push_evaluation_transition(
                &mut transitions,
                &mut last_signature,
                &final_evaluated,
                if final_ambiguous {
                    "ambiguous"
                } else if final_visible {
                    "matched"
                } else {
                    "no_match"
                },
                started.elapsed().as_millis() as u64,
            );
            let stopped = observation.process.cancelled || observation.process.exit_code.is_some();
            let message = if stopped {
                format!(
                    "session exited before '{description}' became {}",
                    if not { "hidden" } else { "visible" }
                )
            } else if matches!(
                final_evaluated.evaluation.diagnostics.failure_reason,
                Some(
                    LocatorFailureReason::Ambiguous
                        | LocatorFailureReason::AnchorAmbiguous
                        | LocatorFailureReason::AnchorNotFound
                )
            ) {
                locator_failure_message(query, &final_evaluated.evaluation.diagnostics)
            } else {
                timeout_message(&description, timeout_ms, not)
            };
            return Err(locator_failure_error(
                "locator.wait",
                Some(timeout_ms),
                message,
                final_evaluated,
                transitions,
                not,
                Some(observation),
            ));
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn push_evaluation_transition(
    transitions: &mut Vec<crate::diagnostics::EvaluationTransition>,
    last_signature: &mut Option<String>,
    evaluated: &EvaluatedLocator,
    outcome: &str,
    elapsed_ms: u64,
) {
    let stage_counts = evaluated
        .evaluation
        .diagnostics
        .stages
        .iter()
        .map(|stage| stage.selected_count)
        .collect::<Vec<_>>();
    let signature = format!(
        "{outcome}:{:?}:{stage_counts:?}",
        evaluated.evaluation.diagnostics.failure_reason
    );
    if last_signature.as_deref() == Some(signature.as_str()) {
        return;
    }
    *last_signature = Some(signature);
    transitions.push(crate::diagnostics::EvaluationTransition {
        elapsed_ms,
        screen_sequence: evaluated.screen_sequence,
        outcome: outcome.to_string(),
        stage_index: evaluated.evaluation.diagnostics.failure_stage,
        stage_counts,
    });
    if transitions.len() > 16 {
        transitions.remove(0);
    }
}

fn locator_failure_error(
    operation: &str,
    timeout_ms: Option<u64>,
    message: String,
    evaluated: EvaluatedLocator,
    transitions: Vec<crate::diagnostics::EvaluationTransition>,
    negated: bool,
    observation: Option<FailureObservation>,
) -> TuiTestError {
    let reason = if negated && !evaluated.evaluation.matches.is_empty() {
        FailureReason::UnexpectedMatch
    } else {
        match evaluated.evaluation.diagnostics.failure_reason {
            Some(LocatorFailureReason::Ambiguous) => FailureReason::LocatorAmbiguous,
            Some(LocatorFailureReason::OutsideViewport)
            | Some(LocatorFailureReason::MatchedNoCells) => FailureReason::MatchNotActionable,
            _ => FailureReason::LocatorNoMatch,
        }
    };
    let mut details = FailureReport::new(operation, timeout_ms, reason, message.clone());
    details.operation.failed_screen_sequence = evaluated.screen_sequence;
    details.locator = Some(evaluated.evaluation.diagnostics);
    details.evaluation_transitions = transitions;
    let mut error = TuiTestError::assertion(message).with_report(details);
    error.observation = observation.map(Box::new);
    error
}

#[allow(clippy::too_many_arguments)]
fn observed_comparison_failure(
    session: &TerminalSession,
    operation: &str,
    timeout_ms: Option<u64>,
    reason: FailureReason,
    message: String,
    kind: &str,
    expected: Option<String>,
    actual: Option<String>,
) -> TuiTestError {
    let mut error = comparison_failure(
        operation, timeout_ms, reason, message, kind, expected, actual,
    );
    error.observation = Some(Box::new(capture_failure_observation(session)));
    error
}

fn resolve_locator_click_point(
    session: &TerminalSession,
    query: &LocatorQuery,
    timeout_ms: u64,
) -> Result<(u16, u16), TuiTestError> {
    validate_locator_query(query)?;
    let description = query.selector.description();
    let started = Instant::now();
    let mut transitions = Vec::new();
    let mut last_signature = None;
    loop {
        let (evaluated, outcome) = {
            let mut state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let visible_len = state.emu.viewable_rows().len();
            let evaluated = evaluate_locator_in_state_with_requirement(&mut state, query, true)
                .map_err(|error| TuiTestError::assertion(error.to_string()))?;
            let full = query.uses_full_grid();
            let viewport_offset = evaluated.evaluation.diagnostics.viewport_origin_y as usize;
            let outcome = click_point_from_candidates(
                evaluated.evaluation.matches.clone(),
                &description,
                full,
                viewport_offset,
                visible_len,
            );
            (evaluated, outcome)
        };
        match outcome {
            Ok(Some(point)) => return Ok(point),
            Ok(None) => push_evaluation_transition(
                &mut transitions,
                &mut last_signature,
                &evaluated,
                "no_match",
                started.elapsed().as_millis() as u64,
            ),
            Err(_) => push_evaluation_transition(
                &mut transitions,
                &mut last_signature,
                &evaluated,
                "not_actionable",
                started.elapsed().as_millis() as u64,
            ),
        }
        if session_stopped(session) || started.elapsed() >= Duration::from_millis(timeout_ms) {
            let (mut final_evaluated, observation) =
                evaluate_locator_with_observation(session, query, true)?;
            let full = query.uses_full_grid();
            let viewport_offset = final_evaluated.evaluation.diagnostics.viewport_origin_y as usize;
            let actionability = click_point_from_candidates(
                final_evaluated.evaluation.matches.clone(),
                &description,
                full,
                viewport_offset,
                final_evaluated.visible_rows,
            );
            let actionability_error = match actionability {
                Ok(Some(point)) => return Ok(point),
                Ok(None) => None,
                Err(error) => Some(error),
            };
            let message =
                if observation.process.cancelled || observation.process.exit_code.is_some() {
                    format!("session exited before '{description}' could be clicked")
                } else if let Some(error) = actionability_error {
                    let reason = if error.message.contains("outside the visible viewport")
                        || error.message.contains("in scrollback")
                    {
                        LocatorFailureReason::OutsideViewport
                    } else {
                        LocatorFailureReason::MatchedNoCells
                    };
                    final_evaluated.evaluation.diagnostics.failure_reason = Some(reason);
                    final_evaluated.evaluation.diagnostics.failure_stage = final_evaluated
                        .evaluation
                        .diagnostics
                        .stages
                        .len()
                        .checked_sub(1);
                    error.message
                } else if matches!(
                    final_evaluated.evaluation.diagnostics.failure_reason,
                    Some(
                        LocatorFailureReason::Ambiguous
                            | LocatorFailureReason::AnchorAmbiguous
                            | LocatorFailureReason::AnchorNotFound
                    )
                ) {
                    locator_failure_message(query, &final_evaluated.evaluation.diagnostics)
                } else {
                    format!(
                        "timed out after {} waiting for exactly one '{description}' match",
                        format_timeout(timeout_ms),
                    )
                };
            return Err(locator_failure_error(
                "locator.click",
                Some(timeout_ms),
                message,
                final_evaluated,
                transitions,
                false,
                Some(observation),
            ));
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn click_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    options: crate::api::MouseOptions,
    clicks: u8,
    timeout_ms: u64,
    input: &mut Option<InputDetails>,
) -> Result<(), TuiTestError> {
    let (x, y) = resolve_locator_click_point(session, query, timeout_ms)?;
    let mut sequence = String::new();
    for _ in 0..clicks.max(1) {
        sequence.push_str(&mouse::click(x, y, options));
    }
    write_input(session, sequence.as_bytes(), input, Some((x, y)))
}

fn click_point_from_candidates(
    mut candidates: Vec<locator::LocatedMatch>,
    description: &str,
    full: bool,
    viewport_offset: usize,
    visible_rows: usize,
) -> Result<Option<(u16, u16)>, TuiTestError> {
    if candidates.len() > 1 {
        return Err(TuiTestError::assertion(format!(
            "click requires one match for '{description}', but found {}",
            candidates.len()
        )));
    }
    let Some(matched) = candidates.pop() else {
        return Ok(None);
    };
    let (x, absolute_y) = matched_center(&matched).ok_or_else(|| {
        TuiTestError::assertion(format!("'{description}' matched no terminal cells"))
    })?;
    let y = if full {
        absolute_y.checked_sub(viewport_offset).ok_or_else(|| {
            TuiTestError::assertion(format!(
                "'{description}' matched in scrollback outside the visible viewport and cannot be clicked"
            ))
        })?
    } else {
        absolute_y
    };
    if y >= visible_rows {
        return Err(TuiTestError::assertion(format!(
            "'{description}' matched outside the visible viewport and cannot be clicked"
        )));
    }
    let x = u16::try_from(x)
        .map_err(|_| TuiTestError::internal("matched column is outside terminal coordinates"))?;
    let y = u16::try_from(y)
        .map_err(|_| TuiTestError::internal("matched row is outside terminal coordinates"))?;
    Ok(Some((x, y)))
}

fn matched_center(matched: &locator::LocatedMatch) -> Option<(usize, usize)> {
    matched
        .cells
        .get(matched.cells.len() / 2)
        .map(|cell| (cell.x, cell.y))
}

fn highlight_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    timeout_ms: u64,
) -> Result<Vec<TextMatch>, TuiTestError> {
    validate_locator_query(query)?;
    let description = query.selector.description();
    let mut resolved = None;
    poll_until(
        || {
            let outcome = {
                let mut state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let full_rows = state.emu.full_rows();
                let visible_rows = state.emu.viewable_rows();
                let viewport_offset = full_rows.len().saturating_sub(visible_rows.len());
                let full = query.uses_full_grid();
                let rows = if full { &full_rows } else { &visible_rows };
                match locator::locate_query(rows, query, &mut |cell, style| {
                    cell_matches_style(cell, style, state.emu.as_ref())
                }) {
                    Ok(candidates) if candidates.is_empty() => Ok(None),
                    Ok(candidates) => {
                        let row_offset = if full { 0 } else { viewport_offset };
                        state.highlight = Some(TextHighlight {
                            cells: candidates
                                .iter()
                                .flat_map(|matched| {
                                    matched
                                        .cells
                                        .iter()
                                        .map(|cell| (cell.x, row_offset.saturating_add(cell.y)))
                                })
                                .collect(),
                            viewport_offset,
                        });
                        Ok(Some(
                            candidates
                                .into_iter()
                                .map(|matched| matched.value)
                                .collect(),
                        ))
                    }
                    Err(error) => Err(TuiTestError::assertion(error.to_string())),
                }
            };
            if let Ok(Some(matches)) = outcome {
                resolved = Some(matches);
            }
            resolved.is_some() || session_stopped(session)
        },
        timeout_ms,
    );
    if let Some(matches) = resolved {
        Ok(matches)
    } else {
        let (evaluated, observation, final_matches) = {
            let mut state = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let evaluated = evaluate_locator_in_state_with_requirement(&mut state, query, false)
                .map_err(|error| TuiTestError::assertion(error.to_string()))?;
            let final_matches = if evaluated.evaluation.matches.is_empty()
                || matches!(
                    evaluated.evaluation.diagnostics.failure_reason,
                    Some(LocatorFailureReason::Ambiguous | LocatorFailureReason::AnchorAmbiguous)
                ) {
                None
            } else {
                let full_rows = state.emu.full_rows();
                let visible_rows = state.emu.viewable_rows();
                let viewport_offset = full_rows.len().saturating_sub(visible_rows.len());
                let row_offset = if query.uses_full_grid() {
                    0
                } else {
                    viewport_offset
                };
                state.highlight = Some(TextHighlight {
                    cells: evaluated
                        .evaluation
                        .matches
                        .iter()
                        .flat_map(|matched| {
                            matched
                                .cells
                                .iter()
                                .map(|cell| (cell.x, row_offset.saturating_add(cell.y)))
                        })
                        .collect(),
                    viewport_offset,
                });
                Some(
                    evaluated
                        .evaluation
                        .matches
                        .iter()
                        .map(|matched| matched.value.clone())
                        .collect::<Vec<_>>(),
                )
            };
            let observation = capture_failure_observation_locked(session, &mut state);
            (evaluated, observation, final_matches)
        };
        if let Some(matches) = final_matches {
            return Ok(matches);
        }
        let message = if observation.process.cancelled || observation.process.exit_code.is_some() {
            format!("session exited before '{description}' could be highlighted")
        } else if matches!(
            evaluated.evaluation.diagnostics.failure_reason,
            Some(
                LocatorFailureReason::Ambiguous
                    | LocatorFailureReason::AnchorAmbiguous
                    | LocatorFailureReason::AnchorNotFound
            )
        ) {
            locator_failure_message(query, &evaluated.evaluation.diagnostics)
        } else {
            format!(
                "timed out after {} waiting for a '{description}' match to highlight",
                format_timeout(timeout_ms),
            )
        };
        Err(locator_failure_error(
            "locator.highlight",
            Some(timeout_ms),
            message,
            evaluated,
            Vec::new(),
            false,
            Some(observation),
        ))
    }
}

fn validate_selector(selector: &TextSelector) -> Result<(), TuiTestError> {
    let validate = |text: &str, regex: bool| {
        Pattern::new(text, regex)
            .map(|_| ())
            .map_err(|error| TuiTestError::usage(format!("invalid regex: {error}")))
    };
    validate(&selector.text, selector.regex)?;
    for TextAnchor { text, regex, .. } in [
        selector.scope.after.as_ref(),
        selector.scope.before.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        validate(text, *regex)?;
    }
    Ok(())
}

fn validate_style(style: &TextStyle) -> Result<(), TuiTestError> {
    for spec in [&style.foreground, &style.background, &style.underline_color]
        .into_iter()
        .flatten()
    {
        Expected::parse(spec).map_err(|error| TuiTestError::usage(error.to_string()))?;
    }
    if let Some(style) = &style.underline_style {
        if !matches!(
            style.as_str(),
            "none" | "single" | "double" | "curly" | "dotted" | "dashed"
        ) {
            return Err(TuiTestError::usage(format!(
                "invalid underline style '{style}'"
            )));
        }
    }
    Ok(())
}

fn cell_matches_style(cell: &EmuCell, style: &TextStyle, colors: &dyn Emulator) -> bool {
    evaluate_cell_style(cell, style, colors, 0, 0, 0).matched
}

fn evaluate_cell_style(
    cell: &EmuCell,
    style: &TextStyle,
    colors: &dyn Emulator,
    x: usize,
    y: usize,
    mismatch_limit: usize,
) -> CellStyleEvaluation {
    let mut result = CellStyleEvaluation {
        matched: true,
        mismatches: Vec::new(),
        mismatches_truncated: false,
    };
    for (property, expected, actual) in [
        ("bold", style.bold, cell.has(Attrs::BOLD)),
        ("dim", style.dim, cell.has(Attrs::DIM)),
        ("italic", style.italic, cell.has(Attrs::ITALIC)),
        ("inverse", style.inverse, cell.has(Attrs::INVERSE)),
        ("hidden", style.hidden, cell.has(Attrs::INVISIBLE)),
        (
            "strikethrough",
            style.strikethrough,
            cell.has(Attrs::STRIKE),
        ),
        ("blink", style.blink, cell.has(Attrs::BLINK)),
    ] {
        if let Some(expected) = expected {
            if expected != actual
                && !result.reject(mismatch_limit, || {
                    style_mismatch(
                        cell,
                        x,
                        y,
                        property,
                        expected.to_string(),
                        actual.to_string(),
                        None,
                    )
                })
            {
                return result;
            }
        }
    }
    if let Some(expected) = style.underline_style.as_deref() {
        let actual = cell.underline.name();
        if expected != actual
            && !result.reject(mismatch_limit, || {
                style_mismatch(
                    cell,
                    x,
                    y,
                    "underline_style",
                    expected.to_string(),
                    actual.to_string(),
                    None,
                )
            })
        {
            return result;
        }
    }
    for (property, spec, actual, foreground) in [
        ("foreground", &style.foreground, cell.fg, true),
        ("background", &style.background, cell.bg, false),
        (
            "underline_color",
            &style.underline_color,
            cell.underline_color,
            true,
        ),
    ] {
        if let Some(spec) = spec {
            if let Ok(expected) = Expected::parse(spec) {
                if !color::matches(actual, &expected, colors, foreground)
                    && !result.reject(mismatch_limit, || {
                        style_mismatch(
                            cell,
                            x,
                            y,
                            property,
                            expected.describe(),
                            logical_color(actual),
                            Some(colors.resolve(actual, foreground).to_hex()),
                        )
                    })
                {
                    return result;
                }
            }
        }
    }
    result
}

fn style_mismatch(
    cell: &EmuCell,
    x: usize,
    y: usize,
    property: &str,
    expected: String,
    actual: String,
    resolved: Option<String>,
) -> CellMismatch {
    CellMismatch {
        location: crate::api::TextPosition {
            row: y.min(u32::MAX as usize) as u32,
            column: x.min(u16::MAX as usize) as u16,
        },
        grapheme: cell.ch.to_string(),
        property: property.to_string(),
        operator: "equals".to_string(),
        expected,
        actual,
        resolved,
        reason: "value_mismatch".to_string(),
    }
}

fn logical_color(color: Option<Color>) -> String {
    match color {
        None => "default".to_string(),
        Some(Color::Rgb(r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        Some(color) => color.to_index().to_string(),
    }
}

fn expect_exit_code(
    session: &TerminalSession,
    code: i32,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let baseline = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .finished_count();
    if !poll_until(|| command_settled(session, baseline), timeout_ms) {
        return Err(TuiTestError::assertion(format!(
            "expected exit code {code}: timed out after {timeout_ms}ms; {}",
            stall_reason(session)
        )));
    }
    let actual = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .last_exit();
    match actual {
        Some(actual) if actual == code => Ok(()),
        Some(actual) => Err(observed_comparison_failure(
            session,
            "expect.exit_code",
            Some(timeout_ms),
            FailureReason::ScalarMismatch,
            format!("expected exit code {code}, got {actual}"),
            "exit_code",
            Some(code.to_string()),
            Some(actual.to_string()),
        )),
        None => Err(observed_comparison_failure(
            session,
            "expect.exit_code",
            Some(timeout_ms),
            FailureReason::ScalarMismatch,
            "no command exit code tracked yet".to_string(),
            "exit_code",
            Some(code.to_string()),
            None,
        )),
    }
}

fn expect_output(session: &TerminalSession, text: &str, regex: bool) -> Result<(), TuiTestError> {
    let output = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .last_output()
        .map(str::to_string)
        .ok_or_else(|| TuiTestError::assertion("no command output tracked yet"))?;
    let matched = if regex {
        regex::Regex::new(text)
            .map_err(|error| TuiTestError::usage(format!("invalid regex: {error}")))?
            .is_match(&output)
    } else {
        output.contains(text)
    };
    if matched {
        Ok(())
    } else {
        Err(TuiTestError::assertion(format!(
            "output did not contain '{text}'\n---\n{output}\n---"
        )))
    }
}

fn expect_bell_count(
    session: &TerminalSession,
    expected: u64,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let mut actual = session.bells.count();
    poll_until(
        || {
            actual = session.bells.count();
            actual >= expected || session_stopped(session)
        },
        timeout_ms,
    );
    if actual >= expected {
        Ok(())
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited at bell count {actual} before reaching {expected}"
        )))
    } else {
        Err(observed_comparison_failure(
            session,
            "expect.bell_count",
            Some(timeout_ms),
            FailureReason::TimedOut,
            format!(
                "expected bell count {expected}: timed out after {timeout_ms}ms; current count is {actual}"
            ),
            "bell_count",
            Some(expected.to_string()),
            Some(actual.to_string()),
        ))
    }
}

fn do_snapshot(
    session: &TerminalSession,
    name: &str,
    update: bool,
    include_style: bool,
    include_title: bool,
    cwd: Option<String>,
) -> Result<SnapshotResult, TuiTestError> {
    // The title is off by default: a shell prompt routinely sets it to a
    // username, hostname, and absolute path, which would pin every baseline to
    // one machine and make it change on `cd` while the screen stayed the same.
    let observation = capture_failure_observation(session);
    let title = include_title.then(|| observation.title.clone()).flatten();
    let content = snapshot::serialize(
        &observation.rows,
        observation.cols,
        include_style,
        title.as_deref(),
    );
    let base = cwd
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    match snapshot::compare(&base, name, &content, update) {
        Ok(SnapshotStatus::Passed) => Ok(SnapshotResult::Passed),
        Ok(SnapshotStatus::Written) => Ok(SnapshotResult::Written),
        Ok(SnapshotStatus::Updated) => Ok(SnapshotResult::Updated),
        Ok(SnapshotStatus::Failed { expected, actual }) => {
            let message = format!(
                "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
            );
            let mut error = comparison_failure(
                "expect.snapshot",
                None,
                FailureReason::SnapshotMismatch,
                message,
                "snapshot",
                Some(expected),
                Some(actual),
            );
            error.observation = Some(Box::new(observation));
            Err(error)
        }
        Err(error) => Err(TuiTestError::internal(error.to_string())),
    }
}

/// Where to draw the cursor within `rows`, or `None` when the terminal is not
/// showing one.
///
/// `Emulator::cursor` is relative to the visible screen, so a full screenshot
/// has to push it down past the scrollback that precedes it.
fn cursor_in(
    rows: &[Vec<EmuCell>],
    emu: &dyn crate::terminal::emu::Emulator,
) -> Option<(u16, usize)> {
    if !emu.cursor_visible() {
        return None;
    }
    let (x, y) = emu.cursor();
    let (_, screen) = emu.size();
    // Counted in `usize`: a full render is as long as the scrollback, which a
    // profile can set past what a `u16` row would hold, and a wrapped offset
    // draws the cursor on a plausible but wrong line.
    let history = rows.len().saturating_sub(screen as usize);
    Some((x, history + y as usize))
}

struct SvgSnapshot {
    rows: Vec<Vec<EmuCell>>,
    cols: u16,
    title: Option<String>,
    cursor: Option<(u16, usize)>,
    render_state: crate::render::svg::RenderState,
}

fn svg_snapshot_from(emu: &dyn Emulator, full: bool) -> SvgSnapshot {
    let rows = if full {
        emu.full_rows()
    } else {
        emu.viewable_rows()
    };
    SvgSnapshot {
        cols: emu.size().0,
        title: emu.title(),
        cursor: cursor_in(&rows, emu),
        render_state: crate::render::svg::RenderState::capture(emu),
        rows,
    }
}

/// Capture everything the SVG renderer can observe while the emulator is
/// locked, then release the reader before doing the expensive string work.
fn svg_snapshot(session: &TerminalSession, full: bool) -> SvgSnapshot {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut snapshot = svg_snapshot_from(state.emu.as_ref(), full);
    apply_highlight(&mut snapshot.rows, state.highlight.as_ref(), full);
    snapshot
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenshotFormat {
    Svg,
    Png,
}

impl ScreenshotFormat {
    fn infer(path: &str) -> Result<Self, TuiTestError> {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            None | Some("svg") => Ok(Self::Svg),
            Some("png") => Ok(Self::Png),
            Some(extension) => Err(TuiTestError::usage(format!(
                "unsupported screenshot extension '.{extension}'; use .svg or .png"
            ))),
        }
    }
}

fn screenshot(
    session: &TerminalSession,
    full: bool,
    path: Option<String>,
    zoom: Option<f64>,
    background: Option<crate::api::CaptureBackground>,
) -> Result<ScreenshotResult, TuiTestError> {
    match path {
        Some(path) => {
            let zoom = crate::api::resolve_zoom(zoom)?;
            let format = ScreenshotFormat::infer(&path)?;
            let snapshot = svg_snapshot(session, full);
            match format {
                ScreenshotFormat::Svg => {
                    let svg = crate::render::svg::render_svg_with_zoom(
                        &snapshot.rows,
                        snapshot.cols,
                        &snapshot.render_state,
                        snapshot.cursor,
                        snapshot.title.as_deref(),
                        zoom,
                        background,
                    );
                    std::fs::write(&path, svg)
                        .map_err(|error| TuiTestError::internal(error.to_string()))?;
                }
                ScreenshotFormat::Png => {
                    #[cfg(feature = "recording-raster")]
                    {
                        let rows = snapshot.rows.len();
                        let frame = crate::record::frames::Frame {
                            grid: snapshot.rows,
                            title: snapshot.title,
                            duration: Duration::ZERO,
                            render_state: snapshot.render_state,
                            cursor: snapshot.cursor,
                        };
                        let mut renderer = crate::render::raster::GridRenderer::for_screenshot(
                            snapshot.cols,
                            rows,
                            zoom,
                            background,
                        )
                        .map_err(|error| TuiTestError::internal(error.to_string()))?;
                        crate::render::encode::encode_png(
                            std::path::Path::new(&path),
                            &frame,
                            &mut renderer,
                        )
                        .map_err(|error| TuiTestError::internal(error.to_string()))?;
                    }
                    #[cfg(not(feature = "recording-raster"))]
                    {
                        return Err(TuiTestError::usage(
                            "PNG screenshots require the tui-test 'recording-raster' feature",
                        ));
                    }
                }
            }
            Ok(ScreenshotResult::Path(path))
        }
        None if zoom.is_some() || background.is_some() => Err(TuiTestError::usage(
            "screenshot zoom and background options require an output path",
        )),
        None => Ok(ScreenshotResult::Text(text_of(&grid(session, full)))),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        AutomaticRecording, AutomaticRecordingMode, TextPosition, TextSpan, Timeouts,
    };
    use crate::profile::Profile;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{NamedColor, UnderlineStyle};
    use crate::terminal::emu::Emulator;

    fn sleeping_program(wait_ready: bool) -> RunOptions {
        let (program, args) = if cfg!(windows) {
            (
                "powershell.exe",
                vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            )
        } else {
            ("sh", vec!["-c", "sleep 30"])
        };
        let defaults = OpenOptions::default();
        RunOptions {
            program: program.into(),
            args: args.into_iter().map(str::to_string).collect(),
            backend: defaults.backend,
            profile: defaults.profile,
            cols: 80,
            rows: 24,
            cwd: None,
            env: Vec::new(),
            wait_ready: Some(wait_ready),
            restart: false,
            timeouts: crate::api::Timeouts {
                ready: Some(20),
                ..Default::default()
            },
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Disabled,
                directory: None,
            },
        }
    }

    fn populate_history(engine: &Engine) {
        let guard = engine.lock_session();
        let session = guard.as_ref().unwrap();
        let mut state = session.state.lock().unwrap();
        for index in 0..32 {
            state.emu.process(format!("\x1b[H{index:02}").as_bytes());
            state.screen_dirty = true;
            capture_visual_state(&mut state, true);
            state.screen_history.pin_current();
        }
    }

    #[test]
    fn returned_failures_release_private_observations_including_failed_open() {
        let root =
            std::env::temp_dir().join(format!("tui-test-error-memory-{}", std::process::id()));
        let context = ExecutionContext {
            artifact: Some(crate::diagnostics::FailureArtifactOptions {
                directory: root.clone(),
                mode: crate::diagnostics::FailureArtifactMode::Text,
                include_recording: false,
            }),
            ..Default::default()
        };
        let engine = Engine::new(
            "error-memory".into(),
            Arc::new(Logger::disabled()),
            root.join("unused.cast"),
        );
        engine
            .execute(Operation::Run(sleeping_program(false)))
            .unwrap();
        populate_history(&engine);
        for _ in 0..3 {
            let error = engine
                .execute_with_context(
                    Operation::WaitLocator {
                        query: LocatorQuery::text("missing diagnostic marker"),
                        not: false,
                        timeout_ms: Some(0),
                    },
                    context.clone(),
                )
                .unwrap_err();
            assert!(error.observation.is_none());
            assert!(error.details.is_some());
            assert!(error.report.is_none());
            assert!(error.artifact.as_ref().unwrap().manifest.is_some());
            let cloned = error.clone();
            assert!(cloned.observation.is_none());
            assert_eq!(cloned.details, error.details);
            assert_eq!(cloned.artifact, error.artifact);
        }
        engine.execute(Operation::Close).unwrap();

        let failed_open = engine
            .execute_with_context(Operation::Run(sleeping_program(true)), context)
            .unwrap_err();
        assert!(failed_open.observation.is_none());
        assert!(failed_open.artifact.is_some());
        let details = failed_open.details.unwrap();
        assert!(!details.summary.is_empty());
        assert!(failed_open.report.is_none());
        let report: crate::diagnostics::FailureArtifactManifest = serde_json::from_slice(
            &std::fs::read(failed_open.artifact.unwrap().manifest.unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            report.details.operation.failed_screen_sequence,
            report
                .details
                .recent_operations
                .last()
                .unwrap()
                .screen_at_return
        );
        engine.execute(Operation::Close).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_snapshots_preserve_the_compared_screen() {
        let root =
            std::env::temp_dir().join(format!("tui-test-snapshot-memory-{}", std::process::id()));
        let directory = allocate_artifact_directory(&root).unwrap();
        let engine = Engine::new(
            "snapshot-memory".into(),
            Arc::new(Logger::disabled()),
            root.join("unused.cast"),
        );
        engine
            .execute(Operation::Run(sleeping_program(false)))
            .unwrap();
        populate_history(&engine);
        {
            let guard = engine.lock_session();
            let session = guard.as_ref().unwrap();
            let cwd = Some(directory.to_string_lossy().into_owned());
            for update in [true, false] {
                let result =
                    do_snapshot(session, "compared", update, false, false, cwd.clone()).unwrap();
                assert!(matches!(
                    result,
                    SnapshotResult::Written | SnapshotResult::Passed
                ));
            }
            let captured = capture_failure_observation(session);
            {
                let mut state = session.state.lock().unwrap();
                state.emu.process(b"\x1b[HLATER OUTPUT");
                state.screen_dirty = true;
                capture_visual_state(&mut state, true);
            }
            assert!(!rows_to_strings(&captured.rows)
                .join(
                    "
"
                )
                .contains("LATER OUTPUT"));
            assert!(!captured
                .terminal()
                .screen_history
                .screens
                .last()
                .unwrap()
                .text
                .contains("LATER OUTPUT"));
            let result = do_snapshot(session, "compared", true, false, false, cwd).unwrap();
            assert!(matches!(result, SnapshotResult::Updated));
        }
        engine.execute(Operation::Close).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_open_stores_the_complete_spawn_spec_and_tracks_resize() {
        let recording_path = std::env::current_dir()
            .unwrap()
            .join(format!("restart-spec-{}.cast", std::process::id()));
        let engine = Engine::new(
            "restart-spec".to_string(),
            Arc::new(Logger::disabled()),
            recording_path,
        );
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let profile = Profile {
            scrollback: 321,
            colors: crate::profile::Colors {
                foreground: crate::profile::Rgb::new(1, 2, 3),
                ..crate::profile::Colors::default()
            },
        };
        let options = OpenOptions {
            backend: crate::Backend::Alacritty,
            shell: None,
            profile,
            cols: 87,
            rows: 29,
            cwd: Some(cwd.clone()),
            env: vec![("RESTART_SPEC".to_string(), "preserved".to_string())],
            wait_ready: Some(false),
            restart: false,
            timeouts: Timeouts {
                text: Some(11),
                idle: Some(12),
                command: Some(13),
                exit: Some(14),
                ready: Some(15),
            },
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Disabled,
                directory: None,
            },
        };

        engine
            .execute(Operation::Open(options.clone()))
            .expect("open session");
        engine
            .execute(Operation::Resize { cols: 99, rows: 31 })
            .expect("resize session");

        let stored = engine
            .spawn_spec
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("stored spawn spec");
        assert_eq!(stored.resolved_cwd, Some(PathBuf::from(&cwd)));
        let SpawnCommand::Open(stored) = stored.command else {
            panic!("expected stored open options");
        };
        assert_eq!(stored.backend, options.backend);
        assert_eq!(stored.shell, options.shell);
        assert_eq!(stored.profile, options.profile);
        assert_eq!((stored.cols, stored.rows), (99, 31));
        assert_eq!(stored.cwd, Some(cwd));
        assert_eq!(stored.env, options.env);
        assert_eq!(stored.wait_ready, options.wait_ready);
        assert_eq!(stored.timeouts, options.timeouts);
        assert_eq!(stored.recording, options.recording);

        engine.execute(Operation::Close).expect("close session");
    }

    #[test]
    fn an_svg_snapshot_freezes_grid_palette_and_cursor_together() {
        let mut emu = AlacrittyEmu::new(2, 2, &Profile::default());
        emu.process(b"X\x1b[1G\x1b]12;#010203\x07");
        let snapshot = svg_snapshot_from(&emu, false);

        // Change every piece that used to be read after the grid lock was
        // released. Rendering the captured value must still show the old
        // character, visible cursor position, shape, and color.
        emu.process(b"Y\x1b[2;2H\x1b[?25l\x1b[6 q\x1b]12;#ff00ff\x07");
        let svg = crate::render::svg::render_svg(
            &snapshot.rows,
            snapshot.cols,
            &snapshot.render_state,
            snapshot.cursor,
            snapshot.title.as_deref(),
        );

        assert_eq!(svg.matches('X').count(), 2, "text plus block redraw: {svg}");
        assert!(!svg.contains('Y'), "later grid contents leaked in: {svg}");
        assert!(
            svg.contains("#010203"),
            "captured cursor color is used: {svg}"
        );
        assert!(
            !svg.contains("#ff00ff"),
            "later cursor state must not leak in: {svg}"
        );
    }

    /// `OSC 10/11/12` move the three defaults, and each reset frees only its
    /// own slot back to the profile.
    #[test]
    fn reported_colors_follow_the_dynamic_color_sequences() {
        let profile = Profile::default();
        let mut emu = AlacrittyEmu::new(10, 2, &profile);
        let before = colors_of(&emu, &profile);

        emu.process(b"\x1b]10;#111111\x07\x1b]11;#222222\x07\x1b]12;#333333\x07");
        let set = colors_of(&emu, &profile);
        assert_eq!(set.foreground, "#111111");
        assert_eq!(set.background, "#222222");
        assert_eq!(set.cursor, "#333333");

        emu.process(b"\x1b]111\x07");
        let reset = colors_of(&emu, &profile);
        assert_eq!(
            reset.background, before.background,
            "111 restores the profile background"
        );
        assert_eq!(reset.foreground, "#111111", "and leaves the others alone");
        assert_eq!(reset.cursor, "#333333");
    }

    /// The palette is reported as what a program changed, so an untouched
    /// terminal reports nothing and `OSC 104` empties it again.
    #[test]
    fn reported_palette_names_only_the_entries_a_program_moved() {
        let profile = Profile::default();
        let mut emu = AlacrittyEmu::new(10, 2, &profile);
        assert!(
            colors_of(&emu, &profile).palette.is_empty(),
            "nothing has overridden the palette yet"
        );

        emu.process(b"\x1b]4;1;#00ff00\x07");
        assert_eq!(
            colors_of(&emu, &profile).palette,
            [(1, "#00ff00".to_string())].into_iter().collect(),
            "only the slot that moved is named"
        );

        emu.process(b"\x1b]104\x07");
        assert!(
            colors_of(&emu, &profile).palette.is_empty(),
            "104 restores the whole palette"
        );
    }

    /// A caller names a color the same way `--fg` lets them, and an index
    /// resolves through this session's palette rather than a fixed table.
    #[test]
    fn an_expected_color_accepts_hex_and_an_ansi_index() {
        let profile = Profile::default();
        let emu = AlacrittyEmu::new(10, 2, &profile);
        assert_eq!(
            resolve_expected_color("#010203", &emu).unwrap(),
            crate::profile::Rgb::new(1, 2, 3)
        );
        assert_eq!(
            resolve_expected_color("1", &emu).unwrap(),
            emu.color(crate::profile::ColorSlot::Indexed(1)),
            "an index reads the session's own palette"
        );
    }

    /// `default` is the one spelling that cannot mean anything here: these
    /// slots *are* the defaults, so there is nothing for it to refer to.
    #[test]
    fn default_is_rejected_as_a_terminal_color() {
        let emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let error = resolve_expected_color("default", &emu).unwrap_err();
        assert!(format!("{error:?}").contains("no meaning"), "{error:?}");
    }

    #[test]
    fn screenshot_format_defaults_to_svg_and_rejects_unknown_extensions() {
        assert_eq!(
            ScreenshotFormat::infer("screen").unwrap(),
            ScreenshotFormat::Svg
        );
        assert_eq!(
            ScreenshotFormat::infer("screen.SVG").unwrap(),
            ScreenshotFormat::Svg
        );
        assert_eq!(
            ScreenshotFormat::infer("screen.PNG").unwrap(),
            ScreenshotFormat::Png
        );
        let error = ScreenshotFormat::infer("screen.gif").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Usage);
        assert!(error.message.contains(".gif"));
        assert!(error.message.contains(".svg"));
        assert!(error.message.contains(".png"));
    }

    #[test]
    fn cell_model_reports_the_whole_vocabulary() {
        let cell = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Named(NamedColor::Red)),
            bg: Some(Color::Idx(196)),
            underline: UnderlineStyle::Curly,
            underline_color: Some(Color::Rgb(1, 2, 3)),
            attrs: Attrs::all(),
            hyperlink: Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: Some("anchor".into()),
                uri: "https://example.com".into(),
            })),
        };
        let value = cell_model(3, 4, &cell);
        assert_eq!(value.x, 3);
        assert_eq!(value.char, "x");
        assert_eq!(value.fg, CellColor::Indexed(1));
        assert_eq!(value.bg, CellColor::Indexed(196));
        assert!(value.bold);
        assert!(value.dim);
        assert!(value.italic);
        assert!(value.inverse);
        assert!(value.invisible);
        assert!(value.strike);
        assert!(value.blink);
        assert!(value.underline);
        assert_eq!(value.underline_style, "curly");
        assert_eq!(value.underline_color, CellColor::Rgb(1, 2, 3));
    }

    #[test]
    fn cell_model_underline_fields_are_never_absent() {
        let value = cell_model(0, 0, &EmuCell::blank());
        assert!(!value.underline);
        assert_eq!(value.underline_style, "none");
        assert_eq!(value.underline_color, CellColor::Default);
        assert!(!value.blink);

        let cell = EmuCell {
            underline: UnderlineStyle::Single,
            underline_color: None,
            ..EmuCell::blank()
        };
        let value = cell_model(0, 0, &cell);
        assert!(value.underline);
        assert_eq!(value.underline_style, "single");
        assert_eq!(value.underline_color, CellColor::Default);
    }

    #[test]
    fn style_locators_resolve_palette_colors() {
        let emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let cell = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Named(NamedColor::Red)),
            ..EmuCell::blank()
        };
        assert!(cell_matches_style(
            &cell,
            &TextStyle {
                foreground: Some("#800000".into()),
                ..TextStyle::default()
            },
            &emu,
        ));
        assert!(!cell_matches_style(
            &cell,
            &TextStyle {
                foreground: Some("#ff0000".into()),
                ..TextStyle::default()
            },
            &emu,
        ));
        let evaluation = evaluate_cell_style(
            &cell,
            &TextStyle {
                foreground: Some("#ff0000".into()),
                bold: Some(true),
                ..TextStyle::default()
            },
            &emu,
            3,
            4,
            usize::MAX,
        );
        assert!(!evaluation.matched);
        assert_eq!(evaluation.mismatches.len(), 2);
        assert_eq!(evaluation.mismatches[0].location.row, 4);
        assert!(evaluation
            .mismatches
            .iter()
            .any(|mismatch| mismatch.property == "foreground"));
        assert!(evaluation
            .mismatches
            .iter()
            .any(|mismatch| mismatch.property == "bold"));
    }

    #[test]
    fn style_mismatch_evidence_respects_the_requested_budget() {
        let emu = AlacrittyEmu::new(80, 24, &Profile::default());
        let cell = EmuCell::blank();
        let style = TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        };
        assert!(!cell_matches_style(&cell, &style, &emu));
        let boolean = evaluate_cell_style(&cell, &style, &emu, 0, 0, 0);
        assert!(!boolean.matched);
        assert!(boolean.mismatches.is_empty());
        assert!(boolean.mismatches_truncated);
        let limited = evaluate_cell_style(
            &cell,
            &TextStyle {
                bold: Some(true),
                italic: Some(true),
                dim: Some(true),
                ..TextStyle::default()
            },
            &emu,
            0,
            0,
            1,
        );
        assert!(!limited.matched);
        assert_eq!(limited.mismatches.len(), 1);
        assert!(limited.mismatches_truncated);
    }

    #[test]
    fn compact_failure_preserves_url_and_style_mismatch_location() {
        let mut emu = AlacrittyEmu::new(10, 2, &Profile::default());
        emu.process(b"\x1b]8;;test:docs\x07docs\x1b]8;;\x07");
        let mut query = LocatorQuery::style(TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        });
        query.within = Some(Box::new(LocatorQuery::link("test:docs")));
        let evaluation = locator::evaluate_query(
            &emu.viewable_rows(),
            &query,
            false,
            &mut |cell, style, x, y, budget| evaluate_cell_style(cell, style, &emu, x, y, budget),
        )
        .unwrap();
        let mut report = FailureReport::new(
            "locator.resolve",
            None,
            FailureReason::LocatorNoMatch,
            "style mismatch",
        );
        report.locator = Some(evaluation.diagnostics);
        let details = report.failure_details();
        let failure = details.locator.unwrap();
        assert_eq!(
            failure.reason,
            Some(LocatorFailureReason::StyleFilterRemovedAll)
        );
        assert!(failure
            .selectors
            .iter()
            .any(|selector| selector.contains("test:docs")));
        assert_eq!(failure.mismatches[0].property, "bold");
        assert_eq!(failure.mismatches[0].expected, "true");
        assert_eq!(failure.mismatches[0].actual, "false");
        assert_eq!(
            failure.mismatches[0].location,
            crate::api::TextPosition { column: 0, row: 0 }
        );
        assert!(!diagnostic_hints(&report)[0].message.contains("text"));
    }

    /// A link is matched by where it points, so a locator can find the cells
    /// of one link and ignore an identical-looking one pointing elsewhere.
    #[test]
    fn link_locators_match_a_cell_by_its_link() {
        let emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let linked = EmuCell {
            ch: "x".into(),
            hyperlink: Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: None,
                uri: "https://example.com".into(),
            })),
            ..EmuCell::blank()
        };

        let rows = vec![vec![linked]];
        for (uri, count) in [("https://example.com", 1), ("https://other.example", 0)] {
            let found =
                locator::locate_query(&rows, &LocatorQuery::link(uri), &mut |cell, style| {
                    cell_matches_style(cell, style, &emu)
                })
                .unwrap();
            assert_eq!(found.len(), count);
        }
    }

    /// An empty link is a real requirement, not an absent one: it asks for a
    /// cell that links nowhere, which is how a test asserts that a link was
    /// closed rather than merely that some other link was not found.
    #[test]
    fn an_empty_link_requires_a_cell_that_links_nowhere() {
        let emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let plain = EmuCell {
            ch: "x".into(),
            ..EmuCell::blank()
        };
        let linked = EmuCell {
            hyperlink: Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: None,
                uri: "https://example.com".into(),
            })),
            ..plain.clone()
        };
        let found = locator::locate_query(
            &[vec![plain, linked]],
            &LocatorQuery::link(""),
            &mut |cell, style| cell_matches_style(cell, style, &emu),
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.spans[0].end, 1);
    }

    #[test]
    fn appearance_matches_independently_of_links() {
        let emu = AlacrittyEmu::new(10, 2, &Profile::default());
        let linked = EmuCell {
            ch: "x".into(),
            attrs: Attrs::BOLD,
            hyperlink: Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: None,
                uri: "https://example.com".into(),
            })),
            ..EmuCell::blank()
        };
        assert!(cell_matches_style(
            &linked,
            &TextStyle {
                bold: Some(true),
                ..TextStyle::default()
            },
            &emu,
        ));
    }

    #[test]
    fn highlight_maps_full_grid_cells_into_the_viewport() {
        let mut rows = vec![vec![EmuCell::blank(); 3]; 2];
        let highlight = TextHighlight {
            cells: vec![(1, 4)],
            viewport_offset: 3,
        };
        apply_highlight(&mut rows, Some(&highlight), false);
        assert!(rows[1][1].has(Attrs::INVERSE));
        assert!(!rows[0][1].has(Attrs::INVERSE));
    }

    #[test]
    fn highlight_uses_absolute_rows_for_full_grid_renders() {
        let mut rows = vec![vec![EmuCell::blank(); 3]; 5];
        let highlight = TextHighlight {
            cells: vec![(1, 4)],
            viewport_offset: 3,
        };
        apply_highlight(&mut rows, Some(&highlight), true);
        assert!(rows[4][1].has(Attrs::INVERSE));
        assert!(!rows[1][1].has(Attrs::INVERSE));
    }

    #[test]
    fn locator_clicks_the_middle_match_cell() {
        let matched = locator::LocatedMatch {
            value: TextMatch {
                text: "save".into(),
                start: TextPosition { row: 2, column: 4 },
                end: TextPosition { row: 2, column: 8 },
                spans: vec![TextSpan {
                    row: 2,
                    start: 4,
                    end: 8,
                }],
            },
            cells: (4..8)
                .map(|x| locator::MatchedCell {
                    x,
                    y: 2,
                    cell: EmuCell::blank(),
                })
                .collect(),
            source_start: 4,
            source_end: 8,
        };
        assert_eq!(matched_center(&matched), Some((6, 2)));
    }

    #[test]
    fn full_grid_clicks_map_visible_rows_to_viewport_coordinates() {
        let rows = ["old", "older", "history", "prompt", "row save"]
            .into_iter()
            .map(|line| {
                line.chars()
                    .map(|ch| EmuCell {
                        ch: ch.to_string().into(),
                        ..EmuCell::blank()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut parent = TextSelector::new("row save");
        parent.full = true;
        let query = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("save")),
            occurrence: crate::api::MatchOccurrence::Unique,
            within: Some(Box::new(LocatorQuery::text(parent))),
            direction: crate::api::LocatorDirection::Within,
            style: Default::default(),
        };
        let candidates = locator::locate_query(&rows, &query, &mut |_, _| false).unwrap();
        assert_eq!(
            click_point_from_candidates(candidates, "save", true, 3, 2).unwrap(),
            Some((6, 1))
        );
    }

    #[test]
    fn full_grid_clicks_reject_matches_above_the_viewport() {
        let rows = ["save", "history", "prompt"]
            .into_iter()
            .map(|line| {
                line.chars()
                    .map(|ch| EmuCell {
                        ch: ch.to_string().into(),
                        ..EmuCell::blank()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut selector = TextSelector::new("save");
        selector.full = true;
        let mut query = LocatorQuery::text(selector);
        query.occurrence = crate::api::MatchOccurrence::Unique;
        let candidates = locator::locate_query(&rows, &query, &mut |_, _| false).unwrap();
        let error = click_point_from_candidates(candidates, "save", true, 1, 2).unwrap_err();
        assert!(error.message.contains("outside the visible viewport"));
    }

    #[test]
    fn panic_payloads_become_internal_errors() {
        let error = std::panic::catch_unwind(|| panic!("ffi-panic"))
            .map_err(|payload| {
                TuiTestError::internal(format!(
                    "native terminal operation panicked: {}",
                    panic_message(payload.as_ref())
                ))
            })
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Internal);
        assert!(error.message.contains("ffi-panic"));
    }

    #[test]
    fn clean_screen_boundaries_reuse_the_grid_but_flush_dirty_output() {
        let engine = Engine::new(
            "cached-screen".into(),
            Arc::new(Logger::disabled()),
            std::env::temp_dir().join("unused-cached-screen.cast"),
        );
        engine
            .execute(Operation::Run(sleeping_program(false)))
            .unwrap();
        {
            let guard = engine.lock_session();
            let session = guard.as_ref().unwrap();
            let mut state = session.state.lock().unwrap();
            let sequence = capture_visual_state(&mut state, true);
            state.screen_history.pin_current();
            let frozen = state.screen_history.clone();
            let sample_time = state.last_screen_sample;
            let repeat_count = frozen.snapshot().screens.last().unwrap().repeat_count;
            for _ in 0..1000 {
                assert_eq!(capture_visual_state(&mut state, true), sequence);
            }
            assert_eq!(state.last_screen_sample, sample_time);
            assert_eq!(
                state
                    .screen_history
                    .snapshot()
                    .screens
                    .last()
                    .unwrap()
                    .repeat_count,
                repeat_count + 1000,
            );
            assert_eq!(
                frozen.snapshot().screens.last().unwrap().repeat_count,
                repeat_count
            );
            state.emu.process(b"\x1b[HFRESH OUTPUT");
            state.screen_dirty = true;
            let changed = capture_visual_state(&mut state, true);
            assert_ne!(changed, sequence);
            assert!(!state.screen_dirty);
            assert!(state
                .screen_history
                .snapshot()
                .screens
                .last()
                .unwrap()
                .text
                .contains("FRESH OUTPUT"));
        }
        engine.execute(Operation::Close).unwrap();
    }
}
