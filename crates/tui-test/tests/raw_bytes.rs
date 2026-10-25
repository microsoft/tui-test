//! U09 raw byte input tests (U09-A through U09-C).
//!
//! The test binary re-executes itself as a fixture child (see
//! `TUI_TEST_RAW_BYTES_FIXTURE`), so no shell or shell startup files are
//! involved. Each fixture puts its PTY slave into raw mode, signals
//! readiness through a file, reads an exact byte count from stdin, and
//! writes those bytes verbatim to an output file for comparison.

#![cfg(unix)]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, ErrorKind, OpenOptions, Operation, RunOptions,
    Session, Timeouts,
};

const FIXTURE: &str = "TUI_TEST_RAW_BYTES_FIXTURE";
const FIXTURE_OUT: &str = "TUI_TEST_RAW_BYTES_OUT";
const FIXTURE_READY: &str = "TUI_TEST_RAW_BYTES_READY";
const FIXTURE_LEN: &str = "TUI_TEST_RAW_BYTES_LEN";

/// Fixture entry point, run inside the test binary as the PTY child.
#[test]
fn child_fixture() {
    let Ok(mode) = std::env::var(FIXTURE) else {
        return;
    };
    assert_eq!(mode, "byte-echo");
    // Put the slave side into raw mode so the line discipline passes every
    // byte through unchanged (no CR translation, no signal characters, no
    // software flow control, no echo).
    set_slave_raw_mode();
    let out = std::env::var(FIXTURE_OUT).expect("fixture output path");
    let ready = std::env::var(FIXTURE_READY).expect("fixture ready path");
    let len: usize = std::env::var(FIXTURE_LEN)
        .expect("fixture length")
        .parse()
        .expect("fixture length parses");
    // Signal readiness only after raw mode is active, so the parent cannot
    // send bytes while the slave is still cooked.
    std::fs::write(&ready, b"ready").expect("signal readiness");
    let mut bytes = vec![0u8; len];
    std::io::stdin()
        .lock()
        .read_exact(&mut bytes)
        .expect("read stdin bytes");
    std::fs::write(&out, &bytes).expect("write output bytes");
}

#[cfg(unix)]
fn set_slave_raw_mode() {
    // SAFETY: `tcgetattr` fills `termios` on success, `cfmakeraw` only flips
    // flag bits in that struct, and `tcsetattr` reads it back. The asserts
    // below abort the fixture child on failure, which surfaces in the parent
    // as a bounded readiness timeout rather than a hang.
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(libc::STDIN_FILENO, &mut termios), 0);
        libc::cfmakeraw(&mut termios);
        assert_eq!(
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios),
            0
        );
    }
}

fn test_directory(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("raw-bytes-tests")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn run_options(out: &Path, ready: &Path, len: usize) -> RunOptions {
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
        env: vec![
            (FIXTURE.to_string(), "byte-echo".to_string()),
            (FIXTURE_OUT.to_string(), out.to_string_lossy().into_owned()),
            (
                FIXTURE_READY.to_string(),
                ready.to_string_lossy().into_owned(),
            ),
            (FIXTURE_LEN.to_string(), len.to_string()),
        ],
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

fn wait_for_len(path: &Path, len: usize, what: &str, timeout: Duration) -> Vec<u8> {
    let started = Instant::now();
    loop {
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() == len => return bytes,
            _ => {}
        }
        assert!(
            started.elapsed() < timeout,
            "{what} did not arrive within {timeout:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_exit(session: &Session, what: &str) {
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(10_000),
        })
        .unwrap_or_else(|error| panic!("{what} did not exit: {error}"));
}

// U09-A: the child receives NUL bytes, escape bytes, and invalid UTF-8
// (a lone continuation byte and a truncated multi-byte sequence) exactly.
#[test]
fn u09_a_raw_bytes_keep_nul_escape_and_invalid_utf8() {
    const PAYLOAD: &[u8] = b"AB\x00\x1b[31m\x80CD\xe2EF\xf0\x9f\x98\x80\x00";
    let directory = test_directory("u09-a");
    let out = directory.join("out.bin");
    let ready = directory.join("ready");
    let session = Session::new("raw-bytes-u09-a");
    session
        .run(run_options(&out, &ready, PAYLOAD.len()))
        .unwrap();
    wait_for_len(
        &ready,
        b"ready".len(),
        "fixture readiness",
        Duration::from_secs(10),
    );
    session.write_bytes(PAYLOAD).unwrap();
    let received = wait_for_len(&out, PAYLOAD.len(), "raw bytes", Duration::from_secs(10));
    // The received bytes are not valid UTF-8, so a lossless byte path was
    // required to deliver them.
    assert!(std::str::from_utf8(&received).is_err());
    assert_eq!(received, PAYLOAD);
    session.close().unwrap();
}

// U09-B: a payload larger than the PTY buffer arrives complete and in order,
// which exercises the short-write retry loop end to end.
#[test]
fn u09_b_large_payload_arrives_complete_and_in_order() {
    const LEN: usize = 256 * 1024;
    let payload: Vec<u8> = (0..LEN).map(|index| (index % 256) as u8).collect();
    let directory = test_directory("u09-b");
    let out = directory.join("out.bin");
    let ready = directory.join("ready");
    let session = Session::new("raw-bytes-u09-b");
    session.run(run_options(&out, &ready, LEN)).unwrap();
    wait_for_len(
        &ready,
        b"ready".len(),
        "fixture readiness",
        Duration::from_secs(10),
    );
    session.write_bytes(&payload).unwrap();
    let received = wait_for_len(&out, LEN, "large payload", Duration::from_secs(30));
    assert_eq!(received, payload);
    session.close().unwrap();
}

// U09-C: writes to a closed session return a typed error without panicking.
#[test]
fn u09_c_write_after_close_returns_typed_error() {
    const PAYLOAD: &[u8] = b"ping";
    let directory = test_directory("u09-c");
    let out = directory.join("out.bin");
    let ready = directory.join("ready");
    let session = Session::new("raw-bytes-u09-c");
    session
        .run(run_options(&out, &ready, PAYLOAD.len()))
        .unwrap();
    wait_for_len(
        &ready,
        b"ready".len(),
        "fixture readiness",
        Duration::from_secs(10),
    );
    session.write_bytes(PAYLOAD).unwrap();
    let received = wait_for_len(&out, PAYLOAD.len(), "raw bytes", Duration::from_secs(10));
    assert_eq!(received, PAYLOAD);
    // The fixture exits after writing the file, so the session is still open
    // but the child is gone.
    wait_exit(&session, "fixture");
    let error = session.write_bytes(b"after exit").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Internal);
    assert!(!error.message.is_empty());
    session.close().unwrap();
    let error = session.write_bytes(b"after close").unwrap_err();
    assert_eq!(error.kind, ErrorKind::NoSession);
    assert!(!error.message.is_empty());
}
