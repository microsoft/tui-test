use shell_use::config::{DEFAULT_COLS, DEFAULT_ROWS};
use shell_use::protocol::{ErrorKind, Request, TimeoutDefaults};
use shell_use::runtime::{global_registry, SessionRegistry};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn named_runtimes_share_a_process_local_terminal() {
    let name = format!("native-runtime-{}", std::process::id());
    let registry = global_registry();
    let response = registry.response(
        &name,
        Request::Open {
            shell: None,
            program: None,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            cwd: None,
            env: Vec::new(),
            wait_ready: None,
            timeouts: TimeoutDefaults::default(),
        },
    );
    assert!(response.ok);
    assert!(
        registry
            .response(
                &name,
                Request::Submit {
                    data: Some("echo native-runtime".to_string()),
                },
            )
            .ok
    );
    assert!(
        registry
            .response(
                &name,
                Request::WaitCommand {
                    timeout_ms: Some(30_000),
                },
            )
            .ok
    );
    assert!(
        registry
            .response(
                &name,
                Request::ExpectText {
                    text: "native-runtime".to_string(),
                    regex: false,
                    full: false,
                    strict: false,
                    not: false,
                    fg: None,
                    bg: None,
                    timeout_ms: Some(5_000),
                },
            )
            .ok
    );

    assert!(registry.sessions().contains(&name));
    registry.close(&name).expect("close terminal");
    assert!(!registry.sessions().contains(&name));
    assert!(registry
        .recording(&name)
        .expect("read closed recording")
        .contains("native-runtime"));

    assert!(
        registry
            .response(
                &name,
                Request::Open {
                    shell: None,
                    program: None,
                    cols: DEFAULT_COLS,
                    rows: DEFAULT_ROWS,
                    cwd: None,
                    env: Vec::new(),
                    wait_ready: Some(false),
                    timeouts: TimeoutDefaults::default(),
                },
            )
            .ok
    );
    registry.close(&name).expect("close replacement");
}

#[test]
fn unrelated_session_state_does_not_wait_behind_another_session() {
    let registry = Arc::new(SessionRegistry::default());
    for name in ["waiting", "responsive"] {
        assert!(
            registry
                .response(
                    name,
                    Request::Open {
                        shell: None,
                        program: None,
                        cols: DEFAULT_COLS,
                        rows: DEFAULT_ROWS,
                        cwd: None,
                        env: Vec::new(),
                        wait_ready: Some(false),
                        timeouts: TimeoutDefaults::default(),
                    },
                )
                .ok
        );
    }

    let waiting = Arc::clone(&registry);
    let wait = std::thread::spawn(move || {
        waiting.response(
            "waiting",
            Request::WaitText {
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
    assert!(registry.response("responsive", Request::State).ok);
    assert!(start.elapsed() < Duration::from_millis(400));
    assert_eq!(wait.join().unwrap().kind, Some(ErrorKind::Assertion));

    registry.close_all();
}
