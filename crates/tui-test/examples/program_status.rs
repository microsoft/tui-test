//! Run with `cargo run -p tui-test-rs --example program_status --all-features`.

use anyhow::{bail, ensure, Result};
use std::io::{self, BufRead, Write};
use tui_test::{
    Backend, OpenOptions, Operation, OperationResult, ProgramState, ProgramStatusKind,
    ProgramStatusMatch, RunOptions, Session,
};

fn demonstrate(backend: Backend) -> Result<()> {
    let session = Session::new(format!("status-example-{}", backend.as_str()));
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
        session.execute(Operation::WaitProgramStatus {
            expected: ProgramStatusMatch {
                id: "build/test".into(),
                state: Some(ProgramState::Blocked),
                kind: Some(ProgramStatusKind::Permission),
                app: Some("cargo".into()),
                message: Some("Approve?".into()),
                progress: Some(40),
                ..Default::default()
            },
            timeout_ms: Some(5_000),
        })?;
        let OperationResult::ProgramStatuses(records) =
            session.execute(Operation::GetProgramStatuses)?
        else {
            bail!("unexpected status result");
        };
        println!("{}: {}", backend.as_str(), serde_json::to_string(&records)?);
        session.execute(Operation::Submit {
            data: Some("approve".into()),
        })?;
        session.execute(Operation::ExpectProgramStatus {
            expected: ProgramStatusMatch {
                id: "build/test".into(),
                state: Some(ProgramState::Done),
                ..Default::default()
            },
            timeout_ms: Some(5_000),
        })?;
        println!("{}: build/test done", backend.as_str());
        session.execute(Operation::Submit {
            data: Some("clear".into()),
        })?;
        session
            .get_by_text("CLEARED")
            .wait_with_timeout(Some(5_000))?;
        ensure!(matches!(
            session.execute(Operation::GetProgramStatus {
                id: "build/test".into()
            })?,
            OperationResult::ProgramStatus(None)
        ));
        let OperationResult::ProgressBar(progress) = session.execute(Operation::GetProgressBar)?
        else {
            bail!("unexpected progress result");
        };
        ensure!(
            progress.progress == Some(40),
            "status reports changed the separate progress bar"
        );
        println!(
            "{}: subtree cleared; progress remains 40%",
            backend.as_str()
        );
        Ok(())
    })();
    let closed = session.close();
    result?;
    closed?;
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--child") {
        print!("\x1b]7501;state=working:app=cargo\x1b\\");
        print!("\x1b]7501;state=blocked:id=build/test:kind=permission:title=VGVzdHM=:msg=QXBwcm92ZT8:progress=40\x07");
        print!("\x1b]9;4;1;40\x07");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            match line?.as_str() {
                "approve" => print!("\x1b]7501;state=done:id=build/test:msg=RG9uZQ==\x07"),
                "clear" => println!("\x1b]7501;state=clear:id=build\x07CLEARED"),
                command => bail!("unexpected command: {command}"),
            }
            io::stdout().flush()?;
        }
        return Ok(());
    }
    demonstrate("ghostty".parse().map_err(anyhow::Error::msg)?)?;
    Ok(())
}
