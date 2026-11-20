//! Run with `cargo run -p tui-test-rs --example progress_bar --all-features`.

use std::io::{self, BufRead, Write};

use anyhow::{bail, ensure, Result};
use tui_test::{
    Backend, OpenOptions, Operation, OperationResult, ProgressBarMatch, ProgressState, RunOptions,
    Session,
};

fn demonstrate(backend: Backend) -> Result<()> {
    let session = Session::new(format!("progress-example-{}", backend.as_str()));
    let defaults = OpenOptions::default();
    session.run(RunOptions {
        backend,
        program: std::env::current_exe()?.to_string_lossy().into_owned(),
        args: vec!["--child".into()],
        cols: defaults.cols,
        rows: defaults.rows,
        profile: defaults.profile,
        style: defaults.style,
        cwd: None,
        env: Vec::new(),
        wait_ready: Some(false),
        restart: false,
        timeouts: defaults.timeouts,
        recording: defaults.recording,
    })?;
    let result = (|| -> Result<()> {
        session.execute(Operation::WaitProgressBar {
            expected: ProgressBarMatch {
                state: Some(ProgressState::Normal),
                progress: Some(25),
            },
            timeout_ms: Some(5_000),
        })?;
        for (command, state, progress) in [
            ("pause", ProgressState::Paused, 25),
            ("indeterminate", ProgressState::Indeterminate, 25),
            ("error", ProgressState::Error, 75),
            ("clear", ProgressState::Inactive, 0),
        ] {
            session.execute(Operation::Submit {
                data: Some(command.into()),
            })?;
            session.execute(Operation::ExpectProgressBar {
                expected: ProgressBarMatch {
                    state: Some(state),
                    progress: Some(progress),
                },
                timeout_ms: Some(5_000),
            })?;
            let OperationResult::ProgressBar(actual) =
                session.execute(Operation::GetProgressBar)?
            else {
                bail!("unexpected progress result");
            };
            let OperationResult::State(snapshot) = session.execute(Operation::State)? else {
                bail!("unexpected state result");
            };
            ensure!(snapshot.progress_bar == Some(actual));
            println!(
                "{}: {} {}%",
                backend.as_str(),
                actual.state.as_str(),
                actual.progress.unwrap()
            );
        }
        Ok(())
    })();
    let closed = session.close();
    result?;
    closed?;
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--child") {
        print!("\x1b]9;4;1;25\x1b\\");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            let report = match line?.as_str() {
                "pause" => "\x1b]9;4;4\x07",
                "indeterminate" => "\x1b]9;4;3\x1b\\",
                "error" => "\x1b]9;4;2;75\x07",
                "clear" => "\x1b]9;4;0\x07",
                command => bail!("unexpected command: {command}"),
            };
            print!("{report}");
            io::stdout().flush()?;
        }
        return Ok(());
    }
    for &backend in Backend::ALL
        .iter()
        .filter(|backend| matches!(backend.as_str(), "ghostty" | "rio"))
    {
        demonstrate(backend)?;
    }
    Ok(())
}
