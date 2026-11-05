use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_tui-test");
const RUN_TAPE: &str = include_str!("../../tui-test/tests/fixtures/tape/run.tape");
const TAPE_REFERENCE: &str = include_str!("../../../references/tape.md");
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/tape-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .current_dir(&self.0)
            .env("TUI_TEST_HOME", self.0.join("daemon-home"))
            .env("TUI_TEST_SESSION", "tape-must-not-use-this-session")
            .env("TAPE_CLI_VALUE", "parent")
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn check_reports_geometry_and_located_warnings_without_effects() {
    let workspace = Workspace::new();
    workspace.write(
        "demo.tape",
        "Output result.gif\nType@0 'echo spawned > child-marker'\nEnter\nSleep .2\n",
    );
    workspace.write("tui-test.toml", "invalid TOML must not be loaded");
    let result = workspace.json(&["--json", "tape", "check", "demo.tape"], 0);
    assert_eq!(result["ok"], true);
    let summary = &result["data"];
    assert_eq!(summary["geometry"]["columns"], 80);
    assert_eq!(summary["geometry"]["rows"], 24);
    assert!(summary["geometry"]["width"].as_u64().unwrap() > 80);
    assert!(summary["geometry"]["height"].as_u64().unwrap() > 24);
    assert_eq!(
        summary["output"].as_str(),
        workspace.0.join("result.gif").to_str()
    );
    assert!(!summary["shell"].as_str().unwrap().is_empty());
    let warning = &summary["warnings"][0];
    assert_eq!(warning["kind"], "NativeGrid");
    assert_eq!(
        warning["location"]["path"].as_str(),
        workspace.0.join("demo.tape").to_str()
    );
    assert_eq!(warning["location"]["line"], 1);
    assert_eq!(warning["location"]["column"], 1);

    let output = workspace.command(&["tape", "check", "demo.tape"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("80 columns x 24 rows"));
    assert!(text.contains("pixels"));
    assert!(text.contains("Warning: "));
    assert!(text.contains("demo.tape:1:1"));
    for path in ["result.gif", "child-marker", "daemon-home"] {
        assert!(!workspace.0.join(path).exists(), "{path}");
    }
}

#[test]
fn invalid_tapes_fail_before_any_child_or_output() {
    let workspace = Workspace::new();
    for (text, line, kind) in [
        (
            "Output result.gif\nType@0 'echo spawned > child-marker'\nEnter\nSleep nope",
            4,
            "InvalidValue",
        ),
        (
            "Output result.gif\nRequire missing-tui-test-tape-executable-83f1\nSleep .1",
            2,
            "usage",
        ),
        (
            "Output result.gif\nSet Width 100\nSleep .1",
            2,
            "Unsupported",
        ),
        ("Output result.png\nSleep .1", 1, "Unsupported"),
        (
            "Output result.gif\nSleep .1\nWait /ready/",
            3,
            "Unsupported",
        ),
        (
            "Output result.gif\nSleep .1\nType 'unterminated",
            3,
            "Syntax",
        ),
    ] {
        workspace.write("invalid.tape", text);
        for action in ["check", "run"] {
            let result = workspace.json(&["tape", action, "invalid.tape", "--json"], 2);
            assert_eq!(result["ok"], false);
            let error = &result["error"];
            assert_eq!(error["kind"], kind, "{result}");
            assert_eq!(
                error["location"]["path"].as_str(),
                workspace.0.join("invalid.tape").to_str()
            );
            assert_eq!(error["location"]["line"], line);
            assert!(error["location"]["column"].as_u64().unwrap() > 0);
            if kind == "usage" {
                assert_eq!(error["phase"], "Preflight");
                assert_eq!(error["cleanup_errors"], serde_json::json!([]));
            }
        }
        let output = workspace.command(&["tape", "check", "invalid.tape"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(&format!("invalid.tape:{line}:")));
    }
    for path in ["result.gif", "result.png", "child-marker", "daemon-home"] {
        assert!(!workspace.0.join(path).exists(), "{path}");
    }
}

#[test]
fn explicit_session_and_diagnostic_flags_are_rejected() {
    let workspace = Workspace::new();
    workspace.write("demo.tape", "Output result.gif Sleep .1");
    for flags in [
        vec!["--session", "other"],
        vec!["--verbose"],
        vec!["--failure-artifacts", "artifacts"],
        vec!["--failure-artifact-mode", "all"],
        vec![
            "--failure-artifacts",
            "artifacts",
            "--failure-artifact-recording",
        ],
        vec!["--diagnostic-context", "not-even-a-pair"],
    ] {
        for action in ["check", "run"] {
            for before in [true, false] {
                let mut args = vec!["--json"];
                if before {
                    args.extend(&flags);
                }
                args.extend(["tape", action, "demo.tape"]);
                if !before {
                    args.extend(&flags);
                }
                let result = workspace.json(&args, 2);
                assert!(result["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(flags[0]));
                assert_eq!(result["error"]["kind"], "Usage");
            }
        }
    }
    assert!(!workspace.0.join("artifacts").exists());
    assert!(!workspace.0.join("result.gif").exists());
    assert!(!workspace.0.join("daemon-home").exists());
}

fn decode_gif(path: &Path) -> (u16, u16, u64) {
    let mut decoder = gif::DecodeOptions::new()
        .read_info(std::fs::File::open(path).unwrap())
        .unwrap();
    let (width, height) = (decoder.width(), decoder.height());
    let mut duration = 0;
    let mut frames = 0;
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        frames += 1;
        duration += u64::from(frame.delay);
    }
    assert!(frames > 0);
    (width, height, duration)
}

#[test]
fn run_fixture_exports_a_gif_and_stops_at_hidden_eof() {
    let workspace = Workspace::new();
    workspace.write("run.tape", &format!("{RUN_TAPE}\nSleep 1\n"));
    let check = workspace.json(&["tape", "check", "run.tape", "--json"], 0);
    let run = workspace.json(&["tape", "run", "run.tape", "--json"], 0);
    assert_eq!(run, check);
    let (width, height, duration) = decode_gif(&workspace.0.join("tape-example.gif"));
    assert_eq!(run["data"]["geometry"]["width"], width);
    assert_eq!(run["data"]["geometry"]["height"], height);
    assert!(
        (300..400).contains(&duration),
        "GIF duration: {duration} cs"
    );
    let files: Vec<_> = std::fs::read_dir(&workspace.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");
}

#[test]
fn documented_cli_example_exports_the_reported_geometry_and_duration() {
    let workspace = Workspace::new();
    let example = TAPE_REFERENCE
        .split("```text\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    workspace.write("demo.tape", example);
    let check = workspace.json(&["tape", "check", "demo.tape", "--json"], 0);
    let output = workspace.command(&["tape", "run", "demo.tape"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let (width, height, duration) = decode_gif(&workspace.0.join("demo.gif"));
    assert_eq!(check["data"]["geometry"]["width"], width);
    assert_eq!(check["data"]["geometry"]["height"], height);
    assert!(
        (120..=130).contains(&duration),
        "GIF duration: {duration} cs"
    );
    println!("Documented CLI example: {width}x{height} pixels, {duration} centiseconds");
}

#[test]
fn run_uses_invocation_paths_and_an_isolated_child_environment() {
    let workspace = Workspace::new();
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let cwd = std::env::current_dir().unwrap();
    workspace.write("tui-test.toml", "invalid TOML must not be loaded");
    workspace.write(
        "scripts/demo.tape",
        "Output result.gif\nSource 'settings.tape'\nSource 'actions.tape'\n",
    );
    workspace.write("scripts/settings.tape", "not the settings file");
    workspace.write("scripts/actions.tape", "not the actions file");
    workspace.write(
        "settings.tape",
        "Set Columns 20\nSet Rows 3\nSet Padding 4\nSet FontSize 12\nSet CursorBlink false\nRequire node\nEnv TAPE_CLI_VALUE child\n",
    );
    workspace.write(
        "actions.tape",
        "Hide\nType@0 'node child.cjs'\nEnter@0\nSleep 1\nShow\nSleep .2\nHide\n",
    );
    workspace.write(
        "child.cjs",
        "require('node:fs').writeFileSync('child.json', JSON.stringify({value: process.env.TAPE_CLI_VALUE, session: process.env.TUI_TEST_SESSION, cwd: process.cwd()}));",
    );
    let result = workspace.json(&["tape", "check", "scripts/demo.tape", "--json"], 0);
    assert_eq!(result["data"]["geometry"]["columns"], 20);
    assert!(!workspace.0.join("child.json").exists());
    let output = workspace.command(&["tape", "run", "scripts/demo.tape"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Wrote GIF: "));
    assert!(text.contains("20 columns x 3 rows"));
    assert!(text.contains("pixels"));
    assert!(text.contains("Warning: "));
    assert!(text.contains("settings.tape:"));
    let child: Value =
        serde_json::from_slice(&std::fs::read(workspace.0.join("child.json")).unwrap()).unwrap();
    assert_eq!(child["value"], "child");
    assert_eq!(child["session"], "tape-must-not-use-this-session");
    assert_eq!(child["cwd"].as_str(), workspace.0.to_str());
    let (width, height, duration) = decode_gif(&workspace.0.join("result.gif"));
    assert_eq!(result["data"]["geometry"]["width"], width);
    assert_eq!(result["data"]["geometry"]["height"], height);
    assert!((20..=25).contains(&duration), "GIF duration: {duration} cs");
    assert!(!workspace.0.join("scripts/result.gif").exists());
    assert!(!workspace.0.join("daemon-home").exists());
    assert_eq!(std::env::vars_os().collect::<Vec<_>>(), environment);
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn help_agent_context_and_skill_include_tape() {
    let workspace = Workspace::new();
    for args in [
        vec!["--help"],
        vec!["tape", "--help"],
        vec!["tape", "check", "--help"],
        vec!["tape", "run", "--help"],
        vec!["usage"],
    ] {
        let output = workspace.command(&args);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("tape"), "{args:?}: {text}");
    }
    let output = workspace.command(&["agent-context"]);
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    for action in ["check", "run"] {
        let args = &schema["commands"]["tape"]["subcommands"][action]["args"];
        assert_eq!(args[0]["name"], "file");
        assert_eq!(args[0]["required"], true);
    }
    let output = workspace.command(&["skill"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("# Tape parser and GIF runner"));
    assert!(text.contains("tui-test tape check"));
}
