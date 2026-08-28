#[cfg(unix)]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{
    global_registry, ErrorKind, LocatorDirection, LocatorExpectOptions, LocatorQuery,
    MatchOccurrence, OpenOptions, Operation, OperationResult, RunOptions, Session, SessionRegistry,
    TextSelector, TextStyle, Timeouts,
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
        .execute(Operation::WaitLocator {
            query: LocatorQuery::text("native-runtime"),
            not: false,
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
            Operation::WaitLocator {
                query: LocatorQuery::text("text-that-will-never-appear"),
                not: false,
                timeout_ms: Some(700),
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
fn text_locators_are_lazy_reusable_queries() {
    let session = Session::new(format!("native-locator-{}", std::process::id()));
    session
        .open(OpenOptions {
            wait_ready: Some(true),
            ..OpenOptions::default()
        })
        .expect("open terminal");

    let locator = session.get_by_text(TextSelector::new("locator-target"));
    assert_eq!(locator.count().expect("count initial matches"), 0);
    assert!(locator
        .location()
        .unwrap_err()
        .message
        .contains("Terminal content:"));
    let command = if cfg!(windows) {
        "Write-Output ('locator-'+'target locator-'+'target')"
    } else {
        "printf 'locator-%s locator-%s\\n' target target"
    };
    session
        .execute(Operation::Submit {
            data: Some(command.to_string()),
        })
        .expect("submit locator output");
    locator
        .wait_with_timeout(Some(5_000))
        .expect("wait with the same locator");
    assert_eq!(locator.count().expect("count current matches"), 2);
    locator.expect().expect("expect any matching result");
    locator.any().expect().expect("expect explicit any match");
    locator.first().expect().expect("expect first match");
    locator.last().expect().expect("expect last match");
    locator.nth(1).expect().expect("expect second match");
    assert!(locator
        .unique()
        .expect_with(LocatorExpectOptions {
            timeout_ms: Some(20),
            ..LocatorExpectOptions::default()
        })
        .is_err());
    let hidden = LocatorExpectOptions {
        not: true,
        timeout_ms: Some(20),
    };
    session
        .get_by_text("missing-locator-target")
        .unique()
        .expect_with(hidden)
        .expect("expect an absent unique locator");
    locator
        .nth(2)
        .expect_with(hidden)
        .expect("expect an absent third match");
    assert!(locator
        .unique()
        .expect_with(hidden)
        .unwrap_err()
        .message
        .contains("found 2"));
    assert!(locator.first().expect_with(hidden).is_err());
    locator.first().expect().expect("expect one selected match");
    let relative = locator
        .first()
        .get_by_text_relative(TextSelector::new("locator-target"), LocatorDirection::After);
    assert_eq!(
        relative.first().location().unwrap().start,
        locator.nth(1).location().unwrap().start
    );
    let nested = session
        .get_by_text("locator-target locator-target")
        .get_by_text("locator-target")
        .get_by_text("target");
    assert_eq!(nested.count().expect("count nested matches"), 2);
    nested.highlight().expect("highlight nested matches");
    locator.highlight().expect("highlight all current matches");
    assert_eq!(locator.all().expect("materialize locators").len(), 2);
    assert_eq!(
        locator
            .nth(1)
            .location()
            .expect("resolve second match")
            .start
            .column,
        locator
            .last()
            .location()
            .expect("resolve last match")
            .start
            .column
    );
    assert_eq!(
        &locator.unique().query().occurrence,
        &MatchOccurrence::Unique
    );
    session.close().expect("close terminal");
}

#[test]
fn text_locator_highlight_marks_the_current_grid() {
    let session = Session::new(format!("native-highlight-{}", std::process::id()));
    session
        .open(OpenOptions {
            wait_ready: Some(true),
            ..OpenOptions::default()
        })
        .expect("open terminal");
    session
        .execute(Operation::Submit {
            data: Some("echo highlight-target".to_string()),
        })
        .expect("submit highlighted output");

    let locator = session.get_by_text("highlight-target").last();
    locator
        .wait_with_timeout(Some(5_000))
        .expect("wait for highlighted text");
    session
        .execute(Operation::WaitIdle {
            timeout_ms: Some(5_000),
        })
        .expect("wait for idle before highlighting");
    let location = locator.location().expect("resolve highlighted location");
    locator.highlight().expect("highlight locator");
    session
        .execute(Operation::WaitIdle {
            timeout_ms: Some(10),
        })
        .expect("highlight must not reset terminal idle timing");
    assert_eq!(
        locator.location().expect("resolve locator after highlight"),
        location
    );
    session.close().expect("close terminal");
}

#[test]
fn text_and_style_locators_chain_against_the_live_grid() {
    let session = Session::new(format!("native-style-locator-{}", std::process::id()));
    session
        .open(OpenOptions {
            wait_ready: Some(true),
            ..OpenOptions::default()
        })
        .expect("open terminal");
    let command = if cfg!(windows) {
        "[Console]::Write(('plain '+[char]27+'[1mBOLD'+[char]27+'[0m'+[Environment]::NewLine))"
    } else {
        "printf 'plain \\033[1mBOLD\\033[0m\\n'"
    };
    session
        .execute(Operation::Submit {
            data: Some(command.to_string()),
        })
        .expect("submit styled output");

    let bold = TextStyle {
        bold: Some(true),
        ..TextStyle::default()
    };
    let styled_text = session
        .get_by_style(bold.clone())
        .get_by_text("BOLD")
        .unique();
    styled_text
        .wait_with_timeout(Some(5_000))
        .expect("wait for text inside bold run");
    assert_eq!(styled_text.location().unwrap().text, "BOLD");

    let text_style = session.get_by_text("BOLD").get_by_style(bold).unique();
    assert_eq!(text_style.location().unwrap().text, "BOLD");
    text_style.expect().expect("expect styled text");
    text_style.highlight().expect("highlight styled text");
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
            Operation::WaitLocator {
                query: LocatorQuery::text("never-appears"),
                not: false,
                timeout_ms: Some(30_000),
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
