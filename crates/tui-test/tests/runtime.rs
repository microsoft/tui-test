#[cfg(unix)]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{
    global_registry, AutomaticRecording, AutomaticRecordingMode, ErrorKind, OpenOptions, Operation,
    OperationResult, RunOptions, Session, SessionRegistry, Timeouts,
};

fn run_options(program: &str, args: &[&str]) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        backend: defaults.backend,
        program: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        profile: defaults.profile,
        cols: defaults.cols,
        rows: defaults.rows,
        cwd: defaults.cwd,
        env: defaults.env,
        wait_ready: Some(false),
        restart: defaults.restart,
        timeouts: defaults.timeouts,
        recording: defaults.recording,
    }
}

fn wait_for_exit(session: &Session) {
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(5_000),
        })
        .expect("wait for process exit");
}

fn process_exit_code(session: &Session) -> Option<i32> {
    let OperationResult::State(state) = session.execute(Operation::State).expect("read state")
    else {
        panic!("unexpected state result");
    };
    state.exited
}

#[test]
fn named_handles_share_a_process_local_terminal() {
    let name = format!("native-runtime-{}", std::process::id());
    let registry = global_registry();
    let first = registry.session(name.clone());
    let second = registry.session(name.clone());

    first.open(OpenOptions::default()).expect("open terminal");
    second
        .execute(Operation::Submit {
            data: Some("echo native-runtime".to_string()),
        })
        .expect("submit command");
    first
        .execute(Operation::WaitCommand {
            timeout_ms: Some(30_000),
        })
        .expect("wait for command");
    second
        .execute(Operation::ExpectText {
            text: "native-runtime".to_string(),
            regex: false,
            full: false,
            strict: false,
            not: false,
            fg: None,
            bg: None,
            timeout_ms: Some(5_000),
        })
        .expect("find command output");
    assert!(second
        .recording()
        .expect("read active recording")
        .contains("native-runtime"));

    assert!(registry.sessions().contains(&name));
    first.close().expect("close terminal");
    assert!(!registry.sessions().contains(&name));
    assert!(second
        .recording()
        .expect("read recording")
        .contains("native-runtime"));

    second
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open replacement");
    first
        .close()
        .expect("close replacement through first handle");
}

