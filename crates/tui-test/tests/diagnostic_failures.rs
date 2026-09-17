use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, FailureReason, OpenOptions, Operation, RunOptions,
    Session,
};

fn run_options(program: &str, args: &[&str]) -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        backend: defaults.backend,
        program: program.into(),
        args: args.iter().map(|arg| (*arg).into()).collect(),
        profile: defaults.profile,
        cols: 80,
        rows: 30,
        cwd: None,
        env: Vec::new(),
        wait_ready: Some(false),
        restart: false,
        timeouts: defaults.timeouts,
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
    }
}

fn snapshot(root: &std::path::Path, update: bool) -> Operation {
    Operation::Snapshot {
        name: "diagnostic".into(),
        update,
        include_style: false,
        include_title: false,
        cwd: Some(root.to_string_lossy().into_owned()),
    }
}

#[test]
fn completed_processes_preserve_comparison_and_internal_failure_causes() {
    let root =
        std::env::temp_dir().join(format!("tui-test-exit-diagnostic-{}", std::process::id()));
    let session = Session::new(format!("exit-diagnostic-{}", std::process::id()));
    let options = if cfg!(windows) {
        run_options("cmd.exe", &["/d", "/c", "exit 0"])
    } else {
        run_options("sh", &["-c", "exit 0"])
    };
    session.run(options).unwrap();
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(15_000),
        })
        .unwrap();
    std::fs::create_dir_all(root.join("__snapshots__")).unwrap();
    std::fs::write(
        root.join("__snapshots__").join("diagnostic.snap"),
        "different baseline\n",
    )
    .unwrap();
    let mismatch = session.execute(snapshot(&root, false)).unwrap_err();
    let scalar = session
        .execute(Operation::ExpectOutput {
            text: "missing output".into(),
            regex: false,
        })
        .unwrap_err();
    let internal = session
        .execute(Operation::Screenshot {
            full: false,
            path: Some(
                root.join("missing")
                    .join("screen.svg")
                    .to_string_lossy()
                    .into_owned(),
            ),
            zoom: None,
            background: None,
        })
        .unwrap_err();
    let stopped = session
        .get_by_text("missing marker")
        .wait_with_timeout(Some(0))
        .unwrap_err();
    session.interrupt();
    let cancelled = session
        .get_by_text("missing marker")
        .wait_with_timeout(Some(0))
        .unwrap_err();
    session.close().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        mismatch.details.unwrap().reason,
        FailureReason::SnapshotMismatch
    );
    assert_eq!(
        scalar.details.unwrap().reason,
        FailureReason::ScalarMismatch
    );
    assert_eq!(
        internal.details.unwrap().reason,
        FailureReason::InternalFailure
    );
    assert_eq!(
        stopped.details.unwrap().reason,
        FailureReason::SessionExited
    );
    assert_eq!(cancelled.details.unwrap().reason, FailureReason::Cancelled);
}

#[test]
fn snapshot_errors_show_the_differing_actual_row_without_artifacts() {
    let root = std::env::temp_dir().join(format!(
        "tui-test-snapshot-diagnostic-{}",
        std::process::id()
    ));
    let session = Session::new(format!("snapshot-diagnostic-{}", std::process::id()));
    let options = if cfg!(windows) {
        run_options(
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "$e=[char]27; while ($true) { [Console]::Write($e.ToString()+'[2J'+$e.ToString()+'[25;1HACTUAL_ROW_25'); Start-Sleep -Milliseconds 100 }",
            ],
        )
    } else {
        run_options(
            "sh",
            &["-c", "printf '\\033[2J\\033[25;1HACTUAL_ROW_25'; sleep 30"],
        )
    };
    session.run(options).unwrap();
    session
        .get_by_text("ACTUAL_ROW_25")
        .wait_with_timeout(Some(15_000))
        .unwrap();
    session.execute(snapshot(&root, true)).unwrap();
    let path = root.join("__snapshots__").join("diagnostic.snap");
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("ACTUAL_ROW_25", "WANTED_ROW_25");
    std::fs::write(&path, &expected).unwrap();
    let error = session.execute(snapshot(&root, false)).unwrap_err();
    let saved = std::fs::read_to_string(&path).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(saved, expected);
    assert!(error.artifact.is_none());
    assert!(error.message.contains("ACTUAL_ROW_25"));
    assert!(error.message.contains("WANTED_ROW_25"));
    assert!(error.message.len() <= 4096);
    let details = error.details.unwrap();
    assert_eq!(details.reason, FailureReason::SnapshotMismatch);
    assert!(details.truncated);
    let comparison = details.comparison.unwrap();
    assert_ne!(comparison.expected, comparison.actual);
    assert!(comparison.expected.unwrap().contains("WANTED_ROW_25"));
    assert!(comparison.actual.unwrap().contains("ACTUAL_ROW_25"));
}

#[test]
fn ambiguous_locations_retain_candidate_positions_in_public_errors() {
    let session = Session::new(format!("ambiguity-diagnostic-{}", std::process::id()));
    let options = if cfg!(windows) {
        run_options(
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "$e=[char]27; while ($true) { [Console]::Write($e.ToString()+'[H'+ 'DUPLICATE DUPLICATE'); Start-Sleep -Milliseconds 100 }",
            ],
        )
    } else {
        run_options("sh", &["-c", "printf 'DUPLICATE DUPLICATE'; sleep 30"])
    };
    session.run(options).unwrap();
    session
        .get_by_text("DUPLICATE")
        .wait_with_timeout(Some(15_000))
        .unwrap();
    let error = session
        .execute(Operation::ResolveLocator {
            query: tui_test::LocatorQuery::text("DUPLICATE"),
        })
        .unwrap_err();
    session.close().unwrap();
    let details = error.details.unwrap();
    assert_eq!(details.reason, FailureReason::LocatorAmbiguous);
    let locator = details.locator.unwrap();
    assert_eq!(
        locator.reason,
        Some(tui_test::LocatorFailureReason::Ambiguous)
    );
    assert_eq!(locator.locations.len(), 2);
    assert_eq!(locator.locations[0].column, 0);
    assert_eq!(locator.locations[1].column, 10);
}
