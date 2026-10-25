//! U10 request waits: quiet intervals and per-request cancellation.
//!
//! Conventions: unique session names per test (parallel-safe), bounded
//! condition waits everywhere, timing assertions with generous upper bounds
//! and structural lower bounds. No process-wide environment changes.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use tui_test::{
    ErrorKind, LocatorQuery, OpenOptions, Operation, RunOptions, ScrollTarget, Session,
    SnapshotScope, WaitQuietMode, WaitQuietOptions,
};

const QUIET: Duration = Duration::from_millis(300);
const SETTLE_TIMEOUT_MS: u64 = 15_000;

fn session_name(tag: &str) -> String {
    format!("u10-{tag}-{}", std::process::id())
}

fn run_options(program: &str, args: &[&str]) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        backend: defaults.backend,
        program: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        profile: defaults.profile,
        style: defaults.style,
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

fn visual_options(quiet: Duration) -> WaitQuietOptions {
    WaitQuietOptions::new(Duration::from_secs(30), quiet).expect("test options are valid")
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

/// Start a scripted child and wait for its `READY` marker.
fn ready_session(tag: &str, script: &str) -> Session {
    let session = Session::new(session_name(tag));
    if cfg!(windows) {
        session
            .run(run_options(
                "powershell.exe",
                &["-NoProfile", "-Command", script],
            ))
            .expect("run child");
    } else {
        session
            .run(run_options("sh", &["-c", script]))
            .expect("run child");
    }
    wait_for_text(&session, "READY");
    session
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
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); $i=0; while ($true) { [Console]::WriteLine(\"tick-$i\"); $i++; Start-Sleep -Milliseconds 20 }"
    } else {
        "printf READY; i=0; while true; do echo tick-$i; i=$((i+1)); sleep 0.02; done"
    };
    let session = ready_session("a-busy", script);
    let options =
        WaitQuietOptions::new(Duration::from_millis(800), QUIET).expect("test options are valid");
    let started = Instant::now();
    let error = session
        .wait_quiet(&options)
        .expect_err("continuous visible output must time out");
    let elapsed = started.elapsed();
    assert!(is_timeout(&error), "unexpected error: {error}");
    assert!(
        elapsed >= Duration::from_millis(700),
        "timed out too early: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "timeout took too long: {elapsed:?}"
    );
    session.close().expect("close terminal");
}

#[test]
fn u10_a_equal_screen_output_restarts_bytes_not_visual() {
    // A stream of screen-neutral output: every read restarts a byte-quiet
    // timer, while visual samples stay equal throughout.
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Milliseconds 300; for ($i=0; $i -lt 50; $i++) { [Console]::Write(\"`e[0m\"); Start-Sleep -Milliseconds 20 }; Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 0.3; for i in $(seq 1 50); do printf '\\033[0m'; sleep 0.02; done; sleep 30"
    };
    let session = ready_session("a-neutral", script);
    let visual = visual_options(QUIET);
    let started = Instant::now();
    session
        .wait_quiet(&visual)
        .expect("equal-screen output must not delay visual quiet");
    let visual_elapsed = started.elapsed();
    assert!(
        visual_elapsed < Duration::from_millis(900),
        "visual wait outlived the neutral stream: {visual_elapsed:?}"
    );

    let bytes = WaitQuietOptions::new(Duration::from_secs(15), QUIET)
        .expect("test options are valid")
        .with_mode(WaitQuietMode::Bytes);
    let session = ready_session("a-neutral-bytes", script);
    let started = Instant::now();
    session
        .wait_quiet(&bytes)
        .expect("byte quiet follows the neutral stream");
    let bytes_elapsed = started.elapsed();
    assert!(
        bytes_elapsed >= Duration::from_secs(1),
        "byte wait ignored equal-screen output: {bytes_elapsed:?}"
    );
    session.close().expect("close terminal");
}

#[test]
fn u10_a_late_visual_change_restarts_then_succeeds() {
    // The change lands mid-wait: the quiet interval exceeds the change delay,
    // so success must follow the change by at least one quiet interval.
    let quiet = Duration::from_millis(1000);
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Milliseconds 600; [Console]::WriteLine('LATE-VISIBLE'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 0.6; echo LATE-VISIBLE; sleep 30"
    };
    let session = ready_session("a-late", script);
    let started = Instant::now();
    session
        .wait_quiet(&visual_options(quiet))
        .expect("late change restarts, then succeeds");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(1350),
        "late change did not restart the timer: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "wait took too long: {elapsed:?}"
    );
    wait_for_text(&session, "LATE-VISIBLE");
    session.close().expect("close terminal");
}

// U10-B: timeout and cancellation finish within a bounded test deadline.
#[test]
fn u10_b_timeout_is_bounded() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); while ($true) { [Console]::WriteLine('busy'); Start-Sleep -Milliseconds 20 }"
    } else {
        "printf READY; while true; do echo busy; sleep 0.02; done"
    };
    let session = ready_session("b-timeout", script);
    let options = WaitQuietOptions::new(Duration::from_secs(1), QUIET)
        .expect("test options are valid")
        .with_mode(WaitQuietMode::Bytes);
    let started = Instant::now();
    let error = session
        .wait_quiet(&options)
        .expect_err("continuous output must time out byte quiet");
    let elapsed = started.elapsed();
    assert!(is_timeout(&error), "unexpected error: {error}");
    assert!(elapsed >= Duration::from_millis(900), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(6), "{elapsed:?}");
    session.close().expect("close terminal");
}

