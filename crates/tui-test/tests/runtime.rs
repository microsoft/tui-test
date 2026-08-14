#[cfg(unix)]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{
    global_registry, ErrorKind, OpenOptions, Operation, OperationResult, RunOptions, Session,
    SessionRegistry, Timeouts,
};

fn run_options(program: &str, args: &[&str]) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        program: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        profile: defaults.profile,
        cols: defaults.cols,
        rows: defaults.rows,
        cwd: defaults.cwd,
        env: defaults.env,
        wait_ready: Some(false),
        timeouts: defaults.timeouts,
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
