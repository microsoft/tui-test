//! Explicit snapshot write-policy tests (U01-A through U01-F, plus the U01-B3
//! temporary-path hardening follow-up).
//!
//! These tests use temporary directories and plain threads only; no PTY or
//! child process is involved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tui_test::assert::snapshot::{
    __test_only_replace_counter, __test_only_temp_file_name, compare, compare_with_policy,
    PolicyOutcome, SnapshotPolicy, SnapshotStatus,
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

// U01-D (compat): legacy `compare` with update preserves a symlink baseline
// and updates its target; only `compare_with_policy` with
// `SnapshotPolicy::Replace` swaps the link itself away. The recorded
// `compare` wrote with `std::fs::write`, which follows a baseline symlink
// (the link survives, the target gains the new bytes). The policy
// implementation writes through an atomic temporary-file-plus-rename step
// instead, so a replace installs a regular file at the baseline path and
// leaves the link target's bytes untouched. These tests lock both halves in.

#[cfg(unix)]
#[test]
fn u01_d_compare_update_preserves_symlink_baseline() {
    let base = temp_base("d-symlink-baseline");
    let dir = base.join("__snapshots__");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("external-target.txt");
    std::fs::write(&target, b"target-old\n").unwrap();
    let baseline = dir.join("snap.snap");
    std::os::unix::fs::symlink(&target, &baseline).unwrap();
    assert_eq!(std::fs::read(&baseline).unwrap(), b"target-old\n".to_vec());

    assert!(matches!(
        compare(&base, "snap", "new", true).unwrap(),
        SnapshotStatus::Updated
    ));
    assert!(
        std::fs::symlink_metadata(&baseline)
            .unwrap()
            .file_type()
            .is_symlink(),
        "a legacy update preserves the link at the baseline path"
    );
    assert_eq!(std::fs::read(&baseline).unwrap(), b"new\n".to_vec());
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"new\n".to_vec(),
        "the link target gains the new bytes"
    );
    remove_base(&base);
}

#[cfg(unix)]
#[test]
fn policy_replace_replaces_symlink_baseline_without_touching_target() {
    let base = temp_base("policy-replace-symlink");
    let dir = base.join("__snapshots__");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("external-target.txt");
    std::fs::write(&target, b"target-old\n").unwrap();
    let baseline = dir.join("snap.snap");
    std::os::unix::fs::symlink(&target, &baseline).unwrap();
    assert_eq!(std::fs::read(&baseline).unwrap(), b"target-old\n".to_vec());

    assert_eq!(
        compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Replaced
    );
    assert!(
        !std::fs::symlink_metadata(&baseline)
            .unwrap()
            .file_type()
            .is_symlink(),
        "a policy replace installs a regular file at the baseline path, not a link"
    );
    assert_eq!(std::fs::read(&baseline).unwrap(), b"new\n".to_vec());
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"target-old\n".to_vec(),
        "the former link target keeps its bytes"
    );
    remove_base(&base);
}

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
// The helpers below derive temp paths through the shared
// `__test_only_temp_file_name` seam (never by mirroring the naming) for the
// snapshot name "snap" (sanitized stem "snap"). The replacement counter
// is process-global: under serial `cargo test` every test in this file shares
// one counter in one process (and runs on concurrent threads), while nextest
// gives each test its own process starting at stamp 0. No test assumes a
// stamp value or a fresh counter. Instead, each collision test reads the
// stamp in play (`__test_only_replace_counter`) and occupies exactly that
// slot; a concurrent replacement elsewhere only shifts the counter, which
// the next cycle re-reads. Every cycle burns at most two stamps, so cycles
// cannot escalate the way a growing planted window would.

/// Temporary path that `atomic_replace` derives for snapshot name "snap".
fn replace_temp(dir: &Path, stamp: u64) -> PathBuf {
    dir.join(__test_only_temp_file_name("snap", "tmp", stamp))
}

