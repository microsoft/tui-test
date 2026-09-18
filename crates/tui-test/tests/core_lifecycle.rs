use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, ErrorKind, OpenOptions, Operation, OperationResult,
    RunOptions, Session, SessionRegistry, Timeouts,
};

const FIXTURE: &str = "TUI_TEST_CORE_LIFECYCLE_FIXTURE";

fn emit(bytes: &[u8]) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(bytes).unwrap();
    stdout.flush().unwrap();
}

fn prompt() {
    emit(b"\x1b]133;A\x1b\\fixture> \x1b]133;B\x1b\\");
}

#[test]
fn child_fixture() {
    let Ok(mode) = std::env::var(FIXTURE) else {
        return;
    };
    match mode.as_str() {
        "commands" => {
            prompt();
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                if line.trim() == "exit-without-start" {
                    std::process::exit(7);
                }
                if line.trim() == "delayed" {
                    std::thread::sleep(Duration::from_millis(400));
                }
                emit(b"\x1b]133;C\x1b\\");
                if line.trim() == "exit-with-start" {
                    std::process::exit(7);
                }
                emit(b"command complete\r\n\x1b]133;D;0\x1b\\");
                prompt();
            }
        }
        "ready-exit" => {
            prompt();
            emit(b"ready-after-marker\r\n");
            std::process::exit(7);
        }
        "nonvisual" => {
            emit(b"fixture-ready\r\n");
            for _ in 0..1500 {
                emit(b"\x07\x1b[31m");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "repaint" => {
            for index in 0..1500 {
                emit(format!("\x1b[H{}", index % 10).as_bytes());
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "quiet" => {
            emit(b"fixture-ready\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("unknown fixture mode: {mode}"),
    }
}

fn options(mode: &str) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        program: std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        args: [
            "--exact",
            "child_fixture",
            "--nocapture",
            "--test-threads=1",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        backend: defaults.backend,
        profile: defaults.profile,
        cols: 80,
        rows: 24,
        cwd: None,
        env: vec![(FIXTURE.to_string(), mode.to_string())],
        wait_ready: Some(false),
        restart: false,
        timeouts: Timeouts {
            ready: Some(5_000),
            ..Default::default()
        },
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
    }
}

fn test_directory(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("core-lifecycle-tests")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "fixture did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_ready(
    session: &Session,
    timeout_ms: u64,
) -> Result<OperationResult, tui_test::TuiTestError> {
    session.execute(Operation::WaitReady {
        timeout_ms: Some(timeout_ms),
    })
}

fn wait_exit(session: &Session) {
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(5_000),
        })
        .unwrap();
}

#[test]
fn interrupt_cancels_startup_readiness_before_a_session_is_published() {
    let directory = test_directory("startup-interrupt");
    let session = Session::new("startup-interrupt");
    let mut run = options("quiet");
    run.wait_ready = Some(true);
    run.recording.mode = AutomaticRecordingMode::Always;
    run.recording.directory = Some(directory.clone());
    let opening = session.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        finished_tx.send(opening.run(run)).unwrap();
    });

    // The recording is published after the child starts but before readiness.
    wait_until(|| session.recording_path().is_some());
    let started = Instant::now();
    session.interrupt();
    let result = finished_rx.recv_timeout(Duration::from_secs(2));
    worker.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    let error = result.expect("startup ignored interrupt").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Assertion);
    assert!(error.message.contains("cancelled"));
    assert!(!session.is_open());
    session.close().unwrap();
    session.run(options("quiet")).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn close_all_cancels_startup_before_acquiring_the_lifecycle_lock() {
    let directory = test_directory("startup-close-all");
    let registry = SessionRegistry::default();
    let session = registry.session("startup-close-all");
    let mut run = options("quiet");
    run.wait_ready = Some(true);
    run.recording.mode = AutomaticRecordingMode::Always;
    run.recording.directory = Some(directory.clone());
    let worker = std::thread::spawn(move || session.run(run));
    wait_until(|| std::fs::read_dir(&directory).unwrap().next().is_some());

    let started = Instant::now();
    registry.close_all();
    assert!(started.elapsed() < Duration::from_secs(2));
    let error = worker.join().unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Assertion);
    assert!(error.message.contains("cancelled"));
    assert!(registry.sessions().is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn close_cancels_a_pending_wait_instead_of_joining_its_queue() {
    for operation in [
        Operation::WaitReady {
            timeout_ms: Some(5_000),
        },
        Operation::WaitCommand {
            timeout_ms: Some(5_000),
        },
        Operation::WaitIdle {
            timeout_ms: Some(5_000),
        },
        Operation::WaitExit {
            timeout_ms: Some(5_000),
        },
    ] {
        let session = Session::new("close-pending-wait");
        session.run(options("quiet")).unwrap();
        let waiting = session.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiting.execute(operation)
        });
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let started = Instant::now();
        session.close().unwrap();
        let waited = worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(waited.unwrap_err().kind, ErrorKind::Assertion);
        assert!(!session.is_open());
    }
}

