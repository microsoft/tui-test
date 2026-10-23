//! Run with `cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example cursor_blink_recording`.

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
        print!("READY: quiet screen, blinking bar\r\n\x1b[5 q");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            line?;
        }
        return Ok(());
    }

    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/cursor-blink".into()),
    );
    std::fs::create_dir_all(&directory)?;
    for (name, blink, interval_ms) in [
        ("slow", true, 500),
        ("fast", true, 250),
        ("steady", false, 500),
    ] {
        let mut defaults = OpenOptions::default();
        defaults.style.cursor.blink = blink;
        defaults.style.cursor.blink_interval_ms = interval_ms;
        let session = Session::new(format!("cursor-blink-{name}-{}", std::process::id()));
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
                .get_by_text("READY")
                .wait_with_timeout(Some(5_000))?;
            session.execute(Operation::ExpectCursor {
                x: None,
                y: None,
                visible: Some(true),
                shape: Some("bar".into()),
                timeout_ms: Some(5_000),
            })?;
            let path = directory.join(format!("{name}.gif"));
            let screenshot = directory.join(format!("{name}.png"));
            let snapshot = || -> anyhow::Result<Vec<u8>> {
                session.execute(Operation::Screenshot {
                    full: false,
                    path: Some(screenshot.to_string_lossy().into_owned()),
                    zoom: Some(1.0),
                    background: None,
                })?;
                Ok(std::fs::read(&screenshot)?)
            };
            let before = snapshot()?;
            let started = Instant::now();
            session.execute(Operation::StartRecording {
                path: path.to_string_lossy().into_owned(),
                format: Some(RecordingFormat::Gif),
                fps: Some(30),
                speed: None,
                idle_time_limit: None,
                zoom: Some(0.5),
                background: None,
            })?;
            // This pause sets presentation duration after the readiness checks.
            std::thread::sleep(Duration::from_millis(2100));
            let elapsed = started.elapsed();
            session.execute(Operation::StopRecording)?;
            ensure!(
                snapshot()? == before,
                "cursor animation changed the screenshot"
            );

            let mut decoder = gif::DecodeOptions::new();
            decoder.set_color_output(gif::ColorOutput::RGBA);
            let mut reader = decoder.read_info(std::fs::File::open(&path)?)?;
            let mut duration = Duration::ZERO;
            let mut frames = 0;
            let mut first_pixels = None;
            let mut pixels_changed = false;
            while let Some(frame) = reader.read_next_frame()? {
                duration += Duration::from_millis(u64::from(frame.delay) * 10);
                frames += 1;
                if let Some(first) = &first_pixels {
                    pixels_changed |= first != frame.buffer.as_ref();
                } else {
                    first_pixels = Some(frame.buffer.to_vec());
                }
            }
            ensure!(
                if blink {
                    frames >= 3 && pixels_changed
                } else {
                    frames == 1
                },
                "{name}: unexpected cursor animation ({frames} frames)",
            );
            ensure!(
                duration.abs_diff(elapsed) < Duration::from_millis(250),
                "encoded duration {duration:?} differs from elapsed time {elapsed:?}",
            );
            println!(
                "{}: blink={blink}, phase={interval_ms}ms, {frames} frames, {:.2}s; screenshot unchanged",
                path.display(), duration.as_secs_f64(),
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
