//! Reusable in-process terminal engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::assert::color::{self, Expected};
use crate::assert::snapshot::{self, SnapshotStatus};
use crate::config::{self, POLL_DELAY_MS};
use crate::input::{keys, mouse};
use crate::logger::Logger;
use crate::protocol::{ErrorKind, GetField, MouseAction, Request, Response, TimeoutDefaults};
use crate::session::{Session, TermState};
use crate::terminal::cell::{rows_to_strings, Attrs, Color, EmuCell};
use crate::terminal::locator::{self, Pattern};

pub struct Engine {
    name: String,
    operations: Mutex<()>,
    session: Mutex<Option<Session>>,
    live: Arc<Mutex<Option<LiveTarget>>>,
    logger: Arc<Logger>,
    recording_path: PathBuf,
}

/// The current session's renderable state, shared with monitor threads so they
/// can read the live grid without contending on the session lock (which long
/// `wait`s hold).
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

/// One-line request description for the verbose log. `Open` redacts env values
/// (they may contain secrets) and reports only the variable count.
fn req_summary(req: &Request) -> String {
    match req {
        Request::Open {
            shell,
            program,
            profile,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            timeouts,
        } => format!(
            "Open {{ shell: {shell:?}, program: {program:?}, scrollback: {}, {cols}x{rows}, cwd: {cwd:?}, wait_ready: {wait_ready:?}, timeouts: {timeouts:?}, env: <{} vars> }}",
            profile.scrollback,
            env.len()
        ),
        other => format!("{other:?}"),
    }
}

impl Engine {
    pub fn new(name: String, logger: Arc<Logger>, recording_path: PathBuf) -> Self {
        Engine {
            name,
            operations: Mutex::new(()),
            session: Mutex::new(None),
            live: Arc::new(Mutex::new(None)),
            logger,
            recording_path,
        }
    }

