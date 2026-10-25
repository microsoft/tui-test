//! Explicit snapshot write-policy tests (U01-A through U01-F, plus the U01-B3
//! temporary-path hardening follow-up).
//!
//! These tests use temporary directories and plain threads only; no PTY or
//! child process is involved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tui_test::assert::snapshot::{
    compare, compare_with_policy, PolicyOutcome, SnapshotPolicy, SnapshotStatus,
};

/// Distinct temporary roots across tests in this process.
static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Make an empty directory under the system temporary directory.
fn temp_base(test: &str) -> PathBuf {
    let n = TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "tui-test-snapshot-policy-{test}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Remove a test directory with its baselines.
fn remove_base(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Read the stored bytes of the baseline for `name`, if any.
fn baseline_bytes(base: &Path, name: &str) -> Option<Vec<u8>> {
    let path = base.join("__snapshots__").join(format!("{name}.snap"));
    std::fs::read(path).ok()
}

/// Sorted file names directly inside `base` (`None` when `base` is gone).
fn dir_names(base: &Path) -> Option<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(base)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Some(names)
}

fn different_outcome(outcome: &PolicyOutcome) -> Option<(&str, &str)> {
    match outcome {
        PolicyOutcome::Different { expected, actual } => Some((expected, actual)),
        _ => None,
    }
}

// U01-A: missing, equal, and different content under each policy.

#[test]
fn u01_a_check_only_reports_without_writing() {
    let base = temp_base("a-check-only");
    // Missing baseline: report only, and no directory appears.
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CheckOnly).unwrap(),
        PolicyOutcome::Missing
    );
    assert!(!base.join("__snapshots__").exists());

    // Equal and different content against a stored baseline.
    compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap();
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CheckOnly).unwrap(),
        PolicyOutcome::Equal
    );
    let outcome = compare_with_policy(&base, "snap", "two", SnapshotPolicy::CheckOnly).unwrap();
    let (expected, actual) = different_outcome(&outcome).unwrap();
    assert_eq!(expected, "one");
    assert_eq!(actual, "two");
    // The losing comparison must not touch the stored bytes.
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    remove_base(&base);
}

#[test]
fn u01_a_create_missing_writes_once_then_reports() {
    let base = temp_base("a-create-missing");
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CreateMissing).unwrap(),
        PolicyOutcome::Created
    );
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CreateMissing).unwrap(),
        PolicyOutcome::Equal
    );
    let outcome = compare_with_policy(&base, "snap", "two", SnapshotPolicy::CreateMissing).unwrap();
    let (expected, actual) = different_outcome(&outcome).unwrap();
    assert_eq!(expected, "one");
    assert_eq!(actual, "two");
    // Different content must not replace the baseline under CreateMissing.
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    remove_base(&base);
}

#[test]
fn u01_a_replace_writes_and_replaces() {
    let base = temp_base("a-replace");
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Created
    );
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Equal
    );
    assert_eq!(
        compare_with_policy(&base, "snap", "two", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Replaced
    );
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"two\n".to_vec());
    remove_base(&base);
}

#[test]
fn u01_a_equal_content_never_writes() {
    // A stored baseline plus surrounding whitespace still compares equal,
    // and the stored bytes keep their recorded trimmed form.
    for policy in [
        SnapshotPolicy::CheckOnly,
        SnapshotPolicy::CreateMissing,
        SnapshotPolicy::Replace,
    ] {
        let base = temp_base("a-equal-trim");
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap();
        assert_eq!(
            compare_with_policy(&base, "snap", "  one\n", policy).unwrap(),
            PolicyOutcome::Equal,
            "policy {policy:?} must return Equal without a write"
        );
        assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
        remove_base(&base);
    }
}

// U01-F: success writes give the specified Created or Replaced outcome.

#[test]
fn u01_f_created_and_replaced_outcomes() {
    let base = temp_base("f-outcomes");
    assert_eq!(
        compare_with_policy(&base, "created", "one", SnapshotPolicy::CreateMissing).unwrap(),
        PolicyOutcome::Created
    );
    assert_eq!(baseline_bytes(&base, "created").unwrap(), b"one\n".to_vec());
    assert_eq!(
        compare_with_policy(&base, "replaced", "one", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Created
    );
    assert_eq!(
        compare_with_policy(&base, "replaced", "two", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Replaced
    );
    assert_eq!(
        baseline_bytes(&base, "replaced").unwrap(),
        b"two\n".to_vec()
    );
    remove_base(&base);
}

#[test]
fn u01_default_policy_is_check_only() {
    assert_eq!(SnapshotPolicy::default(), SnapshotPolicy::CheckOnly);
    let base = temp_base("default");
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::default()).unwrap(),
        PolicyOutcome::Missing
    );
    assert!(!base.join("__snapshots__").exists());
    remove_base(&base);
}

