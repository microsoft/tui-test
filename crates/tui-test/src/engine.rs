//! Reusable in-process terminal engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::api::{
    Cell, CellColor, Cursor, EffectiveTimeouts, ErrorKind, LocatorQuery, LocatorSelector,
    OpenOptions, OpenResult, Operation, OperationResult, PackedScreen, RunOptions, RuntimeStatus,
    ScreenshotResult, Size, SnapshotResult, TextAnchor, TextMatch, TextSelector, TextStyle,
    TuiTestError,
};
use crate::assert::color::{self, Expected};
use crate::assert::snapshot::{self, SnapshotStatus};
use crate::config::{self, POLL_DELAY_MS};
use crate::input::{keys, mouse};
use crate::logger::Logger;
use crate::session::{Session as TerminalSession, TermState, TextHighlight};
use crate::terminal::cell::{rows_to_strings, Attrs, Color, EmuCell};
use crate::terminal::emu::Emulator;
use crate::terminal::locator::{self, Pattern};

pub struct Engine {
    name: String,
    operations: Mutex<()>,
    session: Mutex<Option<TerminalSession>>,
    live: Arc<Mutex<Option<LiveTarget>>>,
    interrupt: Mutex<Option<InterruptTarget>>,
    logger: Arc<Logger>,
    recording_path: PathBuf,
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
            recording_path,
        }
    }

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, TuiTestError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute_inner(operation)
        }))
        .unwrap_or_else(|payload| {
            Err(TuiTestError::internal(format!(
                "native terminal operation panicked: {}",
                panic_message(payload.as_ref())
            )))
        })
    }

    fn execute_inner(&self, operation: Operation) -> Result<OperationResult, TuiTestError> {
        let _operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.logger.enabled() {
            self.logger
                .event(&format!("operation {}", operation_summary(&operation)));
        }
        match operation {
            Operation::Open(options) => self.open(options).map(OperationResult::Open),
            Operation::Run(options) => self.run(options).map(OperationResult::Open),
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
                }
                Ok(OperationResult::Unit)
            }
            other => self.with_session(|session| dispatch(session, other)),
        }
    }

    fn open(&self, options: OpenOptions) -> Result<OpenResult, TuiTestError> {
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
        )
    }

    fn run(&self, options: RunOptions) -> Result<OpenResult, TuiTestError> {
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
    ) -> Result<OpenResult, TuiTestError> {
        let mut current = self.lock_session();
        if let Some(previous) = current.as_ref() {
            if !restart && previous.is_alive()? {
                return Ok(OpenResult {
                    shell_pid: previous.pid(),
                    session: self.name.clone(),
                    ready: previous.is_ready(),
                    recording: self.recording_path.to_string_lossy().into_owned(),
                });
            }
        }

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
        }
        drop(current);
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
            self.logger.clone(),
            self.recording_path.clone(),
        )
        .map_err(|error| TuiTestError::internal(format!("failed to open session: {error}")))?;

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
            let message = assertion_message(
                &session,
                &format!(
                    "open: the session started but reported no prompt within \
                     {ready_timeout}ms; pass --no-wait-ready if it has no shell \
                     integration"
                ),
            );
            session.kill();
            return Err(TuiTestError::assertion(message));
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
            recording: self.recording_path.to_string_lossy().into_owned(),
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
            return Err(TuiTestError::internal(fault));
        }
        match operation(session) {
            Err(mut error) if error.kind == ErrorKind::Assertion => {
                error.message = assertion_message(session, &error.message);
                Err(error)
            }
            result => result,
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

    pub fn recording_path(&self) -> &PathBuf {
        &self.recording_path
    }

    pub fn flush_recording(&self) -> Result<(), TuiTestError> {
        let _operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let guard = self.lock_session();
        let session = guard.as_ref().ok_or_else(TuiTestError::no_session)?;
        session.flush_recording()
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
            }
        }
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

