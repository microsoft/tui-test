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
use crate::diagnostics::{
    allocate_artifact_directory, elapsed_ms, profile_fingerprint, recording_temp_path,
    write_failure_artifact, ArtifactInputs, CellMismatch, CellStyleEvaluation, DiagnosticHint,
    ExecutionContext, FailureArtifactRef, FailureArtifactStatus, FailureDetails,
    FailureObservation, FailureReason, LocatorFailureReason, OperationEvent, OperationHistory,
    PreparedRecording, ProcessDiagnostics, RecordingDiagnostics, RecordingStatus,
    RuntimeDiagnostics, RECORDING_COPY_LIMIT,
};
use crate::input::{keys, mouse};
use crate::logger::Logger;
use crate::session::{capture_visual_state, Session as TerminalSession, TermState, TextHighlight};
use crate::terminal::cell::{rows_to_strings, Attrs, Color, EmuCell};
use crate::terminal::emu::{ClipboardType, Emulator};
use crate::terminal::locator::{self, Pattern};

pub struct Engine {
    name: String,
    operations: Mutex<()>,
    session: Mutex<Option<TerminalSession>>,
    live: Arc<Mutex<Option<LiveTarget>>>,
    interrupt: Mutex<Option<InterruptTarget>>,
    logger: Arc<Logger>,
    default_recording_path: PathBuf,
    recording: Mutex<RecordingState>,
    operation_history: Mutex<OperationHistory>,
}

#[derive(Clone)]
struct RecordingState {
    path: Option<PathBuf>,
    mode: AutomaticRecordingMode,
    failed: bool,
}

#[derive(Clone)]
struct InterruptTarget {
    pty: Arc<Mutex<crate::terminal::pty::Pty>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

struct LiveTarget {
    state: Arc<Mutex<TermState>>,
    shell: Option<&'static str>,
}

struct OperationMetadata {
    name: String,
    timeout_ms: Option<u64>,
    started_at: Instant,
    started_ms: u64,
    screen_before: u64,
    safe_summary: String,
}

pub struct LiveFrame {
    pub grid: Vec<Vec<EmuCell>>,
    pub cursor: (u16, u16),
    pub size: (u16, u16),
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
            live: Arc::new(Mutex::new(None)),
            interrupt: Mutex::new(None),
            logger,
            default_recording_path: recording_path.clone(),
            recording: Mutex::new(RecordingState {
                path: None,
                mode: AutomaticRecordingMode::Always,
                failed: false,
            }),
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
        let screen_before = self.capture_current_screen_sequence(true);
        let started_ms = self.current_session_elapsed_ms();
        let metadata = OperationMetadata {
            name: name.clone(),
            timeout_ms: operation_timeout(&operation),
            started_at: Instant::now(),
            started_ms,
            screen_before,
            safe_summary: safe_operation_summary(&operation),
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
            );
        let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute_inner(operation, &context, &metadata)
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
        let screen_at_return = result
            .as_ref()
            .err()
            .and_then(|error| error.observation.as_deref())
            .map_or_else(
                || self.capture_current_screen_sequence(false),
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
                metadata
                    .started_ms
                    .saturating_add(metadata.started_at.elapsed().as_millis() as u64),
                screen_at_return,
                result_name,
            );
        if let Err(error) = &mut result {
            self.finalize_failure(error, &context, &metadata);
        }
        result
    }

