use std::time::{Duration, Instant};

use crate::render::svg::RenderState;
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::{CursorStyle, Emulator};

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
    pub idle_time_limit: Option<Duration>,
    pub cursor: crate::render::style::CursorStyle,
}

impl Default for TimelineOptions {
    fn default() -> Self {
        Self {
            fps: 30,
            speed: 1.0,
            idle_time_limit: None,
            cursor: Default::default(),
        }
    }
}

pub(crate) struct FrameCapture {
    options: TimelineOptions,
    revision: u64,
    sampled_at: Instant,
    source: TimedGrid,
    pending: Frame,
    pending_at: Instant,
    blink_started: Instant,
    output: Vec<Frame>,
    duration: Duration,
    error: Option<anyhow::Error>,
}

impl FrameCapture {
    #[cfg(test)]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn fail(&mut self, error: anyhow::Error) {
        self.error.get_or_insert(error);
    }

    pub(crate) fn new(
        emulator: &dyn Emulator,
        options: TimelineOptions,
        started: Instant,
        revision: u64,
    ) -> anyhow::Result<Self> {
        validate_options(&options)?;
        let source = TimedGrid::capture(emulator, started)?;
        Ok(Self {
            options,
            revision,
            sampled_at: started,
            pending: source.frame.clone(),
            source,
            pending_at: started,
            blink_started: started,
            output: Vec::new(),
            duration: Duration::ZERO,
            error: None,
        })
    }

    pub(crate) fn capture(&mut self, emulator: &dyn Emulator, at: Instant, revision: u64) {
        if self.error.is_none() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.push(emulator, at, revision)
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("recording frame capture panicked")));
            if let Err(error) = result {
                self.error = Some(error);
            }
        }
    }

    fn push(&mut self, emulator: &dyn Emulator, at: Instant, revision: u64) -> anyhow::Result<()> {
        self.check_time(at)?;
        if let Some(error) = emulator.fault() {
            anyhow::bail!("recording frame capture failed: {error}");
        }
        let mut next_source = None;
        if self.revision != revision {
            self.revision = revision;
            let next = TimedGrid::capture(emulator, at)?;
            if self.source.cursor != next.cursor || self.source.frame.cursor != next.frame.cursor {
                self.blink_started = at;
            }
            if self.source.cursor != next.cursor
                || !self.source.frame.same_visual_state(&next.frame)
            {
                next_source = Some(next);
            }
        }
        let source = next_source.as_ref().unwrap_or(&self.source);
        let blinking = self.options.cursor.blink && source.cursor.style.blinking;
        let phase = at.duration_since(self.blink_started).as_millis()
            / u128::from(self.options.cursor.blink_interval_ms);
        let cursor = if blinking && phase % 2 == 1 {
            None
        } else {
            source.frame.cursor
        };
        if next_source.is_some() || self.pending.cursor != cursor {
            let mut next = source.frame.clone();
            next.cursor = cursor;
            let duration = self.frame_duration(at)?;
            let mut previous = std::mem::replace(&mut self.pending, next);
            previous.duration = duration;
            append_frame(&mut self.output, previous)?;
            self.pending_at = at;
        }
        if let Some(next) = next_source {
            self.source = next;
        }
        Ok(())
    }

    fn check_time(&mut self, at: Instant) -> anyhow::Result<()> {
        if at < self.sampled_at {
            anyhow::bail!("recording timestamps must be monotonic");
        }
        self.sampled_at = at;
        Ok(())
    }

    fn frame_duration(&mut self, at: Instant) -> anyhow::Result<Duration> {
        // Blink phases share the idle budget of the unchanged source screen.
        // Scale cumulative boundaries to avoid rounding each short phase.
        let boundary = |time: Instant| -> anyhow::Result<Duration> {
            let elapsed = time.duration_since(self.source.at);
            let elapsed = self
                .options
                .idle_time_limit
                .map_or(elapsed, |limit| elapsed.min(limit));
            Duration::try_from_secs_f64(elapsed.as_secs_f64() / self.options.speed)
                .map_err(|_| anyhow::anyhow!("recording timeline duration is too large"))
        };
        let duration = boundary(at)? - boundary(self.pending_at)?;
        self.duration = self
            .duration
            .checked_add(duration)
            .ok_or_else(|| anyhow::anyhow!("recording timeline duration is too large"))?;
        Ok(duration)
    }

    pub(crate) fn finish(mut self, at: Instant) -> anyhow::Result<Vec<Frame>> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.check_time(at)?;
        let duration = self.frame_duration(at)?;
        self.pending.duration = duration;
        append_frame(&mut self.output, self.pending)?;
        Ok(self.output)
    }
}

