use std::time::{Duration, Instant};

use crate::render::svg::RenderState;
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::Emulator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub grid: Vec<Vec<EmuCell>>,
    pub title: Option<String>,
    pub duration: Duration,
    pub(crate) render_state: RenderState,
    pub(crate) cursor: Option<(u16, usize)>,
    /// Recorded per-cell column widths beside the grid, one row per grid
    /// row. The offline path fills this from the snapshot; live captures
    /// leave it empty and the renderers derive spans from the grid itself.
    pub(crate) widths: Option<Vec<Vec<u16>>>,
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

pub(crate) struct FrameCapture {
    started: Instant,
    options: TimelineOptions,
    clock: TimelineClock,
    pending: TimedGrid,
    collector: FrameCollector,
    error: Option<anyhow::Error>,
}

impl FrameCapture {
    pub(crate) fn new(
        emulator: &dyn Emulator,
        options: TimelineOptions,
        started: Instant,
    ) -> anyhow::Result<Self> {
        validate_options(&options)?;
        let mut clock = TimelineClock::default();
        clock.advance(0.0, &options)?;
        Ok(Self {
            started,
            collector: FrameCollector::new(Duration::from_secs_f64(1.0 / f64::from(options.fps))),
            options,
            clock,
            pending: TimedGrid::capture(emulator, Duration::ZERO)?,
            error: None,
        })
    }

    pub(crate) fn capture(&mut self, emulator: &dyn Emulator, at: Instant) {
        if self.error.is_none() {
            if let Err(error) = self.push(emulator, at) {
                self.error = Some(error);
            }
        }
    }

    fn push(&mut self, emulator: &dyn Emulator, at: Instant) -> anyhow::Result<()> {
        let at = self.clock.advance(
            at.saturating_duration_since(self.started).as_secs_f64(),
            &self.options,
        )?;
        let next = TimedGrid::capture(emulator, at)?;
        let previous = std::mem::replace(&mut self.pending, next);
        let duration = at.saturating_sub(previous.at);
        self.collector.push(previous.into_frame(duration));
        Ok(())
    }

    pub(crate) fn finish(mut self) -> anyhow::Result<Vec<Frame>> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.collector
            .push(self.pending.into_frame(self.options.last_frame_duration));
        Ok(self.collector.finish())
    }
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
    frame: Frame,
}

impl TimedGrid {
    fn capture(emulator: &dyn Emulator, at: Duration) -> anyhow::Result<Self> {
        let (cols, rows) = emulator.size();
        let frame = Frame {
            grid: normalize_grid(emulator.viewable_rows(), cols, rows),
            title: emulator.title(),
            duration: Duration::ZERO,
            render_state: RenderState::capture(emulator),
            cursor: emulator.viewport_cursor().map(|(x, y)| (x, usize::from(y))),
            widths: None,
        };
        if let Some(error) = emulator.fault() {
            anyhow::bail!("recording frame capture failed: {error}");
        }
        Ok(Self { at, frame })
    }