    pub fn handle(&self, req: Request) -> (Response, bool) {
        let _operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.logger.enabled() {
            self.logger.event(&format!("req {}", req_summary(&req)));
        }
        match req {
            Request::Ping => (Response::ok(), false),
            Request::Shutdown => (Response::ok(), true),
            Request::Open {
                shell,
                program,
                profile,
                cols,
                rows,
                cwd,
                env,
                wait_ready,
                timeouts,
            } => (
                self.open(
                    shell, program, profile, cols, rows, cwd, env, wait_ready, timeouts,
                ),
                false,
            ),
            Request::Close => {
                *self.live.lock().unwrap() = None;
                if let Some(s) = self.lock_session().take() {
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
        profile: crate::profile::Profile,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        timeouts: TimeoutDefaults,
    ) -> Response {
        *self.live.lock().unwrap() = None;
        if let Some(previous) = self.lock_session().take() {
            previous.kill();
        }
        match Session::open(
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
        ) {
            Ok(s) => {
                let shell_pid = s.pid();
                let ready_timeout = open_ready_timeout(&s);
                let ready = if wait_ready.unwrap_or(program.is_none()) {
                    await_ready(&s, ready_timeout)
                } else {
                    s.state.lock().unwrap().tracker.is_ready()
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
                *self.lock_session() = Some(s);
                *self.live.lock().unwrap() = Some(live);
                Response::with(json!({
                    "shell_pid": shell_pid,
                    "session": self.name,
                    "ready": ready,
                    "recording": self.recording_path.to_string_lossy(),
                }))
            }

            Err(e) => Response::internal(format!("failed to open session: {e}")),
        }
    }

    fn status(&self) -> Response {
        let guard = self.lock_session();
        match guard.as_ref() {
            Some(s) => {
                let st = s.state.lock().unwrap();
                Response::with(json!({
                    "session": self.name,
                    "shell_pid": s.pid(),
                    "cols": s.cols,
                    "rows": s.rows,
                    "shell": s.shell.map(|sh| sh.as_str()),
                    "exited": st.exited,
                    "timeouts": effective_timeouts(s),
                }))
            }
            None => Response::with(json!({
                "session": self.name,
                "shell_pid": null,
            })),
        }
    }

    fn with_session<F: FnOnce(&mut Session) -> Response>(&self, f: F) -> Response {
        let mut guard = self.lock_session();
        match guard.as_mut() {
            Some(s) => f(s),
            None => Response::no_session(),
        }
    }

    pub fn frame(&self) -> Option<LiveFrame> {
        let live = self.live.lock().unwrap();
        live.as_ref().map(|target| {
            let state = target.state.lock().unwrap();
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

    pub fn is_open(&self) -> bool {
        self.lock_session().is_some()
    }

    pub fn recording_path(&self) -> &PathBuf {
        &self.recording_path
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<Session>> {
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
            if st.tracker.is_ready() {
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
        "cwd": st.tracker.cwd(),
        "last_command": st.tracker.last_command(),
        "last_exit": st.tracker.last_exit(),
        "exited": st.exited,
        "ready": st.tracker.is_ready(),
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
                out.push(cell_json(col, row, cell));
            }
        }
    }
    Response::with(json!({ "cells": out }))
}

/// One cell in wire form. Every attribute the neutral model carries is
/// reported, including ones no current backend can source, so a client can be
/// written against the full vocabulary rather than against alacritty.
fn cell_json(x: u16, y: u16, cell: &EmuCell) -> serde_json::Value {
    json!({
        "x": x,
        "y": y,
        "char": cell.ch.as_str(),
        "fg": color_json(cell.fg),
        "bg": color_json(cell.bg),
        "bold": cell.has(Attrs::BOLD),
        "dim": cell.has(Attrs::DIM),
        "italic": cell.has(Attrs::ITALIC),
        "inverse": cell.has(Attrs::INVERSE),
        "invisible": cell.has(Attrs::INVISIBLE),
        "strike": cell.has(Attrs::STRIKE),
        "blink": cell.has(Attrs::BLINK),
        "underline": cell.underline.is_underlined(),
        // Never null: an un-underlined cell is the "none" style, and an
        // underline that follows the text color is "default", the same
        // sentinel `fg` and `bg` use. A client can switch on the string
        // without a null check.
        "underline_style": cell.underline.name(),
        "underline_color": color_json(cell.underline_color),
    })
}

/// Wire form: `"default"`, a 256-color index, or `"#rrggbb"`. Named and
/// indexed colors both serialize to their palette index, so the split between
/// them stays internal and the language bindings are unaffected.
fn color_json(c: Option<Color>) -> serde_json::Value {
    match c {
        None => json!(crate::assert::color::DEFAULT),
        Some(Color::Rgb(r, g, b)) => json!(format!("#{r:02x}{g:02x}{b:02x}")),
        Some(c) => json!(c.to_index()),
    }
}

fn get(s: &Session, field: GetField) -> Response {
    let st = s.state.lock().unwrap();
    let value = match field {
        GetField::Command => json!(st.tracker.last_command()),
        GetField::Output => json!(st.tracker.last_output()),
        GetField::ExitCode => json!(st.tracker.last_exit()),
        GetField::Cwd => json!(st.tracker.cwd()),
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

fn awaiting_command_start(st: &TermState) -> bool {
    st.awaiting_start
        .is_some_and(|seen| st.tracker.started_count() == seen)
}

fn command_settled(s: &Session, baseline: u64) -> bool {
    const QUIET: Duration = Duration::from_millis(300);
    let st = s.state.lock().unwrap();
    if st.exited.is_some() {
        return true;
    }
    let tracker = &st.tracker;
    if !tracker.started() {
        return st.last_change.elapsed() >= QUIET;
    }
    if awaiting_command_start(&st) {
        return false;
    }
    tracker.finished_count() > baseline || !tracker.executing()
}

fn wait_command(s: &Session, timeout_ms: u64) -> Response {
    let baseline = s.state.lock().unwrap().tracker.finished_count();
    if poll_until(|| command_settled(s, baseline), timeout_ms) {
        return Response::ok();
    }
    Response::assertion(format!(
        "wait command: timed out after {timeout_ms}ms; {}",
        stall_reason(s)
    ))
}

fn stall_reason(s: &Session) -> String {
    let st = s.state.lock().unwrap();
    if awaiting_command_start(&st) {
        "the shell never started a command for the input that was sent, so there \
         is nothing to wait for (was the line submitted?)"
            .to_string()
    } else {
        "the command was still running".to_string()
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
                if let Some(err) = check_colors(&cells, &fg, &bg, not, &s.profile.colors) {
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
    colors: &crate::profile::Colors,
) -> Option<String> {
    let want = !not;
    if let Some(spec) = fg {
        let expected = Expected::parse(spec).ok()?;
        for c in cells {
            if color::matches(c.cell.fg, &expected, colors) != want {
                return Some(format!(
                    "expected fg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(c.cell.fg, &expected, colors),
                    c.cell.ch,
                    c.x,
                    c.y
                ));
            }
        }
    }
    if let Some(spec) = bg {
        let expected = Expected::parse(spec).ok()?;
        for c in cells {
            if color::matches(c.cell.bg, &expected, colors) != want {
                return Some(format!(
                    "expected bg {} {}, found {} in cell '{}' at {},{}",
                    if not { "absent" } else { "present" },
                    expected.describe(),
                    color::describe_cell(c.cell.bg, &expected, colors),
                    c.cell.ch,
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
    let baseline = s.state.lock().unwrap().tracker.finished_count();
    if !poll_until(|| command_settled(s, baseline), timeout_ms) {
        return Response::assertion(format!(
            "expected exit code {code}: timed out after {timeout_ms}ms; {}",
            stall_reason(s)
        ));
    }
    let actual = s.state.lock().unwrap().tracker.last_exit();
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
        .tracker
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
            let svg = crate::render::svg::render_svg(&rows, s.cols, &s.profile.colors);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::cell::{NamedColor, UnderlineStyle};

    /// Every attribute in the vocabulary reaches the wire, including ones the
    /// alacritty backend can never source (blink), so clients written against
    /// the full model keep working when another backend starts reporting them.
    #[test]
    fn cell_json_reports_the_whole_vocabulary() {
        let cell = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Named(NamedColor::Red)),
            bg: Some(Color::Idx(196)),
            underline: UnderlineStyle::Curly,
            underline_color: Some(Color::Rgb(1, 2, 3)),
            attrs: Attrs::all(),
        };
        let v = cell_json(3, 4, &cell);
        assert_eq!(v["x"], json!(3));
        assert_eq!(v["char"], json!("x"));
        assert_eq!(v["fg"], json!(1));
        assert_eq!(v["bg"], json!(196));
        for key in [
            "bold",
            "dim",
            "italic",
            "inverse",
            "invisible",
            "strike",
            "blink",
            "underline",
        ] {
            assert_eq!(v[key], json!(true), "{key} must be reported");
        }
        assert_eq!(v["underline_style"], json!("curly"));
        assert_eq!(v["underline_color"], json!("#010203"));
    }

    /// The underline fields are never null, so a client can switch on the
    /// style string and compare the color the same way it does `fg`.
    #[test]
    fn cell_json_underline_fields_are_never_null() {
        let v = cell_json(0, 0, &EmuCell::blank());
        assert_eq!(v["underline"], json!(false));
        assert_eq!(v["underline_style"], json!("none"));
        assert_eq!(v["underline_color"], json!("default"));
        assert_eq!(v["blink"], json!(false));

        // Underlined, but with no color of its own: it follows the text color,
        // which is the same thing `fg: "default"` means.
        let cell = EmuCell {
            underline: UnderlineStyle::Single,
            underline_color: None,
            ..EmuCell::blank()
        };
        let v = cell_json(0, 0, &cell);
        assert_eq!(v["underline"], json!(true));
        assert_eq!(v["underline_style"], json!("single"));
        assert_eq!(v["underline_color"], json!("default"));
    }
}
