//! Run with `cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example time_based_recording`.

#[cfg(feature = "recording-raster")]
fn main() -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};
    use std::time::{Duration, Instant};

    use anyhow::{bail, ensure};
    use tui_test::{
        AutomaticRecording, AutomaticRecordingMode, Backend, OpenOptions, Operation,
        OperationResult, RecordingFormat, RunOptions, Session,
    };

    if std::env::args().nth(1).as_deref() == Some("--child") {
        println!("READY: a six-second quiet screen");
        io::stdout().flush()?;
        for line in io::stdin().lock().lines() {
            line?;
            println!("SECOND STATE: a one-second hold");
            io::stdout().flush()?;
        }
        return Ok(());
    }

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/time-based-recording.gif".into());
    let session = Session::new(format!("time-based-recording-{}", std::process::id()));
    let defaults = OpenOptions::default();
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
        let before_start = Instant::now();
        session.execute(Operation::StartRecording {
            path: path.clone(),
            format: Some(RecordingFormat::Gif),
            fps: Some(30),
            speed: None,
            idle_time_limit: None,
            zoom: Some(0.5),
            background: None,
        })?;
        let after_start = Instant::now();

        // These pauses set presentation duration; the locators establish readiness.
        std::thread::sleep(Duration::from_secs(6));
        session.execute(Operation::Submit {
            data: Some("next".into()),
        })?;
        session
            .get_by_text("SECOND STATE")
            .wait_with_timeout(Some(5_000))?;
        std::thread::sleep(Duration::from_secs(1));

        let before_stop = Instant::now();
        let OperationResult::Recording(recorded) = session.execute(Operation::StopRecording)?
        else {
            bail!("unexpected recording result");
        };
        let mut reader = gif::DecodeOptions::new().read_info(std::fs::File::open(&recorded)?)?;
        let mut duration = Duration::ZERO;
        let mut frames = 0;
        while let Some(frame) = reader.read_next_frame()? {
            duration += Duration::from_millis(u64::from(frame.delay) * 10);
            frames += 1;
        }
        let lower = before_stop.duration_since(after_start);
        let upper = before_stop.duration_since(before_start);
        ensure!(
            duration + Duration::from_millis(50) >= lower
                && duration <= upper + Duration::from_millis(250),
            "encoded duration {duration:?} differs from elapsed interval {lower:?}..{upper:?}",
        );
        ensure!(frames >= 2, "the second screen was not captured");
        println!("Saved {recorded}");
        println!("Sampling rate: 30 FPS");
        println!("Encoded frames after identical samples merge: {frames}");
        println!("Encoded duration: {:.2}s", duration.as_secs_f64());
        println!("Elapsed time before export: {:.2}s", lower.as_secs_f64());
        println!("The six-second pause and final one-second hold remain in the GIF.");
        Ok(())
    })();
    let closed = session.close();
    result?;
    closed?;
    Ok(())
}

#[cfg(not(feature = "recording-raster"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("this example requires the recording-raster feature")
}