// U01-B: CheckOnly leaves directory contents and baseline bytes unchanged.

#[test]
fn u01_b_check_only_leaves_everything_untouched() {
    let base = temp_base("b-check-only");
    compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap();
    let before_names = dir_names(&base).unwrap();
    let before_bytes = baseline_bytes(&base, "snap").unwrap();

    assert_eq!(
        compare_with_policy(&base, "missing", "x", SnapshotPolicy::CheckOnly).unwrap(),
        PolicyOutcome::Missing
    );
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CheckOnly).unwrap(),
        PolicyOutcome::Equal
    );
    let outcome = compare_with_policy(&base, "snap", "two", SnapshotPolicy::CheckOnly).unwrap();
    assert!(different_outcome(&outcome).is_some());

    assert_eq!(dir_names(&base).unwrap(), before_names);
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), before_bytes);
    remove_base(&base);
}

#[test]
fn u01_b_check_only_makes_no_directory() {
    let base = temp_base("b-no-dir");
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(
        compare_with_policy(&base, "snap", "one", SnapshotPolicy::CheckOnly).unwrap(),
        PolicyOutcome::Missing
    );
    // Even the base directory itself must not appear.
    assert!(!base.exists());
}

// U01-C: an explicit write failure keeps the previous baseline.

#[cfg(unix)]
#[test]
fn u01_c_failed_replace_keeps_previous_baseline() {
    use std::os::unix::fs::PermissionsExt;

    let base = temp_base("c-read-only");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let before = baseline_bytes(&base, "snap").unwrap();

    let dir = base.join("__snapshots__");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace);
    // Restore access before asserting so cleanup always runs.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        result.is_err(),
        "write into a read-only directory must fail"
    );
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), before);
    remove_base(&base);
}

#[test]
fn u01_c_write_failure_reports_an_error_and_writes_nothing() {
    // A regular file where the snapshot directory belongs makes directory
    // setup fail on every platform, which models an explicit write failure.
    let base = temp_base("c-blocked");
    std::fs::write(base.join("__snapshots__"), b"not a directory").unwrap();
    let result = compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace);
    assert!(result.is_err());
    assert!(baseline_bytes(&base, "snap").is_none());
    remove_base(&base);
}

// U01-D: callers from the recorded commit keep their previous behavior.

#[test]
fn u01_d_compare_without_update_keeps_behavior() {
    let base = temp_base("d-no-update");
    // Missing baseline is written and reported as Written.
    assert!(matches!(
        compare(&base, "snap", "one", false).unwrap(),
        SnapshotStatus::Written
    ));
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    // Equal content passes.
    assert!(matches!(
        compare(&base, "snap", "one", false).unwrap(),
        SnapshotStatus::Passed
    ));
    // Different content fails with both sides and keeps the baseline.
    match compare(&base, "snap", "two", false).unwrap() {
        SnapshotStatus::Failed { expected, actual } => {
            assert_eq!(expected, "one");
            assert_eq!(actual, "two");
        }
        _ => panic!("expected Failed for different content without update"),
    }
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    remove_base(&base);
}

#[test]
fn u01_d_compare_with_update_keeps_behavior() {
    let base = temp_base("d-update");
    assert!(matches!(
        compare(&base, "snap", "one", true).unwrap(),
        SnapshotStatus::Written
    ));
    assert!(matches!(
        compare(&base, "snap", "one", true).unwrap(),
        SnapshotStatus::Passed
    ));
    assert!(matches!(
        compare(&base, "snap", "two", true).unwrap(),
        SnapshotStatus::Updated
    ));
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"two\n".to_vec());
    remove_base(&base);
}

// U01-E: concurrent CreateMissing racers cannot replace each other.

