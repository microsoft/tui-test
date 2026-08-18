use std::time::Duration;

use crate::profile::Profile;
use crate::render::svg::RenderState;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::Emulator;

use std::io::BufRead;

use super::cast::{CastEventKind, CastReader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub grid: Vec<Vec<EmuCell>>,
    pub title: Option<String>,
    pub duration: Duration,
    pub(crate) render_state: RenderState,
    pub(crate) cursor: Option<(u16, usize)>,
}

impl Frame {
    pub(crate) fn dimensions(&self) -> anyhow::Result<(u16, usize)> {
        let rows = self.grid.len();
        let cols = self.grid.first().map_or(0, Vec::len);
        if rows == 0 || cols == 0 {
            anyhow::bail!("recording frame dimensions must be non-zero");
        }
        if self.grid.iter().any(|row| row.len() != cols) {
            anyhow::bail!("recording frame rows have inconsistent widths");
        }
        Ok((
            cols.try_into()
                .map_err(|_| anyhow::anyhow!("recording frame width exceeds u16"))?,
            rows,
        ))
    }

    fn same_visual_state(&self, other: &Self) -> bool {
        self.grid == other.grid
            && self.title == other.title
            && self.render_state == other.render_state
            && self.cursor == other.cursor
    }
}

pub(crate) fn max_dimensions(frames: &[Frame]) -> anyhow::Result<(u16, usize)> {
    if frames.is_empty() {
        anyhow::bail!("recording timeline contains no frames");
    }
    let mut max_cols = 0;
    let mut max_rows = 0;
    for frame in frames {
        let (cols, rows) = frame.dimensions()?;
        max_cols = max_cols.max(cols);
        max_rows = max_rows.max(rows);
    }
    Ok((max_cols, max_rows))
}
#[derive(Debug, Clone)]
pub(crate) struct TimelineOptions {
    pub fps: u8,
    pub speed: f64,
    pub idle_time_limit: Duration,
    pub last_frame_duration: Duration,
}

impl Default for TimelineOptions {
    fn default() -> Self {
        Self {
            fps: 30,
            speed: 1.0,
            idle_time_limit: Duration::from_secs(5),
            last_frame_duration: Duration::from_secs(3),
        }
    }
}

pub(crate) fn from_cast<R: BufRead>(
    mut cast: CastReader<R>,
    options: &TimelineOptions,
) -> anyhow::Result<Vec<Frame>> {
    validate_options(options)?;
    let cols = cast.header.width;
    let rows = cast.header.height;
    let mut emulator = AlacrittyEmu::new(cols, rows, &Profile::default());
    let mut clock = TimelineClock::default();
    let mut pending: Option<TimedGrid> = None;
    let mut collector = FrameCollector::new(Duration::from_secs_f64(1.0 / f64::from(options.fps)));

    for event in &mut cast {
        let event = event?;
        let at = clock.advance(event.time, options)?;
        if let Some(snapshot) = pending.take() {
            collector.push(Frame {
                grid: snapshot.grid,
                title: snapshot.title,
                duration: at.saturating_sub(snapshot.at),
                render_state: snapshot.render_state,
                cursor: snapshot.cursor,
            });
        }
        match event.kind {
            CastEventKind::Output(output) => emulator.process(output.as_bytes()),
            CastEventKind::Resize(width, height) => emulator.resize(width, height),
        }
        pending = Some(TimedGrid {
            at,
            grid: normalize_grid(emulator.viewable_rows(), cols, rows),
            title: emulator.title(),
            render_state: RenderState::capture(&emulator),
            cursor: emulator.cursor_visible().then(|| {
                let (x, y) = emulator.cursor();
                (x, usize::from(y))
            }),
        });
    }

    match pending {
        Some(snapshot) => collector.push(Frame {
            grid: snapshot.grid,
            title: snapshot.title,
            duration: options.last_frame_duration,
            render_state: snapshot.render_state,
            cursor: snapshot.cursor,
        }),
        None => {
            let (x, y) = emulator.cursor();
            collector.push(Frame {
                grid: vec![vec![EmuCell::blank(); usize::from(cols)]; usize::from(rows)],
                title: emulator.title(),
                duration: options.last_frame_duration,
                render_state: RenderState::capture(&emulator),
                cursor: emulator.cursor_visible().then_some((x, usize::from(y))),
            });
        }
    }
    Ok(collector.finish())
}