    fn execute_inner(
        &self,
        operation: Operation,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OperationResult, TuiTestError> {
        match operation {
            Operation::Open(options) => self
                .open(options, context, metadata)
                .map(OperationResult::Open),
            Operation::Run(options) => self
                .run(options, context, metadata)
                .map(OperationResult::Open),
            Operation::Close => {
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
                self.cleanup_recording();
                Ok(OperationResult::Unit)
            }
            other => self.with_session(|session| dispatch(session, other)),
        }
    }

    fn open(
        &self,
        options: OpenOptions,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        self.spawn(
            options.shell,
            None,
            options.backend,
            options.profile,
            options.cols,
            options.rows,
            options.cwd,
            options.env,
            options.wait_ready,
            options.restart,
            options.timeouts,
            options.recording,
            context.retention,
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
        let mut program = Vec::with_capacity(options.args.len() + 1);
        program.push(options.program);
        program.extend(options.args);
        self.spawn(
            None,
            Some(program),
            options.backend,
            options.profile,
            options.cols,
            options.rows,
            options.cwd,
            options.env,
            options.wait_ready,
            options.restart,
            options.timeouts,
            options.recording,
            context.retention,
            context,
            metadata,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &self,
        shell: Option<crate::shell::Shell>,
        program: Option<Vec<String>>,
        backend: crate::terminal::backend::Backend,
        profile: crate::profile::Profile,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        restart: bool,
        timeouts: crate::api::Timeouts,
        recording: AutomaticRecording,
        diagnostics: crate::diagnostics::DiagnosticRetentionOptions,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) -> Result<OpenResult, TuiTestError> {
        recording.validate()?;
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
            previous.kill();
            drop(previous);
        }
        drop(current);
        self.discard_recording();
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
            cwd,
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
            self.finalize_failure_with_session(&mut error, context, metadata, Some(&session), true);
            session.kill();
            return Err(error);
        }
        let live = LiveTarget {
            state: session.state.clone(),
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
                TuiTestError::internal(fault.clone()).with_details(FailureDetails::new(
                    "terminal.operation",
                    None,
                    FailureReason::EmulatorFault,
                    fault,
                )),
            );
        }
        operation(session)
    }

    fn capture_current_screen_sequence(&self, force: bool) -> u64 {
        let mut guard = self.lock_session();
        let Some(session) = guard.as_mut() else {
            return 0;
        };
        let mut state = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        capture_visual_state(&mut state, force)
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
            error.observation = Some(Box::new(capture_failure_observation(session)));
        }
    }

    fn finalize_failure(
        &self,
        error: &mut TuiTestError,
        context: &ExecutionContext,
        metadata: &OperationMetadata,
    ) {
        if error
            .details
            .as_ref()
            .is_some_and(|details| details.terminal.is_some())
        {
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
        if error
            .details
            .as_ref()
            .is_some_and(|details| details.terminal.is_some())
        {
            return;
        }
        let observation = error
            .observation
            .as_deref()
            .cloned()
            .or_else(|| session.map(capture_failure_observation));
        let (summary, summary_truncated) =
            truncate_diagnostic_value(base_error_message(&error.message), 64 * 1024);
        let mut details = FailureDetails::new(
            metadata.name.clone(),
            metadata.timeout_ms,
            failure_reason(error, observation.as_ref()),
            summary,
        );
        details.truncated = summary_truncated;
        details.operation.elapsed_ms = metadata.started_at.elapsed().as_millis() as u64;
        details.operation.started_screen_sequence = metadata.screen_before;
        details.operation.failed_screen_sequence = observation
            .as_ref()
            .map_or(0, |value| value.screen_sequence);
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
            });
        }

        if let Some(existing) = error.details.take() {
            merge_failure_details(&mut details, *existing);
        }
        if let Some(observation) = &observation {
            details.terminal = Some(observation.terminal());
            details.process = Some(observation.process.clone());
            details.runtime = Some(observation.runtime.clone());
            if observation.process.cancelled {
                details.reason = FailureReason::Cancelled;
            } else if observation.process.exit_code.is_some() {
                details.reason = FailureReason::SessionExited;
            }
            details.recording = Some(self.recording_diagnostics(observation));
        }
        details.hints = diagnostic_hints(&details);

        if let Some(observation) = &observation {
            if !error.message.contains("Terminal content:\n") {
                let (screen, truncated) = truncate_diagnostic_value(observation.text(), 512 * 1024);
                details.truncated |= truncated;
                error.message = format!("{}\n\nTerminal content:\n{}", error.message, screen);
            }
        }
        let (message, message_truncated) =
            truncate_diagnostic_value(std::mem::take(&mut error.message), 1024 * 1024);
        error.message = message;
        details.truncated |= message_truncated;
        details.finish_signature();

        if let (Some(options), Some(observation)) = (&context.artifact, &observation) {
            if options.mode != crate::diagnostics::FailureArtifactMode::None {
                let artifact = match allocate_artifact_directory(&options.directory) {
                    Ok(directory) => {
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
                        screen_text: None,
                        screen_svg: None,
                        recording: None,
                        errors: vec![format!(
                            "failed to allocate failure artifact directory: {error}"
                        )],
                    },
                };
                error.artifact = Some(Box::new(artifact));
            }
        }
        error.details = Some(Box::new(details));
    }

    fn prepare_recording_artifact(
        &self,
        session: &TerminalSession,
        observation: &FailureObservation,
        directory: &std::path::Path,
        details: &mut FailureDetails,
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
        if state.visual_revision != observation.output_revision
            || state.screen_dirty
            || state.screen_history.current_sequence() != observation.screen_sequence
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
                exited: state.exited,
                shell: target.shell,
            }
        })
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

