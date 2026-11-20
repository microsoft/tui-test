use clap::{parser::ValueSource, ArgMatches};
use serde::Serialize;
use serde_json::json;
use tui_test::tape::{self, RunnerPhase};

use crate::cli::{TapeArgs, TapeCommand};

pub fn execute(args: &TapeArgs, json: bool, matches: &ArgMatches) -> i32 {
    for (id, flag) in [
        ("session", "--session"),
        ("verbose", "--verbose"),
        ("failure_artifacts", "--failure-artifacts"),
        ("failure_artifact_mode", "--failure-artifact-mode"),
        ("failure_artifact_recording", "--failure-artifact-recording"),
        ("diagnostic_context", "--diagnostic-context"),
        ("context_file", "--context-file"),
        ("operation_name", "--operation-name"),
    ] {
        if matches.value_source(id) == Some(ValueSource::CommandLine) {
            return usage_error(
                &format!("{flag} is not supported by tape; tape uses an isolated local session"),
                json,
            );
        }
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            return usage_error(
                &format!("cannot read the invocation directory: {error}"),
                json,
            )
        }
    };
    let (file, run) = match &args.command {
        TapeCommand::Check(args) => (&args.file, false),
        TapeCommand::Run(args) => (&args.file, true),
    };
    let plan = match tape::load(file, &cwd).and_then(|tape| tape::compile(&tape)) {
        Ok(plan) => plan,
        Err(error) => return report_error(&error, 2, json),
    };
    let result = if run {
        tape::run(&plan, &cwd)
    } else {
        tape::check(&plan, &cwd)
    };
    match result {
        Ok(summary) => {
            if json {
                println!("{}", json!({ "ok": true, "data": summary }));
            } else {
                let action = if run { "Wrote" } else { "Checked" };
                println!("{action} GIF: {}", summary.output.display());
                println!("Shell: {}", summary.shell.display());
                println!(
                    "Terminal: {} columns x {} rows; GIF: {} x {} pixels",
                    summary.geometry.columns,
                    summary.geometry.rows,
                    summary.geometry.width,
                    summary.geometry.height,
                );
                for warning in &summary.warnings {
                    println!("Warning: {warning}");
                }
            }
            0
        }
        Err(error) => {
            let code = match error.phase {
                RunnerPhase::Preflight => 2,
                RunnerPhase::Execution => error.kind.exit_code(),
            };
            report_error(&error, code, json)
        }
    }
}

fn report_error(error: &(impl Serialize + std::fmt::Display), code: i32, json: bool) -> i32 {
    if json {
        println!("{}", json!({ "ok": false, "error": error }));
    } else {
        eprintln!("{error}");
    }
    code
}

fn usage_error(message: &str, json: bool) -> i32 {
    if json {
        println!(
            "{}",
            json!({ "ok": false, "error": { "kind": "Usage", "message": message } })
        );
    } else {
        eprintln!("{message}");
    }
    2
}
