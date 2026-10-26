//! U10 request waits: quiet intervals and per-request cancellation.
//!
//! Deterministic harness (R25): every child is a controlled Rust fixture
//! (this same test binary re-invoked with a mode env var), never a shell, so
//! output bytes are exact on every platform. Threads rendezvous through
//! channel barriers and observed screen markers, never through sleeps or
//! elapsed-time bounds: there are no wall-clock timing assertions anywhere in
//! this file. Exact poll/sample/deadline counting lives in the injected-clock
//! unit tests in `src/wait.rs`; these integration tests assert functional
//! outcomes (success, timeout/cancelled kinds, marker ordering, isolation)
//! over a real PTY.
//!
//! Conventions: unique session names per test (parallel-safe), bounded joins
//! and gate waits everywhere (`recv_timeout`, never a bare `join` on a thread
//! that can block), barrier/event synchronization instead of timing guesses,
//! and no process-wide environment changes.

use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use tui_test::{
    profile::Rgb, AutomaticRecording, AutomaticRecordingMode, ErrorKind, LocatorQuery, OpenOptions,
    Operation, RunOptions, ScrollTarget, Session, SnapshotScope, Timeouts, WaitQuietMode,
    WaitQuietOptions,
};

const FIXTURE: &str = "TUI_TEST_REQUEST_WAITS_FIXTURE";
const JOIN_BOUND: Duration = Duration::from_secs(25);
const GATE_BOUND: Duration = Duration::from_secs(15);
const SETTLE_TIMEOUT_MS: u64 = 15_000;
const QUIET: Duration = Duration::from_millis(300);

fn emit(bytes: &[u8]) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(bytes).unwrap();
    stdout.flush().unwrap();
}

