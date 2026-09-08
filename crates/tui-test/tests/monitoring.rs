use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Stream as _;
use tui_test::monitoring::{
    self, host, ipc,
    protocol::{
        HostSnapshot, MonitorInput, MonitorInputReady, MonitorLeaseReady, Request, Response,
    },
    Metadata, Monitor, Options, Outcome, WaitPolicy,
};
use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, LocatorQuery, OpenOptions, Operation, RunOptions,
    Session, SessionHandle, SessionRegistry,
};

static SERIAL: Mutex<()> = Mutex::new(());

struct Fixture {
    registry: SessionRegistry,
    home: PathBuf,
    previous_home: Option<std::ffi::OsString>,
    _serial: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        let serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        monitoring::clear_sessions();
        let home = std::env::temp_dir().join(format!("tmm-{:x}", std::process::id()));
        std::fs::create_dir(&home).unwrap();
        let previous_home = std::env::var_os("TUI_TEST_HOME");
        std::env::set_var("TUI_TEST_HOME", &home);
        Self {
            registry: SessionRegistry::default(),
            home,
            previous_home,
            _serial: serial,
        }
    }

    fn open(&self, name: &str) -> SessionHandle {
        let handle = self.registry.session(name);
        handle.open(open_options()).unwrap();
        handle
    }

    fn register(&self, handle: &SessionHandle) -> host::DiscoveredHostSession {
        assert!(monitoring::register(
            handle.name(),
            &handle.monitor_target().unwrap(),
            Metadata {
                label: Some("core integration".into()),
                tags: vec!["rust".into()],
                ..Metadata::default()
            }
        )
        .unwrap());
        host::discover()
            .into_iter()
            .find(|entry| entry.session.session == handle.name())
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.registry.force_close_all();
        match &self.previous_home {
            Some(value) => std::env::set_var("TUI_TEST_HOME", value),
            None => std::env::remove_var("TUI_TEST_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn open_options() -> OpenOptions {
    OpenOptions {
        wait_ready: Some(false),
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
        ..OpenOptions::default()
    }
}

#[test]
fn explicit_restart_replaces_an_infinite_inspection_without_waiting_for_a_client() {
    let fixture = Fixture::new();
    let handle = fixture.open("restart-infinite");
    fixture.register(&handle);
    let old = handle.monitor_target().unwrap();
    monitoring::begin_wait_for_target_with_options(handle.name(), &old, "failed", None, true)
        .unwrap();
    let replacement = handle.clone();
    let (sent, received) = mpsc::channel();
    let restarting = std::thread::spawn(move || {
        let result = replacement.open(OpenOptions {
            restart: true,
            ..open_options()
        });
        let _ = sent.send(result);
    });
    received
        .recv_timeout(Duration::from_secs(5))
        .expect("restart blocked behind inspection")
        .unwrap();
    restarting.join().unwrap();
    assert!(!old.is_current());
    old.close().unwrap();
    assert!(handle.monitor_target().unwrap().is_current());
}

fn nonintegrated_program() -> RunOptions {
    let options = open_options();
    RunOptions {
        program: if cfg!(windows) { "powershell" } else { "sh" }.into(),
        args: if cfg!(windows) {
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 2".into(),
            ]
        } else {
            vec!["-c".into(), "sleep 2".into()]
        },
        backend: options.backend,
        profile: options.profile,
        cols: options.cols,
        rows: options.rows,
        cwd: options.cwd,
        env: options.env,
        wait_ready: Some(true),
        restart: false,
        timeouts: tui_test::Timeouts {
            ready: Some(25),
            ..options.timeouts
        },
        recording: options.recording,
    }
}

fn routed(target: &host::DiscoveredHostSession, lease: Option<u64>, request: Request) -> Request {
    Request::Routed {
        session: target.session.session.clone(),
        generation: target.session.generation,
        lease,
        request: Box::new(request),
    }
}

fn handshake(
    target: &host::DiscoveredHostSession,
    lease: Option<u64>,
    request: Request,
) -> (BufReader<ipc::Stream>, Response) {
    let mut stream = ipc::connect(&target.descriptor.endpoint).unwrap();
    stream.set_nonblocking(true).unwrap();
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&routed(target, lease, request)).unwrap()
    )
    .unwrap();
    stream.flush().unwrap();
    let mut reader = BufReader::new(stream);
    let response = read_response(&mut reader);
    (reader, response)
}