#[test]
fn u01_e_concurrent_creators_agree_on_one_baseline() {
    use std::sync::{Arc, Barrier};

    let base = temp_base("e-race-same");
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let base = base.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                compare_with_policy(&base, "snap", "one", SnapshotPolicy::CreateMissing)
            })
        })
        .collect();
    let mut created = 0;
    let mut equal = 0;
    for handle in handles {
        match handle.join().unwrap().unwrap() {
            PolicyOutcome::Created => created += 1,
            PolicyOutcome::Equal => equal += 1,
            other => panic!("racer must return Created or Equal, got {other:?}"),
        }
    }
    assert_eq!(created, 1, "exactly one racer creates the baseline");
    assert_eq!(equal, 7, "every other racer observes the winner");
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"one\n".to_vec());
    remove_base(&base);
}

#[test]
fn u01_e_concurrent_creators_never_replace() {
    use std::sync::{Arc, Barrier};

    let base = temp_base("e-race-mixed");
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let base = base.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                compare_with_policy(
                    &base,
                    "snap",
                    &format!("racer {i}"),
                    SnapshotPolicy::CreateMissing,
                )
            })
        })
        .collect();
    let mut created = 0;
    for handle in handles {
        match handle.join().unwrap().unwrap() {
            PolicyOutcome::Created => created += 1,
            PolicyOutcome::Equal | PolicyOutcome::Different { .. } => {}
            other => panic!("racer must never replace, got {other:?}"),
        }
    }
    assert_eq!(created, 1, "exactly one racer creates the baseline");
    let stored = baseline_bytes(&base, "snap").unwrap();
    let stored = String::from_utf8(stored).unwrap();
    assert!(
        (0..8).any(|i| stored == format!("racer {i}\n")),
        "the stored baseline is one racer's content, untouched afterwards: {stored:?}"
    );
    remove_base(&base);
}

// U01-B3: temporary-path hardening for atomic replacement.
//
// `atomic_replace` must create its temporary file with `create_new` and write
// through the exclusively owned handle: a preexisting temp path (a stale
// file, a planted symlink, or a concurrent writer's temp) is never written
// through, clobbered, or removed, and only an owned temp is cleaned up on
// failure.
//
// The helpers below mirror the temp naming (`.{stem}.tmp-{pid}-{stamp}`) for
// the snapshot name "snap" (sanitized stem "snap"). Stamps start at 0 in a
// fresh test process and each replacement consumes exactly one stamp, so a
// seeding write consumes stamp 0 and the next replacement takes stamp 1. That
// counting holds under per-test process isolation, which is how nextest runs
// these tests; tests that only need *some* slot to collide plant a whole
// range instead of predicting one.

/// Temporary path that `atomic_replace` derives for snapshot name "snap".
fn replace_temp(dir: &Path, stamp: u64) -> PathBuf {
    dir.join(format!(".snap.tmp-{}-{stamp}", std::process::id()))
}

/// Sorted `(name, state)` snapshot of `dir` for before/after comparisons.
/// Symlinks are recorded by target and never followed; regular files by bytes.
fn dir_state(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let state = match std::fs::read_link(entry.path()) {
                Ok(target) => format!("link:{}", target.display()).into_bytes(),
                Err(_) => std::fs::read(entry.path()).unwrap(),
            };
            (name, state)
        })
        .collect();
    entries.sort();
    entries
}

// U01-B3 (1): a regular file already sitting at the temp path must not be
// clobbered, and the API must error without touching the real baseline.

#[test]
fn u01_b3_regular_file_at_temp_path_is_never_clobbered() {
    let base = temp_base("b3-temp-collision");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");

    // Occupy every plausible next temp slot so the exact counter value does
    // not matter; the replacement must collide instead of writing through.
    for stamp in 0..=64 {
        std::fs::write(replace_temp(&dir, stamp), format!("planted-{stamp}")).unwrap();
    }
    let before = dir_state(&dir);

    match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
        Err(error) => assert_eq!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists,
            "a colliding temp path must fail the exclusive create"
        ),
        Ok(outcome) => panic!("replace over a planted temp file must fail, got {outcome:?}"),
    }
    // Neither the baseline nor any planted file may change, and the failed
    // call must add and remove nothing.
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"old\n".to_vec());
    assert_eq!(
        dir_state(&dir),
        before,
        "a failed replace must leave every preexisting path alone"
    );
    remove_base(&base);
}

// U01-B3 (2): a symlink at the temp path pointing at the baseline is the
// verdict's reproduced corruption. The old code wrote through the link (new
// bytes landed in the baseline) and then renamed the link over the baseline,
// returning Ok(Replaced) while leaving a self-referential baseline behind
// (ELOOP on read). The fixed code must error with the baseline untouched.

