mod agent_context;
mod cli;
mod config;
mod daemon;
mod ipc;
mod monitor;
mod protocol;
mod skill;

use std::path::Path;
use std::time::{Duration, Instant};

use clap::{CommandFactory, Parser};

use cli::{Cli, Command, DaemonCmd, ExpectCmd, GetArg, MouseCmd, WaitCmd};
use protocol::{GetField, MouseAction, Request, Response};
/// Long-form agent skill manifest, printed by `shell-use skill`.
const SKILL_MD: &str = include_str!("../../../SKILL.md");

fn main() {
    let cli = Cli::parse();
    let session = config::session_name_from_env(cli.session.clone());

    let Some(command) = cli.command else {
        let _ = Cli::command().print_help();
        std::process::exit(0);
    };

    let code = match command {
        Command::InternalDaemon => {
            if let Err(e) = daemon::run(session, cli.verbose) {
                eprintln!("daemon error: {e}");
                std::process::exit(5);
            }
            0
        }
        Command::Usage => {
            print!("{}", usage_text());
            0
        }
        Command::AgentContext => {
            println!("{}", agent_context::render());
            0
        }
        Command::Skill { add: true } => skill::add(SKILL_MD),
        Command::Skill { add: false } => {
            print!("{SKILL_MD}");
            0
        }
        Command::GetRecording { session: target } => get_recording(target.unwrap_or(session)),
        Command::Sessions => list_sessions(cli.json),
        Command::Close { all } if all => close_all(cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Start,
        } => daemon_start(&session, cli.verbose, cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Status,
        } => daemon_status(&session, cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Stop { all },
        } => daemon_stop(
            &session,
            all,
            config::session_was_specified(&cli.session),
            cli.json,
        ),
        Command::Monitor => monitor::run_client(&session),
        command => run_remote(&session, command, cli.json, cli.verbose),
    };
    std::process::exit(code);
}