fn read_response(reader: &mut BufReader<ipc::Stream>) -> Response {
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match reader.read_line(&mut line) {
            Ok(0) if cfg!(windows) => std::thread::sleep(Duration::from_millis(5)),
            Ok(0) => panic!("connection closed before response"),
            Ok(_) if line.ends_with('\n') => return serde_json::from_str(&line).unwrap(),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("reading response: {error}"),
        }
        assert!(Instant::now() < deadline, "response timed out");
    }
}

fn attach(
    target: &host::DiscoveredHostSession,
    interactive: bool,
) -> (BufReader<ipc::Stream>, u64) {
    let (reader, response) = handshake(target, None, Request::MonitorLeaseStream { interactive });
    assert!(response.ok, "{response:?}");
    let ready: MonitorLeaseReady = serde_json::from_value(response.data.unwrap()).unwrap();
    (reader, ready.lease)
}

fn snapshot(target: &host::DiscoveredHostSession) -> HostSnapshot {
    serde_json::from_value(
        ipc::send(&target.descriptor.endpoint, &Request::HostSessions)
            .unwrap()
            .data
            .unwrap(),
    )
    .unwrap()
}

fn eventually(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < deadline, "condition did not become true");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn monitoring_is_lazy_shared_and_restarts_after_last_close() {
    let fixture = Fixture::new();
    let first = fixture.open("first");
    assert!(
        !host::host_dir().exists(),
        "ordinary sessions do not create discovery files"
    );
    let first_entry = fixture.register(&first);
    let second = fixture.open("second");
    let second_entry = fixture.register(&second);
    assert_eq!(first_entry.descriptor.owner, second_entry.descriptor.owner);
    let initial = snapshot(&first_entry);
    assert_eq!(initial.protocol, host::HOST_PROTOCOL);
    assert_eq!(initial.sessions.len(), 2);
    assert_eq!(initial.sessions[0].tags, ["rust"]);
    first.close().unwrap();
    assert_eq!(snapshot(&second_entry).sessions.len(), 1);
    second.close().unwrap();
    assert!(!host::descriptor_path(&first_entry.descriptor.owner).exists());
    let third = fixture.open("third");
    let third_entry = fixture.register(&third);
    assert_ne!(first_entry.descriptor.owner, third_entry.descriptor.owner);
}

#[test]
fn first_attachment_timeout_and_cancellation_wake_native_close() {
    let fixture = Fixture::new();
    let session = fixture.open("timeout");
    fixture.register(&session);
    let (_, generation) = monitoring::begin_wait_with_options(
        session.name(),
        "failed",
        Some(Duration::from_millis(20)),
        true,
    )
    .unwrap();
    assert!(!monitoring::wait(
        session.name(),
        generation,
        Some(Duration::from_millis(20)),
        true
    )
    .unwrap());
    session.close().unwrap();

    let session = fixture.open("close-before-wait");
    fixture.register(&session);
    monitoring::begin_wait_with_options(
        session.name(),
        "failed",
        Some(Duration::from_millis(40)),
        true,
    )
    .unwrap();
    let started = Instant::now();
    session.close().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(20));

    let session = fixture.open("cancel");
    fixture.register(&session);
    let (_, generation) =
        monitoring::begin_wait_with_options(session.name(), "failed", None, true).unwrap();
    let (waited, wait_result) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        waited
            .send(monitoring::wait("cancel", generation, None, true))
            .unwrap();
    });
    let (closed, observed) = mpsc::channel();
    let clone = session.clone();
    let closer = std::thread::spawn(move || {
        clone.close().unwrap();
        closed.send(()).unwrap();
    });
    assert!(observed.recv_timeout(Duration::from_millis(50)).is_err());
    monitoring::cancel_wait(session.name(), generation);
    assert!(!wait_result
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap());
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
    closer.join().unwrap();
}

#[test]
fn existing_and_brief_attachments_are_counted_once() {
    let fixture = Fixture::new();
    for attached_before_begin in [false, true] {
        let session = fixture.open("observed");
        let entry = fixture.register(&session);
        let existing = attached_before_begin.then(|| attach(&entry, false).0);
        let (_, generation) =
            monitoring::begin_wait_with_options(session.name(), "failed", None, true).unwrap();
        let client = existing.unwrap_or_else(|| attach(&entry, false).0);
        drop(client);
        eventually(|| snapshot(&entry).sessions[0].clients == 0);
        assert!(monitoring::wait(session.name(), generation, Some(Duration::ZERO), true).unwrap());
        session.close().unwrap();
    }
}

