//! Child environment and native argument control (U08-A through U08-K).
//!
//! The fixture is this same test binary, re-run with the
//! `TUI_TEST_LAUNCH_FIXTURE` variable selecting a mode. Tests that need a
//! controlled parent environment run a dedicated driver process (a plain
//! child process, not a PTY session) which sets its own variables, spawns a
//! grandchild through the Engine, and reports a `PASS` marker. The parallel
//! test runner environment is never changed.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, Engine, ErrorKind, LaunchEnvMode, LaunchOptions,
    OpenOptions, Operation, OperationResult, RunOptions, State,
};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

const FIXTURE_ENV: &str = "TUI_TEST_LAUNCH_FIXTURE";
const PROBE_ENV: &str = "TUI_TEST_LAUNCH_PROBE";
const SENTINEL: &str = "TUI_TEST_U08_SENTINEL";
const CHECK_HEX: &str = "TUI_TEST_U08_CHECK_HEX";
const CHECK_NAME: &str = "TUI_TEST_U08_CHECK_NAME";

/// The probe binary has no test harness, so non-UTF8 and shell-looking
/// arguments reach it unchanged. It is a `[[test]]` target with
/// `harness = false` sourced at `tests/fixtures/launch_probe.rs`, not an
/// example. A bare `cargo test --test` selection never builds it, so the
/// first test needing the probe builds it on demand into an isolated target
/// directory (a separate lock from the running test session, and no network
/// use).
fn probe_program() -> std::path::PathBuf {
    let current = std::env::current_exe().expect("test binary path");
    let profile = current
        .parent()
        .and_then(|path| path.parent())
        .expect("test binary under target/<profile>/deps");
    let target_dir = profile.join("launch-probe-target");
    if let Some(probe) = find_probe_binary(&target_dir.join("debug").join("deps")) {
        return probe;
    }
    build_probe(&target_dir)
}

/// Test targets carry a metadata hash in the file name
/// (`launch_probe-<hash>[.exe]`), so locate the freshest build output.
fn find_probe_binary(deps: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(deps).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_str()?;
        let Some(stem) = name.strip_prefix("launch_probe-") else {
            continue;
        };
        let stem = stem
            .strip_suffix(std::env::consts::EXE_SUFFIX)
            .unwrap_or(stem);
        if stem.is_empty()
            || !stem.bytes().all(|byte| byte.is_ascii_hexdigit())
            || (std::env::consts::EXE_SUFFIX.is_empty() && name.ends_with(".d"))
        {
            continue;
        }
        let modified = entry.metadata().ok()?.modified().ok()?;
        if newest.as_ref().is_none_or(|(known, _)| modified >= *known) {
            newest = Some((modified, entry.path()));
        }
    }
    newest.map(|(_, path)| path)
}

fn build_probe(target_dir: &std::path::Path) -> std::path::PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let built = std::process::Command::new(cargo)
        .args([
            "build",
            "--offline",
            "--manifest-path",
            manifest.to_str().expect("manifest path is Unicode"),
            "--test",
            "launch_probe",
            "--target-dir",
        ])
        .arg(target_dir)
        .status();
    let probe = find_probe_binary(&target_dir.join("debug").join("deps"));
    assert!(
        built.is_ok_and(|status| status.success()) && probe.as_ref().is_some_and(|p| p.is_file()),
        "launch probe build failed; run `cargo build -p tui-test-rs --test launch_probe`"
    );
    probe.expect("probe binary located after successful build")
}

fn test_engine(label: &str) -> Engine {
    Engine::new(
        format!("launch-{label}-{}", std::process::id()),
        Arc::new(tui_test::logger::Logger::disabled()),
        std::env::temp_dir().join(format!("launch-{label}-{}.log", std::process::id())),
    )
}

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