fn validate_options(options: &TimelineOptions) -> anyhow::Result<()> {
    if options.fps == 0 {
        anyhow::bail!("recording fps must be greater than zero");
    }
    if !options.speed.is_finite() || options.speed <= 0.0 {
        anyhow::bail!("recording speed must be finite and greater than zero");
    }
    Ok(())
}

#[derive(Default)]
struct TimelineClock {
    previous: Option<f64>,
    adjusted: Duration,
}

impl TimelineClock {
    fn advance(&mut self, current: f64, options: &TimelineOptions) -> anyhow::Result<Duration> {
        let Some(previous) = self.previous.replace(current) else {
            return Ok(Duration::ZERO);
        };
        let gap = (current - previous)
            .max(0.0)
            .min(options.idle_time_limit.as_secs_f64())
            / options.speed;
        let gap = Duration::try_from_secs_f64(gap)
            .map_err(|_| anyhow::anyhow!("recording timeline duration is too large"))?;
        self.adjusted = self.adjusted.saturating_add(gap);
        Ok(self.adjusted)
    }
}

fn normalize_grid(source: Vec<Vec<EmuCell>>, cols: u16, rows: u16) -> Vec<Vec<EmuCell>> {
    let mut output = vec![vec![EmuCell::blank(); usize::from(cols)]; usize::from(rows)];
    for (target, source) in output.iter_mut().zip(source) {
        for (target, source) in target.iter_mut().zip(source) {
            *target = source;
        }
    }
    output
}

struct TimedGrid {
    at: Duration,
    grid: Vec<Vec<EmuCell>>,
    title: Option<String>,
    render_state: RenderState,
    cursor: Option<(u16, usize)>,
}

struct FrameCollector {
    minimum: Duration,
    merged: Option<Frame>,
    output: Vec<Frame>,
}

impl FrameCollector {
    fn new(minimum: Duration) -> Self {
        Self {
            minimum,
            merged: None,
            output: Vec::new(),
        }
    }

    fn push(&mut self, frame: Frame) {
        if let Some(previous) = self
            .merged
            .as_mut()
            .filter(|previous| previous.same_visual_state(&frame))
        {
            previous.duration = previous.duration.saturating_add(frame.duration);
        } else {
            if let Some(previous) = self.merged.replace(frame) {
                self.push_capped(previous);
            }
        }
    }

    fn push_capped(&mut self, frame: Frame) {
        if let Some(previous) = self
            .output
            .last_mut()
            .filter(|previous| previous.duration < self.minimum)
        {
            let duration = previous.duration.saturating_add(frame.duration);
            *previous = Frame { duration, ..frame };
        } else {
            self.output.push(frame);
        }
    }