/// Names of this process's temp files in `dir`.
///
/// Both writers tag their temps `.tmp-`/`.new-` (`atomic_replace` uses
/// `"tmp"`, `create_new` uses `"new"`), so either prefix marks an owned temp.
/// Foreign debris never uses those prefixes.
fn owned_temp_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp-") || name.starts_with(".new-"))
        .collect();
    names.sort();
    names
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
    let baseline = dir.join("snap.snap");

    // Read the stamp in play and occupy exactly that slot: the attempt then
    // takes the planted stamp and must collide instead of writing through. A
    // concurrent replacement elsewhere only shifts the counter, so a miss
    // restores the baseline by direct write (which consumes no stamp) and
    // re-reads. Stamps read across cycles strictly increase — every attempt
    // consumes exactly one — so planted slots are always fresh.
    let mut collided = false;
    for _cycle in 0..1000 {
        let stamp = __test_only_replace_counter();
        std::fs::write(replace_temp(&dir, stamp), format!("planted-{stamp}")).unwrap();
        let before = dir_state(&dir);
        match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Neither the baseline nor any planted file may change, and
                // the failed call must add and remove nothing.
                assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"old\n".to_vec());
                assert_eq!(
                    dir_state(&dir),
                    before,
                    "a failed replace must leave every preexisting path alone"
                );
                collided = true;
                break;
            }
            Err(error) => {
                panic!("a colliding temp path must fail the exclusive create, got {error:?}")
            }
            Ok(PolicyOutcome::Replaced) => {
                std::fs::write(&baseline, b"old\n").unwrap();
            }
            Ok(outcome) => panic!("a replace over a planted temp file must not report {outcome:?}"),
        }
    }
    assert!(
        collided,
        "the exclusive create never collided with a planted temp"
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

    // Same read-the-stamp cycle as the regular-file test, but the occupied
    // slot holds a symlink pointing at the baseline: the old code wrote
    // through the link (new bytes landed in the baseline) and then renamed
    // the link over the baseline. Read slots strictly increase, so every
    // planted link is fresh.
    let mut collided = false;
    for _cycle in 0..1000 {
        let stamp = __test_only_replace_counter();
        std::os::unix::fs::symlink(&baseline, replace_temp(&dir, stamp)).unwrap();
        let before = dir_state(&dir);
        match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // The baseline keeps its own bytes in its own file: no
                // redirected write, and no rename installing a link over it.
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
                collided = true;
                break;
            }
            Err(error) => {
                panic!("a symlink temp path must fail the exclusive create, got {error:?}")
            }
            Ok(PolicyOutcome::Replaced) => {
                std::fs::write(&baseline, b"old\n").unwrap();
            }
            Ok(outcome) => panic!("a replace through a symlink temp must not report {outcome:?}"),
        }
    }
    assert!(
        collided,
        "the exclusive create never collided with a planted symlink"
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

    // Failure path: the colliding temp plus foreign debris all survive, and
    // the failed call changes nothing in the directory. The stamp in play is
    // read like in the tests above; foreign debris is planted once and must
    // never be swept.
    let base = temp_base("b3-owned-cleanup");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    let baseline = dir.join("snap.snap");
    // Foreign debris the implementation must never sweep.
    std::fs::write(dir.join(".other.tmp-999-999"), b"foreign").unwrap();
    std::fs::write(dir.join(".foreign.tmp-424242"), b"foreign").unwrap();
    let mut collided = false;
    for _cycle in 0..1000 {
        let stamp = __test_only_replace_counter();
        std::fs::write(replace_temp(&dir, stamp), format!("planted-{stamp}")).unwrap();
        let before = dir_state(&dir);
        match compare_with_policy(&base, "snap", "new", SnapshotPolicy::Replace) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"old\n".to_vec());
                assert_eq!(
                    dir_state(&dir),
                    before,
                    "a failed replace removes only temps it owns (here: none)"
                );
                collided = true;
                break;
            }
            Err(error) => {
                panic!("a colliding temp path must fail the exclusive create, got {error:?}")
            }
            Ok(PolicyOutcome::Replaced) => {
                std::fs::write(&baseline, b"old\n").unwrap();
            }
            Ok(outcome) => panic!("a replace over a planted temp file must not report {outcome:?}"),
        }
    }
    assert!(
        collided,
        "the exclusive create never collided with a planted temp"
    );
    remove_base(&base);
}

// U01-B3 (4): two concurrent replacers where one temp slot is occupied.
// Exactly one wins with Replaced; the loser errors cleanly with
// AlreadyExists; the baseline holds one racer's complete content.