/// The visible screen and the window title as of a single instant.
///
/// Read under one lock. Taking them separately lets the reader thread advance
/// the terminal in between, which pairs a grid from one moment with a title
/// from another: a shell writes its prompt and then sets its title, so a
/// snapshot of a screen that never changed again could still come out
/// different each time.
fn grid_with_title(
    session: &TerminalSession,
    full: bool,
    include_title: bool,
) -> (Vec<Vec<EmuCell>>, Option<String>) {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let title = if include_title {
        state.emu.title()
    } else {
        None
    };
    let rows = if full {
        state.emu.full_rows()
    } else {
        state.emu.viewable_rows()
    };
    (rows, title)
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
        Operation::FindLocator { query } => {
            Ok(OperationResult::Matches(find_locator(session, &query)?))
        }
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
            button,
            clicks,
            timeout_ms,
        } => {
            click_locator(
                session,
                &query,
                button,
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
            button,
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
                out.push_str(&mouse::click(x, y, button));
            }
            out
        }
        crate::api::MouseAction::Move { x, y } => mouse::motion(x, y),
        crate::api::MouseAction::Down { x, y, button } => mouse::down(x, y, button),
        crate::api::MouseAction::Up { x, y, button } => mouse::up(x, y, button),
        crate::api::MouseAction::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
        } => format!(
            "{}{}{}",
            mouse::down(x1, y1, button),
            mouse::motion(x2, y2),
            mouse::up(x2, y2, button)
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
    let rows = viewable(session);
    let pattern = Pattern::new(text, false).ok()?;
    let cells = locator::find(&rows, &pattern, false).ok()??;
    if cells.is_empty() {
        return None;
    }
    let middle = &cells[cells.len() / 2];
    Some((middle.x as u16, middle.y as u16))
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
        Err(TuiTestError::assertion(title_timeout_message(
            session,
            &pattern.describe(),
            timeout_ms,
            not,
        )))
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
        Err(TuiTestError::assertion(title_timeout_message(
            session,
            &pattern.describe(),
            timeout_ms,
            not,
        )))
    }
}

/// Naming the title actually seen turns "expected X" into a diff a caller can
/// act on, which matters more here than for text because the title is a single
/// short string that the terminal screen does not show.
fn title_timeout_message(
    session: &TerminalSession,
    pattern: &str,
    timeout_ms: u64,
    not: bool,
) -> String {
    let actual = match title_of(session) {
        Some(title) => format!("'{title}'"),
        None => "no title set".to_string(),
    };
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

fn locate_locator_in_state(
    state: &TermState,
    query: &LocatorQuery,
) -> anyhow::Result<Vec<locator::LocatedMatch>> {
    let rows = if query.uses_full_grid() {
        state.emu.full_rows()
    } else {
        state.emu.viewable_rows()
    };
    locator::locate_query(&rows, query, &mut |cell, style| {
        cell_matches_style(cell, style, state.emu.as_ref())
    })
}

fn locate_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
) -> Result<Vec<locator::LocatedMatch>, TuiTestError> {
    validate_locator_query(query)?;
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locate_locator_in_state(&state, query)
        .map_err(|error| TuiTestError::assertion(error.to_string()))
}

fn find_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
) -> Result<Vec<TextMatch>, TuiTestError> {
    locate_locator(session, query)
        .map(|matches| matches.into_iter().map(|matched| matched.value).collect())
}

fn wait_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    not: bool,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    validate_locator_query(query)?;
    let description = query.selector.description();
    let mut matched = false;
    let mut last_error = None;
    poll_until(
        || {
            match locate_locator(session, query) {
                Ok(candidates) => {
                    matched = candidates.is_empty() == not;
                    last_error = None;
                }
                Err(error) => {
                    matched = false;
                    last_error = Some(error);
                }
            }
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        Ok(())
    } else if let Some(error) = last_error {
        Err(error)
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before '{description}' became {}",
            if not { "hidden" } else { "visible" }
        )))
    } else {
        Err(TuiTestError::assertion(timeout_message(
            &description,
            timeout_ms,
            not,
        )))
    }
}

fn resolve_locator_click_point(
    session: &TerminalSession,
    query: &LocatorQuery,
    timeout_ms: u64,
) -> Result<(u16, u16), TuiTestError> {
    validate_locator_query(query)?;
    let description = query.selector.description();
    let mut point = None;
    let mut last_error = None;
    poll_until(
        || {
            let outcome = {
                let state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let visible_rows = state.emu.viewable_rows();
                let visible_len = visible_rows.len();
                let full = query.uses_full_grid();
                let rows = if full {
                    state.emu.full_rows()
                } else {
                    visible_rows
                };
                let candidates = locator::locate_query(&rows, query, &mut |cell, style| {
                    cell_matches_style(cell, style, state.emu.as_ref())
                })
                .map_err(|error| TuiTestError::assertion(error.to_string()));
                candidates.and_then(|candidates| {
                    let viewport_offset = rows.len().saturating_sub(visible_len);
                    click_point_from_candidates(
                        candidates,
                        &description,
                        full,
                        viewport_offset,
                        visible_len,
                    )
                })
            };
            match outcome {
                Ok(Some(value)) => {
                    point = Some(value);
                    last_error = None;
                }
                Ok(None) => {
                    last_error = None;
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
            point.is_some() || session_stopped(session)
        },
        timeout_ms,
    );
    if let Some(point) = point {
        Ok(point)
    } else if let Some(error) = last_error {
        Err(error)
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before '{description}' could be clicked"
        )))
    } else {
        Err(TuiTestError::assertion(format!(
            "timed out after {} waiting for one '{description}' match to click",
            format_timeout(timeout_ms),
        )))
    }
}