fn append_frame(output: &mut Vec<Frame>, frame: Frame) -> anyhow::Result<()> {
    if let Some(previous) = output
        .last_mut()
        .filter(|last| last.same_visual_state(&frame))
    {
        previous.duration = previous
            .duration
            .checked_add(frame.duration)
            .ok_or_else(|| anyhow::anyhow!("recording timeline duration is too large"))?;
    } else {
        output.push(frame);
    }
    Ok(())
}

fn validate_options(options: &TimelineOptions) -> anyhow::Result<()> {
    options.cursor.validate().map_err(anyhow::Error::msg)?;
    if options.fps == 0 {
        anyhow::bail!("recording fps must be greater than zero");
    }
    if !options.speed.is_finite() || options.speed <= 0.0 {
        anyhow::bail!("recording speed must be finite and greater than zero");
    }
    Ok(())
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
    at: Instant,
    frame: Frame,
    cursor: SourceCursor,
}

#[derive(Debug, PartialEq, Eq)]
struct SourceCursor {
    position: (u16, u16),
    visible: bool,
    style: CursorStyle,
}

impl TimedGrid {
    fn capture(emulator: &dyn Emulator, at: Instant) -> anyhow::Result<Self> {
        let (cols, rows) = emulator.size();
        let frame = Frame {
            grid: normalize_grid(emulator.viewable_rows(), cols, rows),
            title: emulator.title(),
            duration: Duration::ZERO,
            render_state: RenderState::capture(emulator),
            cursor: emulator.viewport_cursor().map(|(x, y)| (x, usize::from(y))),
        };
        let cursor = SourceCursor {
            position: emulator.cursor(),
            visible: emulator.cursor_visible(),
            style: emulator.cursor_style(),
        };
        if let Some(error) = emulator.fault() {
            anyhow::bail!("recording frame capture failed: {error}");
        }
        Ok(Self { at, frame, cursor })
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

    #[test]
    fn cursor_phases_follow_native_style_and_recording_settings() {
        for &backend in Backend::ALL {
            for code in 1..=6 {
                for allowed in [false, true] {
                    for interval in [250, 500] {
                        let mut emu = backend.build(4, 2, &Profile::default()).unwrap();
                        emu.process(format!("\x1b[{code} q").as_bytes());
                        let native = emu.cursor_style();
                        let started = Instant::now();
                        let options = TimelineOptions {
                            cursor: crate::render::style::CursorStyle {
                                blink: allowed,
                                blink_interval_ms: interval,
                            },
                            ..Default::default()
                        };
                        let mut capture =
                            FrameCapture::new(emu.as_ref(), options, started, 0).unwrap();
                        for ms in (125..1000).step_by(125) {
                            capture.capture(emu.as_ref(), started + Duration::from_millis(ms), 0);
                        }
                        let frames = capture.finish(started + Duration::from_secs(1)).unwrap();
                        let animates = allowed && code % 2 == 1;
                        let count = if animates { 1000 / interval } else { 1 };
                        assert_eq!(frames.len() as u64, count, "{backend:?}, {code}, {allowed}");
                        for (index, frame) in frames.iter().enumerate() {
                            assert_eq!(frame.cursor.is_some(), !animates || index % 2 == 0);
                            assert_eq!(frame.duration, Duration::from_millis(1000 / count));
                        }
                        assert_eq!(emu.cursor_style(), native);
                        assert!(emu.cursor_visible());
                    }
                }
            }
        }
    }

    #[test]
    fn cursor_only_phases_use_the_cached_screen_and_actual_sample_times() {
        let mut emu = AlacrittyEmu::new(4, 2, &Profile::default());
        emu.process(b"\x1b[1 q");
        let started = Instant::now();
        let mut capture = FrameCapture::new(&emu, TimelineOptions::default(), started, 0).unwrap();
        let grid = emu.viewable_rows();
        // Deliberately withhold the revision to detect an unwanted backend read.
        emu.process(b"NEW");
        capture.capture(&emu, started + Duration::from_millis(1750), 0);
        capture.capture(&emu, started + Duration::from_millis(2000), 0);
        let frames = capture
            .finish(started + Duration::from_millis(2250))
            .unwrap();
        assert_eq!(frames.len(), 3);
        for (frame, (ms, visible)) in frames.iter().zip([(1750, true), (250, false), (250, true)]) {
            assert_eq!(frame.duration, Duration::from_millis(ms));
            assert_eq!(frame.cursor.is_some(), visible);
            assert_eq!(frame.grid, grid);
        }
    }

    #[test]
    fn sampled_cursor_changes_restart_the_visible_phase() {
        for sequence in [&b"\x1b[1;2H"[..], b"\x1b[3 q", b"\x1b[2 q", b"\x1b[?25l"] {
            let mut emu = AlacrittyEmu::new(4, 2, &Profile::default());
            emu.process(b"\x1b[1 q");
            let started = Instant::now();
            let mut capture =
                FrameCapture::new(&emu, TimelineOptions::default(), started, 0).unwrap();
            capture.capture(&emu, started + Duration::from_millis(500), 0);
            emu.process(sequence);
            capture.capture(&emu, started + Duration::from_millis(600), 1);
            let visible = emu.cursor_visible();
            assert_eq!(capture.pending.cursor.is_some(), visible);
            capture.capture(&emu, started + Duration::from_millis(1099), 1);
            assert_eq!(capture.pending.cursor.is_some(), visible);
            capture.capture(&emu, started + Duration::from_millis(1100), 1);
            assert_eq!(
                capture.pending.cursor.is_some(),
                visible && !emu.cursor_style().blinking
            );
            emu.process(b"\x1b[?25h\x1b[5 q");
            capture.capture(&emu, started + Duration::from_millis(1200), 2);
            assert!(capture.pending.cursor.is_some());
            capture
                .finish(started + Duration::from_millis(1300))
                .unwrap();
        }
    }

    #[test]
    fn unrelated_screen_changes_do_not_restart_the_cursor_phase() {
        let mut emu = AlacrittyEmu::new(4, 2, &Profile::default());
        emu.process(b"\x1b[1 q");
        let started = Instant::now();
        let mut capture = FrameCapture::new(&emu, TimelineOptions::default(), started, 0).unwrap();
        capture.capture(&emu, started + Duration::from_millis(500), 0);
        emu.process(b"\x1b]2;new title\x07");
        capture.capture(&emu, started + Duration::from_millis(750), 1);
        assert_eq!(capture.pending.cursor, None);
        capture.capture(&emu, started + Duration::from_millis(1000), 1);
        assert!(capture.pending.cursor.is_some());
        let frames = capture
            .finish(started + Duration::from_millis(1250))
            .unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[2].title.as_deref(), Some("new title"));
    }

