//! Explicit snapshot write-policy tests (U01-A through U01-F).
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