fn spawn_fixture(
    engine: &Engine,
    fixture: &str,
    build: impl FnOnce(LaunchOptions) -> LaunchOptions,
) {
    let launch = build(
        LaunchOptions::new(std::env::current_exe().unwrap())
            .with_args(["--exact", "launch_fixture", "--nocapture"])
            .with_env(FIXTURE_ENV, fixture),
    );
    engine.spawn_with(launch, base_options()).unwrap();
}

fn wait_exit(engine: &Engine) {
    engine
        .execute(Operation::WaitExit {
            timeout_ms: Some(10_000),
        })
        .unwrap();
}

fn engine_state(engine: &Engine) -> State {
    match engine.execute(Operation::State).unwrap() {
        OperationResult::State(state) => *state,
        other => panic!("expected terminal state, got {other:?}"),
    }
}

fn close_engine(engine: &Engine) {
    engine.execute(Operation::Close).unwrap();
}

/// Parse `ENV:key=value` lines from fixture output into a map. Fixture
/// values in these tests contain no trailing spaces.
fn child_env(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("ENV:") {
            if let Some((key, value)) = rest.split_once('=') {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }
    map
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "hex input must pair digits");
    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16).expect("hex input must be valid")
        })
        .collect()
}

fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        value.to_string_lossy().into_owned().into_bytes()
    }
}

/// Run a dedicated driver process with a controlled environment. The driver
/// sets its own variables and reports a `PASS` marker on its stdout.
fn run_driver(driver: &str, marker: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "launch_fixture", "--nocapture"])
        .env(FIXTURE_ENV, driver)
        .output()
        .expect("driver process must run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "driver {driver} failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(marker),
        "driver {driver} reported no {marker}:\n{stdout}"
    );
}

#[test]
fn launch_fixture() {
    let Ok(mode) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    match mode.as_str() {
        "print-env" => print_env(),
        "print-cwd" => print_cwd(),
        "check-native-env" => check_native_env(),
        "driver-a" => driver_a(),
        "driver-b" => driver_b(),
        "driver-h" => driver_h(),
        "driver-cwd-absent" => driver_cwd_absent(),
        "driver-cwd-relative" => driver_cwd_relative(),
        other => panic!("unknown launch fixture mode: {other}"),
    }
}

fn print_cwd() {
    println!(
        "CWD:{}",
        std::env::current_dir()
            .expect("fixture child needs a cwd")
            .display()
    );
}

/// Parse the `CWD:<path>` line from fixture output.
fn child_cwd(text: &str) -> std::path::PathBuf {
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("CWD:") {
            return std::path::PathBuf::from(rest);
        }
    }
    panic!("fixture child reported no CWD line: {text}");
}

fn canonical(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| panic!("test path must exist: {}", path.display()))
}

fn print_env() {
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    vars.sort();
    for (key, value) in vars {
        println!("ENV:{}={}", key.to_string_lossy(), value.to_string_lossy());
    }
}

fn check_native_env() {
    let name = std::env::var(CHECK_NAME).expect("native environment check needs a name");
    let expected =
        hex_decode(&std::env::var(CHECK_HEX).expect("native environment check needs hex"));
    let actual = std::env::vars_os()
        .find(|(key, _)| key.as_os_str() == OsStr::new(&name))
        .map(|(_, value)| value);
    match actual {
        Some(value) if os_bytes(&value) == expected => println!("NATIVE-ENV-PASS"),
        Some(value) => {
            println!("NATIVE-ENV-FAIL got={}", hex_encode(&os_bytes(&value)));
            std::process::exit(1);
        }
        None => {
            println!("NATIVE-ENV-FAIL missing variable");
            std::process::exit(1);
        }
    }
}

fn driver_a() {
    std::env::set_var(SENTINEL, "sentinel-value-a");
    let engine = test_engine("driver-a");
    spawn_fixture(&engine, "print-env", |launch| launch);
    wait_exit(&engine);
    let state = engine_state(&engine);
    let env = child_env(&state.text);
    assert_eq!(
        env.get(SENTINEL).map(String::as_str),
        Some("sentinel-value-a"),
        "inherited child must see the parent sentinel"
    );
    assert_eq!(state.exited, Some(0));
    println!("U08A-PASS");
    close_engine(&engine);
}