fn capture_failure_observation_locked(
    session: &TerminalSession,
    state: &mut TermState,
) -> FailureObservation {
    let screen_sequence = capture_visual_state(state, true);
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
        tui_test_version: env!("CARGO_PKG_VERSION").to_string(),
        backend: session.backend.as_str().to_string(),
        target_os: std::env::consts::OS.to_string(),
        target_arch: std::env::consts::ARCH.to_string(),
        terminal_profile_fingerprint: profile_fingerprint(&session.profile),
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
        history: state.screen_history.snapshot(),
        process,
        runtime,
    }
}

fn capture_error_message(error: &crate::record::CaptureError) -> String {
    match error {
        crate::record::CaptureError::AlreadyActive => {
            "a selected recording is already active".to_string()
        }
        crate::record::CaptureError::NotActive => "no selected recording is active".to_string(),
        crate::record::CaptureError::WorkerStopped => "recording worker stopped".to_string(),
        crate::record::CaptureError::Io(message) => message.clone(),
    }
}

fn failure_reason(error: &TuiTestError, observation: Option<&FailureObservation>) -> FailureReason {
    if let Some(observation) = observation {
        if observation.process.cancelled {
            return FailureReason::Cancelled;
        }
        if observation.process.exit_code.is_some() {
            return FailureReason::SessionExited;
        }
    }
    if let Some(locator) = error
        .details
        .as_ref()
        .and_then(|details| details.locator.as_ref())
    {
        return match locator.failure_reason {
            Some(LocatorFailureReason::Ambiguous) => FailureReason::LocatorAmbiguous,
            Some(LocatorFailureReason::OutsideViewport)
            | Some(LocatorFailureReason::MatchedNoCells) => FailureReason::MatchNotActionable,
            _ => FailureReason::LocatorNoMatch,
        };
    }
    match error.kind {
        ErrorKind::Internal => FailureReason::InternalFailure,
        ErrorKind::Assertion
            if error.message.contains("timed out") || error.message.contains("timeout") =>
        {
            FailureReason::TimedOut
        }
        ErrorKind::Assertion if error.message.contains("snapshot mismatch") => {
            FailureReason::SnapshotMismatch
        }
        ErrorKind::Assertion => FailureReason::ScalarMismatch,
        ErrorKind::Usage | ErrorKind::NoSession => FailureReason::InternalFailure,
    }
}

fn merge_failure_details(target: &mut FailureDetails, source: FailureDetails) {
    target.reason = source.reason;
    let (summary, truncated) = truncate_diagnostic_value(source.summary, 64 * 1024);
    target.summary = summary;
    target.locator = source.locator;
    target.comparison = source.comparison;
    target.evaluation_transitions = source.evaluation_transitions;
    target.hints = source.hints;
    target.truncated |= source.truncated || truncated;
    if source.operation.timeout_ms.is_some() {
        target.operation.timeout_ms = source.operation.timeout_ms;
    }
}