/// Controlled Rust child fixture. One mode per test need; every mode emits a
/// `READY` marker first so the parent can synchronize on observed output.
/// Standard input is echoed back to standard output so `Submit`-based tests
/// observe their input without a shell.
#[test]
fn child_fixture() {
    let Ok(mode) = std::env::var(FIXTURE) else {
        return;
    };
    std::thread::spawn(|| {
        use std::io::Read;
        let mut input = std::io::stdin();
        let mut buf = [0u8; 1024];
        loop {
            match input.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    emit(&buf[..n]);
                }
            }
        }
    });
    match mode.as_str() {
        // `READY`, then silence: the steady state every wait settles into.
        "idle" => {
            emit(b"READY\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        // `READY`, then an endless visible line stream: quiet can never settle.
        "busy" => {
            emit(b"READY\r\n");
            let mut i = 0u64;
            loop {
                emit(format!("tick-{i}\r\n").as_bytes());
                i += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // `READY`, then screen-neutral output (every read restarts a
        // byte-quiet timer while visual samples stay equal), then a done
        // marker both the byte wait and the test can observe.
        "neutral" => {
            emit(b"READY\r\n");
            std::thread::sleep(Duration::from_millis(300));
            for _ in 0..50 {
                emit(b"\x1b[0m");
                std::thread::sleep(Duration::from_millis(20));
            }
            emit(b"NEUTRAL-DONE\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        // `READY`, then one late visible line: the change lands 600ms in, so
        // a 1000ms quiet wait started at `READY` must span it.
        "late" => {
            emit(b"READY\r\n");
            std::thread::sleep(Duration::from_millis(600));
            emit(b"LATE-VISIBLE\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        // `READY`, then a cursor-only move followed by a marker: cells, title,
        // and palette are untouched, so only the cursor state can restart a
        // visual timer.
        "cursor" => {
            emit(b"READY\r\n");
            std::thread::sleep(Duration::from_millis(600));
            emit(b"\x1b[7;25H");
            emit(b"CURSOR-DONE\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        // `READY`, then a scrollback's worth of fill lines for viewport tests.
        "fill" => {
            emit(b"READY\r\n");
            for i in 1..=100 {
                emit(format!("fill-{i}\r\n").as_bytes());
            }
            std::thread::sleep(Duration::from_secs(30));
        }
        // `READY`, then sub-sample screen-neutral pairs: every pair flows
        // through the PTY (byte-quiet observes every read) while 50ms visual
        // samples only ever see the stable screen.
        "transient" => {
            emit(b"READY\r\n");
            std::thread::sleep(Duration::from_millis(300));
            for _ in 0..40 {
                emit(b"X\x08 \x08");
                std::thread::sleep(Duration::from_millis(20));
            }
            emit(b"TRANSIENT-DONE\r\n");
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("unknown fixture mode: {mode}"),
    }
}

fn session_name(tag: &str) -> String {
    format!("u10-{tag}-{}", std::process::id())
}

fn fixture_options(mode: &str) -> RunOptions {
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

/// Start the fixture child in `mode` and wait for its `READY` marker.
fn ready_session(tag: &str, mode: &str) -> Session {
    let session = Session::new(session_name(tag));
    session
        .run(fixture_options(mode))
        .expect("run fixture child");
    wait_for_text(&session, "READY");
    session
}

fn visual_options(quiet: Duration) -> WaitQuietOptions {
    WaitQuietOptions::new(Duration::from_secs(30), quiet).expect("test options are valid")
}

fn byte_options(timeout: Duration, quiet: Duration) -> WaitQuietOptions {
    WaitQuietOptions::new(timeout, quiet)
        .expect("test options are valid")
        .with_mode(WaitQuietMode::Bytes)
}

fn wait_for_text(session: &Session, text: &str) {
    session
        .execute(Operation::WaitLocator {
            query: LocatorQuery::text(text),
            not: false,
            timeout_ms: Some(SETTLE_TIMEOUT_MS),
        })
        .expect("settle marker appears");
}

/// Join a thread with a hard bound; a hang fails the test instead of hanging
/// the suite.
fn join_bounded<T: Send + 'static>(handle: std::thread::JoinHandle<T>, what: &str) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    rx.recv_timeout(JOIN_BOUND)
        .unwrap_or_else(|_| panic!("{what} did not finish within {JOIN_BOUND:?}"))
        .unwrap_or_else(|_| panic!("{what} panicked"))
}

/// One-shot gate proving a wait reached its first poll: the returned holder
/// fires once from inside the cancellation callback, then answers from
/// `flag`. Waiting on the receiver (with `GATE_BOUND`) replaces every
/// "sleep, then assume the wait started" rendezvous.
fn entry_gate() -> (Arc<Mutex<Option<mpsc::Sender<()>>>>, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    (Arc::new(Mutex::new(Some(tx))), rx)
}

fn signal_entry(gate: &Mutex<Option<mpsc::Sender<()>>>) {
    if let Some(tx) = gate.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

fn await_entry(rx: &mpsc::Receiver<()>, what: &str) {
    rx.recv_timeout(GATE_BOUND)
        .unwrap_or_else(|_| panic!("{what} never reached its first poll within {GATE_BOUND:?}"));
}

fn is_timeout(error: &tui_test::TuiTestError) -> bool {
    error.kind == ErrorKind::Assertion && error.to_string().contains("timed out")
}

fn is_cancelled(error: &tui_test::TuiTestError) -> bool {
    error.kind == ErrorKind::Assertion && error.to_string().contains("cancelled")
}

fn cancel_flag() -> (Arc<AtomicBool>, impl Fn() -> bool + Send + Sync + 'static) {
    let flag = Arc::new(AtomicBool::new(false));
    let check = flag.clone();
    (flag, move || check.load(Ordering::Acquire))
}

// U10-A: continuous output, equal-screen output, and a late visual change.
#[test]
fn u10_a_continuous_visible_output_times_out_visual_wait() {
    let session = ready_session("a-busy", "busy");
    let options =
        WaitQuietOptions::new(Duration::from_millis(800), QUIET).expect("test options are valid");
    let error = session
        .wait_quiet(&options)
        .expect_err("continuous visible output must time out");
    assert!(is_timeout(&error), "unexpected error: {error}");
    session.close().expect("close terminal");
}

#[test]
fn u10_a_equal_screen_output_restarts_bytes_not_visual() {
    // A stream of screen-neutral output: every read restarts a byte-quiet
    // timer, while visual samples stay equal throughout.
    let session = ready_session("a-neutral", "neutral");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("equal-screen output must not delay visual quiet");
    wait_for_text(&session, "NEUTRAL-DONE");
    session.close().expect("close terminal");

    // The byte wait spans the whole neutral stream: it starts at READY while
    // the stream is still flowing and succeeds only after the stream's done
    // marker is observed and the line goes quiet.
    let session = ready_session("a-neutral-bytes", "neutral");
    let worker = session.clone();
    let bytes = byte_options(Duration::from_secs(15), QUIET);
    let wait = std::thread::spawn(move || worker.wait_quiet(&bytes));
    wait_for_text(&session, "NEUTRAL-DONE");
    join_bounded(wait, "byte quiet over the neutral stream")
        .expect("byte quiet follows the neutral stream");
    session.close().expect("close terminal");
}

#[test]
fn u10_a_late_visual_change_restarts_then_succeeds() {
    // The change lands 600ms in, inside the 1000ms quiet interval of a wait
    // proven started at READY, so success must follow the change: the wait
    // cannot succeed before the change lands, and the marker proves it did.
    let quiet = Duration::from_millis(1000);
    let session = ready_session("a-late", "late");
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(quiet), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "late-change wait");
    wait_for_text(&session, "LATE-VISIBLE");
    join_bounded(wait, "late-change wait").expect("late change restarts, then succeeds");
    session.close().expect("close terminal");
}

// U10-B: timeout and cancellation finish within a bounded test deadline.
#[test]
fn u10_b_timeout_is_bounded() {
    let session = ready_session("b-timeout", "busy");
    let options = byte_options(Duration::from_secs(1), QUIET);
    let error = session
        .wait_quiet(&options)
        .expect_err("continuous output must time out byte quiet");
    assert!(is_timeout(&error), "unexpected error: {error}");
    session.close().expect("close terminal");
}

#[test]
fn u10_b_cancellation_is_bounded() {
    let session = ready_session("b-cancel", "idle");
    let (flag, check) = cancel_flag();
    let (gate, entered) = entry_gate();
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let worker = session.clone();
    let handle = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&options, &move || {
            signal_entry(&gate);
            check()
        })
    });
    // The gate proves the wait is parked in its poll loop; only then does the
    // test cancel, so no sleep guesses when cancellation lands.
    await_entry(&entered, "cancelled wait");
    flag.store(true, Ordering::Release);
    let error = join_bounded(handle, "cancelled wait")
        .expect_err("cancelled wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");
    session.close().expect("close terminal");
}

// U10-C: cancellation kills neither the child nor a different caller.
#[test]
fn u10_c_cancel_leaves_child_and_other_caller_alone() {
    let session = ready_session("c-alive", "idle");
    let (flag, check) = cancel_flag();
    let (gate, entered) = entry_gate();
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let first = session.clone();
    let cancelled = std::thread::spawn(move || {
        first.wait_quiet_cancellable(&options, &move || {
            signal_entry(&gate);
            check()
        })
    });
    let second = session.clone();
    let other = std::thread::spawn(move || second.wait_quiet(&visual_options(QUIET)));
    await_entry(&entered, "cancelled wait");
    flag.store(true, Ordering::Release);
    let error =
        join_bounded(cancelled, "cancelled wait").expect_err("first wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");
    join_bounded(other, "other wait").expect("other caller is not cancelled");

    // The child still runs: it echoes new input after the cancellation.
    session
        .execute(Operation::Submit {
            data: Some("echo C-ALIVE".to_string()),
        })
        .expect("submit after cancellation");
    wait_for_text(&session, "C-ALIVE");
    session.close().expect("close terminal");
}

// U10-D: input and capture operate correctly after cancellation.
#[test]
fn u10_d_input_and_capture_work_after_cancel() {
    let session = ready_session("d-after", "idle");
    let (flag, check) = cancel_flag();
    flag.store(true, Ordering::Release);
    let error = session
        .wait_quiet_cancellable(&visual_options(QUIET), &check)
        .expect_err("pre-cancelled wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");

    session
        .execute(Operation::Submit {
            data: Some("echo D-AFTER".to_string()),
        })
        .expect("input works after cancellation");
    wait_for_text(&session, "D-AFTER");
    let snapshot = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture works after cancellation");
    assert!(snapshot.rows() > 0, "captured an empty screen");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("quiet wait works after cancellation");
    session.close().expect("close terminal");
}

// U10-E: the wait releases its operation resources after each outcome.
#[test]
fn u10_e_resources_released_after_success_timeout_and_cancel() {
    let session = ready_session("e-release", "idle");

    // Success outcome, then timeout (equal deadlines always time out), then
    // cancellation: the session serves input, capture, and waits after each.
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("success outcome");
    session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after success");

    let equal = WaitQuietOptions::new(Duration::from_millis(400), Duration::from_millis(400))
        .expect("equal deadlines are accepted");
    let error = session
        .wait_quiet(&equal)
        .expect_err("equal deadlines must time out");
    assert!(is_timeout(&error), "unexpected error: {error}");
    session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after timeout");

    let (flag, check) = cancel_flag();
    let (gate, entered) = entry_gate();
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let worker = session.clone();
    let handle = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&options, &move || {
            signal_entry(&gate);
            check()
        })
    });
    await_entry(&entered, "cancelled wait");
    flag.store(true, Ordering::Release);
    let error = join_bounded(handle, "cancelled wait")
        .expect_err("cancelled wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");

    // Two concurrent waits both succeed: no wait holds an exclusive resource.
    let first = session.clone();
    let one = std::thread::spawn(move || first.wait_quiet(&visual_options(QUIET)));
    let second = session.clone();
    let two = std::thread::spawn(move || second.wait_quiet(&visual_options(QUIET)));
    join_bounded(one, "first concurrent wait").expect("first concurrent wait");
    join_bounded(two, "second concurrent wait").expect("second concurrent wait");

    session
        .execute(Operation::Submit {
            data: Some("echo E-RELEASE".to_string()),
        })
        .expect("input after all outcomes");
    wait_for_text(&session, "E-RELEASE");
    session.close().expect("close terminal");
}

// U10-F: other callers and viewport changes cannot reset this request's timer.
#[test]
fn u10_f_other_callers_do_not_reset_visual_timer() {
    let session = ready_session("f-callers", "idle");
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(QUIET), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "wait under concurrent reads");
    // Another caller hammers capture and metadata reads while the wait runs;
    // a fixed iteration count (not a sleep) bounds this phase.
    for _ in 0..10 {
        session
            .capture_snapshot(SnapshotScope::ActiveScreen)
            .expect("concurrent capture");
        session
            .capture_snapshot(SnapshotScope::Viewport)
            .expect("concurrent viewport capture");
    }
    join_bounded(wait, "wait under concurrent reads").expect("wait succeeds");

    // Re-run with a hammer thread driving a fixed burst of reads.
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(QUIET), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "steady-state wait");
    let probe = session.clone();
    let hammer = std::thread::spawn(move || {
        for _ in 0..20 {
            let _ = probe.capture_snapshot(SnapshotScope::ActiveScreen);
        }
    });
    join_bounded(wait, "steady-state wait").expect("wait succeeds under concurrent reads");
    join_bounded(hammer, "capture hammer");
    session.close().expect("close terminal");
}

#[test]
fn u10_f_viewport_scroll_does_not_reset_visual_timer() {
    let session = Session::new(session_name("f-scroll"));
    session
        .run(fixture_options("fill"))
        .expect("run fixture child");
    // The READY marker scrolls out under the fill output, so settle on the
    // last fill line instead of the READY marker.
    wait_for_text(&session, "fill-100");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("screen settles before the scroll test");
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(QUIET), &move || {
            signal_entry(&gate);
            false
        })
    });
    // The gate proves the wait started before the scroll, so the scroll lands
    // mid-wait without any sleep rendezvous.
    await_entry(&entered, "wait across scroll");
    session
        .execute(Operation::Scroll {
            target: ScrollTarget::Lines(5),
        })
        .expect("scroll viewport");
    join_bounded(wait, "wait across scroll").expect("scroll must not reset timer");
    session
        .execute(Operation::Scroll {
            target: ScrollTarget::Bottom,
        })
        .expect("scroll back");
    session.close().expect("close terminal");
}