#[test]
fn named_close_cancels_before_acquiring_the_generation_lock() {
    for through_handle in [true, false] {
        let registry = SessionRegistry::default();
        let session = registry.session("named-close-pending");
        session.run(options("quiet")).unwrap();
        let waiting = session.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiting.execute(Operation::WaitReady {
                timeout_ms: Some(5_000),
            })
        });
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let started = Instant::now();
        if through_handle {
            session.close().unwrap();
        } else {
            registry.close("named-close-pending").unwrap();
        }
        let waited = worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(waited.unwrap_err().kind, ErrorKind::Assertion);
        assert!(registry.sessions().is_empty());
    }
}

#[test]
fn kill_can_overtake_a_pending_operation() {
    let registry = SessionRegistry::default();
    let session = registry.session("kill-pending");
    session.run(options("quiet")).unwrap();
    let waiting = session.clone();
    let worker = std::thread::spawn(move || {
        waiting.execute(Operation::WaitReady {
            timeout_ms: Some(5_000),
        })
    });
    std::thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    session
        .execute(Operation::Signal {
            name: "KILL".into(),
        })
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        worker.join().unwrap().unwrap_err().kind,
        ErrorKind::Assertion
    );
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(2_000),
        })
        .unwrap();
    session.close().unwrap();
}

#[test]
fn wait_ready_rejects_a_prompt_left_by_an_exited_process() {
    let session = Session::new("ready-after-exit");
    session.run(options("ready-exit")).unwrap();
    wait_exit(&session);
    session
        .get_by_text("ready-after-marker")
        .wait_with_timeout(Some(2_000))
        .unwrap();

    assert_eq!(
        wait_ready(&session, 0).unwrap_err().kind,
        ErrorKind::Assertion
    );
    let OperationResult::State(state) = session.execute(Operation::State).unwrap() else {
        panic!("expected terminal state");
    };
    assert!(!state.ready);
    session.close().unwrap();
}

#[test]
fn wait_ready_does_not_reuse_a_prompt_while_submitted_input_is_pending() {
    let session = Session::new("ready-pending-input");
    let mut run = options("commands");
    run.wait_ready = Some(true);
    assert!(session.run(run).unwrap().ready);
    session
        .execute(Operation::Submit {
            data: Some("delayed".into()),
        })
        .unwrap();
    assert_eq!(
        wait_ready(&session, 0).unwrap_err().kind,
        ErrorKind::Assertion
    );
    wait_ready(&session, 5_000).unwrap();
    session
        .execute(Operation::ExpectExitCode {
            code: 0,
            timeout_ms: Some(0),
        })
        .unwrap();
    session.close().unwrap();
}

