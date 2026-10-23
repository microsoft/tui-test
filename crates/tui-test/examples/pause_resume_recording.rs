//! Run with `cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example pause_resume_recording`.

#[cfg(feature = "recording-raster")]
fn main() -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use anyhow::ensure;
    use tui_test::{
        AutomaticRecording, AutomaticRecordingMode, Backend, OpenOptions, Operation,
        RecordingFormat, RunOptions, Session,
    };

    if std::env::args().nth(1).as_deref() == Some("--child") {
        print!("\x1b[2 qBEFORE: this screen is recorded");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            let text = match line?.as_str() {
                "hidden" => "EXCLUDED: the terminal still runs",
                "after" => "AFTER: recording resumes here",
                command => anyhow::bail!("unknown command: {command}"),
            };
            print!("\x1b[2J\x1b[H{text}");
            io::stdout().flush()?;
        }
        return Ok(());
    }

    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/pause-resume".into()),
    );
    std::fs::create_dir_all(&directory)?;
    for (extension, format) in [
        ("gif", RecordingFormat::Gif),
        ("cast", RecordingFormat::Cast),
    ] {
        let defaults = OpenOptions::default();
        let session = Session::new(format!("pause-resume-{extension}-{}", std::process::id()));
        session.run(RunOptions {
            backend: Backend::Alacritty,
            program: std::env::current_exe()?.to_string_lossy().into_owned(),
            args: vec!["--child".into()],
            cols: 40,
            rows: 4,
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
        let result = (|| -> anyhow::Result<()> {
            session
                .get_by_text("BEFORE")
                .wait_with_timeout(Some(5_000))?;
            let path = directory.join(format!("pause-resume.{extension}"));
            let started = Instant::now();
            session.execute(Operation::StartRecording {
                path: path.to_string_lossy().into_owned(),
                format: Some(format),
                fps: Some(30),
                speed: None,
                idle_time_limit: None,
                zoom: (format != RecordingFormat::Cast).then_some(0.75),
                background: None,
            })?;
            // These delays set presentation durations, not readiness deadlines.
            std::thread::sleep(Duration::from_secs(1));
            session.execute(Operation::PauseRecording)?;
            session.execute(Operation::Submit {
                data: Some("hidden".into()),
            })?;
            session
                .get_by_text("EXCLUDED")
                .wait_with_timeout(Some(5_000))?;
            std::thread::sleep(Duration::from_secs(2));
            session.execute(Operation::Submit {
                data: Some("after".into()),
            })?;
            session
                .get_by_text("AFTER")
                .wait_with_timeout(Some(5_000))?;
            session.execute(Operation::ResumeRecording)?;
            std::thread::sleep(Duration::from_secs(1));
            session.execute(Operation::PauseRecording)?;
            std::thread::sleep(Duration::from_millis(300));
            let elapsed = started.elapsed();
            session.execute(Operation::StopRecording)?;

            let duration = if extension == "gif" {
                let mut reader =
                    gif::DecodeOptions::new().read_info(std::fs::File::open(&path)?)?;
                let mut duration = Duration::ZERO;
                let mut frames = 0;
                while let Some(frame) = reader.read_next_frame()? {
                    duration += Duration::from_millis(u64::from(frame.delay) * 10);
                    frames += 1;
                }
                ensure!(
                    frames == 2,
                    "expected only BEFORE and AFTER, got {frames} frames"
                );
                duration
            } else {
                let cast = std::fs::read_to_string(&path)?;
                ensure!(!cast.contains("EXCLUDED"), "paused output entered the cast");
                let mut replay = Backend::Alacritty.build(40, 4, &Default::default())?;
                let mut screens = Vec::new();
                for line in cast.lines().skip(1) {
                    let event: serde_json::Value = serde_json::from_str(line)?;
                    if event[1] == "o" {
                        replay.process(event[2].as_str().unwrap().as_bytes());
                        screens.push(
                            replay
                                .viewable_rows()
                                .iter()
                                .flat_map(|row| row.iter().map(|cell| cell.ch.as_str()))
                                .collect::<String>(),
                        );
                    }
                }
                ensure!(
                    screens.iter().any(|screen| screen.contains("BEFORE")),
                    "missing initial screen"
                );
                ensure!(
                    screens.last().unwrap().contains("AFTER"),
                    "missing resumed screen"
                );
                ensure!(
                    screens.iter().all(|screen| !screen.contains("EXCLUDED")),
                    "paused screen entered the cast"
                );
                let last: serde_json::Value = serde_json::from_str(cast.lines().last().unwrap())?;
                Duration::from_secs_f64(last[0].as_f64().unwrap())
            };
            ensure!(
                duration.abs_diff(Duration::from_secs(2)) < Duration::from_millis(250),
                "unexpected playback duration: {duration:?}"
            );
            ensure!(
                elapsed > duration + Duration::from_secs(2),
                "paused time was not excluded"
            );
            println!(
                "{}: {:.2}s elapsed, {:.2}s playback; paused output excluded",
                path.display(),
                elapsed.as_secs_f64(),
                duration.as_secs_f64()
            );
            Ok(())
        })();
        let closed = session.close();
        result?;
        closed?;
    }
    Ok(())
}

#[cfg(not(feature = "recording-raster"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("this example requires the recording-raster feature")
}