#[cfg(unix)]
#[test]
fn u01_b3_symlink_at_temp_path_never_redirects_into_baseline() {
    let base = temp_base("b3-temp-symlink");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    let baseline = dir.join("snap.snap");

    for stamp in 0..=64 {
        std::os::unix::fs::symlink(&baseline, replace_temp(&dir, stamp)).unwrap();
    }
    let before = dir_state(&dir);

    match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
        Err(error) => assert_eq!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists,
            "a symlink temp path must fail the exclusive create, not redirect bytes"
        ),
        Ok(outcome) => panic!("replace through a symlink temp must fail, got {outcome:?}"),
    }
    // The baseline keeps its own bytes in its own file: no redirected write,
    // and no rename installing a link over it.
    assert_eq!(std::fs::read(&baseline).unwrap(), b"old\n".to_vec());
    assert!(
        !std::fs::symlink_metadata(&baseline)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the baseline must stay a regular file, never a link to itself"
    );
    assert_eq!(
        dir_state(&dir),
        before,
        "a failed replace must leave every preexisting path alone"
    );
    remove_base(&base);
}

// U01-B3 (3): partial-write failure cleanup removes only owned temps. The
// success path must leave no temp debris, and a failed replacement must
// preserve the baseline and every preexisting file while adding nothing.

#[test]
fn u01_b3_failed_replace_removes_only_owned_temps() {
    // Success path: a completed replacement leaves no temp file behind.
    let base = temp_base("b3-no-debris");
    compare_with_policy(&base, "snap", "one", SnapshotPolicy::Replace).unwrap();
    compare_with_policy(&base, "snap", "two", SnapshotPolicy::Replace).unwrap();
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"two\n".to_vec());
    assert_eq!(
        dir_names(&base.join("__snapshots__")).unwrap(),
        vec!["snap.snap".to_string()],
        "a successful replace must leave no temp files"
    );
    remove_base(&base);

    // Failure path: colliding temps plus foreign debris all survive, and the
    // failed call changes nothing in the directory.
    let base = temp_base("b3-owned-cleanup");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    for stamp in 0..=32 {
        std::fs::write(replace_temp(&dir, stamp), format!("stale-{stamp}")).unwrap();
    }
    // Foreign debris the implementation must never sweep.
    std::fs::write(dir.join(".other.tmp-999-999"), b"foreign").unwrap();
    std::fs::write(dir.join(".snap.tmp-424242-424242"), b"foreign").unwrap();
    let before = dir_state(&dir);

    match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
        Err(error) => assert_eq!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists,
            "a colliding temp path must fail the exclusive create"
        ),
        Ok(outcome) => panic!("replace over a planted temp file must fail, got {outcome:?}"),
    }
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"old\n".to_vec());
    assert_eq!(
        dir_state(&dir),
        before,
        "a failed replace removes only temps it owns (here: none)"
    );
    remove_base(&base);
}

// U01-B3 (4): two concurrent replacers where one temp slot is occupied.
// Exactly one wins with Replaced; the loser errors cleanly with
// AlreadyExists; the baseline holds one racer's complete content.

#[test]
fn u01_b3_concurrent_replacers_one_wins_loser_errors() {
    use std::sync::{Arc, Barrier};

    let base = temp_base("b3-replace-race");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    // The seeding write consumed stamp 0 in this process, so the next
    // replacer takes stamp 1: exactly one of the two racers collides with
    // this sentinel. (Relies on per-test process isolation, as nextest runs
    // each test in its own process.)
    let sentinel = replace_temp(&dir, 1);
    std::fs::write(&sentinel, b"sentinel").unwrap();
    let sentinel_name = sentinel.file_name().unwrap().to_string_lossy().into_owned();

    let barrier = Arc::new(Barrier::new(2));
    let contents = [
        format!("racer a {}", "A".repeat(4096)),
        format!("racer b {}", "B".repeat(4096)),
    ];
    let handles: Vec<_> = contents
        .iter()
        .map(|content| {
            let base = base.clone();
            let barrier = barrier.clone();
            let content = content.clone();
            std::thread::spawn(move || {
                barrier.wait();
                compare_with_policy(&base, "snap", &content, SnapshotPolicy::Replace)
            })
        })
        .collect();

    let mut replaced = 0;
    let mut already_exists = 0;
    for handle in handles {
        match handle.join().unwrap() {
            Ok(PolicyOutcome::Replaced) => replaced += 1,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => already_exists += 1,
            other => panic!("a racer must replace or collide cleanly, got {other:?}"),
        }
    }
    assert_eq!(
        (replaced, already_exists),
        (1, 1),
        "exactly one racer wins, the loser errors cleanly"
    );
    let stored = baseline_bytes(&base, "snap").unwrap();
    assert!(
        contents.iter().any(|content| stored == format!("{content}\n").into_bytes()),
        "the baseline is one racer's complete content: {stored:?}"
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"sentinel".to_vec());
    // The winner's temp renamed away and the loser never created one: only
    // the baseline and the sentinel remain.
    assert_eq!(
        dir_names(&dir).unwrap(),
        vec![sentinel_name, "snap.snap".to_string()],
        "racing replacers must leave no temp debris"
    );
    remove_base(&base);
}

