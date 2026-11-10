//! Run with `cargo run -p tui-test-rs --example asciicast_versions`.
use std::io::{self, BufRead, Write};

use anyhow::ensure;
use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, OpenOptions, Operation, RecordingFormat,
    RunOptions, Session,
};

fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--child") {
        println!("READY");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            line?;
            println!("UPDATED");
            io::stdout().flush()?;
        }
        return Ok(());
    }

    for (name, format, version) in [
        ("cast", RecordingFormat::Cast, 2),
        ("cast3", RecordingFormat::Cast3, 3),
    ] {
        let directory = std::env::current_dir()?
            .join("target/asciicast-example")
            .join(name);
        let path = directory.join("demo.cast");
        let session = Session::new(format!("asciicast-{name}-{}", std::process::id()));
        let defaults = OpenOptions::default();
        session.run(RunOptions {
            backend: defaults.backend,
            program: std::env::current_exe()?.to_string_lossy().into_owned(),
            args: vec!["--child".into()],
            cols: 40,
            rows: 4,
            profile: defaults.profile,
            style: defaults.style,
            cwd: None,
            env: vec![],
            wait_ready: Some(false),
            restart: false,
            timeouts: defaults.timeouts,
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(directory),
                format: Some(format),
            },
        })?;
        let result = (|| -> anyhow::Result<()> {
            session
                .get_by_text("READY")
                .wait_with_timeout(Some(5_000))?;
            session.execute(Operation::StartRecording {
                path: path.to_string_lossy().into_owned(),
                format: Some(format),
                fps: None,
                speed: None,
                idle_time_limit: None,
                zoom: None,
                background: None,
            })?;
            session.execute(Operation::Submit {
                data: Some("next".into()),
            })?;
            session
                .get_by_text("UPDATED")
                .wait_with_timeout(Some(5_000))?;
            session.execute(Operation::StopRecording)?;
            for (kind, content) in [
                ("explicit", std::fs::read_to_string(&path)?),
                ("automatic", session.recording()?),
            ] {
                let header: serde_json::Value =
                    serde_json::from_str(content.lines().next().unwrap())?;
                ensure!(header["version"] == version, "incorrect {kind} format");
                ensure!(
                    content.contains("UPDATED"),
                    "missing {kind} terminal output"
                );
                println!("{name} {kind}: version {version}, captured UPDATED");
            }
            println!("Saved {}", path.display());
            Ok(())
        })();
        let closed = session.close();
        result?;
        closed?;
    }
    Ok(())
}
