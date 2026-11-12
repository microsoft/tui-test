//! Checked input through the public [`tui_test::Session`] API.
//!
//! Unlike `tests/checked_input.rs`, which drives [`tui_test::engine::Engine`]
//! directly, every test here calls `mouse_checked`, `paste`, and `focus`
//! through the public session type only, so this file fails to compile if
//! those delegates are missing. One test also runs under a non-default
//! execution context.
//!
//! The fixture child (shared with the engine-level suite in `common`)
//! enables terminal modes on request and reports every stdin byte back as
//! `rx:<hex>` text, so each test observes exactly what the child received.
//! A `@@command@@` token in the input stream changes modes or prints filler
//! instead of being echoed.

use std::time::{Duration, Instant};

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, CheckedInputError, CheckedMouseAction,
    ExecutionContext, MouseButton, MouseOptions, OpenOptions, Operation, OperationResult,
    RunOptions, Session, Timeouts,
};

mod common;

const FIXTURE_ENV: &str = "TUI_TEST_PUBLIC_SESSION_FIXTURE";

#[test]
fn fixture_child() {
    let Ok(spec) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    common::run_fixture(&spec);
}

fn checked_session(label: &str) -> Session {
    Session::new(format!("public-session-{label}-{}", std::process::id()))
}

fn run_options(spec: &str) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        program: std::env::current_exe()
            .expect("test binary path")
            .to_string_lossy()
            .into_owned(),
        args: [
            "--exact",
            "fixture_child",
            "--nocapture",
            "--test-threads=1",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        backend: defaults.backend,
        profile: defaults.profile,
        style: defaults.style,
        cols: 80,
        rows: 24,
        cwd: None,
        env: vec![(FIXTURE_ENV.to_string(), spec.to_string())],
        wait_ready: Some(false),
        restart: false,
        timeouts: Timeouts::default(),
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
    }
}

fn screen_text(session: &Session) -> String {
    match session
        .execute(Operation::Text { full: false })
        .expect("screen text")
    {
        OperationResult::Text(text) => text,
        other => panic!("expected screen text, got {other:?}"),
    }
}

fn wait_for_text(session: &Session, needle: &str) {
    let started = Instant::now();
    while !screen_text(session).contains(needle) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for {needle:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rx_count(session: &Session) -> usize {
    screen_text(session).matches("rx:").count()
}

fn mouse_mode(session: &Session) -> String {
    match session.execute(Operation::State).expect("session state") {
        OperationResult::State(state) => state.mouse_mode,
        other => panic!("expected session state, got {other:?}"),
    }
}

fn wait_for_mouse_mode(session: &Session, mode: &str) {
    let started = Instant::now();
    while mouse_mode(session) != mode {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for mouse mode {mode:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start_session(label: &str, spec: &str) -> Session {
    let session = checked_session(label);
    session
        .execute(Operation::Run(run_options(spec)))
        .expect("fixture session starts");
    wait_for_text(&session, "READY");
    session
}

fn send_command(session: &Session, command: &str) {
    session
        .execute(Operation::Write {
            data: format!("@@{command}@@"),
        })
        .expect("fixture command write");
}

fn left() -> MouseOptions {
    MouseOptions::new(MouseButton::Left)
}

// R10: the public Session exposes mouse_checked, paste, and focus, including
// under a non-default execution context.
#[test]
fn public_session_checked_input_with_context() {
    let session = start_session("public-api", "mouse-click,sgr-on,paste-on,focus-on")
        .with_execution_context(ExecutionContext {
            operation_name: Some("public-session-probe".to_string()),
            ..Default::default()
        });
    // The context travels with the clone; the delegates ignore it otherwise.
    session
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("press through public Session");
    wait_for_text(&session, "rx:1b5b3c303b353b334d");
    session
        .mouse_checked(CheckedMouseAction::Release {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("release through public Session");
    wait_for_text(&session, "rx:1b5b3c303b353b336d");
    session.paste("hi").expect("paste through public Session");
    wait_for_text(&session, "rx:1b5b3230307e68691b5b3230317e");
    session.focus(true).expect("focus through public Session");
    wait_for_text(&session, "rx:1b5b49");
    session.focus(false).expect("blur through public Session");
    wait_for_text(&session, "rx:1b5b4f");
    session.execute(Operation::Close).expect("session closes");
}

// R12: X10 (mode 9) tracking reports button presses only, so a checked
// release must fail before writing any byte.
#[test]
fn public_session_rejects_x10_release_before_write() {
    let session = start_session("public-x10", "x10-on,sgr-on");
    session
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect("X10 press sends");
    wait_for_text(&session, "rx:1b5b3c303b323b324d");
    let written = rx_count(&session);
    let error = session
        .mouse_checked(CheckedMouseAction::Release {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect_err("X10 release is never reported");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        rx_count(&session),
        written,
        "X10 release wrote bytes to the child"
    );
    session.execute(Operation::Close).expect("session closes");
}

// R12: enabling an unsupported tracking selector (1001) must clear the
// previously negotiated level instead of leaving it stale.
#[test]
fn public_session_unsupported_tracking_clears_stale_mode() {
    let session = start_session("public-stale", "mouse-click,sgr-on");
    session
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect("press under 1000 sends");
    wait_for_text(&session, "rx:");

    send_command(&session, "hl-on");
    wait_for_mouse_mode(&session, "none");
    let written = rx_count(&session);
    let error = session
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect_err("unsupported 1001 tracking rejects the press");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        rx_count(&session),
        written,
        "unsupported tracking wrote bytes to the child"
    );

    // Disabling 1001 leaves tracking off rather than restoring the old level.
    send_command(&session, "hl-off");
    std::thread::sleep(Duration::from_millis(300));
    let error = session
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect_err("tracking stays off after 1001 is disabled");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );

    // Re-enabling a supported level works again.
    send_command(&session, "mouse-click");
    wait_for_mouse_mode(&session, "click");
    session
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: left(),
        })
        .expect("press works after 1000 is re-enabled");
    session.execute(Operation::Close).expect("session closes");
}