    #[test]
    fn blinking_does_not_restart_idle_compression() {
        for limit in [None, Some(Duration::from_secs(2)), Some(Duration::ZERO)] {
            let mut emu = AlacrittyEmu::new(4, 2, &Profile::default());
            emu.process(b"\x1b[1 q");
            let started = Instant::now();
            let options = TimelineOptions {
                speed: 2.0,
                idle_time_limit: limit,
                ..Default::default()
            };
            let mut capture = FrameCapture::new(&emu, options, started, 0).unwrap();
            for ms in (250..6000).step_by(250) {
                // Revisions without a source change do not reset the budget.
                capture.capture(&emu, started + Duration::from_millis(ms), ms);
            }
            emu.process(b"A");
            capture.capture(&emu, started + Duration::from_secs(6), 6000);
            capture.capture(&emu, started + Duration::from_millis(6500), 6000);
            let frames = capture.finish(started + Duration::from_secs(7)).unwrap();
            let first = limit.unwrap_or(Duration::from_secs(6));
            let second = limit
                .unwrap_or(Duration::from_secs(1))
                .min(Duration::from_secs(1));
            assert_eq!(
                frames.iter().map(|frame| frame.duration).sum::<Duration>(),
                (first + second).div_f64(2.0)
            );
            for frame in frames.iter().filter(|frame| !frame.duration.is_zero()) {
                assert_eq!(frame.duration, Duration::from_millis(250));
            }
        }
    }

    #[test]
    fn blink_phases_never_reveal_hidden_or_clipped_cursors() {
        for &backend in Backend::ALL {
            for hidden in [false, true] {
                let mut emu = backend.build(4, 2, &Profile::default()).unwrap();
                emu.process(b"0\r\n1\r\n2\r\n3\r\n4\x1b[1 q");
                if hidden {
                    emu.process(b"\x1b[?25l");
                } else {
                    emu.scroll(crate::api::ScrollTarget::Top);
                }
                assert_eq!(emu.viewport_cursor(), None);
                let started = Instant::now();
                let mut capture =
                    FrameCapture::new(emu.as_ref(), TimelineOptions::default(), started, 0)
                        .unwrap();
                for ms in [500, 1000, 1500] {
                    capture.capture(emu.as_ref(), started + Duration::from_millis(ms), 0);
                }
                let frames = capture.finish(started + Duration::from_secs(2)).unwrap();
                assert_eq!(frames.len(), 1, "{backend:?}");
                assert_eq!(frames[0].cursor, None);
            }
        }
    }