// U10-G: a controlled change between sample points shows the sampling limit.
//
// Rapid change-then-revert pairs flow through the PTY (byte-quiet observes
// every read) while 50ms visual samples only ever see the stable screen, so
// the visual wait finishes on its first quiet interval.
#[test]
fn u10_g_rapid_change_and_revert_misses_visual_samples() {
    let session = ready_session("g-transient", "transient");
    let worker = session.clone();
    let wait = std::thread::spawn(move || worker.wait_quiet(&visual_options(QUIET)));
    // The transient stream runs while the wait observes; the done marker
    // proves the stream completed and the screen is stable again.
    wait_for_text(&session, "TRANSIENT-DONE");
    join_bounded(wait, "transient wait")
        .expect("transients between samples must not delay visual quiet");
    let snapshot = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after transient stream");
    assert!(snapshot.rows() > 0, "captured an empty screen");
    session.close().expect("close terminal");
}

// U10-H: palette-only, title-only, cursor-only, and dimension changes restart
// visual quiet. The entry gate proves each wait started before its change
// lands mid-wait, and observed state (title, palette, marker) proves the
// change actually happened; no elapsed-time bounds are asserted.
#[test]
fn u10_h_title_change_restarts_visual_quiet() {
    let session = ready_session("h-title", "idle");
    let quiet = Duration::from_millis(500);
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(quiet), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "title wait");
    // The fixture child echoes raw input back to the PTY, so the emulator
    // processes the title sequence exactly as a shell's output would.
    session
        .execute(Operation::Submit {
            data: Some("\x1b]0;U10-TITLE-CHANGE\x07".to_string()),
        })
        .expect("set title");
    join_bounded(wait, "title wait").expect("title change restarts");
    let snapshot = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after title change");
    assert!(
        snapshot
            .title()
            .is_some_and(|title| title.contains("U10-TITLE-CHANGE")),
        "title change did not land: {snapshot:?}"
    );
    session.close().expect("close terminal");
}

