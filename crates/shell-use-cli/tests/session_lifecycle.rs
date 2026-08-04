//! End-to-end coverage for session lifecycle over the real cli + daemon.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_shell-use");

const CALL_TIMEOUT: Duration = Duration::from_secs(60);

static SANDBOX_SEQ: AtomicU32 = AtomicU32::new(0);

struct Sandbox {
    label: &'static str,
    home: PathBuf,
    session: String,
}

impl Sandbox {
    fn new(label: &'static str) -> Self {
        let id = format!(
            "{:x}-{:x}",
            std::process::id(),
            SANDBOX_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let home = std::env::temp_dir().join(format!("su{id}"));
        std::fs::create_dir_all(&home).expect("create sandbox home");
        Sandbox {
            label,
            session: format!("s{id}"),
            home,
        }
    }

    /// Captures output to catch daemon-inherited stdout pipe hangs.
    fn try_run(&self, args: &[&str]) -> Option<Output> {
        let session = self.session.clone();
        let home = self.home.clone();
        let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = Command::new(BIN)
                .args(["--session", &session])
                .args(&owned)
                .env("SHELL_USE_HOME", &home)
                .output();
            let _ = tx.send(out);
        });
        match rx.recv_timeout(CALL_TIMEOUT) {
            Ok(Ok(out)) => Some(out),
            Ok(Err(e)) => {
                eprintln!("could not spawn `shell-use {}`: {e}", args.join(" "));
                None
            }
            Err(_) => None,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.try_run(args).unwrap_or_else(|| {
            panic!(
                "[{}] `shell-use {}` produced no result within {:?}. Either it could not be \
                 spawned (see stderr above), or the cli process exited but left its stdout pipe \
                 open, which happens when the detached daemon inherits the cli's standard handles.",
                self.label,
                args.join(" "),
                CALL_TIMEOUT
            )
        })
    }

    /// Run without any explicit or environment session target.
    fn run_untargeted(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env("SHELL_USE_HOME", &self.home)
            .env_remove("SHELL_USE_SESSION")
            .output()
            .expect("spawn shell-use")
    }

    fn run_as(&self, suffix: &str, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(["--session", &format!("{}-{suffix}", self.session)])
            .args(args)
            .env("SHELL_USE_HOME", &self.home)
            .env_remove("SHELL_USE_SESSION")
            .output()
            .expect("spawn shell-use")
    }