/// Build the request for a daemon-backed command, then send it.
fn run_remote(session: &str, command: Command, json: bool, verbose: bool) -> i32 {
    let request = match build_request(command) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    let conn = match connect_to_daemon(session, verbose) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 4;
        }
    };
    match ipc::exchange(conn, &request) {
        Ok(resp) => print_response(&resp, json),
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

fn connect_to_daemon(session: &str, verbose: bool) -> anyhow::Result<ipc::Stream> {
    const ATTEMPTS: u32 = 3;
    let socket = config::socket_name(session);
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        ensure_daemon(session, verbose)
            .map_err(|e| anyhow::anyhow!("failed to start daemon: {e}"))?;
        match ipc::connect(&socket) {
            Ok(conn) => return Ok(conn),
            Err(e) => last = Some(e),
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let err = last.expect("at least one connect attempt");
    Err(anyhow::anyhow!("request failed: {err}"))
}

fn ready_flag(wait_ready: bool, no_wait_ready: bool) -> Option<bool> {
    match (wait_ready, no_wait_ready) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

fn build_request(command: Command) -> anyhow::Result<Request> {
    let req = match command {
        Command::Open {
            shell,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            no_wait_ready,
            timeouts,
        } => Request::Open {
            shell: shell.map(Into::into),
            program: None,
            cols,
            rows,
            cwd,
            env: parse_env(&env)?,
            wait_ready: ready_flag(wait_ready, no_wait_ready),
            timeouts: timeouts.into(),
        },
        Command::Run {
            program,
            args,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            no_wait_ready,
            timeouts,
        } => {
            let mut prog = vec![program];
            prog.extend(args);
            Request::Open {
                shell: None,
                program: Some(prog),
                cols,
                rows,
                cwd,
                env: parse_env(&env)?,
                wait_ready: ready_flag(wait_ready, no_wait_ready),
                timeouts: timeouts.into(),
            }
        }
        Command::Close { .. } => Request::Close,
        // Every `daemon` subcommand is handled in `main`: they decide for
        // themselves whether to start a daemon, and requests built here always
        // do.
        Command::Daemon { .. } => {
            anyhow::bail!("internal: `daemon` must be handled before build_request")
        }
        Command::State => Request::State,
        Command::Text { full } => Request::Text { full },
        Command::Screenshot { path, out, full } => Request::Screenshot {
            full,
            path: out.or(path),
        },
        Command::Cells { x, y, w, h } => Request::Cells { x, y, w, h },
        Command::Get { field } => Request::Get {
            field: map_field(field),
        },
        Command::Type { text } => Request::Write { data: text },
        Command::Submit { text } => Request::Submit { data: text },
        Command::Press { keys } => Request::Press { keys },
        Command::Keys { combo } => Request::Press { keys: vec![combo] },
        Command::Mouse { action } => Request::Mouse {
            action: map_mouse(action),
        },
        Command::Resize { cols, rows } => Request::Resize { cols, rows },
        Command::Write { data } => Request::Write { data },
        Command::Signal { name } => Request::Signal {
            name: name.as_str().to_string(),
        },
        Command::Kill => Request::Signal {
            name: "KILL".to_string(),
        },
        Command::Wait { what } => map_wait(what),
        Command::Expect { what } => map_expect(what),
        _ => anyhow::bail!("unsupported command"),
    };
    Ok(req)
}

fn map_field(f: GetArg) -> GetField {
    match f {
        GetArg::Command => GetField::Command,
        GetArg::Output => GetField::Output,
        GetArg::ExitCode => GetField::ExitCode,
        GetArg::Cwd => GetField::Cwd,
        GetArg::Cursor => GetField::Cursor,
        GetArg::Size => GetField::Size,
        GetArg::Bells => GetField::BellCount,
    }
}

fn map_mouse(action: MouseCmd) -> MouseAction {
    match action {
        MouseCmd::Click {
            x,
            y,
            on_text,
            button,
            clicks,
        } => MouseAction::Click {
            x,
            y,
            on_text,
            button,
            clicks,
        },
        MouseCmd::Move { x, y } => MouseAction::Move { x, y },
        MouseCmd::Down { x, y, button } => MouseAction::Down { x, y, button },
        MouseCmd::Up { x, y, button } => MouseAction::Up { x, y, button },
        MouseCmd::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
        } => MouseAction::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
        },
        MouseCmd::Scroll { direction, amount } => MouseAction::Scroll {
            direction: direction.as_str().to_string(),
            amount,
        },
    }
}

fn map_wait(what: WaitCmd) -> Request {
    match what {
        WaitCmd::Text {
            text,
            regex,
            full,
            not,
            timeout,
        } => Request::WaitText {
            text,
            regex,
            full,
            timeout_ms: timeout,
            not,
        },
        WaitCmd::Idle { timeout } => Request::WaitIdle {
            timeout_ms: timeout,
        },
        WaitCmd::Command { timeout } => Request::WaitCommand {
            timeout_ms: timeout,
        },
        WaitCmd::Exit { timeout } => Request::WaitExit {
            timeout_ms: timeout,
        },
        WaitCmd::Ready { timeout } => Request::WaitReady {
            timeout_ms: timeout,
        },
        WaitCmd::Bell { timeout } => Request::WaitBell {
            timeout_ms: timeout,
        },
    }
}

fn map_expect(what: ExpectCmd) -> Request {
    match what {
        ExpectCmd::Text {
            text,
            regex,
            full,
            no_strict,
            not,
            fg,
            bg,
            timeout,
        } => Request::ExpectText {
            text,
            regex,
            full,
            strict: !no_strict,
            not,
            fg,
            bg,
            timeout_ms: timeout,
        },
        ExpectCmd::ExitCode { code, timeout } => Request::ExpectExitCode {
            code,
            timeout_ms: timeout,
        },
        ExpectCmd::Output { text, regex } => Request::ExpectOutput { text, regex },
        ExpectCmd::Bell { count, timeout } => Request::ExpectBellCount {
            count,
            timeout_ms: timeout,
        },
        ExpectCmd::Snapshot {
            name,
            update,
            include_colors,
        } => Request::Snapshot {
            name,
            update,
            include_colors,
            cwd: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
        },
    }
}

fn parse_env(pairs: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    pairs
        .iter()
        .map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| anyhow::anyhow!("invalid env (expected KEY=VALUE): {p}"))
        })
        .collect()
}