    #[test]
    fn cursor_phases_change_pixels_but_not_the_source_screen() {
        for code in [1, 3, 5] {
            let mut emu = AlacrittyEmu::new(4, 2, &Profile::default());
            emu.process(format!("\x1b[{code} q").as_bytes());
            let started = Instant::now();
            let original = TimedGrid::capture(&emu, started).unwrap().frame;
            let mut capture =
                FrameCapture::new(&emu, TimelineOptions::default(), started, 0).unwrap();
            capture.capture(&emu, started + Duration::from_millis(500), 0);
            let frames = capture.finish(started + Duration::from_secs(1)).unwrap();
            let mut renderer = GridRenderer::new(4, 2);
            let visible = renderer.render(&frames[0]).unwrap().into_raw();
            let hidden = renderer.render(&frames[1]).unwrap().into_raw();
            assert_ne!(visible, hidden, "cursor shape {code}");
            assert_eq!(renderer.render(&original).unwrap().into_raw(), visible);
            assert_eq!(TimedGrid::capture(&emu, started).unwrap().frame, original);
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
                FrameCapture::new(emu.as_ref(), TimelineOptions::default(), started, 0).unwrap();
            emu.scroll(ScrollTarget::Top);
            let history = emu.viewable_rows();
            capture.capture(emu.as_ref(), started + Duration::from_secs(1), 1);
            emu.scroll(ScrollTarget::Bottom);
            capture.capture(emu.as_ref(), started + Duration::from_secs(2), 2);
            let frames = capture.finish(started + Duration::from_secs(3)).unwrap();
            assert_eq!(frames.len(), 3, "{backend:?}");
            assert_eq!(frames[0].grid, live);
            assert_eq!(frames[1].grid, history);
            assert_eq!(frames[1].cursor, None);
            assert_eq!(frames[2].grid, live);
            assert!(frames[2].cursor.is_some());
        }
    }

    #[test]
    fn unchanged_samples_share_one_idle_limit_before_speed_scaling() {
        for limit in [None, Some(Duration::from_secs(5)), Some(Duration::ZERO)] {
            let started = Instant::now();
            let mut emulator = AlacrittyEmu::new(1, 1, &Profile::default());
            let options = TimelineOptions {
                speed: 2.0,
                idle_time_limit: limit,
                ..Default::default()
            };
            let mut capture = FrameCapture::new(&emulator, options, started, 0).unwrap();
            for index in 1..10 {
                // A revision can change without a visible change.
                capture.capture(&emulator, started + Duration::from_secs(index), index);
            }
            emulator.process(b"A");
            capture.capture(&emulator, started + Duration::from_secs(10), 10);
            for index in 11..20 {
                capture.capture(&emulator, started + Duration::from_secs(index), 10);
            }
            let frames = capture.finish(started + Duration::from_secs(20)).unwrap();
            assert_eq!(frames.len(), 2);
            let expected = limit.unwrap_or(Duration::from_secs(10)).div_f64(2.0);
            assert_eq!(frames[0].duration, expected);
            assert_eq!(frames[1].duration, expected);
        }
    }