#[test]
fn u01_b3_concurrent_replacers_one_wins_loser_errors() {
    use std::sync::{Arc, Barrier};

    fn race_pair(base: &Path, contents: &[String; 2]) -> (u32, u32) {
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = contents
            .iter()
            .map(|content| {
                let base: PathBuf = base.to_path_buf();
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
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    already_exists += 1;
                }
                other => panic!("a racer must replace or collide cleanly, got {other:?}"),
            }
        }
        (replaced, already_exists)
    }

    let base = temp_base("b3-replace-race");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    let baseline = dir.join("snap.snap");
    let contents = [
        format!("racer a {}", "A".repeat(4096)),
        format!("racer b {}", "B".repeat(4096)),
    ];

    // Read the stamp in play and occupy exactly that slot with a sentinel,
    // then race the pair: the racer taking the sentinel stamp errors cleanly
    // while the other takes the next free stamp and wins. A concurrent
    // replacement elsewhere only shifts the counter, so a round where both
    // racers miss the sentinel restores the seed baseline by direct write
    // (which consumes no stamp) and re-reads. Only the planted slot is ever
    // occupied, both racers take distinct stamps past every previously read
    // one, and every test sharing this process burns only finitely many
    // stamps — so the straddling round always arrives, burning two stamps
    // per cycle at most.
    let mut converged = false;
    for _cycle in 0..1000 {
        let stamp = __test_only_replace_counter();
        std::fs::write(replace_temp(&dir, stamp), format!("planted-{stamp}")).unwrap();
        let before = dir_state(&dir);
        match race_pair(&base, &contents) {
            (1, 1) => {
                converged = true;
                // Exactly one racer won; the loser errored cleanly.
                let after = dir_state(&dir);
                assert_eq!(
                    after.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                    before.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                    "racing replacers must add and remove nothing besides the baseline swap"
                );
                for ((name, old_state), (_, new_state)) in before.iter().zip(after.iter()) {
                    if name == "snap.snap" {
                        assert!(
                            contents
                                .iter()
                                .any(|content| *new_state == format!("{content}\n").into_bytes()),
                            "the baseline is one racer's complete content: {new_state:?}"
                        );
                    } else {
                        assert_eq!(
                            new_state, old_state,
                            "planted path {name} must survive the race untouched"
                        );
                    }
                }
                break;
            }
            (2, 0) => {
                // Both racers landed past the sentinel: another test moved
                // the counter mid-cycle. Restore the seed and re-read.
                std::fs::write(&baseline, b"old\n").unwrap();
            }
            (replaced, already_exists) => {
                panic!("a race must end 1-1 or 2-0, got {replaced}-{already_exists}")
            }
        }
    }
    assert!(
        converged,
        "the race never produced exactly one winner and one clean loser"
    );
    let stored = baseline_bytes(&base, "snap").unwrap();
    assert!(
        contents
            .iter()
            .any(|content| stored == format!("{content}\n").into_bytes()),
        "the race must converge on one racer's complete content: {stored:?}"
    );
    remove_base(&base);
}

// U01-B3 (5): mid-write fault injection. The exclusive create succeeds, so
// the failure lands AFTER partial body bytes travel through the owned
// handle — unlike the collision tests above, which fail before writing a
// byte and therefore cannot detect removal of partial-write cleanup. The old
// baseline must survive, and only the owned temp may be cleaned: no residue,
// and every preexisting path untouched.
//
// The fault hook (`TUI_TEST_SNAPSHOT_WRITE_FAULT=<dir>:<n>`, debug builds
// only) is scoped to this test's snapshot directory, so tests sharing this
// process are unaffected.

/// Sets `TUI_TEST_SNAPSHOT_WRITE_FAULT` for the enclosed scope and removes
/// it on drop, so a panicking assertion cannot leak the fault into another
/// test sharing this process.
struct WriteFaultGuard;

impl WriteFaultGuard {
    fn arm(dir: &Path, after_bytes: usize) -> Self {
        std::env::set_var(
            "TUI_TEST_SNAPSHOT_WRITE_FAULT",
            format!("{}:{after_bytes}", dir.display()),
        );
        WriteFaultGuard
    }
}

impl Drop for WriteFaultGuard {
    fn drop(&mut self) {
        std::env::remove_var("TUI_TEST_SNAPSHOT_WRITE_FAULT");
    }
}

