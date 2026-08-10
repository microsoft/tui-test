use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{
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