fn driver_b() {
    std::env::set_var(SENTINEL, "sentinel-value-b");
    let engine = test_engine("driver-b");
    spawn_fixture(&engine, "print-env", |launch| {
        launch
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("U08B_OVERRIDE", "kept")
    });
    wait_exit(&engine);
    let state = engine_state(&engine);
    let env = child_env(&state.text);
    assert!(
        !env.contains_key(SENTINEL),
        "cleared child must not see the parent sentinel"
    );
    assert_eq!(
        env.get("U08B_OVERRIDE").map(String::as_str),
        Some("kept"),
        "cleared child must accept an explicit override"
    );
    assert_eq!(
        env.get("TERM").map(String::as_str),
        Some("xterm-256color"),
        "cleared child must keep the mandatory terminal default"
    );
    assert_eq!(state.exited, Some(0));
    println!("U08B-PASS");
    close_engine(&engine);
}

fn driver_h() {
    std::env::set_var(SENTINEL, "sentinel-value-h1");
    let engine = test_engine("driver-h");
    spawn_fixture(&engine, "print-env", |launch| launch);
    wait_exit(&engine);
    let before = child_env(&engine_state(&engine).text);
    assert_eq!(
        before.get(SENTINEL).map(String::as_str),
        Some("sentinel-value-h1"),
        "child must see the resolved parent value before restart"
    );
    std::env::set_var(SENTINEL, "sentinel-value-h2");
    engine
        .execute(Operation::Restart {
            graceful_timeout_ms: 1_000,
        })
        .unwrap();
    wait_exit(&engine);
    let state = engine_state(&engine);
    let after = child_env(&state.text);
    assert_eq!(
        after.get(SENTINEL).map(String::as_str),
        Some("sentinel-value-h1"),
        "restart must reuse resolved values, not changed parent values"
    );
    assert_eq!(state.exited, Some(0));
    println!("U08H-PASS");
    close_engine(&engine);
}

fn spawn_cwd_fixture(engine: &Engine, build: impl FnOnce(LaunchOptions) -> LaunchOptions) {
    let launch = build(
        LaunchOptions::new(std::env::current_exe().unwrap())
            .with_args(["--exact", "launch_fixture", "--nocapture"])
            .with_env(FIXTURE_ENV, "print-cwd"),
    );
    engine.spawn_with(launch, base_options()).unwrap();
}

fn restart_engine(engine: &Engine) {
    engine
        .execute(Operation::Restart {
            graceful_timeout_ms: 1_000,
        })
        .unwrap();
}

/// Absent launch cwd: the child starts in the driver's cwd (dir A). After
/// the driver moves to dir B, restart must still start the child in dir A.
fn driver_cwd_absent() {
    let base = std::env::temp_dir().join(format!("u08-cwd-absent-{}", std::process::id()));
    let dir_a = base.join("a");
    let dir_b = base.join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::env::set_current_dir(&dir_a).unwrap();
    let engine = test_engine("driver-cwd-absent");
    spawn_cwd_fixture(&engine, |launch| launch);
    wait_exit(&engine);
    let before = canonical(&child_cwd(&engine_state(&engine).text));
    assert_eq!(
        before,
        canonical(&dir_a),
        "child without a launch cwd must start in the driver directory"
    );
    std::env::set_current_dir(&dir_b).unwrap();
    restart_engine(&engine);
    wait_exit(&engine);
    let state = engine_state(&engine);
    assert_eq!(
        canonical(&child_cwd(&state.text)),
        canonical(&dir_a),
        "restart must reuse the frozen default cwd, not the moved driver directory"
    );
    assert_eq!(state.exited, Some(0));
    println!("U08CWD-ABSENT-PASS");
    close_engine(&engine);
}

