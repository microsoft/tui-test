//! Per-session daemon: owns one [`Session`], listens on the session socket, and
//! services CLI requests (input, inspection, waits, assertions, recording).

pub mod logger;
pub mod session;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use interprocess::local_socket::traits::ListenerExt;
use interprocess::local_socket::Stream;

use crate::assert::color::{self, Expected};
use crate::assert::snapshot::{self, SnapshotStatus};
use crate::config::{self, POLL_DELAY_MS};
use crate::input::{keys, mouse};
use crate::ipc;
use crate::monitor;
use crate::protocol::{ErrorKind, GetField, MouseAction, Request, Response, TimeoutDefaults};
use crate::terminal::emu::{rows_to_strings, Color, EmuCell};
use crate::terminal::locator::{self, Pattern};
use logger::Logger;
use session::{Session, TermState};

pub struct Daemon {
    name: String,
    session: Mutex<Option<Session>>,
    live: Arc<Mutex<Option<LiveTarget>>>,
    logger: Arc<Logger>,
    last_activity: Mutex<Instant>,
}

/// The current session's renderable state, shared with monitor threads so they
/// can read the live grid without contending on the session lock (which long
/// `wait`s hold).
struct LiveTarget {
    state: Arc<Mutex<TermState>>,
    shell: Option<&'static str>,
}

/// Run the daemon for `session_name` until it is closed.
pub fn run(session_name: String, verbose: bool) -> anyhow::Result<()> {
    config::ensure_home()?;
    sweep_recordings(&session_name);
    let socket = config::socket_name(&session_name);
    let listener = ipc::listen(&socket)?;
    std::fs::write(
        config::pid_file(&session_name),
        std::process::id().to_string(),
    )
    .ok();

    let logger = if verbose {
        match Logger::to_file(&config::log_file(&session_name)) {
            Ok(l) => Arc::new(l),
            Err(_) => Arc::new(Logger::disabled()),
        }
    } else {
        Arc::new(Logger::disabled())
    };
    logger.event(&format!(
        "daemon start session={session_name} pid={}",
        std::process::id()
    ));

    let daemon = Arc::new(Daemon {
        name: session_name.clone(),
        session: Mutex::new(None),
        live: Arc::new(Mutex::new(None)),
        logger,
        last_activity: Mutex::new(Instant::now()),
    });

    spawn_idle_watchdog(Arc::clone(&daemon));

    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        let req = match ipc::read_request(&conn) {
            Ok(r) => r,
            Err(_) => continue,
        };
        *daemon.last_activity.lock().unwrap() = Instant::now();
        if let Request::Monitor { cols, rows } = req {
            daemon.spawn_monitor(conn, (cols, rows));
            continue;
        }
        let (resp, shutdown) = daemon.handle(req);
        let _ = ipc::write_response(&mut conn, &resp);
        if shutdown {
            ipc::drain_peer(conn, Duration::from_millis(config::SHUTDOWN_DRAIN_MS));
            break;
        }
    }

    cleanup(&session_name);
    Ok(())
}

fn cleanup(session: &str) {
    let _ = std::fs::remove_file(config::pid_file(session));
    if !cfg!(windows) {
        let _ = std::fs::remove_file(config::socket_name(session));
    }
}

/// Shut the daemon down once it has gone `IDLE_TIMEOUT_MS` without servicing a
/// request, checked every `IDLE_CHECK_INTERVAL_MS`. Kills the session's pty,
/// removes the daemon's state files, and exits the process.
fn spawn_idle_watchdog(daemon: Arc<Daemon>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(config::IDLE_CHECK_INTERVAL_MS));
        let idle = daemon.last_activity.lock().unwrap().elapsed();
        if idle >= Duration::from_millis(config::IDLE_TIMEOUT_MS) {
            daemon.logger.event(&format!(
                "idle timeout: no activity for {}s, shutting down",
                idle.as_secs()
            ));
            if let Some(s) = daemon.session.lock().unwrap().as_ref() {
                s.kill();
            }
            cleanup(&daemon.name);
            std::process::exit(0);
        }
    });
}