#[test]
fn u10_b_cancellation_is_bounded() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("b-cancel", script);
    let (flag, check) = cancel_flag();
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet_cancellable(&options, &check));
    std::thread::sleep(Duration::from_millis(300));
    flag.store(true, Ordering::Release);
    let started = Instant::now();
    let error = handle
        .join()
        .expect("wait thread joins")
        .expect_err("cancelled wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancellation was not prompt"
    );
    session.close().expect("close terminal");
}

// U10-C: cancellation kills neither the child nor a different caller.
#[test]
fn u10_c_cancel_leaves_child_and_other_caller_alone() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("c-alive", script);
    let (flag, check) = cancel_flag();
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let first = session.clone();
    let cancelled = std::thread::spawn(move || first.wait_quiet_cancellable(&options, &check));
    let second = session.clone();
    let other = std::thread::spawn(move || second.wait_quiet(&visual_options(QUIET)));
    std::thread::sleep(Duration::from_millis(300));
    flag.store(true, Ordering::Release);
    let error = cancelled
        .join()
        .expect("cancelled wait joins")
        .expect_err("first wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");
    other
        .join()
        .expect("other wait joins")
        .expect("other caller is not cancelled");

    // The child still runs: it answers new input after the cancellation.
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
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("d-after", script);
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
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("e-release", script);

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
    let options = WaitQuietOptions::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test options are valid");
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet_cancellable(&options, &check));
    std::thread::sleep(Duration::from_millis(200));
    flag.store(true, Ordering::Release);
    let error = handle
        .join()
        .expect("wait thread joins")
        .expect_err("cancelled wait must report cancellation");
    assert!(is_cancelled(&error), "unexpected error: {error}");

    // Two concurrent waits both succeed: no wait holds an exclusive resource.
    let first = session.clone();
    let one = std::thread::spawn(move || first.wait_quiet(&visual_options(QUIET)));
    let second = session.clone();
    let two = std::thread::spawn(move || second.wait_quiet(&visual_options(QUIET)));
    one.join()
        .expect("first join")
        .expect("first concurrent wait");
    two.join()
        .expect("second join")
        .expect("second concurrent wait");

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
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("f-callers", script);
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet(&visual_options(QUIET)));
    // Another caller hammers capture and metadata reads while the wait runs.
    for _ in 0..10 {
        session
            .capture_snapshot(SnapshotScope::ActiveScreen)
            .expect("concurrent capture");
        session
            .capture_snapshot(SnapshotScope::Viewport)
            .expect("concurrent viewport capture");
        std::thread::sleep(Duration::from_millis(20));
    }
    handle.join().expect("wait joins").expect("wait succeeds");
    // Re-run timed to confirm the steady-state duration while a caller reads.
    let probe = session.clone();
    let hammer = std::thread::spawn(move || {
        for _ in 0..20 {
            let _ = probe.capture_snapshot(SnapshotScope::ActiveScreen);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let started = Instant::now();
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("wait succeeds under concurrent reads");
    let elapsed = started.elapsed();
    hammer.join().expect("hammer joins");
    assert!(
        elapsed < QUIET + Duration::from_secs(2),
        "concurrent callers delayed the wait: {elapsed:?}"
    );
    session.close().expect("close terminal");
}

#[test]
fn u10_f_viewport_scroll_does_not_reset_visual_timer() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); for ($i=0; $i -lt 100; $i++) { [Console]::WriteLine(\"fill-$i\") }; Start-Sleep -Seconds 30"
    } else {
        "printf READY; for i in $(seq 1 100); do echo fill-$i; done; sleep 30"
    };
    // The READY marker scrolls out of the viewport under the fill output, so
    // settle on the last fill line instead of the READY marker.
    let session = Session::new(session_name("f-scroll"));
    if cfg!(windows) {
        session
            .run(run_options(
                "powershell.exe",
                &["-NoProfile", "-Command", script],
            ))
            .expect("run child");
    } else {
        session
            .run(run_options("sh", &["-c", script]))
            .expect("run child");
    }
    wait_for_text(&session, "fill-100");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("screen settles before the scroll test");
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet(&visual_options(QUIET)));
    std::thread::sleep(Duration::from_millis(100));
    session
        .execute(Operation::Scroll {
            target: ScrollTarget::Lines(5),
        })
        .expect("scroll viewport");
    let started = Instant::now();
    handle
        .join()
        .expect("wait joins")
        .expect("scroll must not reset timer");
    assert!(
        started.elapsed() < QUIET + Duration::from_secs(2),
        "viewport scroll delayed the wait"
    );
    session
        .execute(Operation::Scroll {
            target: ScrollTarget::Bottom,
        })
        .expect("scroll back");
    session.close().expect("close terminal");
}