/// Spawn the daemon for this session if it is not already running.
fn ensure_daemon(session: &str, verbose: bool) -> anyhow::Result<()> {
    let socket = config::socket_name(session);
    if ipc::is_running(&socket) {
        if verbose {
            eprintln!(
                "note: daemon for session '{session}' is already running; verbose logging only \
                 applies to a freshly started daemon. Run `shell-use --session {session} close` \
                 first, then retry with --verbose."
            );
        }
        return Ok(());
    }
    config::ensure_home()?;
    let exe = std::env::current_exe()?;
    spawn_detached(&exe, session, verbose)?;

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if ipc::is_running(&socket) {
            if verbose {
                eprintln!("daemon logging to {}", config::log_file(session).display());
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("daemon did not become ready")
}

#[cfg(windows)]
fn spawn_detached(exe: &Path, session: &str, verbose: bool) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    disown_std_handles();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__daemon").arg("--session").arg(session);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// Clear stdio inheritance so a detached Windows daemon cannot keep pipe EOF open.
#[cfg(windows)]
fn disown_std_handles() {
    use std::os::windows::io::AsRawHandle;

    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    extern "system" {
        fn SetHandleInformation(
            h_object: *mut std::ffi::c_void,
            dw_mask: u32,
            dw_flags: u32,
        ) -> i32;
    }

    let handles = [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ];
    for handle in handles {
        if !handle.is_null() {
            // Safety: the handle comes from the standard streams, which outlive
            // this call, and clearing the inherit flag never affects our own use.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

#[cfg(not(windows))]
fn spawn_detached(exe: &Path, session: &str, verbose: bool) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__daemon").arg("--session").arg(session);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// Stream a session's recording (asciinema v2 cast) to stdout.
fn get_recording(session: String) -> i32 {
    let path = config::recording_file(&session);
    match std::fs::read(&path) {
        Ok(bytes) => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(&bytes);
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no recording for session '{session}'");
            3
        }
        Err(e) => {
            eprintln!("failed to read recording: {e}");
            5
        }
    }
}

/// Every session in this home whose daemon is currently answering.
fn running_sessions() -> Vec<String> {
    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(config::home_dir()) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stripped) = name.strip_suffix(".pid") {
            if ipc::is_running(&config::socket_name(stripped)) {
                sessions.push(stripped.to_string());
            }
        }
    }
    sessions.sort();
    sessions
}

fn list_sessions(json: bool) -> i32 {
    let sessions = running_sessions();
    if json {
        println!("{}", serde_json::json!({ "sessions": sessions }));
    } else if sessions.is_empty() {
        println!("no active sessions");
    } else {
        for s in sessions {
            println!("{s}");
        }
    }
    0
}

fn close_all(json: bool) -> i32 {
    for name in running_sessions() {
        let _ = ipc::send(&config::socket_name(&name), &Request::Close);
    }
    if json {
        println!("{}", serde_json::json!({ "ok": true }));
    } else {
        println!("closed all sessions");
    }
    0
}

/// Start a session's daemon, returning only after the socket accepts connections.
fn daemon_start(session: &str, verbose: bool, json: bool) -> i32 {
    let running = ipc::is_running(&config::socket_name(session));
    if let Err(e) = ensure_daemon(session, verbose) {
        eprintln!("failed to start daemon: {e}");
        return 4;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "session": session, "started": !running })
        );
    } else if running {
        println!("daemon already running for session '{session}'");
    } else {
        println!("started daemon for session '{session}'");
    }
    0
}

/// Report on a session's daemon without starting one.
fn daemon_status(session: &str, json: bool) -> i32 {
    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        if json {
            println!(
                "{}",
                serde_json::json!({ "session": session, "running": false, "pid": null })
            );
        } else {
            eprintln!("no daemon running for session '{session}'");
        }
        return 3;
    }
    match ipc::send(&socket, &Request::Status) {
        Ok(resp) => print_response(&resp, json),
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

/// Stop targeted daemons without auto-starting anything first.
fn daemon_stop(session: &str, all: bool, targeted: bool, json: bool) -> i32 {
    if all {
        return stop_all_daemons(json);
    }
    if !targeted {
        eprintln!(
            "error: `daemon stop` needs a target; every session has its own daemon\n  \
             --session <NAME>  stop one session's daemon (env: SHELL_USE_SESSION)\n  \
             --all             stop every daemon\n\
             hint: `shell-use sessions` lists what is running"
        );
        return 2;
    }
    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        eprintln!("no daemon running for session '{session}'");
        return 3;
    }
    match ipc::send(&socket, &Request::Shutdown) {
        Ok(resp) if resp.ok => {
            report_stopped(&[session.to_string()], json);
            0
        }
        Ok(resp) => print_response(&resp, json),
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

fn stop_all_daemons(json: bool) -> i32 {
    let mut stopped = Vec::new();
    for name in running_sessions() {
        if ipc::send(&config::socket_name(&name), &Request::Shutdown).is_ok() {
            stopped.push(name);
        }
    }
    report_stopped(&stopped, json);
    0
}

fn report_stopped(stopped: &[String], json: bool) {
    if json {
        println!("{}", serde_json::json!({ "ok": true, "stopped": stopped }));
    } else if stopped.is_empty() {
        println!("no daemons running");
    } else {
        for name in stopped {
            println!("stopped daemon for session '{name}'");
        }
    }
}

fn print_response(resp: &Response, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::to_string(resp).unwrap_or_default());
        return exit_code(resp);
    }
    if resp.ok {
        if let Some(data) = &resp.data {
            println!("{}", format_data(data));
        }
        0
    } else {
        if let Some(msg) = &resp.message {
            eprintln!("{msg}");
        }
        exit_code(resp)
    }
}

