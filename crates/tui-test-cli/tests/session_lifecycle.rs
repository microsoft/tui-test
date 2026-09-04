//! End-to-end coverage for session lifecycle over the real cli + daemon.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced};
use tui_test::Backend;

const BIN: &str = env!("CARGO_BIN_EXE_tui-test");

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
        self.try_run_in(None, args)
    }

    fn try_run_in(&self, cwd: Option<PathBuf>, args: &[&str]) -> Option<Output> {
        let session = self.session.clone();
        let home = self.home.clone();
        let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut command = Command::new(BIN);
            command
                .args(["--session", &session])
                .args(&owned)
                .env("TUI_TEST_HOME", &home);
            if let Some(cwd) = cwd {
                command.current_dir(cwd);
            }
            let out = command.output();
            let _ = tx.send(out);
        });
        match rx.recv_timeout(CALL_TIMEOUT) {
            Ok(Ok(out)) => Some(out),
            Ok(Err(e)) => {
                eprintln!("could not spawn `tui-test {}`: {e}", args.join(" "));
                None
            }
            Err(_) => None,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(None, args)
    }

    fn run_in(&self, cwd: Option<&std::path::Path>, args: &[&str]) -> Output {
        self.try_run_in(cwd.map(std::path::Path::to_path_buf), args)
            .unwrap_or_else(|| {
                panic!(
                    "[{}] `tui-test {}` produced no result within {:?}. Either it could not be \
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
            .env("TUI_TEST_HOME", &self.home)
            .env_remove("TUI_TEST_SESSION")
            .output()
            .expect("spawn tui-test")
    }

    fn run_as(&self, suffix: &str, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(["--session", &format!("{}-{suffix}", self.session)])
            .args(args)
            .env("TUI_TEST_HOME", &self.home)
            .env_remove("TUI_TEST_SESSION")
            .output()
            .expect("spawn tui-test")
    }

    fn ok_as(&self, suffix: &str, args: &[&str]) -> String {
        let out = self.run_as(suffix, args);
        assert!(
            out.status.success(),
            "[{}] `tui-test {}` (session {suffix}) failed with {:?}\nstderr: {}",
            self.label,
            args.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn ok(&self, args: &[&str]) -> String {
        self.ok_in(None, args)
    }

    fn wait_for_text(&self, text: &str, timeout: &str) {
        self.ok(&[
            "expect",
            "text",
            text,
            "--match",
            "first",
            "--timeout",
            timeout,
        ]);
    }

    fn ok_in(&self, cwd: Option<&std::path::Path>, args: &[&str]) -> String {
        let out = self.run_in(cwd, args);
        assert!(
            out.status.success(),
            "[{}] `tui-test {}` failed with {:?}\nstdout: {}\nstderr: {}",
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

/// Connect to the session socket and send one request line, leaving the stream
/// open for whatever the request streams next.
fn monitor_stream(sandbox: &Sandbox, request: &str) -> interprocess::local_socket::Stream {
    let raw = if cfg!(windows) {
        format!("tui-test-{}.sock", sandbox.session)
    } else {
        sandbox
            .home
            .join(format!("{}.sock", sandbox.session))
            .to_string_lossy()
            .into_owned()
    };
    let name = if cfg!(windows) {
        raw.to_ns_name::<GenericNamespaced>()
    } else {
        raw.to_fs_name::<GenericFilePath>()
    }
    .expect("valid session socket name");
    let mut stream =
        interprocess::local_socket::Stream::connect(name).expect("connect session socket");
    stream
        .write_all(request.as_bytes())
        .expect("send monitor request");
    stream.flush().expect("flush monitor request");
    stream
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

/// One monitor holds two streams open: rendered frames out and raw input in.
/// Neither needs a target to exist, and the input stream outlives a restart.
#[test]
fn monitor_frames_and_input_outlive_the_target() {
    let sandbox = Sandbox::new("monitor-input");
    sandbox.ok(&["--verbose", "daemon", "start"]);

    let mut frames = monitor_stream(
        &sandbox,
        "{\"kind\":\"monitor\",\"cols\":80,\"rows\":24,\"interactive\":true}\n",
    );
    let (frame_tx, frame_rx) = std::sync::mpsc::channel();
    let (detach_tx, detach_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let read = frames.read(&mut buffer).expect("read monitor frame");
        frame_tx.send(read).expect("report monitor frame");
        let _ = detach_rx.recv();
    });
    assert!(
        frame_rx.recv_timeout(Duration::from_secs(5)).unwrap_or(0) > 0,
        "monitor did not receive a frame"
    );

    let mut input = monitor_stream(
        &sandbox,
        "{\"kind\":\"monitor_input_stream\",\"cols\":80,\"rows\":30}\n",
    );
    input.write_all(b"ignored").expect("write without target");
    input.flush().expect("flush without target");
    std::thread::sleep(Duration::from_millis(100));

    let secret = "human-secret-monitor-input";
    sandbox.ok(&["open"]);
    input
        .write_all(format!("echo {secret}\r").as_bytes())
        .expect("write to first target");
    input.flush().expect("flush first target input");
    sandbox.wait_for_text(secret, "5000");

    sandbox.ok(&["open", "--restart"]);
    input
        .write_all(b"echo restarted-monitor-marker\r")
        .expect("write to restarted target");
    input.flush().expect("flush restarted target input");
    sandbox.wait_for_text("restarted-monitor-marker", "5000");

    // The keystrokes are the human's, so they stay out of the agent's log.
    let log = std::fs::read_to_string(sandbox.home.join(format!("{}.log", sandbox.session)))
        .expect("read verbose log");
    assert!(
        !log.lines()
            .any(|line| line.contains("WRITE") && line.contains(secret)),
        "monitor keystrokes appeared in the verbose write log: {log}"
    );

    // Typing at an exited child is a normal race, not a daemon failure.
    sandbox.ok(&["submit", "exit"]);
    sandbox.ok(&["wait", "exit", "--timeout", "20000"]);
    input.write_all(b"x").expect("write after exit");
    input.flush().expect("flush after exit");
    sandbox.ok(&["daemon", "status"]);

    detach_tx.send(()).expect("detach monitor");
    reader.join().expect("join monitor reader");
}

/// The accept loop must not park behind a long operation: viewer keystrokes
/// have to reach the child while the agent is still waiting on it.
#[test]
fn monitor_input_is_delivered_while_a_long_operation_is_running() {
    let sandbox = Sandbox::new("monitor-input-concurrent");
    sandbox.ok(&["open"]);
    let marker = "monitor-input-concurrent-marker";
    let session = sandbox.session.clone();
    let home = sandbox.home.clone();
    let waiter = std::thread::spawn(move || {
        Command::new(BIN)
            .args([
                "--session",
                &session,
                "expect",
                "text",
                marker,
                "--match",
                "first",
                "--timeout",
                "5000",
            ])
            .env("TUI_TEST_HOME", home)
            .output()
            .expect("spawn long wait")
    });
    std::thread::sleep(Duration::from_millis(200));

    let started = Instant::now();
    let _input = monitor_stream(
        &sandbox,
        &format!("{{\"kind\":\"monitor_input_stream\",\"cols\":80,\"rows\":30}}\necho {marker}\r"),
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "monitor input waited behind the long operation"
    );

    let output = waiter.join().expect("join long wait");
    assert!(
        output.status.success(),
        "long wait did not observe monitor input: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn get_recording_flushes_queued_output_before_reading() {
    let sandbox = Sandbox::new("recording-flush");
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", "echo recording-flush-marker"]);
    sandbox.ok(&["wait", "command", "--timeout", "30000"]);

    let recording = sandbox.ok(&["get-recording"]);
    assert!(recording.contains("recording-flush-marker"));
}

#[test]
fn relative_recording_path_uses_the_invoking_client_directory() {
    let sandbox = Sandbox::new("recording-client-cwd");
    let daemon_cwd = sandbox.home.join("daemon-cwd");
    let client_cwd = sandbox.home.join("client-cwd");
    std::fs::create_dir_all(&daemon_cwd).unwrap();
    std::fs::create_dir_all(&client_cwd).unwrap();

    sandbox.ok_in(Some(&daemon_cwd), &["open"]);
    sandbox.ok_in(Some(&client_cwd), &["record", "start", "relative.cast"]);
    let stopped = sandbox.ok_in(Some(&client_cwd), &["--json", "record", "stop"]);
    let stopped: serde_json::Value = serde_json::from_str(&stopped).unwrap();

    let expected = client_cwd.join("relative.cast");
    assert!(
        expected.is_file(),
        "recording was not written to {expected:?}"
    );
    assert!(!daemon_cwd.join("relative.cast").exists());
    let actual = std::path::PathBuf::from(stopped["data"]["path"].as_str().unwrap());
    assert!(actual.is_absolute());
    assert_eq!(
        std::fs::canonicalize(actual).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

#[test]
fn close_is_idempotent() {
    let sandbox = Sandbox::new("idempotent");
    sandbox.ok(&["open"]);
    sandbox.ok(&["close"]);
    sandbox.ok(&["close"]);
}

#[test]
fn open_reuses_a_live_child_unless_restart_is_requested() {
    let sandbox = Sandbox::new("open-reuse");
    let first = sandbox.ok(&["--json", "open"]);
    let first: serde_json::Value = serde_json::from_str(&first).expect("first open json");
    let first_pid = first["data"]["shell_pid"]
        .as_u64()
        .expect("first open reports a child pid");

    let reused = sandbox.ok(&["--json", "open"]);
    let reused: serde_json::Value = serde_json::from_str(&reused).expect("reused open json");
    assert_eq!(
        reused["data"]["shell_pid"].as_u64(),
        Some(first_pid),
        "a second open should attach to the live child"
    );

    let restarted = sandbox.ok(&["--json", "open", "--restart"]);
    let restarted: serde_json::Value =
        serde_json::from_str(&restarted).expect("restarted open json");
    assert_ne!(
        restarted["data"]["shell_pid"].as_u64(),
        Some(first_pid),
        "--restart should replace the live child"
    );
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

#[test]
fn bell_count_wait_and_expect_are_exposed_over_the_cli() {
    for &backend in Backend::ALL {
        let sandbox = Sandbox::new("bells");
        sandbox.ok(&["open", "--backend", backend.as_str()]);

        sandbox.ok(&["submit", &two_bells_command()]);
        sandbox.ok(&["expect", "bell", "2", "--timeout", "5000"]);
        sandbox.ok(&["wait", "command"]);

        let state = sandbox.ok(&["state"]);
        assert!(state.contains("bell_count: 2"), "{state}");
        assert!(!state.contains("bell_events"), "{state}");

        let response: serde_json::Value =
            serde_json::from_str(&sandbox.ok(&["--json", "get", "bell-events"]))
                .expect("parse bell events response");
        let events = response["data"]["value"]
            .as_array()
            .expect("bell events array");
        assert_eq!(events.len(), 2, "{}", backend.as_str());
        assert_eq!(events[0]["sequence"], 1, "{}", backend.as_str());
        assert_eq!(events[1]["sequence"], 2, "{}", backend.as_str());
        assert!(
            events[1]["elapsed_ms"].as_u64().expect("second timestamp")
                >= events[0]["elapsed_ms"].as_u64().expect("first timestamp")
        );

        for _ in 0..2 {
            let response: serde_json::Value =
                serde_json::from_str(&sandbox.ok(&["--json", "get", "bells"]))
                    .expect("parse bell count response");
            assert_eq!(response["data"]["value"], 2, "{}", backend.as_str());
        }

        sandbox.ok(&["submit", &delayed_bell_command()]);
        sandbox.ok(&["wait", "bell", "--timeout", "5000"]);
        sandbox.ok(&["expect", "bell", "3", "--timeout", "5000"]);
        let response: serde_json::Value =
            serde_json::from_str(&sandbox.ok(&["--json", "get", "bells"]))
                .expect("parse final bell count response");
        assert_eq!(response["data"]["value"], 3, "{}", backend.as_str());
    }
}

#[test]
fn clipboard_getter_and_change_wait_are_exposed_over_the_cli() {
    let sandbox = Sandbox::new("clipboard");
    sandbox.ok(&["open"]);

    let initial: serde_json::Value =
        serde_json::from_str(&sandbox.ok(&["--json", "get", "clipboard"]))
            .expect("parse initial clipboard response");
    assert_eq!(initial["data"]["value"], "");

    let timeout = sandbox.run(&["wait", "clipboard", "--timeout", "100"]);
    assert_eq!(timeout.status.code(), Some(1));

    sandbox.ok(&["submit", &clipboard_command("Y2hhbmdlZA==")]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&["wait", "clipboard", "--timeout", "5000"]);
    let changed: serde_json::Value =
        serde_json::from_str(&sandbox.ok(&["--json", "get", "clipboard"]))
            .expect("parse changed clipboard response");
    assert_eq!(changed["data"]["value"], "changed");

    sandbox.ok(&["submit", &clipboard_command("cHJlZml4LXJlYWR5LTQy")]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&["wait", "clipboard", "ready", "--timeout", "5000"]);

    sandbox.ok(&["submit", &clipboard_command("YnVpbGQtMTIz")]);
    sandbox.ok(&["wait", "command"]);
    sandbox.ok(&[
        "wait",
        "clipboard",
        "^build-[0-9]+$",
        "--regex",
        "--timeout",
        "5000",
    ]);
}

/// A session timeout default must apply to later commands without `--timeout`.
#[test]
fn a_session_timeout_default_applies_to_later_commands() {
    let sandbox = Sandbox::new("session-default");
    sandbox.ok(&["open", "--timeout-text", "300"]);

    let started = Instant::now();
    let out = sandbox.run(&[
        "expect",
        "text",
        "text-that-never-appears",
        "--match",
        "first",
    ]);
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

#[test]
fn text_actions_share_one_selector_contract() {
    let sandbox = Sandbox::new("text-actions");
    sandbox.ok(&["open"]);
    let command = if cfg!(windows) {
        "[Console]::Write(([char]27+'[1mlocator    target'+[char]27+'[0m'+[Environment]::NewLine))"
    } else {
        "printf '\\033[1mlocator    target\\033[0m\\n'"
    };
    sandbox.ok(&["submit", command]);
    sandbox.ok(&[
        "expect",
        "text",
        "locator target",
        "--whitespace",
        "normalize",
        "--bold",
        "--match",
        "first",
        "--timeout",
        "5000",
    ]);
    sandbox.ok(&["wait", "command", "--timeout", "30000"]);

    let response: serde_json::Value = serde_json::from_str(&sandbox.ok(&[
        "--json",
        "find",
        "text",
        "locator target",
        "--whitespace",
        "normalize",
        "--bold",
    ]))
    .expect("parse text locations");
    assert!(
        response["data"]["matches"]
            .as_array()
            .is_some_and(|matches| !matches.is_empty()),
        "find text should return match locations: {response}"
    );

    let before = sandbox.home.join("before-highlight.svg");
    let after = sandbox.home.join("after-highlight.svg");
    sandbox.ok(&["screenshot", "--out", before.to_str().unwrap()]);
    sandbox.ok(&[
        "highlight",
        "text",
        "locator target",
        "--whitespace",
        "normalize",
        "--bold",
    ]);
    sandbox.ok(&["screenshot", "--out", after.to_str().unwrap()]);
    assert_ne!(
        std::fs::read_to_string(before).unwrap(),
        std::fs::read_to_string(after).unwrap(),
        "highlight should be visible in screenshots"
    );

    sandbox.ok(&[
        "click",
        "text",
        "locator target",
        "--whitespace",
        "normalize",
        "--match",
        "last",
        "--bold",
        "--timeout",
        "5000",
    ]);
}

#[test]
fn config_timeouts_apply_below_command_line_overrides() {
    let sandbox = Sandbox::new("config-timeouts");
    let config = sandbox.home.join("timeouts.toml");
    std::fs::write(
        &config,
        "[profiles.default.timeouts]\ntext = 1234\ncommand = 2345\n",
    )
    .expect("write config");

    sandbox.ok(&[
        "open",
        "--config",
        config.to_str().expect("utf-8 path"),
        "--timeout-text",
        "3456",
    ]);
    let raw = sandbox.ok(&["--json", "state"]);
    let payload: serde_json::Value = serde_json::from_str(&raw).expect("state json");
    assert_eq!(payload["data"]["timeouts"]["text"], 3456);
    assert_eq!(payload["data"]["timeouts"]["command"], 2345);
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
    let print_red = r#"printf "\033[31m%s\033[0m\n" "$(echo QRSX | tr A-Z a-z)"; sleep 30"#;
    sandbox.ok(&[
        "run", "--cols", "44", "--", "bash", "--norc", "-c", print_red,
    ]);

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

    let print_red = r#"printf "\033[31m%s\033[0m\n" "$(echo QRSX | tr A-Z a-z)"; sleep 30"#;
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
        "-c",
        print_red,
    ]);

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

#[test]
fn an_invalid_profile_color_is_a_usage_error_not_a_crash() {
    let sandbox = Sandbox::new("palette-invalid");
    let config = sandbox.home.join("invalid.toml");
    std::fs::write(&config, "[profiles.default.colors]\nred = \"éa\"\n").expect("write config");
    let out = sandbox.run(&["open", "--config", config.to_str().expect("utf-8 path")]);
    assert_eq!(out.status.code(), Some(2));
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("invalid hex color"),
        "the invalid color should be identified: {message}"
    );
    assert!(
        !message.contains("panicked"),
        "invalid config must not crash the cli: {message}"
    );
}

#[test]
fn a_missing_environment_config_is_rejected() {
    let sandbox = Sandbox::new("palette-env-missing");
    let missing = sandbox.home.join("missing.toml");
    let out = Command::new(BIN)
        .args(["--session", &sandbox.session, "open"])
        .env("TUI_TEST_HOME", &sandbox.home)
        .env("TUI_TEST_CONFIG", &missing)
        .output()
        .expect("spawn tui-test");

    assert_eq!(
        out.status.code(),
        Some(2),
        "explicit config is a usage error"
    );
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("missing.toml"),
        "the missing override should be named: {message}"
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
    sandbox.wait_for_text("RESULT", "30000");
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
fn automatic_recording_mode_and_directory_come_from_config() {
    let sandbox = Sandbox::new("recording-config");
    let config = sandbox.home.join("recording.toml");
    std::fs::write(
        &config,
        "[recording]\nmode = \"disabled\"\ndirectory = \"casts\"\n",
    )
    .unwrap();
    let config = config.to_str().unwrap();
    let raw = sandbox.ok(&["--json", "open", "--config", config, "--no-wait-ready"]);
    let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(payload["data"]["recording"], "");
    sandbox.ok(&["close"]);
}

#[test]
fn failed_open_recording_is_readable_before_close() {
    let sandbox = Sandbox::new("recording-failed-open");
    let config = sandbox.home.join("recording.toml");
    std::fs::write(
        &config,
        "[recording]\nmode = \"on-failure\"\ndirectory = \"casts\"\n",
    )
    .unwrap();
    let config = config.to_str().unwrap();
    let mut args = vec![
        "run",
        "--config",
        config,
        "--wait-ready",
        "--timeout-ready",
        "100",
    ];
    args.extend(sleeper());
    assert_eq!(sandbox.run(&args).status.code(), Some(1));

    let recording = sandbox.ok(&["get-recording", "--config", config]);
    assert!(recording.contains("\"version\":2"));
    sandbox.ok(&["close"]);
}

#[test]
fn failed_spawn_does_not_expose_a_previous_custom_recording() {
    let sandbox = Sandbox::new("recording-failed-spawn");
    let config = sandbox.home.join("recording.toml");
    std::fs::write(&config, "[recording]\ndirectory = \"casts\"\n").unwrap();
    let config = config.to_str().unwrap();
    sandbox.ok(&["open", "--config", config, "--no-wait-ready"]);
    sandbox.ok(&["close"]);

    std::fs::write(
        config,
        "[recording]\nmode = \"on-failure\"\ndirectory = \"casts\"\n",
    )
    .unwrap();
    assert!(!sandbox
        .run(&[
            "run",
            "--config",
            config,
            "tui-test-program-that-does-not-exist",
        ])
        .status
        .success());
    sandbox.ok(&["daemon", "stop"]);
    assert_eq!(
        sandbox
            .run(&["get-recording", "--config", config])
            .status
            .code(),
        Some(3)
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

fn two_bells_command() -> String {
    if cfg!(windows) {
        "[Console]::Out.Write([char]7); [Console]::Out.Write([char]7)".to_string()
    } else {
        "printf '\\a\\a'".to_string()
    }
}

fn delayed_bell_command() -> String {
    if cfg!(windows) {
        "Start-Sleep -Seconds 1; [Console]::Out.Write([char]7)".to_string()
    } else {
        "sleep 1; printf '\\a'".to_string()
    }
}

fn clipboard_command(base64: &str) -> String {
    if cfg!(windows) {
        format!(
            "[Console]::Out.Write(([char]27).ToString() + ']52;c;{base64}' + \
             ([char]7).ToString())"
        )
    } else {
        format!("printf '\\033]52;c;{base64}\\a'")
    }
}

fn blinking_program() -> Vec<&'static str> {
    if cfg!(windows) {
        vec![
            "pwsh",
            "-NoLogo",
            "-NoProfile",
            "-Command",
            "[Console]::Write(\"`e[5mX`e[0m\"); Start-Sleep -Seconds 30",
        ]
    } else {
        vec!["sh", "-c", "printf '\\033[5mX\\033[0m'; sleep 30"]
    }
}

fn backend_parity_program() -> Vec<&'static str> {
    if cfg!(windows) {
        vec![
            "pwsh",
            "-NoLogo",
            "-NoProfile",
            "-Command",
            r#"[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); $wide=[char]0x4F60; [Console]::Write("`e[2J`e[H`e]2;backend parity`a`e[1;3;4;31;44mRED`e[0m $wide`r`nline two`e[?25l"); Start-Sleep -Seconds 30"#,
        ]
    } else {
        vec![
            "sh",
            "-c",
            "printf '\\033[2J\\033[H\\033]2;backend parity\\007\\033[1;3;4;31;44mRED\\033[0m 你\\r\\nline two\\033[?25l'; sleep 30",
        ]
    }
}

#[test]
fn ghostty_backend_is_used_end_to_end() {
    let sandbox = Sandbox::new("ghostty-backend");
    let mut args = vec![
        "run",
        "--backend",
        "ghostty",
        "--cols",
        "10",
        "--rows",
        "2",
        "--",
    ];
    args.extend(blinking_program());
    sandbox.ok(&args);
    sandbox.wait_for_text("X", "5000");

    let raw = sandbox.ok(&["--json", "cells", "0", "0"]);
    let payload: serde_json::Value = serde_json::from_str(&raw).expect("cells json");
    assert_eq!(
        payload["data"]["cells"][0]["blink"],
        serde_json::Value::Bool(true),
        "Ghostty preserves SGR blink: {payload}"
    );
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
    sandbox.wait_for_text(RUN_MARKER, "15000");
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
    sandbox.wait_for_text("reader-ready", "15000");

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
    sandbox.wait_for_text("settled-marker", "15000");

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
        stderr.contains("still running") || stderr.contains("never started a command"),
        "expected the failure to say the command had not completed, got: {stderr}"
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
    let status = sandbox.ok(&["--json", "daemon", "status"]);
    let status: serde_json::Value = serde_json::from_str(&status).expect("daemon status json");
    assert!(
        status["data"]["version"].is_string(),
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
fn concurrent_daemon_starts_are_serialized() {
    let sandbox = Sandbox::new("start-race");
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let home = sandbox.home.clone();
            let session = sandbox.session.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Command::new(BIN)
                    .args(["--session", &session, "--json", "daemon", "start"])
                    .env("TUI_TEST_HOME", home)
                    .output()
                    .expect("spawn concurrent daemon start")
            })
        })
        .collect();
    barrier.wait();

    let mut started = 0;
    for worker in workers {
        let output = worker.join().unwrap();
        assert!(
            output.status.success(),
            "concurrent start failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("daemon start json");
        started += usize::from(payload["started"].as_bool() == Some(true));
    }
    assert_eq!(started, 1, "exactly one client should spawn the daemon");
}

#[test]
fn daemon_start_leaves_the_socket_ready() {
    let sandbox = Sandbox::new("start-ready");
    sandbox.ok(&["daemon", "start"]);
    // No `open` in between: this is exactly what a client does after spawning.
    sandbox.ok(&["open"]);
    sandbox.ok(&["submit", "echo started-ok"]);
    sandbox.ok(&["wait", "command"]);
    sandbox.wait_for_text("started-ok", "5000");
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
    for backend in Backend::ALL {
        let sandbox = Sandbox::new("title");
        sandbox.ok(&[
            "run",
            "--backend",
            backend.as_str(),
            "--cols",
            "40",
            "--",
            "bash",
            "--norc",
        ]);
        let before = sandbox.ok(&["get", "title"]);

        sandbox.ok(&["submit", r#"printf '\033]2;vim: notes.md\007'"#]);
        sandbox.ok(&["expect", "title", "vim", "--timeout", "5000"]);
        sandbox.ok(&["expect", "title", "notes\\.\\w+", "--regex"]);
        sandbox.ok(&["expect", "title", "emacs", "--not"]);
        assert_ne!(
            sandbox.ok(&["get", "title"]),
            before,
            "{} did not replace the session's initial title",
            backend.as_str()
        );

        // The title is drawn in the window chrome, not in the grid.
        let svg = sandbox.home.join("titled.svg");
        sandbox.ok(&[
            "screenshot",
            "--out",
            svg.to_str().expect("utf-8 path"),
            "--zoom",
            "0.5",
        ]);
        let image = std::fs::read_to_string(&svg).expect("read svg");
        assert!(
            image.contains(">vim: notes.md - 40x30</text>")
                && image.contains(r#"text-anchor="middle""#),
            "{} did not draw the title centred in the title bar: {image}",
            backend.as_str()
        );
        assert!(
            image.contains(r#"width="239" height="365" viewBox="0 0 478 730""#),
            "{} changed the SVG dimensions at zoom 0.5: {image}",
            backend.as_str()
        );

        // An empty title clears it, which is how programs tidy up on exit.
        sandbox.ok(&["submit", r#"printf '\033]2;\007'"#]);
        sandbox.ok(&["wait", "title", "vim", "--not", "--timeout", "5000"]);
    }
}

/// A snapshot leaves the window title out unless it is asked for.
///
/// A shell prompt routinely sets the title to a username, hostname, and
/// absolute path, so recording it by default would pin every stored baseline
/// to one machine and make it change on `cd` while the screen stayed the same.
#[test]
fn a_snapshot_records_the_title_only_when_asked() {
    for backend in Backend::ALL {
        let sandbox = Sandbox::new("snap-title");
        // Wide enough that the title is not truncated, so the assertion is
        // about whether it was recorded at all rather than how it was shortened.
        let set_title = r#"clear; printf '\033]2;tui-test-user@host: /some/path\007'; sleep 30"#;
        sandbox.ok(&[
            "run",
            "--backend",
            backend.as_str(),
            "--cols",
            "40",
            "--",
            "bash",
            "--norc",
            "-c",
            set_title,
        ]);
        sandbox.ok(&["expect", "title", "tui-test-user@host", "--timeout", "5000"]);

        let plain = sandbox.ok_in(Some(&sandbox.home), &["expect", "snapshot", "plain", "-u"]);
        assert!(
            !plain.contains("tui-test-user@host"),
            "{} included a title by default",
            backend.as_str()
        );
        let stored = std::fs::read_to_string(sandbox.home.join("__snapshots__/plain.snap"))
            .expect("read snapshot");
        assert!(
            stored.starts_with("╭────") && !stored.contains("tui-test-user@host"),
            "{} tied a default snapshot to the session title: {stored}",
            backend.as_str()
        );

        sandbox.ok_in(
            Some(&sandbox.home),
            &["expect", "snapshot", "titled", "-u", "--include-title"],
        );
        let titled = std::fs::read_to_string(sandbox.home.join("__snapshots__/titled.snap"))
            .expect("read snapshot");
        assert!(
            titled.contains("tui-test-user@host: /some/path"),
            "{} left the requested title out of the snapshot: {titled}",
            backend.as_str()
        );
    }
}

#[test]
fn terminal_backends_match_end_to_end_for_cells_state_and_snapshots() {
    let mut expected_cells = None;
    let mut expected_state = None;
    let mut expected_snapshot = None;

    for backend in Backend::ALL {
        let sandbox = Sandbox::new("backend-parity");
        let mut args = vec![
            "run",
            "--backend",
            backend.as_str(),
            "--cols",
            "12",
            "--rows",
            "3",
            "--",
        ];
        args.extend(backend_parity_program());
        sandbox.ok(&args);
        sandbox.wait_for_text("line two", "10000");
        sandbox.ok(&["expect", "title", "backend parity", "--timeout", "5000"]);

        let cells: serde_json::Value =
            serde_json::from_str(&sandbox.ok(&["--json", "cells", "0", "0", "6", "1"]))
                .expect("cells json");
        let cells = cells["data"]["cells"].clone();
        let row = cells.as_array().expect("cell array");
        assert_eq!(row[0]["char"], "R", "{} first cell", backend.as_str());
        assert_eq!(row[0]["fg"], 1, "{} named foreground", backend.as_str());
        assert_eq!(row[0]["bg"], 4, "{} named background", backend.as_str());
        assert_eq!(row[0]["bold"], true, "{} bold", backend.as_str());
        assert_eq!(row[0]["italic"], true, "{} italic", backend.as_str());
        assert_eq!(
            row[0]["underline_style"],
            "single",
            "{} underline",
            backend.as_str()
        );
        assert_eq!(row[4]["char"], "你", "{} wide cell", backend.as_str());
        assert_eq!(row[5]["char"], "", "{} wide continuation", backend.as_str());
        if let Some(expected) = &expected_cells {
            assert_eq!(
                &cells,
                expected,
                "{} produced different cells",
                backend.as_str()
            );
        } else {
            expected_cells = Some(cells);
        }

        let state: serde_json::Value =
            serde_json::from_str(&sandbox.ok(&["--json", "state"])).expect("state json");
        let data = &state["data"];
        assert_eq!(
            data["title"],
            "backend parity",
            "{} title",
            backend.as_str()
        );
        assert_eq!(data["bell_count"], 0, "{} OSC terminator", backend.as_str());
        let state = serde_json::json!({
            "cols": data["cols"],
            "rows": data["rows"],
            "cursor": data["cursor"],
            "title": data["title"],
            "bell_count": data["bell_count"],
            "text": data["text"],
        });
        if let Some(expected) = &expected_state {
            assert_eq!(
                &state,
                expected,
                "{} produced different state",
                backend.as_str()
            );
        } else {
            expected_state = Some(state);
        }

        sandbox.ok_in(
            Some(&sandbox.home),
            &[
                "expect",
                "snapshot",
                "backend-parity",
                "-u",
                "--include-colors",
                "--include-title",
            ],
        );
        let snapshot =
            std::fs::read_to_string(sandbox.home.join("__snapshots__/backend-parity.snap"))
                .expect("read parity snapshot");
        if let Some(expected) = &expected_snapshot {
            assert_eq!(
                &snapshot,
                expected,
                "{} produced a different snapshot",
                backend.as_str()
            );
        } else {
            expected_snapshot = Some(snapshot);
        }

        // Growing the viewport is backend-specific: Ghostty anchors existing
        // rows at the bottom, while Alacritty and Rio keep them at the top.
        // Exercise snapshot round-tripping, but compare its visual content
        // instead of reusing one backend's row layout as the shared baseline.
        sandbox.ok(&["resize", "16", "4"]);
        // ConPTY asynchronously redraws its screen after a resize. Wait for
        // that redraw so both halves of the snapshot round-trip see one frame.
        sandbox.ok(&["wait", "idle", "--timeout", "5000"]);
        sandbox.ok_in(
            Some(&sandbox.home),
            &[
                "expect",
                "snapshot",
                "backend-parity-resized",
                "-u",
                "--include-colors",
                "--include-title",
            ],
        );
        sandbox.ok_in(
            Some(&sandbox.home),
            &[
                "expect",
                "snapshot",
                "backend-parity-resized",
                "--include-colors",
                "--include-title",
            ],
        );
        let resized = std::fs::read_to_string(
            sandbox
                .home
                .join("__snapshots__/backend-parity-resized.snap"),
        )
        .expect("read resized parity snapshot");
        let state: serde_json::Value =
            serde_json::from_str(&sandbox.ok(&["--json", "state"])).expect("resized state json");
        let data = &state["data"];
        assert_eq!(data["cols"], 16, "{} resized columns", backend.as_str());
        assert_eq!(data["rows"], 4, "{} resized rows", backend.as_str());
        assert_eq!(
            data["title"],
            "backend parity",
            "{} resized title",
            backend.as_str()
        );
        let text = data["text"].as_str().expect("resized state text");
        assert!(
            text.contains("RED 你") && text.contains("line two"),
            "{} lost content during resize: {text:?}",
            backend.as_str()
        );
        assert!(
            resized.contains("backend parity")
                && resized.contains("RED 你")
                && resized.contains("line two")
                && resized.contains("\"fg\": 1")
                && resized.contains("\"bg\": 4"),
            "{} lost visual state in the resized snapshot: {resized}",
            backend.as_str()
        );
    }
}

#[test]
fn shell_integration_is_identical_across_terminal_backends() {
    for backend in Backend::ALL {
        let sandbox = Sandbox::new("backend-shell-integration");
        sandbox.ok(&["open", "--backend", backend.as_str()]);
        let command = if cfg!(windows) {
            "Write-Output ('backend-'+'shell-ok')"
        } else {
            "printf '%s\\n' backend-shell-ok"
        };
        sandbox.ok(&["submit", command]);
        sandbox.wait_for_text("backend-shell-ok", "10000");
        sandbox.ok(&["wait", "command"]);
        sandbox.ok(&["expect", "exit-code", "0"]);
        sandbox.ok(&["expect", "output", "backend-shell-ok"]);
    }
}