// U10-G: a controlled change between sample points shows the sampling limit.
//
// Rapid change-then-revert output pairs flow through the PTY (byte-quiet
// observes every read) while 50ms visual samples only ever see the stable
// screen, so the visual wait finishes on its first quiet interval.
#[test]
fn u10_g_rapid_change_and_revert_misses_visual_samples() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Milliseconds 300; for ($i=0; $i -lt 40; $i++) { [Console]::Write(\"X`b `b\") }; Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 0.3; for i in $(seq 1 40); do printf 'X\\b \\b'; sleep 0.02; done; sleep 30"
    };
    let session = ready_session("g-transient", script);
    let started = Instant::now();
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("transients between samples must not delay visual quiet");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(900),
        "visual wait saw a transient it should have missed: {elapsed:?}"
    );
    // The screen is stable again; the transient left no trace behind.
    let snapshot = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("capture after transient stream");
    assert!(snapshot.rows() > 0, "captured an empty screen");
    session.close().expect("close terminal");
}

// U10-H: palette-only, title-only, cursor-only, and dimension changes reset
// visual quiet. Each change lands mid-wait; success must follow the change by
// at least one quiet interval (lower bound), proving the timer restarted.
#[test]
fn u10_h_title_change_restarts_visual_quiet() {
    let session = Session::new(session_name("h-title"));
    session
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open shell");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("shell settles");
    // The change lands well inside the first quiet interval (500ms).
    let quiet = Duration::from_millis(500);
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet(&visual_options(quiet)));
    std::thread::sleep(Duration::from_millis(50));
    let changed_at = Instant::now();
    let command = if cfg!(windows) {
        "[Console]::Write(\"`e]0;U10-TITLE-CHANGE`a\")"
    } else {
        "printf '\\033]0;U10-TITLE-CHANGE\\a'"
    };
    session
        .execute(Operation::Submit {
            data: Some(command.to_string()),
        })
        .expect("set title");
    handle
        .join()
        .expect("wait joins")
        .expect("title change restarts");
    let elapsed = changed_at.elapsed();
    assert!(
        elapsed >= Duration::from_millis(350),
        "title change did not restart the timer"
    );
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    session.close().expect("close terminal");
}