/// Remove leftover `.cast` recordings at daemon start. Recordings of sessions
/// that are still running (their socket is connectable) are kept; the rest are
/// deleted. Called before this daemon's own socket is listening, so the current
/// session's stale cast is included.
fn sweep_recordings(current: &str) {
    let Ok(entries) = std::fs::read_dir(config::recording_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("cast") {
            continue;
        }
        let Some(session) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if session != current && ipc::is_running(&config::socket_name(session)) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// One-line request description for the verbose log. `Open` redacts env values
/// (they may contain secrets) and reports only the variable count.
fn req_summary(req: &Request) -> String {
    match req {
        Request::Open {
            shell,
            program,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            timeouts,
        } => format!(
            "Open {{ shell: {shell:?}, program: {program:?}, {cols}x{rows}, cwd: {cwd:?}, wait_ready: {wait_ready:?}, timeouts: {timeouts:?}, env: <{} vars> }}",
            env.len()
        ),
        other => format!("{other:?}"),
    }
}

impl Daemon {
    fn handle(&self, req: Request) -> (Response, bool) {
        if self.logger.enabled() {
            self.logger.event(&format!("req {}", req_summary(&req)));
        }
        match req {
            Request::Ping => (Response::ok(), false),
            Request::Shutdown => (Response::ok(), true),
            Request::Open {
                shell,
                program,
                cols,
                rows,
                cwd,
                env,
                wait_ready,
                timeouts,
            } => (
                self.open(shell, program, cols, rows, cwd, env, wait_ready, timeouts),
                false,
            ),
            Request::Close => {
                *self.live.lock().unwrap() = None;
                if let Some(s) = self.session.lock().unwrap().as_ref() {
                    s.kill();
                }
                (Response::ok(), true)
            }
            Request::Status => (self.status(), false),
            other => (self.with_session(|s| dispatch(s, other)), false),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn open(
        &self,
        shell: Option<crate::shell::Shell>,
        program: Option<Vec<String>>,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        timeouts: TimeoutDefaults,
    ) -> Response {
        match Session::open(
            shell,
            program.clone(),
            cols,
            rows,
            cwd,
            env,
            timeouts,
            self.logger.clone(),
            config::recording_file(&self.name),
        ) {
            Ok(s) => {
                let shell_pid = s.pid();
                let ready_timeout = open_ready_timeout(&s);
                let ready = if wait_ready.unwrap_or(program.is_none()) {
                    await_ready(&s, ready_timeout)
                } else {
                    s.state.lock().unwrap().emu.tracker().is_ready()
                };
                if wait_ready == Some(true) && !ready {
                    let message = assertion_message(
                        &s,
                        &format!(
                            "open: the session started but reported no prompt within \
                             {ready_timeout}ms; pass --no-wait-ready if it has no shell \
                             integration"
                        ),
                    );
                    s.kill();
                    return Response::assertion(message);
                }
                let live = LiveTarget {
                    state: s.state.clone(),
                    shell: s.shell.map(|sh| sh.as_str()),
                };
                *self.session.lock().unwrap() = Some(s);
                *self.live.lock().unwrap() = Some(live);
                Response::with(json!({
                    "pid": std::process::id(),
                    "shell_pid": shell_pid,
                    "session": self.name,
                    "ready": ready,
                    "recording": config::recording_file(&self.name).to_string_lossy(),
                }))
            }
            Err(e) => Response::internal(format!("failed to open session: {e}")),
        }
    }

    fn status(&self) -> Response {
        let log = self
            .logger
            .enabled()
            .then(|| config::log_file(&self.name).to_string_lossy().into_owned());
        let guard = self.session.lock().unwrap();
        match guard.as_ref() {
            Some(s) => {
                let st = s.state.lock().unwrap();
                Response::with(json!({
                    "session": self.name,
                    "pid": std::process::id(),
                    "shell_pid": s.pid(),
                    "cols": s.cols,
                    "rows": s.rows,
                    "shell": s.shell.map(|sh| sh.as_str()),
                    "exited": st.exited,
                    "timeouts": effective_timeouts(s),
                    "log": log,
                    "version": env!("CARGO_PKG_VERSION"),
                }))
            }
            None => Response::with(json!({
                "session": self.name,
                "pid": std::process::id(),
                "shell_pid": null,
                "log": log,
                "version": env!("CARGO_PKG_VERSION"),
            })),
        }
    }

    fn with_session<F: FnOnce(&mut Session) -> Response>(&self, f: F) -> Response {
        let mut guard = self.session.lock().unwrap();
        match guard.as_mut() {
            Some(s) => f(s),
            None => Response::no_session(),
        }
    }

    /// Stream framed, full-color frames of the live session to a monitor client
    /// until it detaches. Runs on its own thread so the accept loop keeps
    /// serving the agent, and reads only the shared emulator state (never the
    /// session lock) so long `wait`s don't stall the view.
    fn spawn_monitor(&self, conn: Stream, viewer: (u16, u16)) {
        let live = self.live.clone();
        let name = self.name.clone();
        let logger = self.logger.clone();
        std::thread::spawn(move || {
            logger.event("monitor attached");
            let mut conn = conn;
            loop {
                let frame = {
                    let guard = live.lock().unwrap();
                    guard.as_ref().map(|t| {
                        let st = t.state.lock().unwrap();
                        monitor::Frame {
                            grid: st.emu.viewable_rows(),
                            cursor: st.emu.cursor(),
                            size: st.emu.size(),
                            exited: st.exited,
                            shell: t.shell,
                        }
                    })
                };
                let bytes = monitor::render_frame(frame.as_ref(), viewer, &name);
                if conn.write_all(&bytes).is_err() || conn.flush().is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(config::MONITOR_FRAME_MS));
            }
            logger.event("monitor detached");
        });
    }
}

/// Cap only `open`'s implicit ready wait when no ready budget is configured.
fn open_ready_timeout(s: &Session) -> u64 {
    s.timeouts
        .get(config::TimeoutClass::Ready)
        .or_else(|| config::TimeoutClass::Ready.env_ms())
        .unwrap_or(config::OPEN_READY_CAP_MS)
}

/// Poll until the shell reports a prompt, or the session exits or times out.
fn await_ready(s: &Session, timeout_ms: u64) -> bool {
    let start = Instant::now();
    let cap = Duration::from_millis(timeout_ms);
    loop {
        {
            let st = s.state.lock().unwrap();
            if st.emu.tracker().is_ready() {
                return true;
            }
            if st.exited.is_some() {
                return false;
            }
        }
        if start.elapsed() >= cap {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn viewable(s: &Session) -> Vec<Vec<EmuCell>> {
    s.state.lock().unwrap().emu.viewable_rows()
}

fn grid(s: &Session, full: bool) -> Vec<Vec<EmuCell>> {
    let st = s.state.lock().unwrap();
    if full {
        st.emu.full_rows()
    } else {
        st.emu.viewable_rows()
    }
}

fn text_of(rows: &[Vec<EmuCell>]) -> String {
    rows_to_strings(rows)
        .iter()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn dispatch(s: &mut Session, req: Request) -> Response {
    let mut response = match req {
        Request::State => state(s),
        Request::Text { full } => Response::with(json!({ "text": text_of(&grid(s, full)) })),
        Request::Cells { x, y, w, h } => cells(s, x, y, w, h),
        Request::Get { field } => get(s, field),
        Request::Write { data } => act(s.write(data.as_bytes())),
        Request::Submit { data } => act(s.submit(&data.unwrap_or_default())),
        Request::Press { keys } => press(s, keys),
        Request::Mouse { action } => mouse_action(s, action),
        Request::Resize { cols, rows } => act(s.resize(cols, rows)),
        Request::Signal { name } => act(s.pty.lock().unwrap().signal(&name)),
        Request::WaitText {
            text,
            regex,
            full,
            timeout_ms,
            not,
        } => wait_text(
            s,
            &text,
            regex,
            full,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Text)),
            not,
        ),
        Request::WaitIdle { timeout_ms } => wait_idle(
            s,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Idle)),
        ),
        Request::WaitCommand { timeout_ms } => wait_command(
            s,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Command)),
        ),
        Request::WaitExit { timeout_ms } => wait_exit(
            s,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Exit)),
        ),
        Request::WaitReady { timeout_ms } => wait_ready(
            s,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Ready)),
        ),
        Request::ExpectText {
            text,
            regex,
            full,
            strict,
            not,
            fg,
            bg,
            timeout_ms,
        } => expect_text(
            s,
            &text,
            regex,
            full,
            strict,
            not,
            fg,
            bg,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Text)),
        ),
        Request::ExpectExitCode { code, timeout_ms } => expect_exit_code(
            s,
            code,
            timeout_ms.unwrap_or_else(|| s.timeout_for(config::TimeoutClass::Command)),
        ),
        Request::ExpectOutput { text, regex } => expect_output(s, &text, regex),
        Request::Snapshot {
            name,
            update,
            include_colors,
            cwd,
        } => do_snapshot(s, &name, update, include_colors, cwd),
        Request::Screenshot { full, path } => screenshot(s, full, path),
        _ => Response::internal("unsupported request"),
    };
    if response.kind == Some(ErrorKind::Assertion) {
        if let Some(message) = response.message.take() {
            response.message = Some(assertion_message(s, &message));
        }
    }
    response
}

