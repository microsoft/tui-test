//! Run with `cargo run -p tui-test-rs --example scrollback --all-features`.

use std::io::{self, BufRead, Write};

use anyhow::{bail, ensure, Result};
use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, Backend, OpenOptions, Operation, OperationResult,
    RunOptions, ScrollTarget, Session,
};

fn text(session: &Session, full: bool) -> Result<String> {
    match session.execute(Operation::Text { full })? {
        OperationResult::Text(text) => Ok(text),
        _ => bail!("unexpected text result"),
    }
}

fn demonstrate(backend: Backend) -> Result<()> {
    let session = Session::new(format!("scrollback-example-{}", backend.as_str()));
    let defaults = OpenOptions::default();
    session.run(RunOptions {
        backend,
        program: std::env::current_exe()?.to_string_lossy().into_owned(),
        args: vec!["--child".into()],
        cols: 30,
        rows: 5,
        profile: defaults.profile,
        style: defaults.style,
        cwd: None,
        env: Vec::new(),
        wait_ready: Some(false),
        restart: false,
        timeouts: defaults.timeouts,
        recording: AutomaticRecording {
            mode: AutomaticRecordingMode::Disabled,
            directory: None,
        },
    })?;
    let result = (|| -> Result<()> {
        session
            .get_by_text("READY")
            .wait_with_timeout(Some(5_000))?;
        let full = text(&session, true)?;
        println!("{} live:\n{}", backend.as_str(), text(&session, false)?);
        session.execute(Operation::Scroll {
            target: ScrollTarget::Top,
        })?;
        let history = text(&session, false)?;
        ensure!(history.contains("row00"));
        ensure!(
            text(&session, true)? == full,
            "scroll changed the full grid"
        );
        println!("{} history:\n{history}", backend.as_str());
        println!("viewport: {:?}", session.execute(Operation::GetViewport)?);

        session.execute(Operation::Scroll {
            target: ScrollTarget::Lines(-2),
        })?;
        ensure!(!text(&session, false)?.contains("row00"));
        session.execute(Operation::Submit {
            data: Some("hello".into()),
        })?;
        // The child receives only this line, not input from any scroll operation.
        session
            .get_by_text("received: hello")
            .wait_with_timeout(Some(5_000))?;
        let OperationResult::Viewport(viewport) = session.execute(Operation::GetViewport)? else {
            bail!("unexpected viewport result");
        };
        ensure!(
            viewport.offset == 0,
            "keyboard input did not return to live output"
        );
        println!(
            "{} after input:\n{}",
            backend.as_str(),
            text(&session, false)?
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
        for row in 0..20 {
            println!("row{row:02}");
        }
        println!("READY");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            println!("received: {}", line?);
            io::stdout().flush()?;
        }
        return Ok(());
    }
    for &backend in Backend::ALL {
        demonstrate(backend)?;
    }
    Ok(())
}