fn diagnostic_hints(details: &FailureDetails) -> Vec<DiagnosticHint> {
    let mut hints = Vec::new();
    match details.reason {
        FailureReason::LocatorAmbiguous => hints.push(DiagnosticHint {
            code: "choose_occurrence".to_string(),
            message:
                "Narrow the locator or choose first(), last(), or nth() when multiple matches are expected."
                    .to_string(),
        }),
        FailureReason::LocatorNoMatch => {
            if let Some(locator) = &details.locator {
                if locator.failure_reason == Some(LocatorFailureReason::StyleFilterRemovedAll) {
                    hints.push(DiagnosticHint {
                        code: "inspect_style_mismatch".to_string(),
                        message:
                            "The selector found candidate text, but its requested style did not match."
                                .to_string(),
                    });
                } else if let Some(stage) = locator.failure_stage {
                    hints.push(DiagnosticHint {
                        code: "inspect_locator_stage".to_string(),
                        message: format!(
                            "Inspect locator stage {stage}; it produced no selected candidates."
                        ),
                    });
                }
            }
        }
        FailureReason::MatchNotActionable => hints.push(DiagnosticHint {
            code: "make_match_visible".to_string(),
            message:
                "The locator matched, but the result was not actionable in the visible viewport."
                    .to_string(),
        }),
        FailureReason::SessionExited => hints.push(DiagnosticHint {
            code: "inspect_process_exit".to_string(),
            message: "Inspect the process exit code and the final recording output.".to_string(),
        }),
        FailureReason::TimedOut => hints.push(DiagnosticHint {
            code: "inspect_last_change".to_string(),
            message:
                "Inspect the retained screen transitions and the last successful operation before the timeout."
                    .to_string(),
        }),
        _ => {}
    }
    hints
}

fn base_error_message(message: &str) -> String {
    message
        .split_once("\n\nTerminal content:\n")
        .map_or(message, |(base, _)| base)
        .to_string()
}

fn diagnostic_operation_name(operation: &Operation) -> &'static str {
    match operation {
        Operation::Open(_) => "open",
        Operation::Run(_) => "run",
        Operation::Close => "close",
        Operation::State => "state",
        Operation::Text { .. } => "text",
        Operation::PackedScreen { .. } => "packed_screen",
        Operation::Cells { .. } => "cells",
        Operation::GetCommand => "get.command",
        Operation::GetOutput => "get.output",
        Operation::GetExitCode => "get.exit_code",
        Operation::GetCwd => "get.cwd",
        Operation::GetCursor => "get.cursor",
        Operation::GetSize => "get.size",
        Operation::GetTitle => "get.title",
        Operation::GetClipboard => "get.clipboard",
        Operation::GetBellCount => "get.bell_count",
        Operation::GetBellEvents => "get.bell_events",
        Operation::Write { .. } => "write",
        Operation::Submit { .. } => "submit",
        Operation::Key { .. } => "key",
        Operation::Mouse { .. } => "mouse",
        Operation::Resize { .. } => "resize",
        Operation::Signal { .. } => "signal",
        Operation::WaitTitle { .. } => "wait.title",
        Operation::WaitClipboard { .. } => "wait.clipboard",
        Operation::WaitClipboardMatch { .. } => "wait.clipboard_match",
        Operation::WaitIdle { .. } => "wait.idle",
        Operation::WaitCommand { .. } => "wait.command",
        Operation::WaitExit { .. } => "wait.exit",
        Operation::WaitReady { .. } => "wait.ready",
        Operation::WaitBell { .. } => "wait.bell",
        Operation::FindLocator { .. } => "locator.find",
        Operation::ResolveLocator { .. } => "locator.location",
        Operation::WaitLocator { .. } => "locator.wait",
        Operation::ClickLocator { .. } => "locator.click",
        Operation::HighlightLocator { .. } => "locator.highlight",
        Operation::ExpectTitle { .. } => "expect.title",
        Operation::ExpectExitCode { .. } => "expect.exit_code",
        Operation::ExpectOutput { .. } => "expect.output",
        Operation::ExpectBellCount { .. } => "expect.bell_count",
        Operation::Snapshot { .. } => "expect.snapshot",
        Operation::Screenshot { .. } => "screenshot",
        Operation::StartRecording { .. } => "record.start",
        Operation::StopRecording => "record.stop",
    }
}

