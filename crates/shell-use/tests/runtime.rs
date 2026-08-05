use std::sync::Arc;
use std::time::{Duration, Instant};

use shell_use::{
    global_registry, ErrorKind, OpenOptions, Operation, OperationResult, SessionRegistry, Timeouts,
};

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
