use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, Engine, ErrorKind, ExecutionContext, KeyAction,
    LocatorQuery, MouseAction, OpenOptions, Operation, OperationResult, RunOptions, Session,
    SessionRegistry, Timeouts,
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
                if line.trim() == "emit-marker" {
                    emit(b"second-client-marker\r\n");
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
            emit(b"fixture-repaint-ready\r\n");
            for index in 0..1500 {
                emit(format!("\x1b[2;1H{}", index % 10).as_bytes());
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "quiet" => {
            emit(b"fixture-ready\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        #[cfg(windows)]
        "exit-259" => {
            prompt();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            std::process::exit(259);
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

fn expect_marker(timeout_ms: u64) -> Operation {
    Operation::WaitLocator {
        query: LocatorQuery::text("second-client-marker"),
        not: false,
        timeout_ms: Some(timeout_ms),
    }
}

#[test]
fn named_wait_allows_concurrent_input_resize_and_submit() {
    let registry = SessionRegistry::default();
    let session = registry.session("concurrent-input");
    let mut run = options("commands");
    run.wait_ready = Some(true);
    session.run(run).unwrap();
    let waiting = session.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        finished_tx
            .send(waiting.execute(expect_marker(5_000)))
            .unwrap();
    });
    assert!(finished_rx
        .recv_timeout(Duration::from_millis(150))
        .is_err());

    for operation in [
        Operation::Write {
            data: "input".into(),
        },
        Operation::Key {
            keys: vec!["Enter".into()],
            action: KeyAction::Press,
        },
        Operation::Mouse {
            action: MouseAction::Move { x: 1, y: 1 },
        },
        Operation::Submit { data: None },
        Operation::Resize { cols: 90, rows: 26 },
        Operation::State,
    ] {
        let started = Instant::now();
        session.execute(operation).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    assert!(finished_rx.try_recv().is_err());
    session
        .execute(Operation::Submit {
            data: Some("emit-marker".into()),
        })
        .unwrap();
    finished_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    waiter.join().unwrap();
    session.close().unwrap();
}

#[test]
fn pending_wait_cannot_observe_a_replacement_session() {
    let session = Session::new("wait-generation");
    session.run(options("quiet")).unwrap();
    let waiting = session.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        finished_tx
            .send(waiting.execute(expect_marker(30_000)))
            .unwrap();
    });
    assert!(finished_rx
        .recv_timeout(Duration::from_millis(150))
        .is_err());

    let mut run = options("commands");
    run.restart = true;
    run.wait_ready = Some(true);
    let started = Instant::now();
    session.run(run).unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    let error = finished_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Assertion);
    assert!(error.message.contains("cancelled"));
    waiter.join().unwrap();

    session
        .execute(Operation::Submit {
            data: Some("emit-marker".into()),
        })
        .unwrap();
    session.execute(expect_marker(2_000)).unwrap();
    session.close().unwrap();
}

#[test]
fn cancelling_one_wait_preserves_other_waits_and_the_child() {
    let engine = Arc::new(Engine::new(
        "request-cancellation".into(),
        Arc::new(tui_test::logger::Logger::disabled()),
        PathBuf::new(),
    ));
    let mut run = options("commands");
    run.wait_ready = Some(true);
    engine.execute(Operation::Run(run)).unwrap();
    let pid = engine.status().shell_pid;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelling_engine = engine.clone();
    let cancel_token = cancelled.clone();
    let (cancelled_tx, cancelled_rx) = mpsc::channel();
    let cancelled_waiter = std::thread::spawn(move || {
        let result = cancelling_engine.execute_with_context_cancellable(
            Operation::WaitTitle {
                text: "never-set".into(),
                regex: false,
                not: false,
                timeout_ms: Some(30_000),
            },
            ExecutionContext::default(),
            &|| cancel_token.load(Ordering::Acquire),
        );
        cancelled_tx.send(result).unwrap();
    });
    let waiting_engine = engine.clone();
    let waiter = std::thread::spawn(move || waiting_engine.execute(expect_marker(5_000)));
    assert!(cancelled_rx
        .recv_timeout(Duration::from_millis(150))
        .is_err());
    cancelled.store(true, Ordering::Release);
    let error = cancelled_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Assertion);
    assert!(error.message.contains("cancelled"));
    cancelled_waiter.join().unwrap();
    assert_eq!(
        error.details.unwrap().reason,
        tui_test::FailureReason::Cancelled
    );
    assert_eq!(engine.status().shell_pid, pid);
    assert!(engine.status().exited.is_none());
    engine
        .execute(Operation::Submit {
            data: Some("emit-marker".into()),
        })
        .unwrap();
    waiter.join().unwrap().unwrap();
    engine.execute(Operation::Close).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_exit_code_259_is_an_exit_not_still_active() {
    let session = Session::new("exit-259");
    let mut run = options("exit-259");
    run.wait_ready = Some(true);
    let pid = session.run(run).unwrap().shell_pid;
    assert_eq!(
        session
            .execute(Operation::WaitExit {
                timeout_ms: Some(0)
            })
            .unwrap_err()
            .kind,
        ErrorKind::Assertion
    );
    session
        .execute(Operation::Submit {
            data: Some("exit".into()),
        })
        .unwrap();
    wait_exit(&session);
    let OperationResult::State(state) = session.execute(Operation::State).unwrap() else {
        panic!("expected terminal state");
    };
    assert_eq!(state.exited, Some(259));
    assert!(!state.ready);
    assert_ne!(session.run(options("quiet")).unwrap().shell_pid, pid);
    session.close().unwrap();
}

#[cfg(unix)]
#[test]
fn close_and_drop_are_bounded_when_a_descendant_retains_the_pty() {
    struct Descendant(i32);
    impl Drop for Descendant {
        fn drop(&mut self) {
            // This is the explicitly recorded PID of this fixture's child.
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }
    for explicit_close in [true, false] {
        let directory = test_directory("descendant-pty");
        let pid_file = directory.join("descendant.pid");
        let session = Session::new("descendant-pty");
        let mut run = options("quiet");
        run.program = "sh".into();
        run.args = vec![
            "-c".into(),
            format!(
                "trap '' HUP; sleep 60 & printf '%s' \"$!\" > \"$1\"; printf 'descendant-ready\\n'; {}",
                if explicit_close { "wait" } else { "exit 0" }
            ),
            "sh".into(),
            pid_file.to_string_lossy().into_owned(),
        ];
        session.run(run).unwrap();
        wait_until(|| std::fs::read_to_string(&pid_file).is_ok_and(|pid| !pid.is_empty()));
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert!(pid > 1);
        let descendant = Descendant(pid);
        session
            .get_by_text("descendant-ready")
            .wait_with_timeout(Some(2_000))
            .unwrap();
        if !explicit_close {
            let started = Instant::now();
            wait_exit(&session);
            assert!(started.elapsed() < Duration::from_secs(2));
        }
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = if explicit_close {
                session.close()
            } else {
                Ok(())
            };
            drop(session);
            let _ = finished_tx.send(result);
        });
        let result = finished_rx.recv_timeout(Duration::from_secs(2));
        // Release the slave even on failure, so the regression cannot hang the suite.
        drop(descendant);
        worker.join().unwrap();
        result
            .expect("session teardown waited for the descendant's PTY slave")
            .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
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
    session
        .get_by_text("fixture-repaint-ready")
        .wait_with_timeout(Some(2_000))
        .unwrap();
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