fn operation_timeout(operation: &Operation) -> Option<u64> {
    match operation {
        Operation::WaitTitle { timeout_ms, .. }
        | Operation::WaitClipboard { timeout_ms }
        | Operation::WaitClipboardMatch { timeout_ms, .. }
        | Operation::WaitIdle { timeout_ms }
        | Operation::WaitCommand { timeout_ms }
        | Operation::WaitExit { timeout_ms }
        | Operation::WaitReady { timeout_ms }
        | Operation::WaitBell { timeout_ms }
        | Operation::WaitLocator { timeout_ms, .. }
        | Operation::ClickLocator { timeout_ms, .. }
        | Operation::HighlightLocator { timeout_ms, .. }
        | Operation::ExpectTitle { timeout_ms, .. }
        | Operation::ExpectExitCode { timeout_ms, .. }
        | Operation::ExpectBellCount { timeout_ms, .. } => *timeout_ms,
        _ => None,
    }
}

fn safe_operation_summary(operation: &Operation) -> String {
    match operation {
        Operation::Open(options) => format!(
            "opened a {} terminal at {}x{} with {} environment variables",
            options.backend.as_str(),
            options.cols,
            options.rows,
            options.env.len()
        ),
        Operation::Run(options) => format!(
            "ran a program in a {} terminal at {}x{} with {} arguments and {} environment variables",
            options.backend.as_str(),
            options.cols,
            options.rows,
            options.args.len(),
            options.env.len()
        ),
        Operation::Write { data } => format!("wrote {} bytes", data.len()),
        Operation::Submit { data } => {
            format!("submitted {} bytes", data.as_ref().map_or(0, String::len))
        }
        Operation::Key { keys, .. } => format!("sent {} key tokens", keys.len()),
        Operation::Mouse { action } => match action {
            crate::api::MouseAction::Click {
                on_text,
                options,
                clicks,
                ..
            } => format!(
                "clicked {} time(s) with {:?} button{}",
                clicks.max(&1),
                options.button,
                if on_text.is_some() { " on text" } else { "" }
            ),
            _ => "sent a mouse action".to_string(),
        },
        Operation::Resize { cols, rows } => format!("resized terminal to {cols}x{rows}"),
        Operation::FindLocator { query } => {
            format!("resolved a {}-stage locator", locator_stage_count(query))
        }
        Operation::ResolveLocator { query } => format!(
            "resolved a {}-stage locator requiring one match",
            locator_stage_count(query)
        ),
        Operation::WaitLocator { query, not, .. } => format!(
            "waited for a {}-stage locator to become {}",
            locator_stage_count(query),
            if *not { "hidden" } else { "visible" }
        ),
        Operation::ClickLocator {
            query,
            options,
            clicks,
            ..
        } => format!(
            "clicked a {}-stage locator {} time(s) with {:?} button",
            locator_stage_count(query),
            clicks.max(&1),
            options.button
        ),
        Operation::HighlightLocator { query, .. } => {
            format!("highlighted a {}-stage locator", locator_stage_count(query))
        }
        _ => diagnostic_operation_name(operation).to_string(),
    }
}

