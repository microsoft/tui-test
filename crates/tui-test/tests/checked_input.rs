//! U07: checked mouse, paste, and focus input (GOAL 11.7, corpus C13).
//!
//! The fixture child (shared with the public-session suite in
//! `common`) enables terminal modes on request and reports every stdin
//! byte back as `rx:<hex>` text, so each test observes exactly what the
//! child received. A `@@command@@` token in the input stream changes
//! modes or prints filler instead of being echoed.
//!
//! Test map: `mouse_modes_enabled_disabled_changed` and
//! `paste_and_focus_modes` cover U07-A; `click_reports_exact_bytes`,
//! `drag_reports_exact_bytes`, `motion_reports_exact_bytes`, and
//! `paste_and_focus_report_exact_bytes` cover U07-B;
//! `unsupported_encoding_fails_before_write`,
//! `out_of_bounds_fails_before_write`,
//! `history_viewport_fails_before_write`, and
//! `paste_terminator_rejected_before_write` cover U07-C;
//! `legacy_mouse_and_raw_write_keep_behavior` covers U07-D.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::engine::Engine;
use tui_test::logger::Logger;
use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, CheckedInputError, CheckedMouseAction, MouseAction,
    MouseButton, MouseOptions, OpenOptions, Operation, OperationResult, RunOptions, ScrollTarget,
    Timeouts,
};

mod common;

const FIXTURE_ENV: &str = "TUI_TEST_CHECKED_INPUT_FIXTURE";

#[test]
fn fixture_child() {
    let Ok(spec) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    common::run_fixture(&spec);
}

fn checked_engine(label: &str) -> Engine {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("checked-input-tests")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&dir).expect("test directory");
    Engine::new(
        format!("checked-input-{label}"),
        Arc::new(Logger::disabled()),
        dir.join("session.cast"),
    )
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

fn screen_text(engine: &Engine) -> String {
    match engine
        .execute(Operation::Text { full: false })
        .expect("screen text")
    {
        OperationResult::Text(text) => text,
        other => panic!("expected screen text, got {other:?}"),
    }
}

