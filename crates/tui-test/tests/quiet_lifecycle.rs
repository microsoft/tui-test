//! U10 quiet-wait lifecycle regression tests (R7, R8) with a controlled
//! Rust child (R25-part).
//!
//! Conventions: unique session names per test (parallel-safe), bounded joins
//! everywhere (`recv_timeout`, never a bare `join` on a thread that can
//! block), barrier/event synchronization instead of timing guesses, and no
//! process-wide environment changes.

use std::io::Write;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, ErrorKind, LocatorQuery, OpenOptions, Operation,
    RunOptions, Session, Timeouts, WaitQuietMode, WaitQuietOptions,
};

const FIXTURE: &str = "TUI_TEST_QUIET_LIFECYCLE_FIXTURE";
const JOIN_BOUND: Duration = Duration::from_secs(25);
const GATE_BOUND: Duration = Duration::from_secs(15);
const SETTLE_TIMEOUT_MS: u64 = 15_000;

fn emit(bytes: &[u8]) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(bytes).unwrap();
    stdout.flush().unwrap();
}

#[test]
fn child_fixture() {
    let Ok(mode) = std::env::var(FIXTURE) else {
        return;
    };
    match mode.as_str() {
        "quiet" => {
            emit(b"QUIET-READY\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("unknown fixture mode: {mode}"),
    }
}

fn session_name(tag: &str) -> String {
    format!("u10-ql-{tag}-{}", std::process::id())
}

fn fixture_options() -> RunOptions {
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
        style: defaults.style,
        cols: 80,
        rows: 24,
        cwd: None,
        env: vec![(FIXTURE.to_string(), "quiet".to_string())],
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

/// Start the controlled Rust child and wait for its `QUIET-READY` marker.
fn fixture_session(tag: &str) -> Session {
    let session = Session::new(session_name(tag));
    session.run(fixture_options()).expect("run fixture child");
    session
        .execute(Operation::WaitLocator {
            query: LocatorQuery::text("QUIET-READY"),
            not: false,
            timeout_ms: Some(SETTLE_TIMEOUT_MS),
        })
        .expect("fixture ready marker appears");
    session
}

fn byte_options(timeout: Duration, quiet: Duration) -> WaitQuietOptions {
    WaitQuietOptions::new(timeout, quiet)
        .expect("test options are valid")
        .with_mode(WaitQuietMode::Bytes)
}

/// Join a thread with a hard bound; a hang fails the test instead of
/// hanging the suite.
fn join_bounded<T: Send + 'static>(handle: std::thread::JoinHandle<T>, what: &str) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    rx.recv_timeout(JOIN_BOUND)
        .unwrap_or_else(|_| panic!("{what} did not finish within {JOIN_BOUND:?}"))
        .unwrap_or_else(|_| panic!("{what} panicked"))
}

// R7: byte quiet conflates no-session with no-output. A byte wait on a newly
// constructed, unopened session must report NoSession exactly like the visual
// wait does, not success after one quiet interval.
#[test]
fn r7_byte_quiet_on_unopened_session_reports_no_session() {
    let session = Session::new(session_name("r7-unopened"));
    let bytes = byte_options(Duration::from_secs(30), Duration::from_millis(300));
    let error = session
        .wait_quiet(&bytes)
        .expect_err("byte quiet without a session must fail");
    assert_eq!(
        error.kind,
        ErrorKind::NoSession,
        "byte quiet conflated no-session with no-output: {error}"
    );

    let visual = WaitQuietOptions::new(Duration::from_secs(30), Duration::from_millis(300))
        .expect("test options are valid");
    let error = session
        .wait_quiet(&visual)
        .expect_err("visual quiet without a session must fail");
    assert_eq!(
        error.kind,
        ErrorKind::NoSession,
        "visual quiet must report NoSession: {error}"
    );
}

// R8: a completed Close must never escape quiet-wait cancellation. The wait
// parks inside its cancellation callback; Close runs concurrently; when the
// gate releases, the wait must report an error, never success.
#[test]
fn r8_close_during_wait_never_reports_success() {
    let session = fixture_session("r8-close");
    let options = byte_options(Duration::from_secs(30), Duration::from_millis(500));

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(Some(entered_tx));
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&options, &|| {
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                let _ = tx.send(());
                let _ = release_rx.recv();
            }
            false
        })
    });
    entered_rx
        .recv_timeout(GATE_BOUND)
        .expect("wait reaches the cancellation gate");

    let (done_tx, done_rx) = mpsc::channel();
    let closer = session.clone();
    std::thread::spawn(move || {
        let _ = done_tx.send(closer.close());
    });
    // Grace for Close to announce itself (fixed code: it blocks on the
    // lifecycle guard; base code: it completes underneath the parked wait).
    let pre_release_close = done_rx.recv_timeout(Duration::from_secs(3)).ok();
    let _ = release_tx.send(());
    assert!(
        pre_release_close.is_none(),
        "Close completed underneath a parked quiet wait (R8: no lifecycle coordination)"
    );

    let outcome = join_bounded(wait, "quiet wait");
    let close_result = done_rx
        .recv_timeout(JOIN_BOUND)
        .expect("close finishes after the wait yields");
    assert!(
        close_result.is_ok(),
        "close must complete: {close_result:?}"
    );
    assert!(
        outcome.is_err(),
        "completed Close escaped quiet-wait cancellation (R8)"
    );
}

// R8: same escape through Restart. The restarted session is a different
// generation; the parked wait must not report quiet success on it.
#[test]
fn r8_restart_during_wait_never_reports_success() {
    let session = fixture_session("r8-restart");
    let options = byte_options(Duration::from_secs(30), Duration::from_millis(500));

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(Some(entered_tx));
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&options, &|| {
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                let _ = tx.send(());
                let _ = release_rx.recv();
            }
            false
        })
    });
    entered_rx
        .recv_timeout(GATE_BOUND)
        .expect("wait reaches the cancellation gate");

    let (done_tx, done_rx) = mpsc::channel();
    let restarter = session.clone();
    std::thread::spawn(move || {
        let _ = done_tx.send(restarter.execute(Operation::Restart {
            graceful_timeout_ms: 1_000,
        }));
    });
    let pre_release_restart = done_rx.recv_timeout(Duration::from_secs(3)).ok();
    let _ = release_tx.send(());
    assert!(
        pre_release_restart.is_none(),
        "Restart completed underneath a parked quiet wait (R8: no lifecycle coordination)"
    );

    let outcome = join_bounded(wait, "quiet wait");
    let restart_result = done_rx
        .recv_timeout(JOIN_BOUND)
        .expect("restart finishes after the wait yields");
    assert!(
        restart_result.is_ok(),
        "restart must complete: {restart_result:?}"
    );
    assert!(
        outcome.is_err(),
        "completed Restart escaped quiet-wait cancellation (R8)"
    );
    session.close().expect("close terminal");
}