#[test]
fn automatic_recording_supports_disabled_and_custom_directory() {
    let registry = SessionRegistry::default();
    let root = std::env::temp_dir().join(format!("tui-test-recording-mode-{}", std::process::id()));

    let disabled = registry.session("recording-disabled");
    let opened = disabled
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Disabled,
                directory: Some(root.clone()),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    assert!(opened.recording.is_empty());
    disabled.close().unwrap();
    assert!(disabled.recording().is_err());
    assert!(!root.exists());

    let always = registry.session("recording-always");
    let opened = always
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(root.clone()),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    let path = std::path::PathBuf::from(opened.recording);
    assert!(path.starts_with(&root));
    always.close().unwrap();
    assert!(always.recording().is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn on_failure_recording_is_kept_only_after_an_operation_failure() {
    let registry = SessionRegistry::default();
    let root =
        std::env::temp_dir().join(format!("tui-test-recording-failure-{}", std::process::id()));

    let success = registry.session("recording-success");
    let opened = success
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::OnFailure,
                directory: Some(root.clone()),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    let success_path = std::path::PathBuf::from(opened.recording);
    success.close().unwrap();
    assert!(!success_path.exists());

    let failed = registry.session("recording-failed");
    failed
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::OnFailure,
                directory: Some(root.clone()),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    assert_eq!(
        failed
            .execute(Operation::ExpectText {
                text: "never-present".to_string(),
                regex: false,
                full: false,
                strict: false,
                not: false,
                fg: None,
                bg: None,
                timeout_ms: Some(1),
            })
            .unwrap_err()
            .kind,
        ErrorKind::Assertion
    );
    failed.close().unwrap();
    assert!(failed.recording().is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failed_open_recording_is_readable_before_close() {
    let registry = SessionRegistry::default();
    let session = registry.session("recording-failed-open");
    let root = std::env::temp_dir().join(format!("tui-test-failed-open-{}", std::process::id()));
    let (program, args) = if cfg!(windows) {
        (
            "powershell",
            vec![
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Start-Sleep -Seconds 2",
            ],
        )
    } else {
        ("sh", vec!["-c", "sleep 2"])
    };
    let mut options = run_options(program, &args);
    options.wait_ready = Some(true);
    options.timeouts.ready = Some(50);
    options.recording = AutomaticRecording {
        mode: AutomaticRecordingMode::OnFailure,
        directory: Some(root.clone()),
    };

    assert_eq!(session.run(options).unwrap_err().kind, ErrorKind::Assertion);
    assert!(session.recording().unwrap().contains("\"version\":2"));
    session.close().unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn restart_discards_the_previous_recording_path() {
    let registry = SessionRegistry::default();
    let session = registry.session("recording-restart");
    let root =
        std::env::temp_dir().join(format!("tui-test-recording-restart-{}", std::process::id()));
    let first = session
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(root.join("first")),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    let first = std::path::PathBuf::from(first.recording);
    assert!(first.is_file());

    let second = session
        .open(OpenOptions {
            wait_ready: Some(false),
            restart: true,
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(root.join("second")),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    assert!(!first.exists());
    assert!(std::path::Path::new(&second.recording).is_file());
    session.close().unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failed_recorder_initialization_does_not_claim_a_stale_file() {
    let session = Session::new("recording-stale-file");
    let root =
        std::env::temp_dir().join(format!("tui-test-recording-stale-{}", std::process::id()));
    let first = session
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(root.join("first")),
            },
            ..OpenOptions::default()
        })
        .unwrap();
    session.close().unwrap();

    let blocked = root.join("blocked");
    std::fs::create_dir_all(&blocked).unwrap();
    let stale = blocked.join(std::path::Path::new(&first.recording).file_name().unwrap());
    std::fs::write(&stale, "stale").unwrap();
    let original_permissions = std::fs::metadata(&stale).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&stale, read_only).unwrap();

    assert!(session
        .open(OpenOptions {
            wait_ready: Some(false),
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::OnFailure,
                directory: Some(blocked),
            },
            ..OpenOptions::default()
        })
        .is_err());
    assert!(session.recording().is_err());
    assert_eq!(std::fs::read_to_string(&stale).unwrap(), "stale");

    std::fs::set_permissions(&stale, original_permissions).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failed_process_spawn_does_not_publish_a_recording() {
    let session = Session::new("recording-failed-spawn");
    let root = std::env::temp_dir().join(format!(
        "tui-test-recording-failed-spawn-{}",
        std::process::id()
    ));
    let mut options = run_options("tui-test-program-that-does-not-exist", &[]);
    options.recording = AutomaticRecording {
        mode: AutomaticRecordingMode::OnFailure,
        directory: Some(root.clone()),
    };

    assert!(session.run(options).is_err());
    assert!(session.recording().is_err());
    assert!(std::fs::read_dir(&root)
        .map(|entries| entries.flatten().next().is_none())
        .unwrap_or(true));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opening_a_live_named_session_reuses_it_unless_restart_is_requested() {
    let registry = SessionRegistry::default();
    let session = registry.session("native-reuse");

    let first = session
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open first terminal");
    let reused = session
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("reuse live terminal");
    assert_eq!(reused.shell_pid, first.shell_pid);

    let restarted = session
        .open(OpenOptions {
            wait_ready: Some(false),
            restart: true,
            ..OpenOptions::default()
        })
        .expect("restart live terminal");
    assert_ne!(restarted.shell_pid, first.shell_pid);
    session.close().expect("close restarted terminal");
}

#[test]
fn unrelated_session_state_does_not_wait_behind_another_session() {
    let registry = Arc::new(SessionRegistry::default());
    for name in ["waiting", "responsive"] {
        registry
            .session(name)
            .open(OpenOptions {
                wait_ready: Some(false),
                timeouts: Timeouts::default(),
                ..OpenOptions::default()
            })
            .expect("open terminal");
    }

    let waiting = Arc::clone(&registry);
    let wait = std::thread::spawn(move || {
        waiting.execute(
            "waiting",
            Operation::WaitText {
                text: "text-that-will-never-appear".to_string(),
                regex: false,
                full: false,
                timeout_ms: Some(700),
                not: false,
            },
        )
    });
    std::thread::sleep(Duration::from_millis(100));

    let start = Instant::now();
    assert!(matches!(
        registry.execute("responsive", Operation::State),
        Ok(OperationResult::State(_))
    ));
    assert!(start.elapsed() < Duration::from_millis(400));
    assert_eq!(wait.join().unwrap().unwrap_err().kind, ErrorKind::Assertion);

    registry.close_all();
}

#[test]
fn packed_screen_is_native_owned_utf8() {
    let registry = SessionRegistry::default();
    let session = registry.session("packed-screen");
    session
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open terminal");
    let OperationResult::PackedScreen(screen) = session
        .execute(Operation::PackedScreen { full: false })
        .expect("capture packed screen")
    else {
        panic!("unexpected packed screen result");
    };
    let text = String::from_utf8(screen.utf8).expect("packed screen is UTF-8");
    assert_eq!(text.split('\n').count(), screen.rows as usize);
    session.close().expect("close terminal");
}

#[test]
fn close_all_interrupts_in_flight_waits() {
    let registry = Arc::new(SessionRegistry::default());
    registry
        .session("interrupt-wait")
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open terminal");

    let waiting = Arc::clone(&registry);
    let wait = std::thread::spawn(move || {
        waiting.execute(
            "interrupt-wait",
            Operation::WaitText {
                text: "never-appears".to_string(),
                regex: false,
                full: false,
                timeout_ms: Some(30_000),
                not: false,
            },
        )
    });
    std::thread::sleep(Duration::from_millis(100));

    let start = Instant::now();
    registry.close_all();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(wait.join().unwrap().unwrap_err().kind, ErrorKind::Assertion);
}

#[test]
fn bell_counts_waits_and_expectations_are_cumulative() {
    let registry = SessionRegistry::default();
    let session = registry.session("bells");
    session.open(OpenOptions::default()).expect("open terminal");
    session
        .execute(Operation::Submit {
            data: Some(two_bells_command()),
        })
        .expect("submit bell command");
    session
        .execute(Operation::ExpectBellCount {
            count: 2,
            timeout_ms: Some(5_000),
        })
        .expect("wait for two bells");
    session
        .execute(Operation::WaitCommand {
            timeout_ms: Some(30_000),
        })
        .expect("wait for bell command");

    let OperationResult::State(state) = session.execute(Operation::State).expect("read state")
    else {
        panic!("unexpected state result");
    };
    assert_eq!(state.bell_count, 2);

    let OperationResult::BellEvents(events) = session
        .execute(Operation::GetBellEvents)
        .expect("read bell events")
    else {
        panic!("unexpected bell events result");
    };
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(events[1].elapsed_ms >= events[0].elapsed_ms);
    for _ in 0..2 {
        assert!(matches!(
            session
                .execute(Operation::GetBellCount)
                .expect("read bell count"),
            OperationResult::BellCount(2)
        ));
    }

    session
        .execute(Operation::Submit {
            data: Some(delayed_bell_command()),
        })
        .expect("submit delayed bell");
    session
        .execute(Operation::WaitBell {
            timeout_ms: Some(5_000),
        })
        .expect("wait for the next bell");
    assert!(matches!(
        session
            .execute(Operation::GetBellCount)
            .expect("read final bell count"),
        OperationResult::BellCount(3)
    ));

    let OperationResult::State(state) =
        session.execute(Operation::State).expect("read final state")
    else {
        panic!("unexpected final state result");
    };
    assert_eq!(state.bell_count, 3);

    let OperationResult::BellEvents(events) = session
        .execute(Operation::GetBellEvents)
        .expect("read final bell events")
    else {
        panic!("unexpected final bell events result");
    };
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(events[2].elapsed_ms >= events[1].elapsed_ms);

    session.close().expect("close terminal");
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
        "Start-Sleep -Milliseconds 300; [Console]::Out.Write([char]7)".to_string()
    } else {
        "sleep 0.3; printf '\\a'".to_string()
    }
}

#[cfg(unix)]
#[test]
fn pty_eof_waits_for_the_real_delayed_exit_status() {
    let session = Session::new(format!("delayed-exit-{}", std::process::id()));
    session
        .run(run_options(
            "sh",
            &["-c", "exec 0<&- 1>&- 2>&-; sleep 0.4; exit 7"],
        ))
        .expect("run delayed exit");

    let start = Instant::now();
    wait_for_exit(&session);

    assert!(
        start.elapsed() >= Duration::from_millis(250),
        "wait exit returned when the PTY reached EOF"
    );
    assert_eq!(process_exit_code(&session), Some(7));
    session.close().expect("close delayed exit");
}

#[cfg(any(unix, windows))]
#[test]
fn direct_zero_exit_status_is_preserved() {
    let session = Session::new(format!("zero-exit-{}", std::process::id()));
    #[cfg(unix)]
    let options = run_options("sh", &["-c", "exit 0"]);
    #[cfg(windows)]
    let options = run_options("cmd.exe", &["/C", "exit 0"]);

    session.run(options).expect("run zero exit");
    wait_for_exit(&session);

    assert_eq!(process_exit_code(&session), Some(0));
    session.close().expect("close zero exit");
}

#[cfg(unix)]
#[test]
fn signal_derived_exit_status_is_preserved() {
    let session = Session::new(format!("signal-exit-{}", std::process::id()));
    session
        .run(run_options("sh", &["-c", "kill -TERM $$"]))
        .expect("run signal exit");

    wait_for_exit(&session);

    assert_eq!(process_exit_code(&session), Some(1));
    session.close().expect("close signal exit");
}

#[cfg(unix)]
#[test]
fn killing_after_pty_eof_does_not_deadlock() {
    let session = Session::new(format!("kill-after-eof-{}", std::process::id()));
    session
        .run(run_options("sh", &["-c", "exec 0<&- 1>&- 2>&-; sleep 5"]))
        .expect("run process that closes its PTY");
    std::thread::sleep(Duration::from_millis(300));

    let killer = session.clone();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let result = killer
            .execute(Operation::Signal {
                name: "KILL".to_string(),
            })
            .map(|_| ())
            .map_err(|error| error.message);
        let _ = sent.send(result);
    });

    match received.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result.expect("kill process after PTY EOF"),
        Err(error) => {
            std::mem::forget(session);
            panic!("kill deadlocked after PTY EOF: {error}");
        }
    }
    wait_for_exit(&session);
    assert_eq!(process_exit_code(&session), Some(1));
    session.close().expect("close killed process");
}

#[cfg(unix)]
#[test]
fn closing_after_pty_eof_does_not_deadlock() {
    let session = Session::new(format!("close-after-eof-{}", std::process::id()));
    session
        .run(run_options("sh", &["-c", "exec 0<&- 1>&- 2>&-; sleep 5"]))
        .expect("run process that closes its PTY");
    std::thread::sleep(Duration::from_millis(300));

    let closer = session.clone();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let result = closer.close().map_err(|error| error.message);
        let _ = sent.send(result);
    });

    match received.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result.expect("close process after PTY EOF"),
        Err(error) => {
            std::mem::forget(session);
            panic!("close deadlocked after PTY EOF: {error}");
        }
    }
}