fn wait_for_text(engine: &Engine, needle: &str) {
    let started = Instant::now();
    while !screen_text(engine).contains(needle) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for {needle:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rx_count(engine: &Engine) -> usize {
    screen_text(engine).matches("rx:").count()
}

fn mouse_mode(engine: &Engine) -> String {
    match engine.execute(Operation::State).expect("session state") {
        OperationResult::State(state) => state.mouse_mode,
        other => panic!("expected session state, got {other:?}"),
    }
}

fn wait_for_mode(engine: &Engine, name: &str, enabled: bool) {
    let started = Instant::now();
    loop {
        let modes = match engine.execute(Operation::GetModes).expect("terminal modes") {
            OperationResult::Modes(modes) => modes,
            other => panic!("expected terminal modes, got {other:?}"),
        };
        if modes.get(name) == Some(&enabled) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for mode {name:?} to be {enabled}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_mouse_mode(engine: &Engine, mode: &str) {
    let started = Instant::now();
    while mouse_mode(engine) != mode {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for mouse mode {mode:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start_session(label: &str, spec: &str) -> Engine {
    let engine = checked_engine(label);
    engine
        .execute(Operation::Run(run_options(spec)))
        .expect("fixture session starts");
    wait_for_text(&engine, "READY");
    engine
}

fn send_command(engine: &Engine, command: &str) {
    engine
        .execute(Operation::Write {
            data: format!("@@{command}@@"),
        })
        .expect("fixture command write");
}

fn left() -> MouseOptions {
    MouseOptions::new(MouseButton::Left)
}

// U07-A: mouse tracking accepts press and release under click mode, accepts
// pressed movement under drag mode, accepts unpressed movement only under
// motion mode, and follows mode changes mid-session.
#[test]
fn mouse_modes_enabled_disabled_changed() {
    let engine = start_session("mouse-modes", "mouse-click,sgr-on");
    assert_eq!(mouse_mode(&engine), "click");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("press under click mode");
    wait_for_text(&engine, "rx:1b5b3c303b353b334d");

    let error = engine
        .mouse_checked(CheckedMouseAction::Motion { x: 6, y: 3 })
        .expect_err("click mode rejects unpressed movement");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    let error = engine
        .mouse_checked(CheckedMouseAction::Drag {
            x: 6,
            y: 3,
            options: left(),
        })
        .expect_err("click mode rejects pressed movement");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );

    send_command(&engine, "mouse-drag");
    wait_for_mouse_mode(&engine, "drag");
    engine
        .mouse_checked(CheckedMouseAction::Drag {
            x: 6,
            y: 3,
            options: left(),
        })
        .expect("pressed movement under drag mode");
    wait_for_text(&engine, "rx:1b5b3c33323b373b344d");
    let error = engine
        .mouse_checked(CheckedMouseAction::Motion { x: 6, y: 3 })
        .expect_err("drag mode rejects unpressed movement");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );

    send_command(&engine, "mouse-motion");
    wait_for_mouse_mode(&engine, "motion");
    engine
        .mouse_checked(CheckedMouseAction::Motion { x: 6, y: 3 })
        .expect("unpressed movement under motion mode");
    wait_for_text(&engine, "rx:1b5b3c33353b373b344d");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("press under motion mode");

    send_command(&engine, "mouse-off");
    wait_for_mouse_mode(&engine, "none");
    let error = engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect_err("disabled tracking rejects press");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    engine.execute(Operation::Close).expect("session closes");
}

// U07-A: bracketed paste sends raw text without markers while disabled and
// framed text while enabled; focus reports need focus events; both follow
// mode changes mid-session.
#[test]
fn paste_and_focus_modes() {
    let engine = start_session("paste-focus-modes", "none");
    engine.paste("ab").expect("paste without markers");
    wait_for_text(&engine, "rx:6162");
    assert!(
        !screen_text(&engine).contains("3230307e"),
        "disabled paste sends no bracketed-paste start marker",
    );
    let error = engine
        .focus(true)
        .expect_err("disabled focus events reject the report");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );

    send_command(&engine, "paste-on");
    wait_for_mode(&engine, "bracketed_paste", true);
    engine.paste("ab").expect("framed paste");
    wait_for_text(&engine, "rx:1b5b3230307e61621b5b3230317e");

    send_command(&engine, "paste-off");
    wait_for_mode(&engine, "bracketed_paste", false);
    engine.paste("ab").expect("raw paste after disable");
    wait_for_text(&engine, "rx:6162");

    send_command(&engine, "focus-on");
    wait_for_mode(&engine, "focus_events", true);
    engine.focus(true).expect("focus report");
    wait_for_text(&engine, "rx:1b5b49");
    send_command(&engine, "focus-off");
    wait_for_mode(&engine, "focus_events", false);
    let error = engine.focus(false).expect_err("focus report after disable");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    engine.execute(Operation::Close).expect("session closes");
}

// U07-B: a click sends the exact SGR press and release bytes.
#[test]
fn click_reports_exact_bytes() {
    let engine = start_session("click-bytes", "mouse-click,sgr-on");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("press sends");
    wait_for_text(&engine, "rx:1b5b3c303b353b334d");
    engine
        .mouse_checked(CheckedMouseAction::Release {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("release sends");
    wait_for_text(&engine, "rx:1b5b3c303b353b336d");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 1,
            y: 1,
            options: MouseOptions::new(MouseButton::Right).with_shift(),
        })
        .expect("modified press sends");
    wait_for_text(&engine, "rx:1b5b3c363b323b324d");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-B: pressed movement sends the exact SGR drag bytes.
#[test]
fn drag_reports_exact_bytes() {
    let engine = start_session("drag-bytes", "mouse-drag,sgr-on");
    engine
        .mouse_checked(CheckedMouseAction::Drag {
            x: 6,
            y: 3,
            options: left(),
        })
        .expect("drag sends");
    wait_for_text(&engine, "rx:1b5b3c33323b373b344d");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-B: unpressed movement sends the exact SGR motion bytes.
#[test]
fn motion_reports_exact_bytes() {
    let engine = start_session("motion-bytes", "mouse-motion,sgr-on");
    engine
        .mouse_checked(CheckedMouseAction::Motion { x: 6, y: 3 })
        .expect("motion sends");
    wait_for_text(&engine, "rx:1b5b3c33353b373b344d");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-B: paste sends framed bytes and focus sends the exact reports.
#[test]
fn paste_and_focus_report_exact_bytes() {
    let engine = start_session("paste-focus-bytes", "paste-on,focus-on");
    engine.paste("hi").expect("framed paste");
    wait_for_text(&engine, "rx:1b5b3230307e68691b5b3230317e");
    engine.focus(true).expect("focus gained");
    wait_for_text(&engine, "rx:1b5b49");
    engine.focus(false).expect("focus lost");
    wait_for_text(&engine, "rx:1b5b4f");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-C: tracking without SGR encoding fails before any byte write, and
// click tracking never accepts movement reports.
#[test]
fn unsupported_encoding_fails_before_write() {
    let engine = start_session("bad-encoding", "mouse-click");
    assert_eq!(rx_count(&engine), 0);
    let error = engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect_err("legacy encoding is unsupported");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    let error = engine
        .mouse_checked(CheckedMouseAction::Drag {
            x: 6,
            y: 3,
            options: left(),
        })
        .expect_err("click mode rejects pressed movement");
    assert!(
        matches!(error, CheckedInputError::Unsupported(_)),
        "{error:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rx_count(&engine), 0, "child received nothing");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-C: coordinates outside the active screen fail before any byte write.
#[test]
fn out_of_bounds_fails_before_write() {
    let engine = start_session("out-of-bounds", "mouse-motion,sgr-on,paste-on,focus-on");
    for action in [
        CheckedMouseAction::Press {
            x: 80,
            y: 0,
            options: left(),
        },
        CheckedMouseAction::Release {
            x: 0,
            y: 24,
            options: left(),
        },
        CheckedMouseAction::Motion { x: 80, y: 24 },
        CheckedMouseAction::Drag {
            x: 79,
            y: 24,
            options: left(),
        },
    ] {
        let error = engine
            .mouse_checked(action)
            .expect_err("out-of-bounds coordinates fail");
        assert!(matches!(error, CheckedInputError::Rejected(_)), "{error:?}");
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rx_count(&engine), 0, "child received nothing");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 79,
            y: 23,
            options: left(),
        })
        .expect("last cell stays valid");
    wait_for_text(&engine, "rx:");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-C: mouse input to a history viewport fails before any byte write.
#[test]
fn history_viewport_fails_before_write() {
    let engine = start_session("history", "mouse-motion,sgr-on");
    send_command(&engine, "fill");
    wait_for_text(&engine, "line 59");
    engine
        .execute(Operation::Scroll {
            target: ScrollTarget::Lines(10),
        })
        .expect("scroll to history");
    let error = engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect_err("history viewport rejects mouse input");
    assert!(matches!(error, CheckedInputError::Rejected(_)), "{error:?}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rx_count(&engine), 0, "child received nothing");
    engine
        .execute(Operation::Scroll {
            target: ScrollTarget::Bottom,
        })
        .expect("scroll to bottom");
    engine
        .mouse_checked(CheckedMouseAction::Press {
            x: 4,
            y: 2,
            options: left(),
        })
        .expect("live viewport accepts mouse input");
    wait_for_text(&engine, "rx:1b5b3c303b353b334d");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-C: paste text containing the bracketed-paste terminator fails while
// markers are on, before any byte write.
#[test]
fn paste_terminator_rejected_before_write() {
    let engine = start_session("paste-terminator", "paste-on");
    wait_for_mode(&engine, "bracketed_paste", true);
    assert_eq!(rx_count(&engine), 0);
    let error = engine
        .paste("ab\x1b[201~cd")
        .expect_err("paste containing the terminator fails while markers are on");
    assert!(matches!(error, CheckedInputError::Rejected(_)), "{error:?}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rx_count(&engine), 0, "child received nothing");
    engine.execute(Operation::Close).expect("session closes");
}

// U07-D: the raw input and legacy mouse paths keep their behavior.
#[test]
fn legacy_mouse_and_raw_write_keep_behavior() {
    let engine = start_session("legacy", "none");
    engine
        .execute(Operation::Mouse {
            action: MouseAction::Move { x: 4, y: 2 },
        })
        .expect("legacy move sends without tracking");
    wait_for_text(&engine, "rx:1b5b3c33353b353b334d");
    engine
        .execute(Operation::Mouse {
            action: MouseAction::Down {
                x: 0,
                y: 0,
                options: left(),
            },
        })
        .expect("legacy press sends without tracking");
    wait_for_text(&engine, "rx:1b5b3c303b313b314d");
    engine
        .execute(Operation::Write {
            data: "A\x1b[ZB".to_string(),
        })
        .expect("raw write sends");
    wait_for_text(&engine, "rx:411b5b5a42");
    engine.execute(Operation::Close).expect("session closes");
}