/// Relative launch cwd: the child starts in the resolved directory. After
/// the driver moves elsewhere, restart must still start the child in the
/// first resolved directory.
fn driver_cwd_relative() {
    let base = std::env::temp_dir().join(format!("u08-cwd-relative-{}", std::process::id()));
    let parent = base.join("parent");
    let dir_a = parent.join("rel-a");
    let dir_b = base.join("elsewhere");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::env::set_current_dir(&parent).unwrap();
    let engine = test_engine("driver-cwd-relative");
    spawn_cwd_fixture(&engine, |launch| launch.with_cwd("rel-a"));
    wait_exit(&engine);
    let before = canonical(&child_cwd(&engine_state(&engine).text));
    assert_eq!(
        before,
        canonical(&dir_a),
        "child with a relative launch cwd must start in the resolved directory"
    );
    std::env::set_current_dir(&dir_b).unwrap();
    restart_engine(&engine);
    wait_exit(&engine);
    let state = engine_state(&engine);
    assert_eq!(
        canonical(&child_cwd(&state.text)),
        canonical(&dir_a),
        "restart must reuse the frozen relative cwd, not re-resolve it"
    );
    assert_eq!(state.exited, Some(0));
    println!("U08CWD-RELATIVE-PASS");
    close_engine(&engine);
}

#[test]
fn u08_a_inherited_mode_sees_parent_sentinel() {
    run_driver("driver-a", "U08A-PASS");
}

#[test]
fn u08_b_cleared_mode_removes_sentinel_and_keeps_override() {
    run_driver("driver-b", "U08B-PASS");
}

#[test]
fn u08_c_removal_and_override_precedence() {
    // Mandatory defaults return even when removed, and overrides win.
    let resolved = LaunchOptions::new("prog")
        .with_env_mode(LaunchEnvMode::Cleared)
        .with_remove_env("TERM")
        .resolve()
        .unwrap();
    assert_eq!(
        resolved
            .env()
            .iter()
            .find(|(key, _)| key == "TERM")
            .map(|(_, value)| value.to_string_lossy().into_owned()),
        Some("xterm-256color".to_string()),
        "mandatory defaults apply after removals"
    );
    let resolved = LaunchOptions::new("prog")
        .with_env_mode(LaunchEnvMode::Cleared)
        .with_remove_env("SHARED")
        .with_env("SHARED", "override-wins")
        .with_env("TERM", "dumb")
        .resolve()
        .unwrap();
    let env: BTreeMap<&str, &str> = resolved
        .env()
        .iter()
        .map(|(key, value)| {
            (
                key.to_str().expect("test keys are Unicode"),
                value.to_str().expect("test values are Unicode"),
            )
        })
        .collect();
    assert_eq!(
        env.get("SHARED"),
        Some(&"override-wins"),
        "overrides apply after removals"
    );
    assert_eq!(
        env.get("TERM"),
        Some(&"dumb"),
        "overrides win over mandatory defaults"
    );
    // Later overrides win over earlier ones.
    let resolved = LaunchOptions::new("prog")
        .with_env_mode(LaunchEnvMode::Cleared)
        .with_env("ORDER", "first")
        .with_env("ORDER", "second")
        .resolve()
        .unwrap();
    assert_eq!(
        resolved
            .env()
            .iter()
            .find(|(key, _)| key == "ORDER")
            .map(|(_, value)| value.to_string_lossy().into_owned()),
        Some("second".to_string())
    );
    // Removals drop inherited values.
    let resolved = LaunchOptions::new("prog")
        .with_remove_env("PATH")
        .resolve()
        .unwrap();
    assert!(
        !resolved.env().iter().any(|(key, _)| key == "PATH"),
        "removals drop inherited values"
    );
}