// U01-B3 (5): concurrent readers never observe a partial baseline. Every
// read across a window of racing replacements must be one complete
// generation (the seed or one fully written payload), never a mix.

#[test]
fn u01_b3_concurrent_readers_never_observe_partial_baseline() {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::{Arc, Barrier};

    const WRITERS: usize = 2;
    const ITERS: usize = 60;
    const PAD: usize = 65536;
    const TOTAL: usize = WRITERS * ITERS;

    fn payload(idx: usize) -> String {
        format!("payload-{:06}-{}", idx, "P".repeat(PAD))
    }

    /// Accept exactly the seed bytes or one fully written payload: a torn
    /// read (short, long, or mixed bytes) fails every check below.
    fn complete(raw: &[u8], total: usize, seed: &[u8]) -> bool {
        if raw == seed {
            return true;
        }
        if raw.last() != Some(&b'\n') {
            return false;
        }
        let body = &raw[..raw.len() - 1];
        if body.len() != 8 + 6 + 1 + PAD || !body.starts_with(b"payload-") {
            return false;
        }
        let idx: usize = match std::str::from_utf8(&body[8..14])
            .ok()
            .and_then(|text| text.parse().ok())
        {
            Some(idx) => idx,
            None => return false,
        };
        if idx >= total || body[14] != b'-' {
            return false;
        }
        body[15..].iter().all(|&byte| byte == b'P')
    }

    let base = temp_base("b3-read-race");
    let seed_raw = format!("seed-{}\n", "S".repeat(PAD)).into_bytes();
    let path = base.join("__snapshots__").join("snap.snap");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &seed_raw).unwrap();

    let finished = Arc::new(AtomicBool::new(false));
    let bad = Arc::new(AtomicU64::new(0));
    let read_errors = Arc::new(AtomicU64::new(0));
    let reads = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(Barrier::new(WRITERS + 4));

    let writers: Vec<_> = (0..WRITERS)
        .map(|writer| {
            let base = base.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for i in 0..ITERS {
                    let content = payload(writer * ITERS + i);
                    compare_with_policy(&base, "snap", &content, SnapshotPolicy::Replace).unwrap();
                }
            })
        })
        .collect();
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let seed = seed_raw.clone();
            let barrier = barrier.clone();
            let finished = finished.clone();
            let bad = bad.clone();
            let read_errors = read_errors.clone();
            let reads = reads.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut iters = 0u64;
                loop {
                    match std::fs::read(&path) {
                        Ok(raw) => {
                            reads.fetch_add(1, Ordering::SeqCst);
                            if !complete(&raw, TOTAL, &seed) {
                                bad.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        Err(_) => {
                            read_errors.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    iters += 1;
                    // Keep reading until the writers are done so the reads
                    // genuinely overlap the replacement window.
                    if finished.load(Ordering::SeqCst) && iters >= 200 {
                        break;
                    }
                    if iters >= 1_000_000 {
                        break;
                    }
                }
            })
        })
        .collect();

    for writer in writers {
        writer.join().unwrap();
    }
    finished.store(true, Ordering::SeqCst);
    for reader in readers {
        reader.join().unwrap();
    }

    assert!(reads.load(Ordering::SeqCst) >= 800, "readers must overlap the writers");
    assert_eq!(read_errors.load(Ordering::SeqCst), 0, "reads must never fail mid-replacement");
    assert_eq!(bad.load(Ordering::SeqCst), 0, "readers must never observe a partial baseline");
    let final_raw = std::fs::read(&path).unwrap();
    assert!(
        complete(&final_raw, TOTAL, &seed_raw),
        "the final baseline is one complete generation"
    );
    remove_base(&base);
}