#[test]
fn readonly_leases_coexist_but_interactive_lease_is_exclusive() {
    let fixture = Fixture::new();
    let session = fixture.open("leases");
    let entry = fixture.register(&session);
    let (first, readonly_token) = attach(&entry, false);
    let (second, _) = attach(&entry, false);
    let (interactive, interactive_token) = attach(&entry, true);
    assert_eq!(snapshot(&entry).sessions[0].clients, 3);
    assert_eq!(snapshot(&entry).sessions[0].interactive_clients, 1);
    assert!(
        !handshake(
            &entry,
            None,
            Request::MonitorLeaseStream { interactive: true }
        )
        .1
        .ok
    );
    assert!(
        !handshake(
            &entry,
            Some(readonly_token),
            Request::MonitorInputStream { cols: 80, rows: 24 }
        )
        .1
        .ok
    );
    assert!(
        !handshake(
            &entry,
            None,
            Request::MonitorInputStream { cols: 80, rows: 24 }
        )
        .1
        .ok
    );
    let (input, response) = handshake(
        &entry,
        Some(interactive_token),
        Request::MonitorInputStream { cols: 80, rows: 24 },
    );
    assert!(response.ok);
    assert_eq!(
        snapshot(&entry).sessions[0].clients,
        3,
        "input is part of its lease, not a fourth client"
    );
    drop(input);
    drop((first, second, interactive));
    eventually(|| snapshot(&entry).sessions[0].clients == 0);
}

#[test]
fn closing_rejects_new_clients_and_close_all_cannot_bypass_failure_hold() {
    let fixture = Fixture::new();
    let session = fixture.open("held");
    let entry = fixture.register(&session);
    let target = session.monitor_target().unwrap();
    monitoring::begin_wait_with_options(session.name(), "failed", None, true).unwrap();
    let registry = fixture.registry.clone();
    let (closed, observed) = mpsc::channel();
    let closer = std::thread::spawn(move || {
        registry.close_all();
        closed.send(()).unwrap();
    });
    assert!(observed.recv_timeout(Duration::from_millis(50)).is_err());
    assert!(target.frame().is_some());
    let (first, _) = attach(&entry, false);
    let (second, _) = attach(&entry, false);
    assert!(observed.recv_timeout(Duration::from_millis(50)).is_err());
    drop(first);
    assert!(observed.recv_timeout(Duration::from_millis(50)).is_err());
    drop(second);
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    closer.join().unwrap();
    assert!(!target.is_current());

    let session = fixture.open("ordinary-closing");
    let entry = fixture.register(&session);
    let target = session.monitor_target().unwrap();
    let (first, _) = attach(&entry, false);
    let (second, _) = attach(&entry, false);
    let (closed, observed) = mpsc::channel();
    let closer = std::thread::spawn(move || {
        session.close().unwrap();
        closed.send(()).unwrap();
    });
    eventually(|| snapshot(&entry).sessions[0].status == "closing");
    assert!(
        !handshake(
            &entry,
            None,
            Request::MonitorLeaseStream { interactive: false }
        )
        .1
        .ok
    );
    drop(first);
    assert!(observed.recv_timeout(Duration::from_millis(50)).is_err());
    drop(second);
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    closer.join().unwrap();
    assert!(!target.is_current());
}

#[test]
fn replacement_is_generation_safe_and_child_exit_keeps_the_final_grid() {
    let fixture = Fixture::new();
    let session = fixture.open("generation");
    let entry = fixture.register(&session);
    let old_target = session.monitor_target().unwrap();
    session
        .open(OpenOptions {
            restart: true,
            ..open_options()
        })
        .unwrap();
    let replacement = fixture.register(&session);
    assert_ne!(entry.session.generation, replacement.session.generation);
    let stale_wait = monitoring::begin_wait_for_target_with_options(
        session.name(),
        &old_target,
        "failed",
        None,
        true,
    )
    .unwrap_err();
    assert_eq!(stale_wait.kind, tui_test::ErrorKind::NoSession);
    monitoring::cancel_target(session.name(), &old_target);
    let unchanged = snapshot(&replacement);
    assert_eq!(unchanged.sessions[0].status, "running");
    assert!(unchanged.sessions[0].outcome.is_none());
    old_target.close().unwrap();
    assert!(session.monitor_target().unwrap().is_current());
    assert!(monitoring::wait_target(session.name(), entry.session.generation).is_none());
    let mut stale = replacement.clone();
    stale.session.generation = entry.session.generation;
    assert!(
        !handshake(
            &stale,
            None,
            Request::MonitorLeaseStream { interactive: false }
        )
        .1
        .ok
    );
    let target = session.monitor_target().unwrap();
    target.write_monitor_input_raw(b"exit\r").unwrap();
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(10_000),
        })
        .unwrap();
    assert!(target.frame().unwrap().exited.is_some());
    let (client, _) = attach(&replacement, false);
    assert!(snapshot(&replacement).sessions[0].child_exited);
    target.write_monitor_input_raw(b"ignored").unwrap();
    target.resize(90, 25).unwrap();
    assert_eq!(target.frame().unwrap().size, (90, 25));
    drop(client);
}