fn locator_stage_count(query: &LocatorQuery) -> usize {
    1 + query.within.as_deref().map_or(0, locator_stage_count)
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
) -> Result<OperationResult, TuiTestError> {
    match operation {
        Operation::State => Ok(OperationResult::State(state(session))),
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
            let (x, y) = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .emu
                .cursor();
            Ok(OperationResult::Cursor(Cursor { x, y }))
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
            act(session.write(data.as_bytes()))?;
            Ok(OperationResult::Unit)
        }
        Operation::Submit { data } => {
            act(session.submit(&data.unwrap_or_default()))?;
            Ok(OperationResult::Unit)
        }
        Operation::Key { keys, action } => {
            key_action(session, keys, action)?;
            Ok(OperationResult::Unit)
        }
        Operation::Mouse { action } => {
            mouse_action(session, action)?;
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
            include_colors,
            include_title,
            cwd,
        } => Ok(OperationResult::Snapshot(do_snapshot(
            session,
            &name,
            update,
            include_colors,
            include_title,
            cwd,
        )?)),
        Operation::Screenshot { full, path, zoom } => Ok(OperationResult::Screenshot(screenshot(
            session, full, path, zoom,
        )?)),
        Operation::StartRecording {
            path,
            format,
            fps,
            speed,
            idle_time_limit,
            zoom,
        } => {
            session.start_recording(path, format, fps, speed, idle_time_limit, zoom)?;
            Ok(OperationResult::Unit)
        }
        Operation::StopRecording => Ok(OperationResult::Recording(session.stop_recording()?)),
        Operation::Open(_) | Operation::Run(_) | Operation::Close => {
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
    let (x, y) = state.emu.cursor();
    let (cols, rows) = state.emu.size();
    let bells = session.bells.snapshot();
    crate::api::State {
        session_shell: session.shell.map(|value| value.as_str().to_string()),
        cols,
        rows,
        cursor: Cursor { x, y },
        title: state.emu.title(),
        cwd: state.tracker.cwd().map(str::to_string),
        last_command: state.tracker.last_command().map(str::to_string),
        last_exit: state.tracker.last_exit(),
        exited: state.exited,
        ready: state.tracker.is_ready(),
        bell_count: bells.count,
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
    }
}

fn cell_color(color: Option<Color>) -> CellColor {
    match color {
        None => CellColor::Default,
        Some(Color::Rgb(r, g, b)) => CellColor::Rgb(r, g, b),
        Some(color) => CellColor::Indexed(color.to_index()),
    }
}

fn key_action(
    session: &TerminalSession,
    tokens: Vec<String>,
    action: crate::api::KeyAction,
) -> Result<(), TuiTestError> {
    let keyboard_mode = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .emu
        .keyboard_mode();
    let sequence = keys::tokens_to_seq_for_action_with_mode(&tokens, action, keyboard_mode)
        .map_err(|error| TuiTestError::usage(error.to_string()))?;
    if sequence.is_empty() {
        Ok(())
    } else {
        act(session.write(sequence.as_bytes()))
    }
}

fn mouse_action(
    session: &TerminalSession,
    action: crate::api::MouseAction,
) -> Result<(), TuiTestError> {
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
            let mut out = String::new();
            for _ in 0..clicks.max(1) {
                out.push_str(&mouse::click(x, y, options));
            }
            out
        }
        crate::api::MouseAction::Move { x, y } => mouse::motion(x, y),
        crate::api::MouseAction::Down { x, y, options } => mouse::down(x, y, options),
        crate::api::MouseAction::Up { x, y, options } => mouse::up(x, y, options),
        crate::api::MouseAction::Drag {
            x1,
            y1,
            x2,
            y2,
            options,
        } => format!(
            "{}{}{}",
            mouse::down(x1, y1, options),
            mouse::drag_motion(x2, y2, options),
            mouse::up(x2, y2, options)
        ),
        crate::api::MouseAction::Scroll { direction, amount } => {
            let up = direction.eq_ignore_ascii_case("up");
            (0..amount.max(1))
                .map(|_| mouse::scroll(0, 0, up))
                .collect()
        }
    };
    act(session.write(sequence.as_bytes()))
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