fn act(r: anyhow::Result<()>) -> Response {
    match r {
        Ok(()) => Response::ok(),
        Err(e) => Response::internal(e.to_string()),
    }
}

fn state(s: &Session) -> Response {
    let st = s.state.lock().unwrap();
    let (cx, cy) = st.emu.cursor();
    let (cols, rows) = st.emu.size();
    let text = text_of(&st.emu.viewable_rows());
    Response::with(json!({
        "session_shell": s.shell.map(|sh| sh.as_str()),
        "cols": cols,
        "rows": rows,
        "cursor": { "x": cx, "y": cy },
        "cwd": st.emu.tracker().cwd(),
        "last_command": st.emu.tracker().last_command(),
        "last_exit": st.emu.tracker().last_exit(),
        "exited": st.exited,
        "ready": st.emu.tracker().is_ready(),
        "timeouts": effective_timeouts(s),
        "text": text,
    }))
}

fn effective_timeouts(s: &Session) -> serde_json::Value {
    use config::TimeoutClass::*;
    json!({
        "text": s.timeout_for(Text),
        "idle": s.timeout_for(Idle),
        "command": s.timeout_for(Command),
        "exit": s.timeout_for(Exit),
        "ready": s.timeout_for(Ready),
    })
}

fn cells(s: &Session, x: u16, y: u16, w: u16, h: u16) -> Response {
    let rows = viewable(s);
    let mut out = Vec::new();
    for row in y..y.saturating_add(h.max(1)) {
        for col in x..x.saturating_add(w.max(1)) {
            if let Some(cell) = rows.get(row as usize).and_then(|r| r.get(col as usize)) {
                out.push(json!({
                    "x": col,
                    "y": row,
                    "char": if cell.ch.is_empty() { " ".to_string() } else { cell.ch.clone() },
                    "fg": color_json(cell.fg),
                    "bg": color_json(cell.bg),
                    "bold": cell.bold,
                    "italic": cell.italic,
                    "underline": cell.underline,
                    "inverse": cell.inverse,
                }));
            }
        }
    }
    Response::with(json!({ "cells": out }))
}