#[test]
fn persistent_input_and_resize_bypass_terminal_waits_and_preserve_pipelined_bytes() {
    let fixture = Fixture::new();
    let session = fixture.open("input");
    let entry = fixture.register(&session);
    let (lease, token) = attach(&entry, true);
    let clone = session.clone();
    let (done, result) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        done.send(clone.execute(Operation::WaitLocator {
            query: LocatorQuery::text("bridge-pipelined-marker"),
            not: false,
            timeout_ms: Some(10_000),
        }))
        .unwrap();
    });
    std::thread::sleep(Duration::from_millis(30));
    let mut input = ipc::connect(&entry.descriptor.endpoint).unwrap();
    input.set_nonblocking(true).unwrap();
    let messages = [
        serde_json::to_string(&routed(
            &entry,
            Some(token),
            Request::MonitorInputStream { cols: 91, rows: 29 },
        ))
        .unwrap(),
        serde_json::to_string(&MonitorInput::Resize {
            cols: 101,
            rows: 31,
        })
        .unwrap(),
        serde_json::to_string(&MonitorInput::Write {
            data: b"echo bridge-pipelined-marker\r".to_vec(),
        })
        .unwrap(),
    ];
    writeln!(input, "{}", messages.join("\n")).unwrap();
    input.flush().unwrap();
    let mut input = BufReader::new(input);
    let response = read_response(&mut input);
    assert!(response.ok, "{response:?}");
    let ready: MonitorInputReady = serde_json::from_value(response.data.unwrap()).unwrap();
    assert!(!ready.initial_frame.is_empty());
    result
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    waiter.join().unwrap();
    assert_eq!(
        session.monitor_target().unwrap().frame().unwrap().size,
        (99, 29)
    );
    drop((input, lease));
}

#[test]
fn rust_facade_preserves_original_error_and_only_closes_its_child() {
    let _fixture = Fixture::new();
    let session = Session::new("rust-facade");
    session.open(open_options()).unwrap();
    let mut monitor = Monitor::for_session(
        &session,
        Options {
            enabled: true,
            wait_at_end: WaitPolicy::Failure,
            first_attach_timeout: Some(Duration::ZERO),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(monitor.id().is_some());
    let original = Box::new("original error identity");
    let identity = (&*original) as *const &str;
    let original = monitor.finish_failure(original);
    assert_eq!((&*original) as *const &str, identity);
    assert!(!session.is_open());

    session.open(open_options()).unwrap();
    let mut monitor = Monitor::for_session(&session, Options::default()).unwrap();
    assert!(monitor.id().is_none());
    session
        .open(OpenOptions {
            restart: true,
            ..open_options()
        })
        .unwrap();
    monitor.finish(Outcome::Passed).unwrap();
    assert!(session.is_open());
    session.close().unwrap();
}

#[test]
fn interactive_resize_is_recorded_before_following_output() {
    let fixture = Fixture::new();
    let session = fixture.registry.session("recorded-resize");
    session
        .open(OpenOptions {
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(fixture.home.join("casts")),
            },
            ..open_options()
        })
        .unwrap();
    let entry = fixture.register(&session);
    let (lease, token) = attach(&entry, true);
    let (mut input, ready) = handshake(
        &entry,
        Some(token),
        Request::MonitorInputStream { cols: 90, rows: 30 },
    );
    assert!(ready.ok, "{ready:?}");
    for message in [
        MonitorInput::Resize {
            cols: 100,
            rows: 31,
        },
        MonitorInput::Write {
            data: b"echo ordered-capture\r".to_vec(),
        },
    ] {
        writeln!(
            input.get_mut(),
            "{}",
            serde_json::to_string(&message).unwrap()
        )
        .unwrap();
    }
    session
        .execute(Operation::WaitLocator {
            query: LocatorQuery::text("ordered-capture"),
            not: false,
            timeout_ms: Some(5_000),
        })
        .unwrap();
    let recording = session.recording().unwrap();
    let events = recording
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let resize = events
        .iter()
        .position(|event| event[1] == "r" && event[2] == "98x29")
        .unwrap();
    let following_output = events[resize + 1..]
        .iter()
        .filter(|event| event[1] == "o")
        .filter_map(|event| event[2].as_str())
        .collect::<String>();
    assert!(
        following_output.contains("ordered-capture"),
        "{following_output:?}"
    );
    drop((input, lease));
}

#[test]
fn force_shutdown_cancels_infinite_holds_and_removes_discovery() {
    let fixture = Fixture::new();
    let session = fixture.open("force");
    let entry = fixture.register(&session);
    let (client, _) = attach(&entry, false);
    let (_, generation) =
        monitoring::begin_wait_with_options(session.name(), "failed", None, true).unwrap();
    let (done, result) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        done.send(monitoring::wait("force", generation, None, true))
            .unwrap();
    });
    assert!(result.recv_timeout(Duration::from_millis(50)).is_err());
    fixture.registry.force_close_all();
    assert!(!result
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap());
    waiter.join().unwrap();
    assert!(!host::descriptor_path(&entry.descriptor.owner).exists());
    assert!(session.monitor_target().is_none());
    drop(client);
}