/// Naming the title actually seen turns "expected X" into a diff a caller can
/// act on, which matters more here than for text because the title is a single
/// short string that the terminal screen does not show.
fn title_timeout_message_from_actual(
    actual: Option<&str>,
    pattern: &str,
    timeout_ms: u64,
    not: bool,
) -> String {
    let actual = actual
        .map(|title| format!("'{title}'"))
        .unwrap_or_else(|| "no title set".to_string());
    format!(
        "timed out after {} waiting for the title '{pattern}' to be {}; the title is {actual}",
        format_timeout(timeout_ms),
        if not { "hidden" } else { "visible" },
    )
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
    }
    if let Some(parent) = query.within.as_deref() {
        validate_locator_query(parent)?;
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
    let mut evaluation =
        locator::evaluate_query(&rows, query, require_one, &mut |cell, style, x, y| {
            evaluate_cell_style(cell, style, state.emu.as_ref(), x, y)
        })?;
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
        let message =
            locator_failure_message(query, &evaluated.evaluation.diagnostics, require_one, None);
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
                locator_failure_message(
                    query,
                    &final_evaluated.evaluation.diagnostics,
                    false,
                    Some(timeout_ms),
                )
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

fn locator_failure_message(
    query: &LocatorQuery,
    diagnostics: &crate::diagnostics::LocatorDiagnostics,
    require_one: bool,
    _timeout_ms: Option<u64>,
) -> String {
    let description = query.selector.description();
    match diagnostics.failure_reason {
        Some(LocatorFailureReason::Ambiguous) => {
            let count = diagnostics
                .failure_stage
                .and_then(|index| diagnostics.stages.get(index))
                .map_or(diagnostics.final_candidate_count, |stage| {
                    stage.style_candidate_count
                });
            format!("expected '{description}' to match once, but found {count} matches")
        }
        Some(LocatorFailureReason::NthOutOfRange) => {
            format!("no match found for '{description}': selected occurrence is out of range")
        }
        Some(LocatorFailureReason::AnchorAmbiguous) => {
            format!("locator anchor for '{description}' matched more than once")
        }
        Some(LocatorFailureReason::AnchorNotFound) => {
            format!("locator anchor for '{description}' was not found")
        }
        _ if require_one => format!("no match found for '{description}'"),
        _ => format!("no match found for '{description}'"),
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
    let mut details = FailureDetails::new(operation, timeout_ms, reason, message.clone());
    details.operation.failed_screen_sequence = evaluated.screen_sequence;
    details.locator = Some(evaluated.evaluation.diagnostics);
    details.evaluation_transitions = transitions;
    let mut error = TuiTestError::assertion(message).with_details(details);
    error.observation = observation.map(Box::new);
    error
}

fn comparison_failure(
    operation: &str,
    timeout_ms: Option<u64>,
    reason: FailureReason,
    message: String,
    kind: &str,
    expected: Option<String>,
    actual: Option<String>,
) -> TuiTestError {
    let mut details = FailureDetails::new(operation, timeout_ms, reason, message.clone());
    let (expected, expected_truncated) = expected.map_or((None, false), |value| {
        let (value, truncated) = truncate_diagnostic_value(value, 256 * 1024);
        (Some(value), truncated)
    });
    let (actual, actual_truncated) = actual.map_or((None, false), |value| {
        let (value, truncated) = truncate_diagnostic_value(value, 256 * 1024);
        (Some(value), truncated)
    });
    details.truncated = expected_truncated || actual_truncated;
    details.comparison = Some(crate::diagnostics::ComparisonDiagnostics {
        kind: kind.to_string(),
        expected,
        actual,
    });
    TuiTestError::assertion(message).with_details(details)
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

fn truncate_diagnostic_value(mut value: String, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value, false);
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str("\n... diagnostic value truncated ...");
    (value, true)
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
                    locator_failure_message(
                        query,
                        &final_evaluated.evaluation.diagnostics,
                        true,
                        Some(timeout_ms),
                    )
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
) -> Result<(), TuiTestError> {
    let (x, y) = resolve_locator_click_point(session, query, timeout_ms)?;
    let mut sequence = String::new();
    for _ in 0..clicks.max(1) {
        sequence.push_str(&mouse::click(x, y, options));
    }
    act(session.write(sequence.as_bytes()))
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
            locator_failure_message(
                query,
                &evaluated.evaluation.diagnostics,
                false,
                Some(timeout_ms),
            )
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
    evaluate_cell_style(cell, style, colors, 0, 0).matched
}

fn evaluate_cell_style(
    cell: &EmuCell,
    style: &TextStyle,
    colors: &dyn Emulator,
    x: usize,
    y: usize,
) -> CellStyleEvaluation {
    let mut mismatches = Vec::new();
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
            if expected != actual {
                mismatches.push(style_mismatch(
                    cell,
                    x,
                    y,
                    property,
                    expected.to_string(),
                    actual.to_string(),
                    None,
                ));
            }
        }
    }
    if let Some(expected) = style.underline_style.as_deref() {
        let actual = cell.underline.name();
        if expected != actual {
            mismatches.push(style_mismatch(
                cell,
                x,
                y,
                "underline_style",
                expected.to_string(),
                actual.to_string(),
                None,
            ));
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
                if !color::matches(actual, &expected, colors, foreground) {
                    mismatches.push(style_mismatch(
                        cell,
                        x,
                        y,
                        property,
                        expected.describe(),
                        logical_color(actual),
                        Some(colors.resolve(actual, foreground).to_hex()),
                    ));
                }
            }
        }
    }
    CellStyleEvaluation {
        matched: mismatches.is_empty(),
        mismatches,
    }
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
    include_colors: bool,
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
        include_colors,
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