    fn into_frame(self, duration: Duration) -> Frame {
        Frame {
            duration,
            ..self.frame
        }
    }
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
    use crate::profile::Profile;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::backend::Backend;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "recording-raster")]
    use crate::api::RecordingFormat;
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
            widths: None,
        }
    }

    #[test]
    fn viewport_movements_capture_frames_without_pty_output() {
        use crate::api::ScrollTarget;
        for &backend in Backend::ALL {
            let mut emu = backend.build(12, 3, &Profile::default()).unwrap();
            emu.process(b"row0\r\nrow1\r\nrow2\r\nrow3\r\nrow4\r\nrow5");
            let live = emu.viewable_rows();
            let started = Instant::now();
            let mut capture =
                FrameCapture::new(emu.as_ref(), TimelineOptions::default(), started).unwrap();
            emu.scroll(ScrollTarget::Top);
            let history = emu.viewable_rows();
            capture.capture(emu.as_ref(), started + Duration::from_secs(1));
            emu.scroll(ScrollTarget::Bottom);
            capture.capture(emu.as_ref(), started + Duration::from_secs(2));
            let frames = capture.finish().unwrap();
            assert_eq!(frames.len(), 3, "{backend:?}");
            assert_eq!(frames[0].grid, live);
            assert_eq!(frames[1].grid, history);
            assert_eq!(frames[1].cursor, None);
            assert_eq!(frames[2].grid, live);
            assert!(frames[2].cursor.is_some());
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
    fn capture_retains_only_fps_limited_frames() {
        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started).unwrap();
        for index in 0..1_000 {
            emulator.process(if index % 2 == 0 { b"\rA" } else { b"\rB" });
            capture.capture(&emulator, started + Duration::from_millis(index));
        }

        let frames = capture.finish().unwrap();
        assert!(frames.len() <= 31, "retained {} frames", frames.len());
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(3_999)
        );
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn capture_preserves_title_palette_and_cursor_changes() {
        use crate::profile::ColorSlot;
        use crate::render::svg::RenderColors;
        use crate::terminal::emu::CursorShape;

        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(2, 1, &Profile::default());
        emulator.process(
            b"\x1b]2;before\x07\x1b]11;#010203\x07\x1b]12;#040506\x07\
              \x1b[1;2H\x1b[6 q",
        );
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started).unwrap();
        emulator.process(b"\x1b]2;after\x07\x1b]11;#070809\x07\x1b[1;1H\x1b[?25l");
        capture.capture(&emulator, started + Duration::from_millis(100));
        let frames = capture.finish().unwrap();

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
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn capture_preserves_resize_dimensions() {
        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(2, 1, &Profile::default());
        emulator.process(b"A");
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started).unwrap();
        emulator.resize(4, 2);
        capture.capture(&emulator, started + Duration::from_millis(100));
        emulator.process(b"\x1b[2J\x1b[HB");
        capture.capture(&emulator, started + Duration::from_millis(200));
        emulator.resize(1, 1);
        capture.capture(&emulator, started + Duration::from_millis(300));
        emulator.process(b"\x1b[2J\x1b[HC");
        capture.capture(&emulator, started + Duration::from_millis(400));
        let frames = capture.finish().unwrap();
        let mut dimensions = frames
            .iter()
            .map(|frame| frame.dimensions().unwrap())
            .collect::<Vec<_>>();
        dimensions.dedup();
        assert_eq!(dimensions, vec![(2, 1), (4, 2), (1, 1)]);
        assert_eq!(max_dimensions(&frames).unwrap(), (4, 2));
    }

    #[test]
    fn capture_uses_the_live_backend_state_and_profile() {
        let started = Instant::now();
        let profile = Profile {
            scrollback: 1,
            colors: crate::profile::Colors {
                background: crate::profile::Rgb::new(12, 34, 56),
                ..Default::default()
            },
        };
        for &backend in Backend::ALL {
            let mut live = backend.build(16, 2, &profile).unwrap();
            live.process(b"PRIMARY\x1b[?47h\x1b[H\x1b[2JALT");
            let initial = live.viewable_rows();
            let mut capture =
                FrameCapture::new(live.as_ref(), TimelineOptions::default(), started).unwrap();
            live.process(b"\x1b]11;#abcdef\x07\x1b]111\x07\x1b[?1049l");
            capture.capture(live.as_ref(), started + Duration::from_millis(100));
            let frames = capture.finish().unwrap();
            assert_eq!(frames[0].grid, initial, "{backend:?}");
            let last = frames.last().unwrap();
            assert_eq!(last.grid, live.viewable_rows(), "{backend:?}");
            assert_eq!(
                last.render_state,
                RenderState::capture(live.as_ref()),
                "{backend:?}"
            );
        }
    }

    #[test]
    fn capture_reports_emulator_faults() {
        let started = Instant::now();
        for &backend in Backend::ALL {
            let mut live = backend.build(2, 1, &Profile::default()).unwrap();
            let mut capture =
                FrameCapture::new(live.as_ref(), TimelineOptions::default(), started).unwrap();
            live.process(b"\x1b]52;0;Y3V0IGJ1ZmZlcg==\x07");
            capture.capture(live.as_ref(), started + Duration::from_millis(100));
            let error = capture.finish().unwrap_err();
            let message = error.to_string();
            assert!(message.contains("frame capture failed"), "{message}");
            assert!(message.contains("clipboard"), "{message}");
        }
    }

    #[test]
    fn capture_without_new_output_keeps_the_initial_frame_and_pending_replies() {
        let mut live = AlacrittyEmu::new(16, 2, &Profile::default());
        live.process(b"existing screen\x1b[6n");
        let capture = FrameCapture::new(&live, TimelineOptions::default(), Instant::now()).unwrap();
        let frames = capture.finish().unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].grid, live.viewable_rows());
        assert_eq!(frames[0].duration, Duration::from_secs(3));
        assert!(!live.take_pending_writes().is_empty());
    }

    #[test]
    fn captured_frames_encode_to_apng() {
        let apng_path = temp_path("png");
        let started = Instant::now();
        let mut live = AlacrittyEmu::new(2, 1, &Profile::default());
        live.process(b"\x1b[2J\x1b[H\x1b[48;2;200;10;20mA");
        let mut capture = FrameCapture::new(&live, TimelineOptions::default(), started).unwrap();
        live.process(b"\x1b[48;2;10;20;200mB");
        capture.capture(&live, started + Duration::from_millis(100));
        let frames = capture.finish().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(3100)
        );

        let mut renderer = GridRenderer::with_scale(2, 1, 2);
        crate::render::encode::encode(
            &apng_path,
            RecordingFormat::Apng,
            &frames,
            &mut renderer,
            30,
            None,
        )
        .unwrap();
        let encoded = std::fs::read(&apng_path).unwrap();
        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");
        assert!(encoded.windows(4).any(|window| window == b"acTL"));
        // Derived from the style rather than pinned, so a deliberate change to
        // a default gap does not read as a broken encoder test.
        let style = crate::render::style::Style::default();
        let (panel_width, panel_height) = crate::render::svg::test_pixel_size(2, 1, &style);
        assert_eq!(
            renderer.pixel_size(),
            // The renderer is built at scale 2.
            (
                (panel_width + style.canvas_horizontal().unwrap()) * 2,
                (panel_height + style.canvas_vertical().unwrap()) * 2
            )
        );

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