#[test]
fn independent_live_sessions_cannot_replace_a_registered_name() {
    let _fixture = Fixture::new();
    let first = Session::new("duplicate-rust-name");
    let second = Session::new("duplicate-rust-name");
    first.open(open_options()).unwrap();
    second.open(open_options()).unwrap();
    let first_target = first.monitor_target().unwrap();
    let second_target = second.monitor_target().unwrap();
    assert!(monitoring::register(first.name(), &first_target, Metadata::default()).unwrap());
    let original = host::discover().into_iter().next().unwrap();
    let error =
        monitoring::register(second.name(), &second_target, Metadata::default()).unwrap_err();
    assert_eq!(error.kind, tui_test::ErrorKind::Usage);
    assert!(first_target.is_current());
    assert!(second_target.is_current());
    assert!(
        monitoring::wait_target(first.name(), original.session.generation)
            .unwrap()
            .same_target(&first_target)
    );
    assert_eq!(
        snapshot(&original).sessions[0].generation,
        original.session.generation
    );
    first.close().unwrap();
    assert!(monitoring::register(second.name(), &second_target, Metadata::default()).unwrap());
    second.close().unwrap();
}

#[test]
fn explicit_no_hold_completion_closes_even_when_clients_keep_their_leases() {
    let fixture = Fixture::new();
    for cleanup in 0..3 {
        let session = fixture.open("no-hold");
        let entry = fixture.register(&session);
        let target = session.monitor_target().unwrap();
        let (readonly, _) = attach(&entry, false);
        let (interactive, token) = attach(&entry, true);
        let (input, ready) = handshake(
            &entry,
            Some(token),
            Request::MonitorInputStream { cols: 80, rows: 25 },
        );
        assert!(ready.ok, "{ready:?}");
        let mut monitor = if cleanup == 2 {
            Some(
                Monitor::for_handle(
                    &session,
                    Options {
                        enabled: true,
                        wait_at_end: WaitPolicy::Never,
                        hold_while_attached: false,
                        ..Options::default()
                    },
                )
                .unwrap(),
            )
        } else {
            let (_, generation) =
                monitoring::begin_wait_with_options(session.name(), "failed", None, false).unwrap();
            assert!(monitoring::wait(session.name(), generation, None, false).unwrap());
            None
        };
        let registry = fixture.registry.clone();
        let (done, closed) = mpsc::channel();
        let closer = std::thread::spawn(move || {
            let result = match cleanup {
                0 => session.close(),
                1 => {
                    registry.close_all();
                    Ok(())
                }

                _ => monitor.as_mut().unwrap().finish(Outcome::Failed),
            };
            done.send(result).unwrap();
        });
        closed
            .recv_timeout(Duration::from_secs(5))
            .expect("no-hold close waited for live clients")
            .unwrap();
        closer.join().unwrap();
        assert!(!target.is_current());
        drop((readonly, interactive, input));
    }
}

