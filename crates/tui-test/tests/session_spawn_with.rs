//! Public `Session` native-launch path (U08-R10).
//!
//! External-consumer probe: this target touches only the public `tui_test`
//! API (`Session`, `LaunchOptions`, `RunOptions`, `ExecutionContext`). It
//! spawns through `Session::spawn_with` under a non-default execution
//! context and asserts the child output is visible.

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, ExecutionContext, LaunchOptions, OpenOptions,
    Operation, OperationResult, RunOptions, Session,
};

const FIXTURE_ENV: &str = "TUI_TEST_SESSION_SPAWN_FIXTURE";
const SENTINEL: &str = "TUI_TEST_U08_R10_SENTINEL";
const OPERATION: &str = "u08-r10-spawn";

/// Session settings for launch tests. The launch supplies the program,
/// arguments, environment, and working directory as native values; the
/// matching fields below stay empty and are ignored.
fn base_options() -> RunOptions {
    let defaults = OpenOptions::default();
    RunOptions {
        backend: defaults.backend,
        program: String::new(),
        args: Vec::new(),
        profile: defaults.profile,
        style: defaults.style,
        cols: 200,
        rows: 200,
        cwd: None,
        env: Vec::new(),
        wait_ready: Some(false),
        restart: false,
        timeouts: Default::default(),
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
    }
}

#[test]
fn session_spawn_fixture() {
    if std::env::var(FIXTURE_ENV).as_deref() == Ok("print-env") {
        let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
        vars.sort();
        for (key, value) in vars {
            println!("ENV:{}={}", key.to_string_lossy(), value.to_string_lossy());
        }
    }
}

#[test]
fn u08_r10_session_spawn_with_uses_session_context() {
    let session = Session::new(format!("u08-r10-{}", std::process::id()))
        .with_execution_context(ExecutionContext::default().with_operation(OPERATION));
    let launch = LaunchOptions::new(std::env::current_exe().unwrap())
        .with_args(["--exact", "session_spawn_fixture", "--nocapture"])
        .with_env(FIXTURE_ENV, "print-env")
        .with_env(SENTINEL, "r10-value");
    session.spawn_with(launch, base_options()).unwrap();
    session
        .execute(Operation::WaitExit {
            timeout_ms: Some(10_000),
        })
        .unwrap();
    let text = match session.execute(Operation::State).unwrap() {
        OperationResult::State(state) => state.text.clone(),
        other => panic!("expected terminal state, got {other:?}"),
    };
    assert!(
        text.contains("ENV:TUI_TEST_U08_R10_SENTINEL=r10-value"),
        "child spawned through Session::spawn_with must see the launch override: {text}"
    );
    session.close().unwrap();
    // The session context must reach the spawn request itself, not just the
    // child environment: the check above would pass even against a
    // `spawn_with` that ignored the session context, so a failed spawn under
    // the same context must also carry the configured operation name in its
    // structured failure metadata.
    let failing = Session::new(format!("u08-r10-failing-{}", std::process::id()))
        .with_execution_context(ExecutionContext::default().with_operation(OPERATION));
    let error = failing
        .spawn_with(
            LaunchOptions::new("/nonexistent-tui-test-u08-r10-probe"),
            base_options(),
        )
        .unwrap_err();
    let details = error
        .details
        .as_ref()
        .expect("spawn_with through a session context must carry structured failure metadata");
    assert_eq!(
        details.operation, OPERATION,
        "spawn_with must record the session context operation name"
    );
}