fn click_locator(
    session: &TerminalSession,
    query: &LocatorQuery,
    button: u8,
    clicks: u8,
    timeout_ms: u64,
) -> Result<(), TuiTestError> {
    let (x, y) = resolve_locator_click_point(session, query, timeout_ms)?;
    let mut sequence = String::new();
    for _ in 0..clicks.max(1) {
        sequence.push_str(&mouse::click(x, y, button));
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
    let mut last_error = None;
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
            match outcome {
                Ok(Some(matches)) => {
                    resolved = Some(matches);
                    last_error = None;
                }
                Ok(None) => {
                    last_error = None;
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
            resolved.is_some() || session_stopped(session)
        },
        timeout_ms,
    );
    if let Some(matches) = resolved {
        Ok(matches)
    } else if let Some(error) = last_error {
        Err(error)
    } else if session_stopped(session) {
        Err(TuiTestError::assertion(format!(
            "session exited before '{description}' could be highlighted"
        )))
    } else {
        Err(TuiTestError::assertion(format!(
            "timed out after {} waiting for '{description}' to highlight",
            format_timeout(timeout_ms),
        )))
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
    for (expected, actual) in [
        (style.bold, cell.has(Attrs::BOLD)),
        (style.dim, cell.has(Attrs::DIM)),
        (style.italic, cell.has(Attrs::ITALIC)),
        (style.inverse, cell.has(Attrs::INVERSE)),
        (style.hidden, cell.has(Attrs::INVISIBLE)),
        (style.strikethrough, cell.has(Attrs::STRIKE)),
        (style.blink, cell.has(Attrs::BLINK)),
    ] {
        if expected.is_some_and(|expected| expected != actual) {
            return false;
        }
    }
    if style
        .underline_style
        .as_deref()
        .is_some_and(|expected| expected != cell.underline.name())
    {
        return false;
    }
    for (spec, actual, foreground) in [
        (&style.foreground, cell.fg, true),
        (&style.background, cell.bg, false),
        (&style.underline_color, cell.underline_color, true),
    ] {
        if let Some(spec) = spec {
            let Ok(expected) = Expected::parse(spec) else {
                return false;
            };
            if !color::matches(actual, &expected, colors, foreground) {
                return false;
            }
        }
    }
    true
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
    match session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .last_exit()
    {
        Some(actual) if actual == code => Ok(()),
        Some(actual) => Err(TuiTestError::assertion(format!(
            "expected exit code {code}, got {actual}"
        ))),
        None => Err(TuiTestError::assertion("no command exit code tracked yet")),
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
        Err(TuiTestError::assertion(format!(
            "expected bell count {expected}: timed out after {timeout_ms}ms; current count is {actual}"
        )))
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
    let (rows, title) = grid_with_title(session, false, include_title);
    let content = snapshot::serialize(&rows, session.cols, include_colors, title.as_deref());
    let base = cwd
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    match snapshot::compare(&base, name, &content, update) {
        Ok(SnapshotStatus::Passed) => Ok(SnapshotResult::Passed),
        Ok(SnapshotStatus::Written) => Ok(SnapshotResult::Written),
        Ok(SnapshotStatus::Updated) => Ok(SnapshotResult::Updated),
        Ok(SnapshotStatus::Failed { expected, actual }) => Err(TuiTestError::assertion(format!(
            "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
        ))),
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

fn assertion_message(session: &TerminalSession, message: &str) -> String {
    let (rows, title) = grid_with_title(session, false, true);
    let screen = snapshot::serialize(&rows, session.cols, false, title.as_deref());
    format!("{message}\n\nTerminal content:\n{screen}")
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
