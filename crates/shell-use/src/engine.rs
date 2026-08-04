//! Reusable in-process terminal engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::api::{
    Cell, CellColor, Cursor, EffectiveTimeouts, ErrorKind, OpenOptions, OpenResult, Operation,
    OperationResult, PackedScreen, RunOptions, RuntimeStatus, ScreenshotResult, ShellUseError,
    Size, SnapshotResult,
};
use crate::assert::color::{self, Expected};
use crate::assert::snapshot::{self, SnapshotStatus};
use crate::config::{self, POLL_DELAY_MS};
use crate::input::{keys, mouse};
use crate::logger::Logger;
use crate::session::{Session as TerminalSession, TermState};
use crate::terminal::cell::{rows_to_strings, Attrs, Color, EmuCell};
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
            "Open {{ shell: {:?}, scrollback: {}, {}x{}, cwd: {:?}, wait_ready: {:?}, timeouts: {:?}, env: <{} vars> }}",
            options.shell,
            options.profile.scrollback,
            options.cols,
            options.rows,
            options.cwd,
            options.wait_ready,
            options.timeouts,
            options.env.len()
        ),
        Operation::Run(options) => format!(
            "Run {{ program: {:?}, args: {:?}, scrollback: {}, {}x{}, cwd: {:?}, wait_ready: {:?}, timeouts: {:?}, env: <{} vars> }}",
            options.program,
            options.args,
            options.profile.scrollback,
            options.cols,
            options.rows,
            options.cwd,
            options.wait_ready,
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

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, ShellUseError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute_inner(operation)
        }))
        .unwrap_or_else(|payload| {
            Err(ShellUseError::internal(format!(
                "native terminal operation panicked: {}",
                panic_message(payload.as_ref())
            )))
        })
    }

    fn execute_inner(&self, operation: Operation) -> Result<OperationResult, ShellUseError> {
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

    fn open(&self, options: OpenOptions) -> Result<OpenResult, ShellUseError> {
        self.spawn(
            options.shell,
            None,
            options.profile,
            options.cols,
            options.rows,
            options.cwd,
            options.env,
            options.wait_ready,
            options.timeouts,
        )
    }

    fn run(&self, options: RunOptions) -> Result<OpenResult, ShellUseError> {
        let mut program = Vec::with_capacity(options.args.len() + 1);
        program.push(options.program);
        program.extend(options.args);
        self.spawn(
            None,
            Some(program),
            options.profile,
            options.cols,
            options.rows,
            options.cwd,
            options.env,
            options.wait_ready,
            options.timeouts,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &self,
        shell: Option<crate::shell::Shell>,
        program: Option<Vec<String>>,
        profile: crate::profile::Profile,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        timeouts: crate::api::Timeouts,
    ) -> Result<OpenResult, ShellUseError> {
        *self
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .interrupt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        if let Some(previous) = self.lock_session().take() {
            previous.kill();
        }
        let session = TerminalSession::open(
            shell,
            program.clone(),
            profile,
            cols,
            rows,
            cwd,
            env,
            timeouts,
            self.logger.clone(),
            self.recording_path.clone(),
        )
        .map_err(|error| ShellUseError::internal(format!("failed to open session: {error}")))?;

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
            return Err(ShellUseError::assertion(message));
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

    fn with_session<F>(&self, operation: F) -> Result<OperationResult, ShellUseError>
    where
        F: FnOnce(&mut TerminalSession) -> Result<OperationResult, ShellUseError>,
    {
        let mut guard = self.lock_session();
        let session = guard.as_mut().ok_or_else(ShellUseError::no_session)?;
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
                grid: state.emu.viewable_rows(),
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
) -> Result<OperationResult, ShellUseError> {
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
        Operation::Write { data } => {
            act(session.write(data.as_bytes()))?;
            Ok(OperationResult::Unit)
        }
        Operation::Submit { data } => {
            act(session.submit(&data.unwrap_or_default()))?;
            Ok(OperationResult::Unit)
        }
        Operation::Press { keys } => {
            press(session, keys)?;
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
        Operation::WaitText {
            text,
            regex,
            full,
            timeout_ms,
            not,
        } => {
            wait_text(
                session,
                &text,
                regex,
                full,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
                not,
            )?;
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
        Operation::ExpectText {
            text,
            regex,
            full,
            strict,
            not,
            fg,
            bg,
            timeout_ms,
        } => {
            expect_text(
                session,
                &text,
                regex,
                full,
                strict,
                not,
                fg,
                bg,
                timeout_ms.unwrap_or_else(|| session.timeout_for(config::TimeoutClass::Text)),
            )?;
            Ok(OperationResult::Unit)
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
        Operation::Screenshot { full, path } => Ok(OperationResult::Screenshot(screenshot(
            session, full, path,
        )?)),
        Operation::Open(_) | Operation::Run(_) | Operation::Close => {
            Err(ShellUseError::internal("unsupported nested operation"))
        }
    }
}

fn act(result: anyhow::Result<()>) -> Result<(), ShellUseError> {
    result.map_err(|error| ShellUseError::internal(error.to_string()))
}

fn state(session: &TerminalSession) -> crate::api::State {
    let state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (x, y) = state.emu.cursor();
    let (cols, rows) = state.emu.size();
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

fn press(session: &TerminalSession, tokens: Vec<String>) -> Result<(), ShellUseError> {
    let sequence =
        keys::tokens_to_seq(&tokens).map_err(|error| ShellUseError::usage(error.to_string()))?;
    act(session.write(sequence.as_bytes()))
}

fn mouse_action(
    session: &TerminalSession,
    action: crate::api::MouseAction,
) -> Result<(), ShellUseError> {
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
                    ShellUseError::assertion(format!("text not found on screen: {text}"))
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

fn matches_now(
    session: &TerminalSession,
    pattern: &Pattern,
    full: bool,
    strict: bool,
) -> anyhow::Result<bool> {
    Ok(locator::find(&grid(session, full), pattern, strict)?.is_some())
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

fn wait_text(
    session: &TerminalSession,
    text: &str,
    regex: bool,
    full: bool,
    timeout_ms: u64,
    not: bool,
) -> Result<(), ShellUseError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| ShellUseError::usage(format!("invalid regex: {error}")))?;
    let mut matched = false;
    poll_until(
        || {
            matched = matches_now(session, &pattern, full, false).unwrap_or(false) != not;
            matched || session_stopped(session)
        },
        timeout_ms,
    );
    if matched {
        Ok(())
    } else if session_stopped(session) {
        Err(ShellUseError::assertion(format!(
            "session exited before '{}' became {}",
            pattern.describe(),
            if not { "hidden" } else { "visible" }
        )))
    } else {
        Err(ShellUseError::assertion(timeout_message(
            &pattern.describe(),
            timeout_ms,
            not,
        )))
    }
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
) -> Result<(), ShellUseError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| ShellUseError::usage(format!("invalid regex: {error}")))?;
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
        Err(ShellUseError::assertion(format!(
            "session exited before the title '{}' became {}",
            pattern.describe(),
            if not { "hidden" } else { "visible" }
        )))
    } else {
        Err(ShellUseError::assertion(title_timeout_message(
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
) -> Result<(), ShellUseError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| ShellUseError::usage(format!("invalid regex: {error}")))?;
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
        Err(ShellUseError::assertion(format!(
            "session exited before the title '{}' became {}",
            pattern.describe(),
            if not { "hidden" } else { "visible" }
        )))
    } else {
        Err(ShellUseError::assertion(title_timeout_message(
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

fn wait_idle(session: &TerminalSession, timeout_ms: u64) -> Result<(), ShellUseError> {
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
        Err(ShellUseError::assertion(
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

fn wait_command(session: &TerminalSession, timeout_ms: u64) -> Result<(), ShellUseError> {
    let baseline = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .finished_count();
    if poll_until(|| command_settled(session, baseline), timeout_ms) {
        Ok(())
    } else {
        Err(ShellUseError::assertion(format!(
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

fn wait_exit(session: &TerminalSession, timeout_ms: u64) -> Result<(), ShellUseError> {
    if poll_until(
        || {
            session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .exited
                .is_some()
                || session.cancelled.load(std::sync::atomic::Ordering::Acquire)
        },
        timeout_ms,
    ) {
        Ok(())
    } else {
        Err(ShellUseError::assertion(
            "wait exit: session still running at timeout",
        ))
    }
}

fn wait_ready(session: &TerminalSession, timeout_ms: u64) -> Result<(), ShellUseError> {
    if await_ready(session, timeout_ms) {
        Ok(())
    } else {
        Err(ShellUseError::assertion(
            "wait ready: no prompt was reported within timeout",
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn expect_text(
    session: &TerminalSession,
    text: &str,
    regex: bool,
    full: bool,
    strict: bool,
    not: bool,
    fg: Option<String>,
    bg: Option<String>,
    timeout_ms: u64,
) -> Result<(), ShellUseError> {
    let pattern = Pattern::new(text, regex)
        .map_err(|error| ShellUseError::usage(format!("invalid regex: {error}")))?;

    for spec in [&fg, &bg].into_iter().flatten() {
        Expected::parse(spec).map_err(|error| ShellUseError::usage(error.to_string()))?;
    }

    if fg.is_none() && bg.is_none() && not {
        let mut gone = false;
        poll_until(
            || {
                gone = !matches_now(session, &pattern, full, false).unwrap_or(true);
                gone || session_stopped(session)
            },
            timeout_ms,
        );
        return if gone {
            Ok(())
        } else if session_stopped(session) {
            Err(ShellUseError::assertion(format!(
                "session exited before '{}' became hidden",
                pattern.describe()
            )))
        } else {
            Err(ShellUseError::assertion(timeout_message(
                &pattern.describe(),
                timeout_ms,
                true,
            )))
        };
    }

    let mut last_error = None;
    let mut matched = false;
    poll_until(
        || {
            matched = match locator::find(&grid(session, full), &pattern, strict) {
                Ok(Some(cells)) if !cells.is_empty() => {
                    if let Some(error) = check_colors(
                        &cells,
                        &fg,
                        &bg,
                        not,
                        session
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .emu
                            .as_ref(),
                    ) {
                        last_error = Some(error);
                        false
                    } else {
                        true
                    }
                }
                Ok(_) => false,
                Err(error) => {
                    last_error = Some(error.to_string());
                    false
                }
            };
            matched || session_stopped(session)
        },
        timeout_ms,
    );

    if matched {
        Ok(())
    } else if let Some(error) = last_error {
        Err(ShellUseError::assertion(error))
    } else if session_stopped(session) {
        Err(ShellUseError::assertion(format!(
            "session exited before '{}' matched",
            pattern.describe()
        )))
    } else {
        Err(ShellUseError::assertion(timeout_message(
            &pattern.describe(),
            timeout_ms,
            false,
        )))
    }
}

fn check_colors(
    cells: &[locator::MatchedCell],
    fg: &Option<String>,
    bg: &Option<String>,
    not: bool,
    colors: &dyn crate::terminal::emu::Emulator,
) -> Option<String> {
    let want = !not;
    if let Some(spec) = fg {
        let expected = Expected::parse(spec).ok()?;
        for cell in cells {
            if color::matches(cell.cell.fg, &expected, colors) != want {
                return Some(format!(
                    "expected fg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(cell.cell.fg, &expected, colors),
                    cell.cell.ch,
                    cell.x,
                    cell.y
                ));
            }
        }
    }
    if let Some(spec) = bg {
        let expected = Expected::parse(spec).ok()?;
        for cell in cells {
            if color::matches(cell.cell.bg, &expected, colors) != want {
                return Some(format!(
                    "expected bg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(cell.cell.bg, &expected, colors),
                    cell.cell.ch,
                    cell.x,
                    cell.y
                ));
            }
        }
    }
    None
}

fn expect_exit_code(
    session: &TerminalSession,
    code: i32,
    timeout_ms: u64,
) -> Result<(), ShellUseError> {
    let baseline = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .finished_count();
    if !poll_until(|| command_settled(session, baseline), timeout_ms) {
        return Err(ShellUseError::assertion(format!(
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
        Some(actual) => Err(ShellUseError::assertion(format!(
            "expected exit code {code}, got {actual}"
        ))),
        None => Err(ShellUseError::assertion("no command exit code tracked yet")),
    }
}

fn expect_output(session: &TerminalSession, text: &str, regex: bool) -> Result<(), ShellUseError> {
    let output = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tracker
        .last_output()
        .map(str::to_string)
        .ok_or_else(|| ShellUseError::assertion("no command output tracked yet"))?;
    let matched = if regex {
        regex::Regex::new(text)
            .map_err(|error| ShellUseError::usage(format!("invalid regex: {error}")))?
            .is_match(&output)
    } else {
        output.contains(text)
    };
    if matched {
        Ok(())
    } else {
        Err(ShellUseError::assertion(format!(
            "output did not contain '{text}'\n---\n{output}\n---"
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
) -> Result<SnapshotResult, ShellUseError> {
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
        Ok(SnapshotStatus::Failed { expected, actual }) => Err(ShellUseError::assertion(format!(
            "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
        ))),
        Err(error) => Err(ShellUseError::internal(error.to_string())),
    }
}

fn screenshot(
    session: &TerminalSession,
    full: bool,
    path: Option<String>,
) -> Result<ScreenshotResult, ShellUseError> {
    let (rows, title) = grid_with_title(session, full, true);
    match path {
        Some(path) => {
            let svg = crate::render::svg::render_svg(
                &rows,
                session.cols,
                session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .emu
                    .as_ref(),
                title.as_deref(),
            );
            std::fs::write(&path, svg)
                .map_err(|error| ShellUseError::internal(error.to_string()))?;
            Ok(ScreenshotResult::Path(path))
        }
        None => Ok(ScreenshotResult::Text(text_of(&rows))),
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
    use crate::terminal::cell::{NamedColor, UnderlineStyle};

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
    fn panic_payloads_become_internal_errors() {
        let error = std::panic::catch_unwind(|| panic!("ffi-panic"))
            .map_err(|payload| {
                ShellUseError::internal(format!(
                    "native terminal operation panicked: {}",
                    panic_message(payload.as_ref())
                ))
            })
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Internal);
        assert!(error.message.contains("ffi-panic"));
    }
}