fn screenshot(
    session: &TerminalSession,
    full: bool,
    path: Option<String>,
    zoom: Option<f64>,
) -> Result<ScreenshotResult, TuiTestError> {
    match path {
        Some(path) => {
            let zoom = crate::api::resolve_zoom(zoom)?;
            let snapshot = svg_snapshot(session, full);
            let svg = crate::render::svg::render_svg_with_zoom(
                &snapshot.rows,
                snapshot.cols,
                &snapshot.render_state,
                snapshot.cursor,
                snapshot.title.as_deref(),
                zoom,
            );
            std::fs::write(&path, svg)
                .map_err(|error| TuiTestError::internal(error.to_string()))?;
            Ok(ScreenshotResult::Path(path))
        }
        None if zoom.is_some() => Err(TuiTestError::usage(
            "screenshot zoom requires an output path",
        )),
        None => Ok(ScreenshotResult::Text(text_of(&grid(session, full)))),
    }
}

fn timeout_message(pattern: &str, timeout_ms: u64, not: bool) -> String {
    format!(
        "timed out after {} waiting for '{pattern}' to be {}",
        format_timeout(timeout_ms),
        if not { "hidden" } else { "visible" }
    )
}

fn format_timeout(timeout_ms: u64) -> String {
    if timeout_ms.is_multiple_of(1_000) {
        format!("{}s", timeout_ms / 1_000)
    } else {
        format!("{timeout_ms}ms")
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
    use crate::api::{TextPosition, TextSpan};
    use crate::profile::Profile;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{NamedColor, UnderlineStyle};
    use crate::terminal::emu::Emulator;

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

    #[test]
    fn cell_model_reports_the_whole_vocabulary() {
        let cell = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Named(NamedColor::Red)),
            bg: Some(Color::Idx(196)),
            underline: UnderlineStyle::Curly,
            underline_color: Some(Color::Rgb(1, 2, 3)),
            attrs: Attrs::all(),
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
}