/// Render a successful payload for a human, keeping text-only output bare.
fn format_data(data: &serde_json::Value) -> String {
    let text = data.get("text").and_then(|v| v.as_str());
    let Some(map) = data.as_object() else {
        return serde_json::to_string_pretty(data).unwrap_or_default();
    };
    match text {
        Some(text) if map.len() == 1 => text.to_string(),
        None => serde_json::to_string_pretty(data).unwrap_or_default(),
        Some(text) => {
            let mut out = String::new();
            for (key, value) in map {
                if key != "text" {
                    out.push_str(&format!("{key}: {}\n", compact(value)));
                }
            }
            out.push_str(text);
            out
        }
    }
}

/// One-line rendering of a field value, unquoted for plain strings.
fn compact(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn usage_text() -> &'static str {
    "shell-use: headless terminal cli + daemon\n\
\n\
SESSION   open [--shell S] [--cols N --rows N] [--cwd D] [--env K=V]\n\
          run <program> [args...]\n\
          sessions | close [--all] | daemon start|status | daemon stop --session N|--all\n\
INSPECT   state | text [--full] | screenshot [-o file.svg] [--full]\n\
          cells X Y [W H] | get command|output|exit-code|cwd|cursor|size|bells\n\
INPUT     type \"text\" | submit [\"text\"] | press <Key...> | keys \"Ctrl+a\"\n\
          mouse click X Y | mouse click --on-text \"OK\" | mouse move|down|up|drag|scroll\n\
PTY       resize COLS ROWS | write <data> | signal INT|TERM|KILL|QUIT | kill\n\
WAIT      wait text \"T\" [--regex --full --not --timeout MS]\n\
          wait idle | wait command | wait exit | wait ready | wait bell\n\
EXPECT    expect text \"T\" [--regex --full --not --fg C --bg C --timeout MS]\n\
          expect exit-code N | expect output \"T\" [--regex] | expect bell N\n\
          expect snapshot NAME [-u] [--include-colors]\n\
RECORD    sessions auto-record; get-recording [session] > out.cast (asciinema v2)\n\
          play with `asciinema play out.cast`, render GIF with `agg out.cast out.gif`\n\
WATCH     monitor (live full-color view in another terminal; q/Esc/Ctrl-C to detach)\n\
AGENT     agent-context (JSON cli schema) | skill [--add] (workflow guide)\n\
GLOBAL    --session NAME | --json | --verbose (log PTY traffic to ~/.shell-use/<session>.log)\n\
EXIT      0 ok | 1 assertion/wait failed | 2 usage | 3 no session | 4 daemon/IPC | 5 internal\n\
"
}

/// Map a response to a stable process exit code (see the exit-code taxonomy).
fn exit_code(resp: &Response) -> i32 {
    if resp.ok {
        0
    } else {
        resp.kind.map(|k| k.exit_code()).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_text_only_payload_prints_the_bare_screen() {
        assert_eq!(
            format_data(&json!({ "text": "hello\nworld" })),
            "hello\nworld"
        );
    }

    #[test]
    fn a_rich_payload_prints_its_fields_before_the_screen() {
        let rendered = format_data(&json!({
            "cwd": "/tmp",
            "cols": 80,
            "timeouts": { "text": 300 },
            "text": "screen",
        }));
        assert!(rendered.contains("cwd: /tmp"), "{rendered}");
        assert!(rendered.contains("cols: 80"), "{rendered}");
        assert!(rendered.contains("timeouts: {\"text\":300}"), "{rendered}");
        assert!(rendered.ends_with("screen"), "{rendered}");
    }

    #[test]
    fn a_payload_without_text_falls_back_to_json() {
        let rendered = format_data(&json!({ "pid": 42 }));
        assert!(rendered.contains("\"pid\""), "{rendered}");
        assert!(rendered.contains("42"), "{rendered}");
    }

    #[test]
    fn ready_flag_resolves_the_paired_switches() {
        assert_eq!(ready_flag(false, false), None);
        assert_eq!(ready_flag(true, false), Some(true));
        assert_eq!(ready_flag(false, true), Some(false));
    }
}