#[test]
fn invalid_resize_preserves_the_grid_and_does_not_poison_later_operations() {
    let session = Session::new("invalid-resize");
    session.run(options("quiet")).unwrap();
    session
        .get_by_text("fixture-ready")
        .wait_with_timeout(Some(2_000))
        .unwrap();
    session
        .execute(Operation::WaitIdle {
            timeout_ms: Some(2_000),
        })
        .unwrap();
    let OperationResult::State(before) = session.execute(Operation::State).unwrap() else {
        panic!("expected terminal state");
    };
    let mut invalid = vec![(0, 24), (80, 0), (0, 0)];
    if cfg!(windows) {
        invalid.extend([(32768, 24), (80, 32768)]);
    }
    for (cols, rows) in invalid {
        assert_eq!(
            session
                .execute(Operation::Resize { cols, rows })
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
        let OperationResult::State(after) = session.execute(Operation::State).unwrap() else {
            panic!("expected terminal state");
        };
        assert_eq!((after.cols, after.rows), (before.cols, before.rows));
        assert_eq!(after.text, before.text);
    }
    session
        .execute(Operation::Resize { cols: 90, rows: 25 })
        .unwrap();
    let OperationResult::Size(size) = session.execute(Operation::GetSize).unwrap() else {
        panic!("expected terminal size");
    };
    assert_eq!((size.cols, size.rows), (90, 25));
    session.close().unwrap();
}

#[test]
fn wait_idle_ignores_bells_and_nonvisual_escape_traffic() {
    let session = Session::new("nonvisual-idle");
    session.run(options("nonvisual")).unwrap();
    session
        .execute(Operation::ExpectBellCount {
            count: 1,
            timeout_ms: Some(2_000),
        })
        .unwrap();
    let OperationResult::BellCount(before) = session.execute(Operation::GetBellCount).unwrap()
    else {
        panic!("expected bell count");
    };
    session
        .execute(Operation::WaitIdle {
            timeout_ms: Some(1_500),
        })
        .unwrap();
    let OperationResult::BellCount(after) = session.execute(Operation::GetBellCount).unwrap()
    else {
        panic!("expected bell count");
    };
    assert!(after > before, "nonvisual output stopped during the wait");
    session.close().unwrap();
}

#[test]
fn wait_idle_still_waits_for_visual_repaints() {
    let session = Session::new("visual-idle");
    session.run(options("repaint")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let result = session.execute(Operation::WaitIdle {
        timeout_ms: Some(500),
    });
    session.close().unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Assertion);
}

#[test]
fn exit_code_expectations_never_reuse_the_previous_command_after_shell_exit() {
    for command in ["exit-with-start", "exit-without-start"] {
        let session = Session::new(format!("stale-exit-code-{command}"));
        let mut run = options("commands");
        run.wait_ready = Some(true);
        session.run(run).unwrap();
        session
            .execute(Operation::Submit {
                data: Some("ok".into()),
            })
            .unwrap();
        session
            .execute(Operation::ExpectExitCode {
                code: 0,
                timeout_ms: Some(2_000),
            })
            .unwrap();
        wait_ready(&session, 2_000).unwrap();
        session
            .execute(Operation::Submit {
                data: Some(command.into()),
            })
            .unwrap();
        wait_exit(&session);
        assert_eq!(
            session
                .execute(Operation::ExpectExitCode {
                    code: 0,
                    timeout_ms: Some(0),
                })
                .unwrap_err()
                .kind,
            ErrorKind::Assertion
        );
        let OperationResult::State(state) = session.execute(Operation::State).unwrap() else {
            panic!("expected terminal state");
        };
        assert_eq!(state.exited, Some(7));
        session.close().unwrap();
    }
}

#[test]
fn explicit_missing_or_file_cwd_is_rejected_before_opening_a_pty() {
    let directory = test_directory("invalid-cwd");
    let file = directory.join("not-a-directory");
    std::fs::write(&file, b"file").unwrap();
    for cwd in [directory.join("missing"), file] {
        let session = Session::new("invalid-cwd");
        let cwd = cwd.to_string_lossy().into_owned();
        let mut run = options("quiet");
        run.cwd = Some(cwd.clone());
        let error = session.run(run).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Usage);
        assert!(error.message.contains("cwd"));
        assert!(!session.is_open());

        let error = session
            .open(OpenOptions {
                cwd: Some(cwd),
                wait_ready: Some(false),
                ..OpenOptions::default()
            })
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Usage);
        assert!(!session.is_open());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_initial_dimensions_are_usage_errors() {
    let session = Session::new("invalid-initial-size");
    let mut run = options("quiet");
    run.cols = 0;
    assert_eq!(session.run(run).unwrap_err().kind, ErrorKind::Usage);
    assert!(!session.is_open());
    let error = session
        .open(OpenOptions {
            rows: 0,
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);
    assert!(!session.is_open());
}