#[test]
fn u08_d_restart_keeps_launch_policy() {
    let engine = test_engine("restart-policy");
    spawn_fixture(&engine, "print-env", |launch| {
        launch
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("U08D_KEEP", "restart-kept")
            .with_remove_env("PATH")
    });
    wait_exit(&engine);
    let before = child_env(&engine_state(&engine).text);
    assert_eq!(
        before.get("U08D_KEEP").map(String::as_str),
        Some("restart-kept")
    );
    assert_eq!(
        before.get("TERM").map(String::as_str),
        Some("xterm-256color")
    );
    assert!(
        !before.contains_key("PATH"),
        "cleared launch must not inherit PATH"
    );
    engine
        .execute(Operation::Restart {
            graceful_timeout_ms: 1_000,
        })
        .unwrap();
    wait_exit(&engine);
    let state = engine_state(&engine);
    let after = child_env(&state.text);
    assert_eq!(
        after.get("U08D_KEEP").map(String::as_str),
        Some("restart-kept"),
        "restart must keep the launch policy"
    );
    assert_eq!(
        after.get("TERM").map(String::as_str),
        Some("xterm-256color")
    );
    assert!(
        !after.contains_key("PATH"),
        "restart must keep the cleared environment"
    );
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[test]
fn u08_e_args_with_shell_metacharacters_pass_through() {
    let tricky = [
        "spaced arg",
        "--quoted\"double",
        "'single'",
        "$VAR",
        "`backtick`",
        "semi;colon",
        "pipe|line",
        "amp&ersand",
        "paren(sub)",
        "star*glob",
        "back\\slash",
        "$(subshell)",
    ];
    let engine = test_engine("metacharacters");
    let extra: Vec<OsString> = tricky.iter().map(OsString::from).collect();
    let launch = LaunchOptions::new(probe_program())
        .with_args(extra)
        .with_env(PROBE_ENV, "echo-args");
    engine.spawn_with(launch, base_options()).unwrap();
    wait_exit(&engine);
    let state = engine_state(&engine);
    for arg in tricky {
        assert!(
            state.text.contains(&format!("ARG:{arg}")),
            "argument {arg:?} must reach the child unchanged"
        );
    }
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[cfg(unix)]
#[test]
fn u08_f_unix_native_argument_bytes_are_kept() {
    let raw = vec![0x66, 0xFF, 0xFE, 0x62, 0x80, 0x61, 0x72];
    let expected_hex = hex_encode(&raw);
    let engine = test_engine("native-arg");
    let launch = LaunchOptions::new(probe_program())
        .with_env(PROBE_ENV, "check-native-arg")
        .with_env(CHECK_HEX, expected_hex)
        .with_arg(OsString::from_vec(raw));
    engine.spawn_with(launch, base_options()).unwrap();
    wait_exit(&engine);
    let state = engine_state(&engine);
    assert!(
        state.text.contains("NATIVE-ARG-PASS"),
        "non-UTF8 argument bytes must reach the child unchanged: {}",
        state.text
    );
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[test]
fn u08_g_cleared_environment_contains_only_specified_values() {
    let engine = test_engine("cleared-exact");
    spawn_fixture(&engine, "print-env", |launch| {
        launch
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("U08G_ONE", "one")
            .with_env("U08G_TWO", "two")
    });
    wait_exit(&engine);
    let state = engine_state(&engine);
    let env = child_env(&state.text);
    let mut expected: BTreeSet<&str> = ["TERM", FIXTURE_ENV, "U08G_ONE", "U08G_TWO"]
        .into_iter()
        .collect();
    // The transport forces SHELL on Unix even for cleared launches.
    #[cfg(unix)]
    expected.insert("SHELL");
    assert_eq!(
        env.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected,
        "cleared child must contain only specified values and transport defaults"
    );
    assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
    assert_eq!(env.get("U08G_ONE").map(String::as_str), Some("one"));
    assert_eq!(env.get("U08G_TWO").map(String::as_str), Some("two"));
    #[cfg(unix)]
    assert!(
        env.get("SHELL").is_some_and(|shell| !shell.is_empty()),
        "transport SHELL default must be present on Unix"
    );
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[test]
fn u08_h_restart_reuses_resolved_values_after_parent_change() {
    run_driver("driver-h", "U08H-PASS");
}

#[test]
fn u08_r11_restart_keeps_default_cwd() {
    run_driver("driver-cwd-absent", "U08CWD-ABSENT-PASS");
}

#[test]
fn u08_r11_restart_keeps_relative_cwd() {
    run_driver("driver-cwd-relative", "U08CWD-RELATIVE-PASS");
}

#[cfg(unix)]
#[test]
fn u08_i_unix_native_environment_values_are_unchanged() {
    let raw = vec![0x63, 0xFF, 0x61, 0xFE, 0x62];
    let expected_hex = hex_encode(&raw);
    let engine = test_engine("native-env");
    spawn_fixture(&engine, "check-native-env", |launch| {
        launch
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("U08_NATIVE_VAR", OsString::from_vec(raw))
            .with_env(CHECK_NAME, "U08_NATIVE_VAR")
            .with_env(CHECK_HEX, expected_hex)
    });
    wait_exit(&engine);
    let state = engine_state(&engine);
    assert!(
        state.text.contains("NATIVE-ENV-PASS"),
        "non-UTF8 environment bytes must reach the child unchanged: {}",
        state.text
    );
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[test]
fn u08_j_environment_keys_obey_platform_comparison() {
    assert!(
        std::env::var_os("PATH").is_some(),
        "u08_j needs PATH in the runner environment to compare platform key semantics"
    );
    // Removal with a different case must not remove PATH on case-sensitive
    // platforms, and must remove it where comparison ignores case.
    let engine = test_engine("key-comparison");
    spawn_fixture(&engine, "print-env", |launch| {
        launch.with_remove_env("path").with_env("Path", "odd-case")
    });
    wait_exit(&engine);
    let state = engine_state(&engine);
    let env = child_env(&state.text);
    #[cfg(windows)]
    assert!(
        !env.contains_key("PATH"),
        "Windows removal must ignore key case"
    );
    #[cfg(not(windows))]
    assert!(
        env.contains_key("PATH"),
        "Unix removal must compare key bytes exactly"
    );
    assert_eq!(
        env.get("Path").map(String::as_str),
        Some("odd-case"),
        "explicit overrides apply with the caller spelling"
    );
    assert_eq!(state.exited, Some(0));
    close_engine(&engine);
}

#[test]
fn u08_k_invalid_native_values_are_typed_errors() {
    assert_eq!(
        LaunchOptions::new("").resolve().unwrap_err().kind,
        ErrorKind::Usage,
        "empty program must be a usage error"
    );
    assert_eq!(
        LaunchOptions::new("prog\0").resolve().unwrap_err().kind,
        ErrorKind::Usage,
        "NUL in program must be a usage error"
    );
    assert_eq!(
        LaunchOptions::new("prog")
            .with_env("", "value")
            .resolve()
            .unwrap_err()
            .kind,
        ErrorKind::Usage,
        "empty key must be a usage error"
    );
    assert_eq!(
        LaunchOptions::new("prog")
            .with_env("BAD=KEY", "value")
            .resolve()
            .unwrap_err()
            .kind,
        ErrorKind::Usage,
        "equality sign in key must be a usage error"
    );
    assert_eq!(
        LaunchOptions::new("prog")
            .with_env("KEY", "bad\0value")
            .resolve()
            .unwrap_err()
            .kind,
        ErrorKind::Usage,
        "NUL in value must be a usage error"
    );
    assert_eq!(
        LaunchOptions::new("prog")
            .with_arg("bad\0arg")
            .resolve()
            .unwrap_err()
            .kind,
        ErrorKind::Usage,
        "NUL in argument must be a usage error"
    );
    let engine = test_engine("invalid-launch");
    let error = engine
        .spawn_with(
            LaunchOptions::new("prog").with_arg("bad\0arg"),
            base_options(),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);
    assert!(
        error.message.contains("NUL"),
        "error must name the cause, got: {}",
        error.message
    );
    assert!(
        !engine.is_open(),
        "a rejected launch must not open a session"
    );
}