#[test]
fn u10_h_palette_change_restarts_visual_quiet() {
    let session = Session::new(session_name("h-palette"));
    session
        .open(OpenOptions {
            wait_ready: Some(false),
            ..OpenOptions::default()
        })
        .expect("open shell");
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("shell settles");
    // The change lands well inside the first quiet interval (500ms).
    let quiet = Duration::from_millis(500);
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet(&visual_options(quiet)));
    std::thread::sleep(Duration::from_millis(50));
    let changed_at = Instant::now();
    let command = if cfg!(windows) {
        "[Console]::Write(\"`e]4;9;rgb:ff/00/00`a\")"
    } else {
        "printf '\\033]4;9;rgb:ff/00/00\\a'"
    };
    session
        .execute(Operation::Submit {
            data: Some(command.to_string()),
        })
        .expect("set palette entry");
    handle
        .join()
        .expect("wait joins")
        .expect("palette change restarts");
    let elapsed = changed_at.elapsed();
    assert!(
        elapsed >= Duration::from_millis(350),
        "palette change did not restart the timer"
    );
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    session.close().expect("close terminal");
}

#[test]
fn u10_h_cursor_move_restarts_visual_quiet() {
    // The child moves only the cursor mid-wait; cells, title, and palette are
    // untouched, so only the cursor state can restart the timer.
    let quiet = Duration::from_millis(1000);
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Milliseconds 600; [Console]::Write(\"`e[7;25H\"); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 0.6; printf '\\033[7;25H'; sleep 30"
    };
    let session = ready_session("h-cursor", script);
    let started = Instant::now();
    session
        .wait_quiet(&visual_options(quiet))
        .expect("cursor move restarts, then succeeds");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(1350),
        "cursor move did not restart the timer: {elapsed:?}"
    );
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    session.close().expect("close terminal");
}

#[test]
fn u10_h_resize_restarts_visual_quiet() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("h-resize", script);
    // The resize lands well inside the first quiet interval (500ms).
    let quiet = Duration::from_millis(500);
    let worker = session.clone();
    let handle = std::thread::spawn(move || worker.wait_quiet(&visual_options(quiet)));
    std::thread::sleep(Duration::from_millis(50));
    let changed_at = Instant::now();
    session
        .execute(Operation::Resize {
            cols: 100,
            rows: 30,
        })
        .expect("resize terminal");
    handle.join().expect("wait joins").expect("resize restarts");
    let elapsed = changed_at.elapsed();
    assert!(
        elapsed >= Duration::from_millis(350),
        "dimension change did not restart the timer"
    );
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    session.close().expect("close terminal");
}

// U10-I: the quiet interval starts with the request after a long idle period.
#[test]
fn u10_i_quiet_starts_at_request_after_long_idle() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("i-idle", script);
    std::thread::sleep(QUIET * 4);
    let started = Instant::now();
    session
        .wait_quiet(&visual_options(QUIET))
        .expect("quiet wait succeeds after idle");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= QUIET - Duration::from_millis(50),
        "wait reused idle time from before the request: {elapsed:?}"
    );
    assert!(
        elapsed < QUIET + Duration::from_secs(2),
        "wait took too long: {elapsed:?}"
    );
    let started = Instant::now();
    let bytes = visual_options(QUIET).with_mode(WaitQuietMode::Bytes);
    session.wait_quiet(&bytes).expect("byte quiet after idle");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= QUIET - Duration::from_millis(50),
        "byte wait reused idle time from before the request: {elapsed:?}"
    );
    assert!(
        elapsed < QUIET + Duration::from_secs(2),
        "byte wait took too long: {elapsed:?}"
    );
    session.close().expect("close terminal");
}

// U10-J: equal quiet and overall deadlines return timeout.
#[test]
fn u10_j_equal_deadlines_return_timeout() {
    let script = if cfg!(windows) {
        "[Console]::Write('READY'); Start-Sleep -Seconds 30"
    } else {
        "printf READY; sleep 30"
    };
    let session = ready_session("j-equal", script);
    for quiet_ms in [400, 800] {
        let options = WaitQuietOptions::new(
            Duration::from_millis(quiet_ms),
            Duration::from_millis(quiet_ms),
        )
        .expect("equal deadlines are accepted");
        let started = Instant::now();
        let error = session
            .wait_quiet(&options)
            .expect_err("equal deadlines must time out");
        let elapsed = started.elapsed();
        assert!(is_timeout(&error), "unexpected error: {error}");
        assert!(
            elapsed + Duration::from_millis(50) >= Duration::from_millis(quiet_ms),
            "timed out before the deadline: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(quiet_ms) + Duration::from_secs(3),
            "timeout took too long: {elapsed:?}"
        );
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