    fn ok_as(&self, suffix: &str, args: &[&str]) -> String {
        let out = self.run_as(suffix, args);
        assert!(
            out.status.success(),
            "[{}] `shell-use {}` (session {suffix}) failed with {:?}\nstderr: {}",
            self.label,
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "[{}] `shell-use {}` failed with {:?}\nstdout: {}\nstderr: {}",
            self.label,
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Sandbox {
    /// Best-effort teardown must not double-panic during unwinding.
    fn drop(&mut self) {
        let _ = self.try_run(&["close"]);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[test]
fn sandbox_paths_fit_in_a_unix_socket_address() {
    const SUN_PATH_MAX: usize = 103;
    const MACOS_TMPDIR: usize = 49;

    let sandbox = Sandbox::new("path-budget");
    let home = sandbox
        .home
        .file_name()
        .expect("sandbox home has a name")
        .to_string_lossy()
        .len();
    let socket = format!("{}-three.sock", sandbox.session).len();
    let total = MACOS_TMPDIR + home + 1 + socket;
    assert!(
        total <= SUN_PATH_MAX,
        "a sandbox socket would be {total} bytes on macOS; shorten the naming"
    );
}

/// Repeats `close` to catch the final-response drain race.
#[test]
fn close_always_reports_success() {
    let sandbox = Sandbox::new("close");
    for attempt in 0..5 {
        sandbox.ok(&["open"]);
        let out = sandbox.run(&["close"]);
        assert!(
            out.status.success(),
            "close #{attempt} failed with {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// Auto-started daemons must not inherit stdout and keep captures waiting for EOF.
#[test]
fn capturing_output_terminates_after_the_daemon_starts() {
    let sandbox = Sandbox::new("capture");
    let stdout = sandbox.ok(&["open"]);
    assert!(
        stdout.contains("\"session\""),
        "expected the open payload on stdout, got: {stdout}"
    );
    sandbox.ok(&["text"]);
}

#[test]
fn close_is_idempotent() {
    let sandbox = Sandbox::new("idempotent");
    sandbox.ok(&["open"]);
    sandbox.ok(&["close"]);
    sandbox.ok(&["close"]);
}

#[test]
fn wait_ready_succeeds_on_an_open_shell() {
    let sandbox = Sandbox::new("ready");
    sandbox.ok(&["open"]);
    sandbox.ok(&["wait", "ready", "--timeout", "30000"]);
}

/// A ready timeout is an assertion failure, not a crash.
#[test]
fn wait_ready_times_out_as_an_assertion() {
    let sandbox = Sandbox::new("ready-timeout");
    let mut args = vec!["run"];
    args.extend(sleeper());
    sandbox.ok(&args);

    let out = sandbox.run(&["wait", "ready", "--timeout", "300"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected an assertion exit code, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn wait_ready_without_a_session_reports_no_session() {
    let sandbox = Sandbox::new("ready-nosession");
    let out = sandbox.run(&["wait", "ready", "--timeout", "1"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected exit 3, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A session timeout default must apply to later commands without `--timeout`.
#[test]
fn a_session_timeout_default_applies_to_later_commands() {
    let sandbox = Sandbox::new("session-default");
    sandbox.ok(&["open", "--timeout-text", "300"]);

    let started = Instant::now();
    let out = sandbox.run(&["wait", "text", "text-that-never-appears"]);
    let elapsed = started.elapsed();

    assert_eq!(
        out.status.code(),
        Some(1),
        "expected an assertion failure, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    let mut baseline = Duration::ZERO;
    for _ in 0..3 {
        let round_trip = Instant::now();
        sandbox.ok(&["state"]);
        baseline = baseline.max(round_trip.elapsed());
    }
    assert!(
        elapsed < baseline + Duration::from_millis(2_500),
        "the 300ms session default was ignored; the wait took {elapsed:?} \
         against a {baseline:?} round-trip, which suggests it fell back to the \
         5s built-in",
    );
}

/// The color a screenshot paints is the color an assertion matches.
///
/// These came from two separate hardcoded tables that disagreed on every ANSI
/// slot, so `expect --fg "#800000"` passed on a cell the screenshot painted
/// `#e88388`. Both now resolve through the session profile, and this drives the
/// whole path — daemon, renderer, assertion — rather than the resolver alone.
#[test]
fn a_screenshot_and_an_assertion_agree_on_a_color() {
    let sandbox = Sandbox::new("palette-agree");
    // Printed lowercase so the match is the output, not the echoed command.
    let print_red = r#"printf "\033[31m%s\033[0m\n" "$(echo QRSX | tr A-Z a-z)""#;
    sandbox.ok(&["run", "--cols", "44", "--", "bash", "--norc"]);
    sandbox.ok(&["submit", print_red]);
    sandbox.ok(&["wait", "command"]);

    // The default profile is the VGA palette, so slot 1 is #800000.
    sandbox.ok(&["expect", "text", "qrsx", "--fg", "#800000"]);

    let svg = sandbox.home.join("shot.svg");
    let path = svg.to_str().expect("utf-8 path");
    sandbox.ok(&["screenshot", "--out", path]);
    let drawing = std::fs::read_to_string(&svg).expect("read screenshot");
    assert!(
        drawing.contains("fill=\"#800000\""),
        "the screenshot must paint the color the assertion matched"
    );
}

/// A profile's palette drives both, so recoloring a slot moves the screenshot
/// and the assertion together.
#[test]
fn a_custom_profile_recolors_screenshots_and_assertions_together() {
    let sandbox = Sandbox::new("palette-profile");
    let config = sandbox.home.join("custom.toml");
    std::fs::write(&config, "[profiles.neon.colors]\nred = \"#ff00ff\"\n").expect("write config");
    let config_path = config.to_str().expect("utf-8 path");

    let print_red = r#"printf "\033[31m%s\033[0m\n" "$(echo QRSX | tr A-Z a-z)""#;
    sandbox.ok(&[
        "run",
        "--config",
        config_path,
        "--profile",
        "neon",
        "--cols",
        "44",
        "--",
        "bash",
        "--norc",
    ]);
    sandbox.ok(&["submit", print_red]);
    sandbox.ok(&["wait", "command"]);

    sandbox.ok(&["expect", "text", "qrsx", "--fg", "#ff00ff"]);
    let out = sandbox.run(&["expect", "text", "qrsx", "--fg", "#800000"]);
    assert!(
        !out.status.success(),
        "the profile replaced the default red, so the default must no longer match"
    );

    let svg = sandbox.home.join("neon.svg");
    let path = svg.to_str().expect("utf-8 path");
    sandbox.ok(&["screenshot", "--out", path]);
    let drawing = std::fs::read_to_string(&svg).expect("read screenshot");
    assert!(
        drawing.contains("fill=\"#ff00ff\""),
        "the screenshot follows the profile too"
    );
}

/// A profile that does not exist is an error naming the ones that do, rather
/// than a session that silently ran with the defaults.
#[test]
fn an_unknown_profile_is_rejected() {
    let sandbox = Sandbox::new("palette-unknown");
    let config = sandbox.home.join("c.toml");
    std::fs::write(&config, "[profiles.ci]\n").expect("write config");
    let out = sandbox.run(&[
        "open",
        "--config",
        config.to_str().expect("utf-8 path"),
        "--profile",
        "nope",
    ]);
    assert!(!out.status.success(), "an unknown profile must not open");
    let msg = String::from_utf8_lossy(&out.stderr) + String::from_utf8_lossy(&out.stdout);
    assert!(
        msg.contains("ci"),
        "the error should name the real profile: {msg}"
    );
}

/// A program that asks the terminal what color it is gets an answer.
///
/// This is how tools decide whether they are on a light or a dark background.
/// A terminal that stays silent leaves them blocked until they time out and
/// guess, so this drives the whole path: daemon, emulator, and the reply on
/// its way back up the PTY.
///
/// Unix only, because the probe has to put its own terminal in raw mode to
/// read a reply that arrives without a newline and must not be echoed, and
/// `termios` does not exist on Windows CPython. The reply itself is not
/// platform specific: how it is formatted is covered by conformance cases
/// that run against every backend, and the write that carries it to the child
/// is the same `pty.write` every `type` and `submit` on Windows already uses.
#[cfg(unix)]
#[test]
fn a_color_query_is_answered_over_the_pty() {
    let sandbox = Sandbox::new("osc-query");
    let probe = sandbox.home.join("probe.py");
    std::fs::write(
        &probe,
        r#"
import os, sys, termios, tty, select

# Unbuffered reads: a buffered reader would take bytes off the fd that
# select() then cannot see, and the reply would look truncated.
def ask(fd, query):
    os.write(1, query)
    buf = b""
    while select.select([fd], [], [], 2.0)[0]:
        buf += os.read(fd, 64)
        if buf.endswith(b"\x07"):
            break
    return buf.decode("utf8", "replace")

fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    configured = ask(fd, b"\x1b]11;?\x07")
    # Every dynamic colour, not just the background: a program that sets the
    # foreground and cursor has to be answered about those too.
    os.write(1, b"\x1b]10;#abcdef\x07\x1b]11;#654321\x07\x1b]12;#fedcba\x07")
    fg = ask(fd, b"\x1b]10;?\x07")
    overridden = ask(fd, b"\x1b]11;?\x07")
    cursor = ask(fd, b"\x1b]12;?\x07")
    os.write(1, b"\x1b]111\x07")
    restored = ask(fd, b"\x1b]11;?\x07")
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)

strip = lambda s: s.replace("\x1b", "").replace("\x07", "")
print("\r\nRESULT %s %s %s %s %s\r" % (
    strip(configured), strip(fg), strip(overridden), strip(cursor), strip(restored)))
"#,
    )
    .expect("write probe");

    // Wide enough that the report is one unwrapped line: `text` returns the
    // grid, so a wrapped reply would be split across rows.
    sandbox.ok(&["run", "--cols", "200", "--", "bash", "--norc"]);
    sandbox.ok(&[
        "submit",
        &format!("python3 {}", probe.to_str().expect("utf-8 path")),
    ]);
    // Wait for the line this test reads, not for the command.
    //
    // The probe prints nothing until it is done: its queries go to the
    // terminal, which answers them rather than echoing them, so the screen
    // stays unchanged for as long as python takes to start. `bash --norc` has
    // no shell integration, so `wait command` falls back to "the prompt came
    // back and the screen is idle", and on a loaded machine an idle screen
    // arrives long before the report does.
    sandbox.ok(&["wait", "text", "RESULT", "--timeout", "30000"]);
    let text = sandbox.ok(&["text", "--full"]);

    let line = text
        .lines()
        .find(|l| l.contains("RESULT"))
        .unwrap_or_else(|| panic!("the probe never reported: {text}"));

    // The default profile's background is black, so the terminal reports it,
    // then the color the program set, then the configured one again.
    assert!(
        line.contains("]11;rgb:0000/0000/0000"),
        "the configured background should be reported: {line}"
    );
    assert!(
        line.contains("]11;rgb:6565/4343/2121"),
        "a set background should be reported back: {line}"
    );
    assert!(
        line.contains("]10;rgb:abab/cdcd/efef"),
        "a set foreground should be reported back: {line}"
    );
    assert!(
        line.contains("]12;rgb:fefe/dcdc/baba"),
        "a set cursor color should be reported back: {line}"
    );
    assert_eq!(
        line.matches("]11;rgb:0000/0000/0000").count(),
        2,
        "a reset should restore the configured background: {line}"
    );
}

#[test]
fn state_reports_effective_timeouts() {
    let sandbox = Sandbox::new("state-timeouts");
    sandbox.ok(&["open", "--timeout-text", "1234"]);

    let json = sandbox.ok(&["--json", "state"]);
    assert!(
        json.contains("\"timeouts\"") && json.contains("1234"),
        "expected the configured text timeout in --json state: {json}"
    );

    let human = sandbox.ok(&["state"]);
    assert!(
        human.contains("timeouts:") && human.contains("1234"),
        "expected the configured text timeout in plain state: {human}"
    );
    assert!(
        human.contains("cwd:"),
        "plain state should report its other fields, not just the screen: {human}"
    );
}

#[test]
fn open_reports_the_daemon_pid_the_child_and_readiness() {
    let sandbox = Sandbox::new("open-payload");
    let raw = sandbox.ok(&["--json", "open"]);
    let payload: serde_json::Value = serde_json::from_str(&raw).expect("open json");
    let data = &payload["data"];

    let pid = data["pid"].as_u64().expect("a daemon pid");
    let recorded = std::fs::read_to_string(sandbox.home.join(format!("{}.pid", sandbox.session)))
        .expect("read pid file");
    assert_eq!(
        recorded.trim(),
        pid.to_string(),
        "`open` should report the daemon pid, matching `daemon status`: {payload}"
    );
    assert!(
        data["shell_pid"].as_u64().is_some_and(|c| c != pid),
        "the child pid belongs under shell_pid: {payload}"
    );
    assert_eq!(
        data["ready"].as_bool(),
        Some(true),
        "a shell session should report a prompt: {payload}"
    );
}

#[test]
fn explicit_wait_ready_fails_when_no_prompt_is_reported() {
    let sandbox = Sandbox::new("run-wait-ready");
    let mut args = vec!["run", "--wait-ready", "--timeout-ready", "700"];
    args.extend(sleeper());

    let started = Instant::now();
    let out = sandbox.run(&args);
    let elapsed = started.elapsed();

    assert_eq!(
        out.status.code(),
        Some(1),
        "a program with no shell integration never reports a prompt, so an \
         explicit --wait-ready must fail; got {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no prompt"),
        "the failure should say why: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Terminal content:"),
        "the failure should show the screen it gave up on: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "--timeout-ready should cap the wait, but it took {elapsed:?}"
    );

    let after = sandbox.run(&["text"]);
    assert_eq!(
        after.status.code(),
        Some(3),
        "the failed open left a session behind: {}",
        String::from_utf8_lossy(&after.stdout),
    );
}

#[test]
fn run_without_wait_ready_returns_immediately() {
    let sandbox = Sandbox::new("run-no-wait");
    let mut args = vec!["--json", "run"];
    args.extend(sleeper());

    let started = Instant::now();
    let raw = sandbox.ok(&args);
    let elapsed = started.elapsed();

    let payload: serde_json::Value = serde_json::from_str(&raw).expect("run json");
    assert_eq!(
        payload["data"]["ready"].as_bool(),
        Some(false),
        "a program with no shell integration is not ready: {payload}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "`run` should not wait for a prompt, but it took {elapsed:?}"
    );
}

fn exit_with(code: i32) -> String {
    if cfg!(windows) {
        format!("cmd /c exit {code}")
    } else {
        format!("(exit {code})")
    }
}

/// Printed by [`slow_exit_with`] the moment the shell starts executing it.
const RUN_MARKER: &str = "command-is-running";

/// The marker is assembled from two literals so the joined text only reaches
/// the screen when the shell *executes* the line, not when it echoes it back.
/// Until then the session still looks like an idle prompt carrying the previous
/// command's exit code, so an assertion issued too early settles against it.
fn slow_exit_with(code: i32) -> String {
    if cfg!(windows) {
        format!("echo ('command-is-'+'running'); Start-Sleep -Seconds 6; cmd /c exit {code}")
    } else {
        format!("echo \"command-is-\"\"running\"; sleep 6; (exit {code})")
    }
}

fn sleeper() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["cmd", "/c", "timeout /t 30 /nobreak >nul"]
    } else {
        vec!["sleep", "30"]
    }
}

fn interactive_reader() -> &'static str {
    if cfg!(windows) {
        "Write-Output ('reader-'+'ready'); $null = Read-Host"
    } else {
        "echo \"reader-\"\"ready\"; read answer"
    }
}

fn start_command_with_stale_exit(sandbox: &Sandbox) {
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", &exit_with(3)]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&["submit", &slow_exit_with(9)]);
    sandbox.ok(&["wait", "text", RUN_MARKER, "--timeout", "15000"]);
}

/// Must wait for the current command instead of accepting a stale exit code.
#[test]
fn expect_exit_code_waits_for_the_current_command() {
    let sandbox = Sandbox::new("exit-code-stale");
    start_command_with_stale_exit(&sandbox);
    let out = sandbox.run(&["expect", "exit-code", "3", "--timeout", "20000"]);

    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(1),
        "the stale exit code 3 was accepted while the running command exits 9: {stderr}",
    );
    assert!(
        stderr.contains("got 9"),
        "expected the running command's code in the failure, got: {stderr}"
    );
}

/// Timing out must not fall back to a stale exit code.
#[test]
fn expect_exit_code_timing_out_does_not_accept_a_stale_code() {
    let sandbox = Sandbox::new("exit-code-timeout");
    start_command_with_stale_exit(&sandbox);
    let out = sandbox.run(&["expect", "exit-code", "3", "--timeout", "300"]);

    assert_eq!(
        out.status.code(),
        Some(1),
        "the stale exit code 3 was accepted after the wait timed out: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("still running"),
        "expected the failure to say the command had not finished, got: {stderr}"
    );
}

#[test]
fn unsubmitted_input_never_settles_as_a_finished_command() {
    let sandbox = Sandbox::new("unsubmitted");
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", &exit_with(3)]);
    sandbox.ok(&["wait", "command", "--timeout", "20000"]);

    sandbox.ok(&["type", "echo not-submitted"]);

    let out = sandbox.run(&["expect", "exit-code", "3", "--timeout", "600"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the previous command's exit code was accepted for input that never ran: {stderr}",
    );
    assert!(
        stderr.contains("never started a command"),
        "the failure should explain that nothing was submitted, got: {stderr}"
    );

    let out = sandbox.run(&["wait", "command", "--timeout", "600"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "`wait command` has nothing to wait for: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn input_consumed_by_a_running_command_does_not_stall_completion_waits() {
    let sandbox = Sandbox::new("running-input");
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", interactive_reader()]);
    sandbox.ok(&["wait", "text", "reader-ready", "--timeout", "15000"]);

    sandbox.ok(&["submit", "typed-answer"]);
    sandbox.ok(&["wait", "command", "--timeout", "5000"]);
    sandbox.ok(&["expect", "exit-code", "0", "--timeout", "5000"]);
}

#[test]
fn expect_exit_code_fails_promptly_when_nothing_ran() {
    let sandbox = Sandbox::new("exit-code-idle");
    sandbox.ok(&["open"]);

    let started = Instant::now();
    let out = sandbox.run(&["expect", "exit-code", "0", "--timeout", "20000"]);
    let elapsed = started.elapsed();

    assert_eq!(
        out.status.code(),
        Some(1),
        "expected an assertion failure, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "no command had run, so the assertion should not wait for the full \
         budget; it took {elapsed:?}",
    );
}

#[test]
fn expect_exit_code_is_immediate_once_the_command_has_finished() {
    let sandbox = Sandbox::new("exit-code-fast");
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", "echo settled-marker"]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&["wait", "text", "settled-marker", "--timeout", "15000"]);

    let mut baseline = Duration::ZERO;
    for _ in 0..3 {
        let started = Instant::now();
        sandbox.ok(&["state"]);
        baseline = baseline.max(started.elapsed());
    }

    let started = Instant::now();
    sandbox.ok(&["expect", "exit-code", "0"]);
    let elapsed = started.elapsed();

    assert!(
        elapsed < baseline + Duration::from_millis(250),
        "the command had already finished, so this should settle on the \
         completion marker rather than waiting out the 300ms quiet window; \
         it took {elapsed:?} against a {baseline:?} round-trip",
    );
}

#[test]
fn a_repainting_prompt_neither_stalls_nor_short_circuits_exit_codes() {
    if !has_nushell() {
        eprintln!("skipping: nushell is not installed");
        return;
    }
    let sandbox = Sandbox::new("nu-repaint");
    sandbox.ok(&["open", "--shell", "nushell", "--timeout-ready", "20000"]);

    sandbox.ok(&["submit", "print repaint-marker"]);
    sandbox.ok(&["wait", "command", "--timeout", "20000"]);

    let mut baseline = Duration::ZERO;
    for _ in 0..3 {
        let started = Instant::now();
        sandbox.ok(&["state"]);
        baseline = baseline.max(started.elapsed());
    }
    let started = Instant::now();
    sandbox.ok(&["expect", "exit-code", "0"]);
    let elapsed = started.elapsed();
    assert!(
        elapsed < baseline + Duration::from_millis(500),
        "a repainting prompt must not stall a settled exit code; it took \
         {elapsed:?} against a {baseline:?} round-trip",
    );

    sandbox.ok(&["submit", "sleep 15sec; exit 7"]);
    let out = sandbox.run(&["expect", "exit-code", "0", "--timeout", "1500"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the prompt repaint after our input was mistaken for the command \
         finishing, so the stale exit code 0 was accepted: {stderr}",
    );
    assert!(
        stderr.contains("still running"),
        "expected the failure to say the command had not finished, got: {stderr}"
    );
}

fn has_nushell() -> bool {
    Command::new("nu")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------------------
// `daemon stop` targeting
// ---------------------------------------------------------------------------

/// A bare `daemon stop` is a usage error and must not start or stop anything.
#[test]
fn daemon_stop_without_a_target_is_a_usage_error() {
    let sandbox = Sandbox::new("stop-untargeted");
    sandbox.ok(&["open"]);

    let out = sandbox.run_untargeted(&["daemon", "stop"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected a usage error, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--all") && stderr.contains("--session"),
        "the error should name both ways to pick a target: {stderr}"
    );

    // The refusal must be inert: the running daemon is untouched.
    assert!(
        sandbox.ok(&["sessions"]).contains(&sandbox.session),
        "a rejected `daemon stop` must not stop anything"
    );

    let empty = Sandbox::new("stop-inert");
    let out = empty.run_untargeted(&["daemon", "stop"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        empty.ok(&["sessions"]).trim(),
        "no active sessions",
        "refusing to stop must not spawn a daemon"
    );
}

#[test]
fn daemon_stop_all_stops_every_daemon() {
    let sandbox = Sandbox::new("stop-all");
    for name in ["one", "two", "three"] {
        sandbox.ok_as(name, &["open"]);
    }
    let listed = sandbox.ok(&["sessions"]);
    for name in ["one", "two", "three"] {
        assert!(listed.contains(name), "expected {name} in: {listed}");
    }

    let out = sandbox.run_untargeted(&["daemon", "stop", "--all"]);
    assert!(
        out.status.success(),
        "`daemon stop --all` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        sandbox.ok(&["sessions"]).trim(),
        "no active sessions",
        "--all must stop every daemon"
    );
}

#[test]
fn daemon_stop_reports_a_session_with_no_daemon() {
    let sandbox = Sandbox::new("stop-missing");
    let out = sandbox.run(&["daemon", "stop"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected a no-session exit, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(
        sandbox.ok(&["sessions"]).trim(),
        "no active sessions",
        "stopping a missing daemon must not start one"
    );
}

#[test]
fn daemon_stop_targets_a_single_session() {
    let sandbox = Sandbox::new("stop-one");
    sandbox.ok_as("keep", &["open"]);
    sandbox.ok_as("drop", &["open"]);

    let out = sandbox.run_as("drop", &["daemon", "stop"]);
    assert!(
        out.status.success(),
        "targeted stop failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let listed = sandbox.ok(&["sessions"]);
    assert!(
        listed.contains("keep") && !listed.contains("drop"),
        "expected only 'drop' to be stopped, got: {listed}"
    );
    sandbox.run_as("keep", &["close"]);
}

#[test]
fn daemon_stop_all_is_fine_with_nothing_running() {
    let sandbox = Sandbox::new("stop-all-empty");
    let out = sandbox.run_untargeted(&["daemon", "stop", "--all"]);
    assert!(out.status.success(), "expected exit 0 with nothing running");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("no daemons running"),
        "expected an explicit note that nothing was running"
    );
}

/// `daemon status` must not auto-start a daemon just to answer.
#[test]
fn daemon_status_does_not_start_a_daemon() {
    let sandbox = Sandbox::new("status-inert");
    let out = sandbox.run(&["daemon", "status"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected a no-session exit, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(
        sandbox.ok(&["sessions"]).trim(),
        "no active sessions",
        "`daemon status` must not spawn a daemon"
    );
}

#[test]
fn daemon_status_reports_not_running_as_json() {
    let sandbox = Sandbox::new("status-json");
    let out = sandbox.run(&["--json", "daemon", "status"]);
    assert_eq!(out.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"running\"") && stdout.contains("false"),
        "expected a machine-readable payload, got: {stdout}"
    );
}

/// `daemon start` is the client spawn path and is idempotent.
#[test]
fn daemon_start_is_idempotent_and_makes_status_answer() {
    let sandbox = Sandbox::new("start");
    sandbox.ok(&["daemon", "start"]);
    let status = sandbox.ok(&["daemon", "status"]);
    assert!(
        status.contains("version"),
        "a started daemon should answer status: {status}"
    );

    let again = sandbox.ok(&["daemon", "start"]);
    assert!(
        again.contains("already running"),
        "a second start should report the daemon was already up: {again}"
    );
    sandbox.ok(&["daemon", "stop"]);
}

#[test]
fn daemon_start_leaves_the_socket_ready() {
    let sandbox = Sandbox::new("start-ready");
    sandbox.ok(&["daemon", "start"]);
    // No `open` in between: this is exactly what a client does after spawning.
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", "echo started-ok"]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&["expect", "text", "started-ok", "--no-strict"]);
}

/// Status `pid` is the daemon, not the child, so idle daemons are not `pid: null`.
#[test]
fn status_reports_the_daemon_pid_not_the_child() {
    let sandbox = Sandbox::new("status-pid");

    // No daemon: null, and explicitly flagged as not running.
    let down = sandbox.run(&["--json", "daemon", "status"]);
    assert_eq!(down.status.code(), Some(3));
    let down = String::from_utf8_lossy(&down.stdout);
    assert!(
        down.contains("\"pid\":null") && down.contains("\"running\":false"),
        "expected a null pid while nothing is running: {down}"
    );

    // Daemon up but no session yet: the daemon's own pid, and no child.
    sandbox.ok(&["daemon", "start"]);
    let idle = sandbox.ok(&["--json", "daemon", "status"]);
    let idle: serde_json::Value = serde_json::from_str(&idle).expect("status json");
    let pid = idle["data"]["pid"].as_u64().unwrap_or_else(|| {
        panic!("a running daemon must report its own pid, got: {idle}");
    });
    assert!(
        idle["data"]["shell_pid"].is_null(),
        "no session is open, so there is no child: {idle}"
    );

    // The reported pid is really the daemon: it matches the pid file.
    let recorded = std::fs::read_to_string(sandbox.home.join(format!("{}.pid", sandbox.session)))
        .expect("read pid file");
    assert_eq!(
        recorded.trim(),
        pid.to_string(),
        "the reported pid should be the daemon process"
    );

    // Once a session exists the child shows up separately, and `pid` is
    // unchanged — the daemon did not restart.
    sandbox.ok(&["open"]);
    let live = sandbox.ok(&["--json", "daemon", "status"]);
    let live: serde_json::Value = serde_json::from_str(&live).expect("status json");
    assert_eq!(live["data"]["pid"].as_u64(), Some(pid));
    assert!(
        live["data"]["shell_pid"].as_u64().is_some_and(|c| c != pid),
        "the child pid should be reported separately: {live}"
    );
}

/// A program that sets the window title is tracked, asserted on, and drawn.
///
/// This drives the whole path in one session: the emulator picking `OSC 2` out
/// of the PTY stream, the getter, the assertion, the screenshot, and the reset
/// that an empty title performs. Each of those is unit tested on its own; what
/// only an end-to-end run proves is that a title set by a real program in a
/// real shell arrives intact.
///
/// It deliberately does not assert what the title is *before* the program sets
/// one. A session does not necessarily start without a title: Windows ConPTY
/// supplies the program's path (`C:\Program Files\Git\bin\bash.EXE`) as soon as
/// the session opens. That a fresh emulator reports no title, and that an empty
/// one resets rather than storing a blank, are claims about the emulator rather
/// than about the platform, so they are pinned in the conformance suite where
/// no PTY is involved and every backend is covered.
#[test]
fn a_window_title_is_tracked_asserted_and_drawn() {
    let sandbox = Sandbox::new("title");
    sandbox.ok(&["run", "--cols", "40", "--", "bash", "--norc"]);
    let before = sandbox.ok(&["get", "title"]);

    sandbox.ok(&["submit", r#"printf '\033]2;vim: notes.md\007'"#]);
    sandbox.ok(&["expect", "title", "vim", "--timeout", "5000"]);
    sandbox.ok(&["expect", "title", "notes\\.\\w+", "--regex"]);
    sandbox.ok(&["expect", "title", "emacs", "--not"]);
    assert_ne!(
        sandbox.ok(&["get", "title"]),
        before,
        "the title the program set replaced whatever the session started with"
    );

    // The title is drawn in the window chrome, not in the grid.
    let svg = sandbox.home.join("titled.svg");
    sandbox.ok(&["screenshot", "--out", svg.to_str().expect("utf-8 path")]);
    let image = std::fs::read_to_string(&svg).expect("read svg");
    assert!(
        image.contains(">vim: notes.md</text>") && image.contains(r#"text-anchor="middle""#),
        "the title is drawn centred in the title bar: {image}"
    );

    // An empty title clears it, which is how programs tidy up on exit.
    sandbox.ok(&["submit", r#"printf '\033]2;\007'"#]);
    sandbox.ok(&["wait", "title", "vim", "--not", "--timeout", "5000"]);
}

/// A snapshot leaves the window title out unless it is asked for.
///
/// A shell prompt routinely sets the title to a username, hostname, and
/// absolute path, so recording it by default would pin every stored baseline
/// to one machine and make it change on `cd` while the screen stayed the same.
#[test]
fn a_snapshot_records_the_title_only_when_asked() {
    let sandbox = Sandbox::new("snap-title");
    // Wide enough that the title is not truncated, so the assertion is about
    // whether it was recorded at all rather than about how it was shortened.
    sandbox.ok(&["run", "--cols", "40", "--", "bash", "--norc"]);
    sandbox.ok(&[
        "submit",
        r#"clear; printf '\033]2;ayman@host: /some/path\007'"#,
    ]);
    sandbox.ok(&["expect", "title", "ayman@host", "--timeout", "5000"]);

    let plain = sandbox.ok(&["expect", "snapshot", "plain", "-u"]);
    assert!(!plain.contains("ayman@host"), "default keeps the title out");
    let stored = std::fs::read_to_string(
        std::env::current_dir()
            .expect("cwd")
            .join("__snapshots__/plain.snap"),
    )
    .expect("read snapshot");
    assert!(
        stored.starts_with("╭────") && !stored.contains("ayman@host"),
        "the border is plain, so a baseline is not tied to a machine: {stored}"
    );

    sandbox.ok(&["expect", "snapshot", "titled", "-u", "--include-title"]);
    let titled = std::fs::read_to_string(
        std::env::current_dir()
            .expect("cwd")
            .join("__snapshots__/titled.snap"),
    )
    .expect("read snapshot");
    assert!(
        titled.contains("ayman@host: /some/path"),
        "asking for it puts it in the border: {titled}"
    );

    for name in ["plain", "titled"] {
        let _ = std::fs::remove_file(
            std::env::current_dir()
                .expect("cwd")
                .join(format!("__snapshots__/{name}.snap")),
        );
    }
}