#[test]
fn u01_b3_partial_write_failure_cleans_only_owned_temp() {
    let base = temp_base("b3-mid-write-fault");
    compare_with_policy(&base, "snap", "old", SnapshotPolicy::Replace).unwrap();
    let dir = base.join("__snapshots__");
    // Only foreign-pattern debris here: the faulted call's own exclusive
    // create must succeed (its stamp is fresh and unique to this directory),
    // so no real-pattern slot may be occupied.
    std::fs::write(dir.join(".other.tmp-999-999"), b"foreign").unwrap();
    std::fs::write(dir.join("unrelated.txt"), b"unrelated").unwrap();
    let before = dir_state(&dir);

    // Fail after 2 of the 24 body bytes travel through the owned handle.
    let _guard = WriteFaultGuard::arm(&dir, 2);
    let error = match compare_with_policy(
        &base,
        "snap",
        "new-fault-probe-content",
        SnapshotPolicy::Replace,
    ) {
        Err(error) => error,
        Ok(outcome) => panic!("the injected mid-write fault must fail, got {outcome:?}"),
    };
    drop(_guard);
    assert!(
        error.to_string().contains("injected partial-write fault"),
        "the failure must be the injected mid-write fault, not another error: {error}"
    );
    // The previous baseline survives with its exact bytes.
    assert_eq!(baseline_bytes(&base, "snap").unwrap(), b"old\n".to_vec());
    // Only the owned temp is cleaned: no residue under any temp name, and
    // every preexisting path — including the foreign debris — is untouched.
    assert_eq!(
        dir_state(&dir),
        before,
        "a failed replace removes only its owned temp and changes nothing else"
    );
    assert!(
        owned_temp_names(&dir).is_empty(),
        "no temporary file may survive the failed replace"
    );

    // Control: without the fault the same replacement succeeds and leaves no
    // debris behind, proving the failure above came from the injected fault.
    assert_eq!(
        compare_with_policy(
            &base,
            "snap",
            "new-fault-probe-content",
            SnapshotPolicy::Replace
        )
        .unwrap(),
        PolicyOutcome::Replaced
    );
    assert_eq!(
        baseline_bytes(&base, "snap").unwrap(),
        b"new-fault-probe-content\n".to_vec()
    );
    assert_eq!(
        dir_names(&dir).unwrap(),
        vec![
            ".other.tmp-999-999".to_string(),
            "snap.snap".to_string(),
            "unrelated.txt".to_string()
        ],
        "a successful replace leaves no temp debris"
    );
    remove_base(&base);
}

// U01-B3 (6): concurrent readers never observe a partial baseline. Every
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

    assert!(
        reads.load(Ordering::SeqCst) >= 800,
        "readers must overlap the writers"
    );
    assert_eq!(
        read_errors.load(Ordering::SeqCst),
        0,
        "reads must never fail mid-replacement"
    );
    assert_eq!(
        bad.load(Ordering::SeqCst),
        0,
        "readers must never observe a partial baseline"
    );
    let final_raw = std::fs::read(&path).unwrap();
    assert!(
        complete(&final_raw, TOTAL, &seed_raw),
        "the final baseline is one complete generation"
    );
    remove_base(&base);
}

// U01-B3 (7): bounded temporary names near the filename limit.
//
// A 250-byte snapshot name makes a valid 255-byte `.snap` file, but a
// temporary name embedding the whole stem (`.{stem}.tmp-{pid}-{stamp}`) is
// 263 bytes and fails with ENAMETOOLONG. Both writers must use short bounded
// temp names, so creation and replacement succeed and leave no debris.

#[test]
fn u01_b3_long_snapshot_name_uses_bounded_temps() {
    let base = temp_base("b3-long-name");
    let name: String = "n".repeat(250);
    assert_eq!(
        format!("{name}.snap").len(),
        255,
        "the baseline itself sits exactly at the filename limit"
    );
    for kind in ["new", "tmp"] {
        let temp = __test_only_temp_file_name(&name, kind, 0);
        assert!(
            temp.len() <= 64,
            "temp names stay bounded for any stem, got {temp:?}"
        );
    }
    assert_eq!(
        compare_with_policy(&base, &name, "one", SnapshotPolicy::CreateMissing).unwrap(),
        PolicyOutcome::Created
    );
    assert_eq!(baseline_bytes(&base, &name).unwrap(), b"one\n".to_vec());
    assert_eq!(
        compare_with_policy(&base, &name, "two", SnapshotPolicy::Replace).unwrap(),
        PolicyOutcome::Replaced
    );
    assert_eq!(baseline_bytes(&base, &name).unwrap(), b"two\n".to_vec());
    assert_eq!(
        dir_names(&base.join("__snapshots__")).unwrap(),
        vec![format!("{name}.snap")],
        "bounded temps leave no debris behind"
    );
    remove_base(&base);
}