fn color_json(c: Color) -> serde_json::Value {
    match c {
        Color::Default => json!("default"),
        Color::Idx(i) => json!(i),
        Color::Rgb(r, g, b) => json!(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

fn get(s: &Session, field: GetField) -> Response {
    let st = s.state.lock().unwrap();
    let value = match field {
        GetField::Command => json!(st.emu.tracker().last_command()),
        GetField::Output => json!(st.emu.tracker().last_output()),
        GetField::ExitCode => json!(st.emu.tracker().last_exit()),
        GetField::Cwd => json!(st.emu.tracker().cwd()),
        GetField::Cursor => {
            let (x, y) = st.emu.cursor();
            json!({ "x": x, "y": y })
        }
        GetField::Size => {
            let (cols, rows) = st.emu.size();
            json!({ "cols": cols, "rows": rows })
        }
    };
    Response::with(json!({ "value": value }))
}

fn press(s: &Session, tokens: Vec<String>) -> Response {
    match keys::tokens_to_seq(&tokens) {
        Ok(seq) => act(s.write(seq.as_bytes())),
        Err(e) => Response::usage(e.to_string()),
    }
}

fn mouse_action(s: &Session, action: MouseAction) -> Response {
    let seq = match action {
        MouseAction::Click {
            x,
            y,
            on_text,
            button,
            clicks,
        } => {
            let (cx, cy) = if let Some(text) = on_text {
                match locate_center(s, &text) {
                    Some(p) => p,
                    None => {
                        return Response::assertion(format!("text not found on screen: {text}"))
                    }
                }
            } else {
                (x.unwrap_or(0), y.unwrap_or(0))
            };
            let mut out = String::new();
            for _ in 0..clicks.max(1) {
                out.push_str(&mouse::click(cx, cy, button));
            }
            out
        }
        MouseAction::Move { x, y } => mouse::motion(x, y),
        MouseAction::Down { x, y, button } => mouse::down(x, y, button),
        MouseAction::Up { x, y, button } => mouse::up(x, y, button),
        MouseAction::Drag {
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
        MouseAction::Scroll { direction, amount } => {
            let up = direction.eq_ignore_ascii_case("up");
            let (cx, cy) = (0, 0);
            (0..amount.max(1))
                .map(|_| mouse::scroll(cx, cy, up))
                .collect()
        }
    };
    act(s.write(seq.as_bytes()))
}

fn locate_center(s: &Session, text: &str) -> Option<(u16, u16)> {
    let rows = viewable(s);
    let pattern = Pattern::new(text, false).ok()?;
    let cells = locator::find(&rows, &pattern, false).ok()??;
    if cells.is_empty() {
        return None;
    }
    let mid = &cells[cells.len() / 2];
    Some((mid.x as u16, mid.y as u16))
}

fn poll_until<F: FnMut() -> bool>(mut f: F, timeout_ms: u64) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(timeout_ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
    }
}

fn matches_now(s: &Session, pattern: &Pattern, full: bool, strict: bool) -> anyhow::Result<bool> {
    let rows = grid(s, full);
    Ok(locator::find(&rows, pattern, strict)?.is_some())
}

fn wait_text(
    s: &Session,
    text: &str,
    regex: bool,
    full: bool,
    timeout_ms: u64,
    not: bool,
) -> Response {
    let pattern = match Pattern::new(text, regex) {
        Ok(p) => p,
        Err(e) => return Response::usage(format!("invalid regex: {e}")),
    };
    let found = poll_until(
        || matches_now(s, &pattern, full, false).unwrap_or(false) != not,
        timeout_ms,
    );
    if found {
        Response::ok()
    } else if not {
        Response::assertion(timeout_message(&pattern.describe(), timeout_ms, true))
    } else {
        Response::assertion(timeout_message(&pattern.describe(), timeout_ms, false))
    }
}

fn wait_idle(s: &Session, timeout_ms: u64) -> Response {
    let quiet = Duration::from_millis(250);
    let ok = poll_until(
        || {
            let st = s.state.lock().unwrap();
            st.last_change.elapsed() >= quiet
        },
        timeout_ms,
    );
    if ok {
        Response::ok()
    } else {
        Response::assertion("wait idle: screen kept changing until timeout")
    }
}

fn command_done_since_input(st: &TermState) -> bool {
    st.emu
        .tracker()
        .finished_at()
        .is_some_and(|at| at >= st.last_input)
}

/// Whether a new command finished, exited, or fell back to idle-at-prompt.
fn command_settled(s: &Session, baseline: u64) -> bool {
    const QUIET: Duration = Duration::from_millis(300);
    let st = s.state.lock().unwrap();
    if st.exited.is_some() || st.emu.tracker().finished_count() > baseline {
        return true;
    }
    if command_done_since_input(&st) {
        return true;
    }
    let idle = st.last_change.elapsed() >= QUIET;
    idle && (!st.emu.tracker().started() || st.emu.tracker().is_ready())
}

fn wait_command(s: &Session, timeout_ms: u64) -> Response {
    let baseline = s.state.lock().unwrap().emu.tracker().finished_count();
    let ok = poll_until(|| command_settled(s, baseline), timeout_ms);
    if ok {
        Response::ok()
    } else {
        Response::assertion("wait command: no command completion within timeout")
    }
}

fn wait_exit(s: &Session, timeout_ms: u64) -> Response {
    let ok = poll_until(|| s.state.lock().unwrap().exited.is_some(), timeout_ms);
    if ok {
        Response::ok()
    } else {
        Response::assertion("wait exit: session still running at timeout")
    }
}

fn wait_ready(s: &Session, timeout_ms: u64) -> Response {
    if await_ready(s, timeout_ms) {
        Response::ok()
    } else {
        Response::assertion("wait ready: no prompt was reported within timeout")
    }
}

#[allow(clippy::too_many_arguments)]
fn expect_text(
    s: &Session,
    text: &str,
    regex: bool,
    full: bool,
    strict: bool,
    not: bool,
    fg: Option<String>,
    bg: Option<String>,
    timeout_ms: u64,
) -> Response {
    let pattern = match Pattern::new(text, regex) {
        Ok(p) => p,
        Err(e) => return Response::usage(format!("invalid regex: {e}")),
    };

    for spec in [&fg, &bg].into_iter().flatten() {
        if let Err(e) = Expected::parse(spec) {
            return Response::usage(e.to_string());
        }
    }

    if fg.is_none() && bg.is_none() && not {
        let gone = poll_until(
            || !matches_now(s, &pattern, full, false).unwrap_or(true),
            timeout_ms,
        );
        return if gone {
            Response::ok()
        } else {
            Response::assertion(timeout_message(&pattern.describe(), timeout_ms, true))
        };
    }

    let mut last_err: Option<String> = None;
    let ok = poll_until(
        || match locator::find(&grid(s, full), &pattern, strict) {
            Ok(Some(cells)) if !cells.is_empty() => {
                if let Some(err) = check_colors(&cells, &fg, &bg, not) {
                    last_err = Some(err);
                    false
                } else {
                    true
                }
            }
            Ok(_) => false,
            Err(e) => {
                last_err = Some(e.to_string());
                false
            }
        },
        timeout_ms,
    );

    if ok {
        Response::ok()
    } else if let Some(err) = last_err {
        Response::assertion(err)
    } else {
        Response::assertion(timeout_message(&pattern.describe(), timeout_ms, false))
    }
}

fn check_colors(
    cells: &[locator::MatchedCell],
    fg: &Option<String>,
    bg: &Option<String>,
    not: bool,
) -> Option<String> {
    let want = !not;
    if let Some(spec) = fg {
        let expected = Expected::parse(spec).ok()?;
        for c in cells {
            if color::matches(c.cell.fg, &expected) != want {
                return Some(format!(
                    "expected fg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(c.cell.fg, &expected),
                    if c.cell.ch.is_empty() {
                        " "
                    } else {
                        &c.cell.ch
                    },
                    c.x,
                    c.y
                ));
            }
        }
    }
    if let Some(spec) = bg {
        let expected = Expected::parse(spec).ok()?;
        for c in cells {
            if color::matches(c.cell.bg, &expected) != want {
                return Some(format!(
                    "expected bg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(c.cell.bg, &expected),
                    if c.cell.ch.is_empty() {
                        " "
                    } else {
                        &c.cell.ch
                    },
                    c.x,
                    c.y
                ));
            }
        }
    }
    None
}

/// Assert the last completed command's exit code.
/// Wait first: `last_exit` holds the previous code until a new command finishes.
fn expect_exit_code(s: &Session, code: i32, timeout_ms: u64) -> Response {
    let baseline = s.state.lock().unwrap().emu.tracker().finished_count();
    if !poll_until(|| command_settled(s, baseline), timeout_ms) {
        return Response::assertion(format!(
            "expected exit code {code}, but the command was still running after {timeout_ms}ms"
        ));
    }
    let actual = s.state.lock().unwrap().emu.tracker().last_exit();
    match actual {
        Some(a) if a == code => Response::ok(),
        Some(a) => Response::assertion(format!("expected exit code {code}, got {a}")),
        None => Response::assertion("no command exit code tracked yet"),
    }
}

fn expect_output(s: &Session, text: &str, regex: bool) -> Response {
    let output = s
        .state
        .lock()
        .unwrap()
        .emu
        .tracker()
        .last_output()
        .map(|o| o.to_string());
    let Some(output) = output else {
        return Response::assertion("no command output tracked yet");
    };
    let hit = if regex {
        match regex::Regex::new(text) {
            Ok(re) => re.is_match(&output),
            Err(e) => return Response::usage(format!("invalid regex: {e}")),
        }
    } else {
        output.contains(text)
    };
    if hit {
        Response::ok()
    } else {
        Response::assertion(format!(
            "output did not contain '{text}'\n---\n{output}\n---"
        ))
    }
}

fn do_snapshot(
    s: &Session,
    name: &str,
    update: bool,
    include_colors: bool,
    cwd: Option<String>,
) -> Response {
    let rows = viewable(s);
    let cols = s.cols;
    let content = snapshot::serialize(&rows, cols, include_colors);
    let base = cwd
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    match snapshot::compare(&base, name, &content, update) {
        Ok(SnapshotStatus::Passed) => Response::with(json!({ "status": "passed" })),
        Ok(SnapshotStatus::Written) => Response::with(json!({ "status": "written" })),
        Ok(SnapshotStatus::Updated) => Response::with(json!({ "status": "updated" })),
        Ok(SnapshotStatus::Failed { expected, actual }) => Response::assertion(format!(
            "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
        )),
        Err(e) => Response::internal(e.to_string()),
    }
}

fn screenshot(s: &Session, full: bool, path: Option<String>) -> Response {
    let rows = grid(s, full);
    match path {
        Some(path) => {
            let svg = crate::render::svg::render_svg(&rows, s.cols);
            match std::fs::write(&path, svg) {
                Ok(()) => Response::with(json!({ "path": path })),
                Err(e) => Response::internal(e.to_string()),
            }
        }
        None => Response::with(json!({ "text": text_of(&rows) })),
    }
}

fn timeout_message(pattern: &str, timeout_ms: u64, not: bool) -> String {
    format!(
        "timed out after {} waiting for '{pattern}' to be {}",
        format_timeout(timeout_ms),
        if not { "hidden" } else { "visible" }
    )
}

fn assertion_message(s: &Session, message: &str) -> String {
    let screen = snapshot::serialize(&viewable(s), s.cols, false);
    format!("{message}\n\nTerminal content:\n{screen}")
}

fn format_timeout(timeout_ms: u64) -> String {
    if timeout_ms.is_multiple_of(1_000) {
        format!("{}s", timeout_ms / 1_000)
    } else {
        format!("{timeout_ms}ms")
    }
}
