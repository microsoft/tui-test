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
#[cfg(feature = "recording-raster")]
fn session_records_and_exports_an_apng() {
    let registry = SessionRegistry::default();
    let session = registry.session(format!("recording-export-{}", std::process::id()));
    let path = std::env::temp_dir().join(format!(
        "shell-use-recording-export-{}.png",
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
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 1660);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 1364);

    session.close().expect("close terminal");
    std::fs::remove_file(path).expect("remove apng");
}