#[test]
#[cfg(feature = "recording-raster")]
fn session_records_and_exports_an_apng() {
    let registry = SessionRegistry::default();
    let session = registry.session(format!("recording-export-{}", std::process::id()));
    let path = std::env::temp_dir().join(format!(
        "tui-test-recording-export-{}.png",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    session.open(OpenOptions::default()).expect("open terminal");
    session
        .execute(Operation::StartRecording {
            path: path.to_string_lossy().into_owned(),
            format: None,
            fps: Some(30),
            speed: Some(1.0),
            idle_time_limit: Some(5.0),
            zoom: Some(0.5),
        })
        .expect("start recording");
    session
        .execute(Operation::Submit {
            data: Some("echo animated-recording".to_string()),
        })
        .expect("submit command");
    session
        .execute(Operation::WaitCommand {
            timeout_ms: Some(30_000),
        })
        .expect("wait for command");
    let OperationResult::Recording(recorded) = session
        .execute(Operation::StopRecording)
        .expect("stop recording")
    else {
        panic!("unexpected recording result");
    };
    assert_eq!(recorded, path.to_string_lossy());
    let bytes = std::fs::read(&path).expect("read apng");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(bytes.windows(4).any(|window| window == b"acTL"));
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 878);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 730);

    session.close().expect("close terminal");
    std::fs::remove_file(path).expect("remove apng");
}