    #[test]
    fn delayed_samples_preserve_real_durations_without_historical_frames() {
        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started, 0).unwrap();
        emulator.process(b"\rA");
        emulator.process(b"\rB");
        capture.capture(&emulator, started + Duration::from_secs(8), 2);
        let frames = capture
            .finish(started + Duration::from_millis(8123))
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].duration, Duration::from_secs(8));
        assert_eq!(frames[1].duration, Duration::from_millis(123));
        assert_eq!(frames[1].grid[0][0].ch, "B");
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(8123)
        );
    }

    #[test]
    fn unchanged_revision_skips_the_grid_copy_but_keeps_elapsed_time() {
        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        let initial = emulator.viewable_rows();
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started, 0).unwrap();
        emulator.process(b"A");
        capture.capture(&emulator, started + Duration::from_secs(1), 0);
        let frames = capture.finish(started + Duration::from_secs(2)).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].grid, initial);
        assert_eq!(frames[0].duration, Duration::from_secs(2));
    }

    #[test]
    fn invalid_timestamps_and_duration_overflow_are_errors() {
        let started = Instant::now();
        let emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        let options = TimelineOptions {
            speed: f64::MIN_POSITIVE,
            ..Default::default()
        };
        let capture = FrameCapture::new(&emulator, options, started, 0).unwrap();
        assert!(capture.finish(started + Duration::from_secs(1)).is_err());
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started, 0).unwrap();
        capture.capture(&emulator, started + Duration::from_secs(2), 0);
        assert!(capture.finish(started + Duration::from_secs(1)).is_err());
        for options in [
            TimelineOptions {
                fps: 0,
                ..Default::default()
            },
            TimelineOptions {
                speed: f64::NAN,
                ..Default::default()
            },
            TimelineOptions {
                speed: 0.0,
                ..Default::default()
            },
            TimelineOptions {
                cursor: crate::render::style::CursorStyle {
                    blink_interval_ms: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
        ] {
            assert!(FrameCapture::new(&emulator, options, started, 0).is_err());
        }
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
            FrameCapture::new(&emulator, TimelineOptions::default(), started, 0).unwrap();
        emulator.process(b"\x1b]2;after\x07\x1b]11;#070809\x07\x1b[1;1H\x1b[?25l");
        capture.capture(&emulator, started + Duration::from_millis(100), 1);
        let frames = capture
            .finish(started + Duration::from_millis(200))
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
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn capture_preserves_resize_dimensions() {
        let started = Instant::now();
        let mut emulator = AlacrittyEmu::new(2, 1, &Profile::default());
        emulator.process(b"A");
        let mut capture =
            FrameCapture::new(&emulator, TimelineOptions::default(), started, 0).unwrap();
        emulator.resize(4, 2);
        capture.capture(&emulator, started + Duration::from_millis(100), 1);
        emulator.process(b"\x1b[2J\x1b[HB");
        capture.capture(&emulator, started + Duration::from_millis(200), 2);
        emulator.resize(1, 1);
        capture.capture(&emulator, started + Duration::from_millis(300), 3);
        emulator.process(b"\x1b[2J\x1b[HC");
        capture.capture(&emulator, started + Duration::from_millis(400), 4);
        let frames = capture
            .finish(started + Duration::from_millis(500))
            .unwrap();
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
                FrameCapture::new(live.as_ref(), TimelineOptions::default(), started, 0).unwrap();
            live.process(b"\x1b]11;#abcdef\x07\x1b]111\x07\x1b[?1049l");
            capture.capture(live.as_ref(), started + Duration::from_millis(100), 1);
            let frames = capture
                .finish(started + Duration::from_millis(200))
                .unwrap();
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
                FrameCapture::new(live.as_ref(), TimelineOptions::default(), started, 0).unwrap();
            live.process(b"\x1b]52;0;Y3V0IGJ1ZmZlcg==\x07");
            capture.capture(live.as_ref(), started + Duration::from_millis(100), 0);
            let error = capture
                .finish(started + Duration::from_millis(200))
                .unwrap_err();
            let message = error.to_string();
            assert!(message.contains("frame capture failed"), "{message}");
            assert!(message.contains("clipboard"), "{message}");
        }
    }

    #[test]
    fn capture_without_new_output_keeps_the_initial_frame_and_pending_replies() {
        let mut live = AlacrittyEmu::new(16, 2, &Profile::default());
        live.process(b"existing screen\x1b[6n");
        let started = Instant::now();
        let capture = FrameCapture::new(&live, TimelineOptions::default(), started, 0).unwrap();
        let frames = capture
            .finish(started + Duration::from_millis(7123))
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].grid, live.viewable_rows());
        assert_eq!(frames[0].duration, Duration::from_millis(7123));
        assert!(!live.take_pending_writes().is_empty());
    }

    #[test]
    fn captured_frames_encode_to_apng() {
        let apng_path = temp_path("png");
        let started = Instant::now();
        let mut live = AlacrittyEmu::new(2, 1, &Profile::default());
        live.process(b"\x1b[2J\x1b[H\x1b[48;2;200;10;20mA");
        let mut capture = FrameCapture::new(&live, TimelineOptions::default(), started, 0).unwrap();
        live.process(b"\x1b[48;2;10;20;200mB");
        capture.capture(&live, started + Duration::from_millis(100), 1);
        let frames = capture
            .finish(started + Duration::from_millis(200))
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames.iter().map(|frame| frame.duration).sum::<Duration>(),
            Duration::from_millis(200)
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
        let (panel_width, panel_height) = crate::render::svg::pixel_size(2, 1, &style);
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