#[test]
fn target_interruption_closes_without_starting_a_wait_or_requiring_client_disconnect() {
    let fixture = Fixture::new();
    let session = fixture.open("pre-wait-interruption");
    let entry = fixture.register(&session);
    let target = session.monitor_target().unwrap();
    let (client, _) = attach(&entry, false);
    monitoring::cancel_target(session.name(), &target);
    let interrupted = snapshot(&entry);
    assert_eq!(interrupted.sessions[0].generation, entry.session.generation);
    assert_eq!(interrupted.sessions[0].status, "closing");
    assert!(interrupted.sessions[0].outcome.is_none());
    assert!(interrupted.sessions[0].completed_at.is_none());
    let (done, completed) = mpsc::channel();
    let cleanup = target.clone();
    let closer = std::thread::spawn(move || {
        done.send(cleanup.close()).unwrap();
    });
    completed
        .recv_timeout(Duration::from_secs(5))
        .expect("interruption waited for an attached client")
        .unwrap();
    closer.join().unwrap();
    assert!(!target.is_current());
    drop(client);
}

#[test]
fn monitoring_startup_retains_readiness_failure_without_changing_default_run() {
    let fixture = Fixture::new();
    let ordinary = fixture.registry.session("ordinary-ready-failure");
    let error = ordinary.run(nonintegrated_program()).unwrap_err();
    assert_eq!(error.kind, tui_test::ErrorKind::Assertion);
    assert!(ordinary.monitor_target().is_none());

    let session = fixture.registry.session("monitored-ready-failure");
    let (result, target) = session.run_for_monitoring(nonintegrated_program());
    let error = result.unwrap_err();
    assert_eq!(error.kind, tui_test::ErrorKind::Assertion);
    assert!(error.message.contains("reported no prompt within 25ms"));
    let target =
        target.expect("spawned child should remain available for failed-startup inspection");
    assert!(target.is_current());
    assert!(target.frame().is_some());
    assert!(
        !host::host_dir().exists(),
        "retaining a target alone is not registration"
    );
    let entry = fixture.register(&session);
    let (client, _) = attach(&entry, false);
    let (_, generation) =
        monitoring::begin_wait_with_options(session.name(), "failed", None, false).unwrap();
    assert!(monitoring::wait(session.name(), generation, None, false).unwrap());
    assert_eq!(
        snapshot(&entry).sessions[0].outcome.as_deref(),
        Some("failed")
    );
    session.close_target(&target).unwrap();
    assert!(!target.is_current());
    drop(client);
}

#[test]
fn monitoring_startup_reports_unspawnable_children_without_a_target() {
    let fixture = Fixture::new();
    let session = Session::new("standalone-monitored-startup");
    let (opened, target) = session.open_for_monitoring(open_options());
    opened.unwrap();
    assert!(target.unwrap().is_current());
    session.close().unwrap();
    assert!(!host::host_dir().exists());

    let handle = fixture.registry.session("unspawnable-startup");
    let (result, target) = handle.run_for_monitoring(RunOptions {
        program: fixture
            .home
            .join("does-not-exist.exe")
            .to_string_lossy()
            .into_owned(),
        args: Vec::new(),
        ..nonintegrated_program()
    });
    let error = result.unwrap_err();
    assert_eq!(error.kind, tui_test::ErrorKind::Internal);
    assert!(error.message.contains("failed to open session"));
    assert!(target.is_none());
    assert!(handle.monitor_target().is_none());
    assert!(!host::host_dir().exists());
}

#[test]
fn explicit_cancellation_releases_live_client_holds_for_target_cleanup() {
    let fixture = Fixture::new();
    let session = fixture.open("cancel-with-clients");
    let entry = fixture.register(&session);
    let target = session.monitor_target().unwrap();
    let (readonly, _) = attach(&entry, false);
    let (interactive, token) = attach(&entry, true);
    let (input, ready) = handshake(
        &entry,
        Some(token),
        Request::MonitorInputStream { cols: 80, rows: 25 },
    );
    assert!(ready.ok, "{ready:?}");
    let (_, generation) =
        monitoring::begin_wait_with_options(session.name(), "failed", None, true).unwrap();
    let (done, completed) = mpsc::channel();
    let cleanup = target.clone();
    let waiter = std::thread::spawn(move || {
        let result = monitoring::wait("cancel-with-clients", generation, None, true)
            .and_then(|attached| cleanup.close().map(|_| attached));
        done.send(result).unwrap();
    });
    assert!(completed.recv_timeout(Duration::from_millis(50)).is_err());
    monitoring::cancel_wait(session.name(), generation);
    assert!(completed
        .recv_timeout(Duration::from_secs(5))
        .expect("cancelled cleanup waited for client disconnect")
        .unwrap());
    waiter.join().unwrap();
    assert!(!target.is_current());
    drop((readonly, interactive, input));
}