#[test]
fn u10_h_palette_change_restarts_visual_quiet() {
    let session = ready_session("h-palette", "idle");
    let quiet = Duration::from_millis(500);
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(quiet), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "palette wait");
    session
        .execute(Operation::Submit {
            data: Some("\x1b]4;9;rgb:ff/00/00\x07".to_string()),
        })
        .expect("set palette entry");
    join_bounded(wait, "palette wait").expect("palette change restarts");
    let snapshot = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after palette change");
    assert_eq!(
        snapshot.palette()[9],
        Rgb::new(255, 0, 0),
        "palette change did not land"
    );
    session.close().expect("close terminal");
}

#[test]
fn u10_h_cursor_move_restarts_visual_quiet() {
    // The child moves only the cursor 600ms in, inside the 1000ms quiet
    // interval of a wait proven started at READY; the marker proves the move
    // happened and success proves the wait spanned it.
    let quiet = Duration::from_millis(1000);
    let session = ready_session("h-cursor", "cursor");
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(quiet), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "cursor wait");
    wait_for_text(&session, "CURSOR-DONE");
    join_bounded(wait, "cursor wait").expect("cursor move restarts, then succeeds");
    session.close().expect("close terminal");
}

#[test]
fn u10_h_resize_restarts_visual_quiet() {
    let session = ready_session("h-resize", "idle");
    let quiet = Duration::from_millis(500);
    let (gate, entered) = entry_gate();
    let worker = session.clone();
    let wait = std::thread::spawn(move || {
        worker.wait_quiet_cancellable(&visual_options(quiet), &move || {
            signal_entry(&gate);
            false
        })
    });
    await_entry(&entered, "resize wait");
    session
        .execute(Operation::Resize {
            cols: 100,
            rows: 30,
        })
        .expect("resize terminal");
    join_bounded(wait, "resize wait").expect("resize restarts");
    session.close().expect("close terminal");
}