    fn finish(mut self) -> Vec<Frame> {
        if let Some(frame) = self.merged.take() {
            self.push_capped(frame);
        }
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "recording-raster")]
    use crate::api::RecordingFormat;
    #[cfg(feature = "recording-raster")]
    use crate::record::cast::CastWriter;
    #[cfg(feature = "recording-raster")]
    use crate::render::raster::{FrameRenderer, GridRenderer};

    fn grid(ch: &str) -> Vec<Vec<EmuCell>> {
        vec![vec![EmuCell {
            ch: ch.into(),
            ..EmuCell::blank()
        }]]
    }

    fn frame(ch: &str, duration: Duration) -> Frame {
        let emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        Frame {
            grid: grid(ch),
            title: None,
            duration,
            render_state: RenderState::capture(&emulator),
            cursor: Some((0, 0)),
        }
    }

    #[test]
    fn idle_gaps_are_clamped_before_speed_scaling() {
        let options = TimelineOptions {
            speed: 2.0,
            idle_time_limit: Duration::from_secs(5),
            ..TimelineOptions::default()
        };
        let mut clock = TimelineClock::default();
        assert_eq!(clock.advance(0.0, &options).unwrap(), Duration::ZERO);
        assert_eq!(
            clock.advance(10.0, &options).unwrap(),
            Duration::from_millis(2500)
        );
    }

    #[test]
    fn identical_frames_extend_the_previous_duration() {
        let frames = vec![
            frame("a", Duration::from_millis(20)),
            frame("a", Duration::from_millis(30)),
        ];
        let mut collector = FrameCollector::new(Duration::ZERO);
        for frame in frames {
            collector.push(frame);
        }
        let merged = collector.finish();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].duration, Duration::from_millis(50));
    }

    #[test]
    fn fps_cap_coalesces_short_frames_into_the_latest_grid() {
        let frames = vec![
            frame("a", Duration::from_millis(10)),
            frame("b", Duration::from_millis(10)),
            frame("c", Duration::from_millis(100)),
        ];
        let mut collector = FrameCollector::new(Duration::from_millis(34));
        for frame in frames {
            collector.push(frame);
        }
        let capped = collector.finish();
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].grid, grid("c"));
        assert_eq!(capped[0].duration, Duration::from_millis(120));
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn replay_retains_only_fps_limited_frames() {
        let cast_path = temp_path("cast");
        let started = std::time::Instant::now();
        let mut writer = CastWriter::create(&cast_path, 1, 1, &[], started).unwrap();
        for index in 0..1_000 {
            writer
                .write_output(
                    started + Duration::from_millis(index),
                    if index % 2 == 0 { "\rA" } else { "\rB" },
                )
                .unwrap();
        }
        writer.flush().unwrap();

        let frames = from_cast(
            crate::record::cast::read(&cast_path).unwrap(),
            &TimelineOptions::default(),
        )
        .unwrap();
        assert!(frames.len() <= 31, "retained {} frames", frames.len());
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(3_999)
        );

        std::fs::remove_file(cast_path).unwrap();
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn replay_preserves_title_palette_and_cursor_changes() {
        use crate::profile::ColorSlot;
        use crate::render::svg::RenderColors;
        use crate::terminal::emu::CursorShape;

        let cast_path = temp_path("cast");
        let started = std::time::Instant::now();
        let mut writer = CastWriter::create(&cast_path, 2, 1, &[], started).unwrap();
        writer
            .write_output(
                started,
                "\x1b]2;before\x07\x1b]11;#010203\x07\x1b]12;#040506\x07\
                 \x1b[1;2H\x1b[6 q",
            )
            .unwrap();
        writer
            .write_output(
                started + Duration::from_millis(100),
                "\x1b]2;after\x07\x1b]11;#070809\x07\x1b[1;1H\x1b[?25l",
            )
            .unwrap();
        writer.flush().unwrap();

        let frames = from_cast(
            crate::record::cast::read(&cast_path).unwrap(),
            &TimelineOptions::default(),
        )
        .unwrap();

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].title.as_deref(), Some("before"));
        assert_eq!(frames[1].title.as_deref(), Some("after"));
        assert_eq!(
            frames[0].render_state.color(ColorSlot::Background),
            crate::profile::Rgb::new(1, 2, 3)
        );
        assert_eq!(
            frames[1].render_state.color(ColorSlot::Background),
            crate::profile::Rgb::new(7, 8, 9)
        );
        assert_eq!(
            frames[0].render_state.color(ColorSlot::Cursor),
            crate::profile::Rgb::new(4, 5, 6)
        );
        assert_eq!(frames[0].render_state.cursor_shape(), CursorShape::Bar);
        assert_eq!(frames[0].cursor, Some((1, 0)));
        assert_eq!(frames[1].cursor, None);

        std::fs::remove_file(cast_path).unwrap();
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn scripted_cast_round_trips_to_apng() {
        let cast_path = temp_path("cast");
        let apng_path = temp_path("png");
        let started = std::time::Instant::now();
        let mut writer = CastWriter::create(&cast_path, 2, 1, &[], started).unwrap();
        writer
            .write_output(started, "\x1b[2J\x1b[H\x1b[48;2;200;10;20mA")
            .unwrap();
        writer
            .write_output(
                started + Duration::from_millis(100),
                "\x1b[48;2;10;20;200mB",
            )
            .unwrap();
        writer.flush().unwrap();

        let frames = from_cast(
            crate::record::cast::read(&cast_path).unwrap(),
            &TimelineOptions::default(),
        )
        .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(3100)
        );

        let mut renderer = GridRenderer::with_scale(2, 1, 2);
        crate::render::encode::encode(&apng_path, RecordingFormat::Apng, &frames, &mut renderer)
            .unwrap();
        let encoded = std::fs::read(&apng_path).unwrap();
        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");
        assert!(encoded.windows(4).any(|window| window == b"acTL"));
        assert_eq!(renderer.pixel_size(), (100, 148));
        std::fs::remove_file(cast_path).unwrap();
        std::fs::remove_file(apng_path).unwrap();
    }

    #[cfg(feature = "recording-raster")]
    fn temp_path(extension: &str) -> std::path::PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tui-test-cast-roundtrip-{}-{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            extension
        ))
    }
}