// U10-I: the quiet interval starts with the request after a long idle period.
#[test]
fn u10_i_quiet_starts_at_request_after_long_idle() {
    let session = ready_session("i-idle", "idle");
    // Settle first: this success proves the line is quiet, so the measured
    // waits below start after a long idle period by construction.
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("quiet wait succeeds after idle");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("repeat quiet wait");
    let bytes = visual_options(QUIET).with_mode(WaitQuietMode::Bytes);
    session.wait_quiet(&bytes).expect("byte quiet after idle");
    session.close().expect("close terminal");
}

// U10-J: equal quiet and overall deadlines return timeout.
#[test]
fn u10_j_equal_deadlines_return_timeout() {
    let session = ready_session("j-equal", "idle");
    for quiet_ms in [400, 800] {
        let options = WaitQuietOptions::new(
            Duration::from_millis(quiet_ms),
            Duration::from_millis(quiet_ms),
        )
        .expect("equal deadlines are accepted");
        let error = session
            .wait_quiet(&options)
            .expect_err("equal deadlines must time out");
        assert!(is_timeout(&error), "unexpected error: {error}");
    }
    session.close().expect("close terminal");
}

#[test]
fn u10_options_validation_rejects_zero_and_overlong() {
    assert!(WaitQuietOptions::new(Duration::ZERO, QUIET).is_err());
    assert!(WaitQuietOptions::new(Duration::from_secs(1), Duration::ZERO).is_err());
    assert!(WaitQuietOptions::new(Duration::from_millis(100), Duration::from_millis(200)).is_err());
    assert!(WaitQuietOptions::new(Duration::from_millis(200), Duration::from_millis(200)).is_ok());
    let defaults = WaitQuietOptions::default();
    assert_eq!(defaults.quiet(), Duration::from_millis(250));
    assert_eq!(defaults.timeout(), Duration::from_secs(5));
    assert_eq!(defaults.mode(), WaitQuietMode::Visual);
}
