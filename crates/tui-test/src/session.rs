//! A single managed terminal session: a PTY feeding an emulator and command
//! tracker, with a background reader thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::diagnostics::{elapsed_ms, DiagnosticRetentionOptions, ScreenHistory};
use crate::event::BellTracker;
use crate::logger::Logger;
use crate::profile::Profile;
use crate::record::{self, CaptureError, Recorder, StartRecording};
#[cfg(feature = "recording-raster")]
use crate::render::raster::GridRenderer;
use crate::shell::{self, Shell};
use crate::terminal::backend::Backend;
use crate::terminal::emu::{Emulator, MouseModeTracker};
use crate::terminal::integration::CommandTracker;
use crate::terminal::pty::{validate_cwd, validate_size, Pty, SpawnOptions};

#[derive(Debug, Clone)]
pub(crate) struct TextHighlight {
    /// Match cells in full-grid coordinates.
    pub cells: Vec<(usize, usize)>,
    /// Full-grid row where the viewport began when the highlight was created.
    pub viewport_offset: usize,
}

pub(crate) struct ManualRecordingOptions {
    pub path: String,
    pub format: Option<crate::api::RecordingFormat>,
    pub fps: Option<u8>,
    pub speed: Option<f64>,
    pub idle_time_limit: Option<f64>,
    pub zoom: Option<f64>,
    pub background: Option<crate::api::CaptureBackground>,
}

pub struct TermState {
    pub emu: Box<dyn Emulator>,
    /// The profile this session started with, kept so a palette entry a
    /// program overrode can be told apart from one it never touched.
    pub(crate) profile: Profile,
    /// How this session's screenshots and recordings are drawn.
    pub(crate) style: crate::render::style::Style,
    #[cfg(feature = "recording-raster")]
    recording_frames: Option<record::frames::FrameCapture>,
    /// Shell-integration state, derived from the raw PTY stream rather than
    /// the emulator, so it is identical across backends.
    pub tracker: CommandTracker,
    pub(crate) mouse_mode: MouseModeTracker,
    pub observed_clipboard_revision: u64,
    pub started_at: Instant,
    pub visual_revision: u64,
    pub screen_history: ScreenHistory,
    pub screen_dirty: bool,
    pub last_screen_sample: Instant,
    pub last_visual_change_ms: u64,
    pub diagnostic_error: Option<String>,
    /// Time of the last observed visual change, not the last raw PTY output.
    pub last_change: Instant,
    pub awaiting_start: Option<u64>,
    pub exited: Option<i32>,
    pub exit_signal: Option<String>,
    pub exit_error: Option<String>,
    pub highlight: Option<TextHighlight>,
}

impl TermState {
    pub(crate) fn awaiting_command_start(&self) -> bool {
        self.awaiting_start
            .is_some_and(|seen| self.tracker.started_count() == seen)
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.exited.is_none()
            && self.exit_error.is_none()
            && !self.awaiting_command_start()
            && self.tracker.is_ready()
    }

    pub(crate) fn command_exit_code(&self) -> Option<i32> {
        if self.awaiting_command_start() || self.tracker.executing() {
            None
        } else {
            self.tracker.last_exit()
        }
    }
}

pub struct Session {
    pub shell: Option<Shell>,
    pub backend: Backend,
    pub child_pid: Option<u32>,
    /// Per-class timeout defaults for the lifetime of this session.
    pub timeouts: crate::api::Timeouts,
    pub pty: Arc<Pty>,
    pub state: Arc<Mutex<TermState>>,
    /// Serialize viewport movement with input without blocking the output reader.
    pub(crate) viewport_input: Arc<Mutex<()>>,
    pub cancelled: Arc<AtomicBool>,
    pub(crate) bells: BellTracker,
    recorder: Recorder,
    #[cfg(feature = "recording-raster")]
    recording_sampler: Mutex<Option<RecordingSampler>>,
    logger: Arc<Logger>,
    reader: Option<JoinHandle<()>>,
    _process_watcher: JoinHandle<()>,
}

#[cfg(feature = "recording-raster")]
struct RecordingSampler {
    stop: std::sync::mpsc::Sender<()>,
    worker: JoinHandle<()>,
}

#[cfg(feature = "recording-raster")]
impl RecordingSampler {
    fn start(state: &Arc<Mutex<TermState>>, started: Instant, fps: u8) -> std::io::Result<Self> {
        let state = Arc::downgrade(state);
        let (stop, receiver) = std::sync::mpsc::channel();
        let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
        let worker = std::thread::Builder::new()
            .name("tui-test-recording".into())
            .spawn(move || loop {
                let deadline = next_recording_deadline(started, Instant::now(), interval);
                if let Ok(deadline) = deadline.as_ref() {
                    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
                let Some(state) = state.upgrade() else { break };
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let TermState {
                    emu,
                    recording_frames,
                    visual_revision,
                    ..
                } = &mut *state;
                let Some(frames) = recording_frames.as_mut() else {
                    break;
                };
                match deadline {
                    Ok(_) => frames.capture(emu.as_ref(), Instant::now(), *visual_revision),
                    Err(error) => {
                        frames.fail(error);
                        break;
                    }
                }
            })?;
        Ok(Self { stop, worker })
    }

    fn stop(self) {
        let _ = self.stop.send(());
        // Capture panics are retained in FrameCapture and reported during export.
        self.worker.join().expect("recording sampler panicked");
    }
}

#[cfg(feature = "recording-raster")]
fn next_recording_deadline(
    started: Instant,
    now: Instant,
    interval: Duration,
) -> anyhow::Result<Instant> {
    let elapsed = now.duration_since(started);
    let remainder = Duration::from_nanos((elapsed.as_nanos() % interval.as_nanos()) as u64);
    now.checked_add(interval - remainder)
        .ok_or_else(|| anyhow::anyhow!("recording sample deadline is too large"))
}

impl Session {
    /// The session default for `class`, else the environment, else the built-in.
    pub fn timeout_for(&self, class: crate::config::TimeoutClass) -> u64 {
        self.timeouts
            .get(class)
            .unwrap_or_else(|| class.default_ms())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        shell: Option<Shell>,
        program: Option<Vec<String>>,
        backend: Backend,
        profile: Profile,
        style: crate::render::style::Style,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
        env: Vec<(String, String)>,
        timeouts: crate::api::Timeouts,
        diagnostics: DiagnosticRetentionOptions,
        logger: Arc<Logger>,
        recording_path: Option<PathBuf>,
        recording_required: bool,
    ) -> anyhow::Result<Self> {
        style.validate().map_err(anyhow::Error::msg)?;
        validate_size(cols, rows)?;
        if let Some(cwd) = &cwd {
            validate_cwd(cwd)?;
        }
        diagnostics.validate().map_err(anyhow::Error::msg)?;
        let started_at = Instant::now();
        let bells = BellTracker::new(started_at);
        let emu = backend.build_with_bells(cols, rows, &profile, bells.clone())?;
        let mut initial_state = TermState {
            emu,
            profile,
            style,
            #[cfg(feature = "recording-raster")]
            recording_frames: None,
            tracker: CommandTracker::new(),
            mouse_mode: MouseModeTracker::new(),
            observed_clipboard_revision: 0,
            started_at,
            visual_revision: 0,
            screen_history: ScreenHistory::new(diagnostics.screen_history_limit),
            screen_dirty: true,
            last_screen_sample: started_at,
            last_visual_change_ms: 0,
            diagnostic_error: None,
            last_change: Instant::now(),
            awaiting_start: None,
            exited: None,
            exit_signal: None,
            exit_error: None,
            highlight: None,
        };
        let _ = try_capture_visual_state(&mut initial_state, true);
        let state = Arc::new(Mutex::new(initial_state));

        let mut rec_env = vec![("TERM".to_string(), "xterm-256color".to_string())];
        if let Some(sh) = shell {
            rec_env.push(("SHELL".to_string(), sh.as_str().to_string()));
        }
        let recorder = Recorder::create_at(
            recording_path.clone(),
            cols,
            rows,
            &rec_env,
            recording_required,
            logger.clone(),
            started_at,
        )?;
        let spawned = (|| {
            if let Some(program) = &program {
                let (target, args) = program
                    .split_first()
                    .ok_or_else(|| anyhow::anyhow!("empty program"))?;
                let opts = SpawnOptions {
                    cols,
                    rows,
                    cwd: None,
                    env,
                };
                Pty::spawn_with_cwd(target, args, &opts, cwd.as_deref())
            } else {
                let sh = shell.unwrap_or_else(shell::default_shell);
                let mut launch = shell::shell_launch(sh)?;
                launch.env.extend(env);
                Pty::spawn_launch_with_cwd(&launch, cols, rows, cwd)
            }
        })();
        let (pty, reader) = match spawned {
            Ok(spawned) => spawned,
            Err(error) => {
                let remove = recorder.automatic_enabled();
                drop(recorder);
                if remove {
                    if let Some(path) = recording_path {
                        let _ = std::fs::remove_file(path);
                    }
                }
                return Err(error);
            }
        };

        let child_pid = pty.pid();
        let pty = Arc::new(pty);
        let cancelled = Arc::new(AtomicBool::new(false));

        let reader_state = state.clone();
        let reader_pty = pty.clone();
        let reader_logger = logger.clone();
        let reader_recorder = recorder.capture();
        let reader_finished = Arc::new(AtomicBool::new(false));
        let finished = reader_finished.clone();
        let mut reader = reader;
        let reader_handle = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        reader_logger.event("pty stream reached EOF");
                        break;
                    }
                    Err(error) => {
                        reader_logger.event(&format!("pty read failed error={error}"));
                        break;
                    }
                    Ok(n) => {
                        reader_logger.read(&buf[..n]);
                        let pending = {
                            let mut st = reader_state
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            st.emu.process(&buf[..n]);
                            st.tracker.feed(&buf[..n]);
                            st.visual_revision = st.visual_revision.wrapping_add(1);
                            st.screen_dirty = true;
                            st.mouse_mode.process(&buf[..n]);
                            st.highlight = None;
                            reader_recorder.on_data(&buf[..n]);
                            let _ = try_capture_visual_state(&mut st, false);
                            st.emu.take_pending_writes()
                        };
                        if !pending.is_empty() {
                            reader_logger.reply(&pending);
                            let _ = reader_pty.write(&pending);
                        }
                    }
                }
            }
            finished.store(true, Ordering::Release);
        });

        let watcher_state = state.clone();
        let watcher_pty = pty.clone();
        let watcher_logger = logger.clone();
        let process_watcher = std::thread::spawn(move || loop {
            let status = watcher_pty.try_wait();
            match status {
                Ok(Some(status)) => {
                    // Preserve trailing output before publishing exit, without
                    // waiting forever for descendants that inherited the PTY.
                    let draining = Instant::now();
                    while !reader_finished.load(Ordering::Acquire)
                        && draining.elapsed() < Duration::from_millis(250)
                    {
                        std::thread::sleep(Duration::from_millis(crate::config::POLL_DELAY_MS));
                    }
                    watcher_logger.event(&format!(
                        "process exited code={} signal={:?}",
                        status.code, status.signal
                    ));
                    let mut st = watcher_state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    st.exited = Some(status.code);
                    st.exit_signal = status.signal;
                    break;
                }
                Ok(None) => {
                    std::thread::sleep(Duration::from_millis(crate::config::POLL_DELAY_MS));
                }
                Err(error) => {
                    let error = error.to_string();
                    watcher_logger.event(&format!("process wait failed error={error}"));
                    let mut st = watcher_state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    st.exit_error = Some(error);
                    break;
                }
            }
        });

        logger.event(&format!(
            "session open shell={:?} program={:?} backend={} {}x{}",
            shell,
            program,
            backend.as_str(),
            cols,
            rows
        ));

        Ok(Session {
            shell,
            backend,
            child_pid,
            timeouts,
            pty,
            state,
            viewport_input: Arc::new(Mutex::new(())),
            cancelled,
            bells,
            recorder,
            #[cfg(feature = "recording-raster")]
            recording_sampler: Mutex::new(None),
            logger,
            reader: Some(reader_handle),
            _process_watcher: process_watcher,
        })
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.logger.write(data);
        {
            let mut st = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !st.tracker.executing() {
                let started_count = st.tracker.started_count();
                st.awaiting_start = Some(started_count);
            }
        }
        self.pty.write(data)?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), crate::api::TuiTestError> {
        validate_size(cols, rows)?;
        self.logger.event(&format!("resize {cols}x{rows}"));
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Keep output processing and recording on the old size unless the PTY
        // accepts the resize. In particular, never feed zero sizes to an emulator.
        self.pty.resize(cols, rows).map_err(|error| {
            crate::api::TuiTestError::internal(format!("failed to resize PTY: {error}"))
        })?;
        resize_emulator_and_record(&mut state, &self.recorder, cols, rows);
        Ok(())
    }

    pub fn scroll(&self, target: crate::api::ScrollTarget) -> Result<(), crate::api::TuiTestError> {
        let _input = self
            .viewport_input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        scroll_emulator_and_record(&mut state, target)
    }

    pub fn start_recording(
        &self,
        options: ManualRecordingOptions,
    ) -> Result<(), crate::api::TuiTestError> {
        let ManualRecordingOptions {
            path,
            format,
            fps,
            speed,
            idle_time_limit,
            zoom,
            background,
        } = options;
        if path.trim().is_empty() {
            return Err(crate::api::TuiTestError::usage(
                "recording path must not be empty",
            ));
        }
        let format = format
            .or_else(|| crate::api::RecordingFormat::infer(&path))
            .ok_or_else(|| {
                crate::api::TuiTestError::usage(
                    "cannot infer recording format; use .png, .apng, .gif, .mp4, or .cast",
                )
            })?;
        let zoom = crate::api::resolve_zoom(zoom)?;
        if format == crate::api::RecordingFormat::Cast && zoom != 1.0 {
            return Err(crate::api::TuiTestError::usage(
                "zoom is only supported for image and video recordings",
            ));
        }
        if format == crate::api::RecordingFormat::Cast && background.is_some() {
            return Err(crate::api::TuiTestError::usage(
                "background customization is only supported for image and video recordings",
            ));
        }
        if format == crate::api::RecordingFormat::Mp4
            && background == Some(crate::api::CaptureBackground::Transparent)
        {
            return Err(crate::api::TuiTestError::usage(
                "transparent backgrounds are not supported for MP4 recordings",
            ));
        }
        #[cfg(not(feature = "recording-raster"))]
        if format != crate::api::RecordingFormat::Cast {
            return Err(crate::api::TuiTestError::usage(
                "APNG, GIF, and MP4 recording require the tui-test 'recording-raster' feature",
            ));
        }
        #[cfg(feature = "recording-raster")]
        let ffmpeg_path = recording_ffmpeg_path(format)?;
        let fps = fps.unwrap_or(30);
        if fps == 0 {
            return Err(crate::api::TuiTestError::usage(
                "recording fps must be greater than zero",
            ));
        }
        let speed = speed.unwrap_or(1.0);
        if !speed.is_finite() || speed <= 0.0 {
            return Err(crate::api::TuiTestError::usage(
                "recording speed must be finite and greater than zero",
            ));
        }
        if idle_time_limit.is_some_and(|limit| !limit.is_finite() || limit < 0.0) {
            return Err(crate::api::TuiTestError::usage(
                "idle time limit must be a finite, non-negative number of seconds",
            ));
        }
        let idle_time_limit = idle_time_limit
            .map(Duration::try_from_secs_f64)
            .transpose()
            .map_err(|_| crate::api::TuiTestError::usage("idle time limit is too large"))?;
        std::time::Duration::try_from_secs_f64(
            idle_time_limit
                .unwrap_or(Duration::from_secs(1))
                .as_secs_f64()
                / speed,
        )
        .map_err(|_| crate::api::TuiTestError::usage("recording speed is too small"))?;

        let target_path = PathBuf::from(path);
        let capture_path = if format == crate::api::RecordingFormat::Cast {
            target_path.clone()
        } else {
            record::sidecar_path(&target_path)
        };
        let mut env = vec![("TERM".to_string(), "xterm-256color".to_string())];
        if let Some(shell) = self.shell {
            env.push(("SHELL".to_string(), shell.as_str().to_string()));
        }
        #[cfg(feature = "recording-raster")]
        let mut sampler = self
            .recording_sampler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.load(Ordering::Acquire) {
            return Err(crate::api::TuiTestError::internal("session is closed"));
        }
        let (cols, rows) = state.emu.size();
        let initial_output = record::cast::snapshot_to_ansi(state.emu.as_ref());
        let request = StartRecording {
            target_path,
            capture_path,
            format,
            cols,
            rows,
            env,
            initial_output,
            #[cfg(feature = "recording-raster")]
            zoom,
            #[cfg(feature = "recording-raster")]
            style: state.style.clone(),
            #[cfg(feature = "recording-raster")]
            background,
            #[cfg(feature = "recording-raster")]
            timeline: record::frames::TimelineOptions {
                fps,
                speed,
                idle_time_limit,
                cursor: state.style.cursor,
            },
            #[cfg(feature = "recording-raster")]
            ffmpeg_path,
        };
        let started = Instant::now();
        #[cfg(feature = "recording-raster")]
        let frames = if format == crate::api::RecordingFormat::Cast {
            None
        } else {
            Some(
                record::frames::FrameCapture::new(
                    state.emu.as_ref(),
                    request.timeline.clone(),
                    started,
                    state.visual_revision,
                )
                .map_err(|error| crate::api::TuiTestError::internal(error.to_string()))?,
            )
        };
        self.recorder
            .start_at(request, started)
            .map_err(capture_error)?;
        #[cfg(feature = "recording-raster")]
        {
            let mut state = state;
            state.recording_frames = frames;
            if state.recording_frames.is_some() {
                match RecordingSampler::start(&self.state, started, fps) {
                    Ok(worker) => *sampler = Some(worker),
                    Err(error) => {
                        state.recording_frames = None;
                        let _ = self.recorder.stop();
                        return Err(crate::api::TuiTestError::internal(format!(
                            "failed to start recording sampler: {error}"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn stop_recording(&self) -> Result<String, crate::api::TuiTestError> {
        #[cfg(feature = "recording-raster")]
        let mut sampler = self
            .recording_sampler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(feature = "recording-raster")]
        let mut state = state;
        #[cfg(feature = "recording-raster")]
        let frames = state.recording_frames.take().map(|mut frames| {
            let stopped = Instant::now();
            // Check faults without sampling a new visual state at the stop boundary.
            if let Some(error) = state.emu.fault() {
                frames.fail(anyhow::anyhow!("recording frame capture failed: {error}"));
            }
            frames.finish(stopped)
        });
        let stopped = self.recorder.stop();
        drop(state);
        #[cfg(feature = "recording-raster")]
        {
            if let Some(worker) = sampler.take() {
                worker.stop();
            }
            drop(sampler);
        }
        let stopped = stopped.map_err(capture_error)?;
        if stopped.format == crate::api::RecordingFormat::Cast {
            return Ok(stopped.target_path.to_string_lossy().into_owned());
        }

        #[cfg(not(feature = "recording-raster"))]
        return Err(crate::api::TuiTestError::internal(
            "raster recording was started without the 'recording-raster' feature",
        ));

        #[cfg(feature = "recording-raster")]
        {
            let temporary_path = temporary_output_path(&stopped.target_path);
            let result = (|| -> anyhow::Result<()> {
                let frames =
                    frames.ok_or_else(|| anyhow::anyhow!("recording frames are unavailable"))??;
                let (max_cols, max_rows) = record::frames::max_dimensions(&frames)?;
                let mut renderer = GridRenderer::with_zoom_and_background(
                    max_cols,
                    max_rows,
                    2.0 * stopped.zoom,
                    stopped.style.clone(),
                    stopped.background,
                )?;
                crate::render::encode::encode(
                    &temporary_path,
                    stopped.format,
                    &frames,
                    &mut renderer,
                    stopped.timeline.fps,
                    stopped.ffmpeg_path.as_deref(),
                )?;
                replace_output(&temporary_path, &stopped.target_path)?;
                cleanup_sidecar(&stopped.capture_path, |message| self.logger.event(message));
                Ok(())
            })();

            match result {
                Ok(()) => Ok(stopped.target_path.to_string_lossy().into_owned()),
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary_path);
                    Err(crate::api::TuiTestError::internal(format!(
                        "failed to export recording; captured cast retained at {}: {error}",
                        stopped.capture_path.display()
                    )))
                }
            }
        }
    }

    pub fn kill(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(feature = "recording-raster")]
        self.stop_recording_sampler();
        self.pty.close();
    }

    #[cfg(feature = "recording-raster")]
    fn stop_recording_sampler(&self) {
        let mut sampler = self
            .recording_sampler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recording_frames = None;
        if let Some(worker) = sampler.take() {
            worker.stop();
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.child_pid
    }

    /// A parse failure the emulator hit on the reader thread, if any.
    ///
    /// See [`crate::terminal::emu::Emulator::fault`]. The reader has nobody to
    /// return an error to, so the failure is recorded there and reported by
    /// whichever operation runs next.
    pub fn fault(&self) -> Option<String> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.emu.fault().or_else(|| state.diagnostic_error.clone())
    }

    pub fn is_alive(&self) -> Result<bool, crate::api::TuiTestError> {
        if self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exited
            .is_some()
        {
            return Ok(false);
        }

        let exit_code = self.pty.try_wait().map_err(|error| {
            crate::api::TuiTestError::internal(format!("failed to query process status: {error}"))
        })?;
        let Some(status) = exit_code else {
            return Ok(true);
        };

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.exited.get_or_insert(status.code);
        if state.exit_signal.is_none() {
            state.exit_signal = status.signal;
        }
        Ok(false)
    }

    pub fn is_ready(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_ready()
    }

    pub fn flush_recording(&self) -> Result<(), crate::api::TuiTestError> {
        self.recorder.flush().map_err(capture_error)
    }

    pub(crate) fn snapshot_automatic_recording(
        &self,
        target_path: PathBuf,
        max_bytes: u64,
    ) -> Result<record::AutomaticRecordingSnapshot, CaptureError> {
        self.recorder.snapshot_automatic(target_path, max_bytes)
    }

    pub fn automatic_recording_enabled(&self) -> bool {
        self.recorder.automatic_enabled()
    }
}

fn resize_emulator_and_record(state: &mut TermState, recorder: &Recorder, cols: u16, rows: u16) {
    recorder.on_resize(cols, rows);
    state.emu.resize(cols, rows);
    invalidate_emulator_view(state);
    let _ = try_capture_visual_state(state, true);
}

pub(crate) fn scroll_emulator_and_record(
    state: &mut TermState,
    target: crate::api::ScrollTarget,
) -> Result<(), crate::api::TuiTestError> {
    let before = state.emu.viewport();
    state.emu.scroll(target);
    let after = state.emu.viewport();
    if let Some(error) = state.emu.fault() {
        return Err(crate::api::TuiTestError::internal(error));
    }
    if before != after {
        invalidate_emulator_view(state);
        try_capture_visual_state(state, true).map_err(crate::api::TuiTestError::internal)?;
    }
    Ok(())
}

fn invalidate_emulator_view(state: &mut TermState) {
    state.visual_revision = state.visual_revision.wrapping_add(1);
    state.screen_dirty = true;
    state.last_change = Instant::now();
    state.highlight = None;
}

pub(crate) fn capture_visual_state(state: &mut TermState, force: bool) -> u64 {
    if !state.screen_dirty {
        return state
            .screen_history
            .observe_current(elapsed_ms(state.started_at));
    }
    if !force
        && state.last_screen_sample.elapsed() < Duration::from_millis(crate::config::POLL_DELAY_MS)
    {
        return state.screen_history.current_sequence();
    }
    let rows = state.emu.viewable_rows();
    let (cols, _) = state.emu.size();
    let title = state.emu.title();
    let viewport_cursor = state.emu.viewport_cursor();
    let cursor = viewport_cursor.unwrap_or_else(|| state.emu.cursor());
    let cursor_visible = viewport_cursor.is_some();
    let cursor_shape = state.emu.cursor_shape();
    let previous_sequence = state.screen_history.current_sequence();
    let elapsed = elapsed_ms(state.started_at);
    let sequence = state.screen_history.capture(
        rows,
        cols,
        title,
        cursor,
        cursor_visible,
        cursor_shape,
        elapsed,
        crate::render::svg::RenderState::capture(state.emu.as_ref()),
    );
    if sequence != previous_sequence {
        state.last_visual_change_ms = elapsed;
        state.last_change = Instant::now();
    }
    state.screen_dirty = false;
    state.last_screen_sample = Instant::now();
    sequence
}

pub(crate) fn try_capture_visual_state(state: &mut TermState, force: bool) -> Result<u64, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture_visual_state(state, force)
    })) {
        Ok(sequence) => Ok(sequence),
        Err(payload) => {
            let message = format!(
                "terminal diagnostic capture panicked: {}",
                diagnostic_panic_message(payload.as_ref())
            );
            if state.diagnostic_error.is_none() {
                state.diagnostic_error = Some(message.clone());
            }
            Err(message)
        }
    }
}

fn diagnostic_panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic"
    }
}

fn drain_reader_and_recorder(reader: &mut Option<JoinHandle<()>>, recorder: &mut Recorder) {
    if let Some(reader) = reader.take() {
        let _ = reader.join();
    }
    recorder.shutdown();
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(feature = "recording-raster")]
        self.stop_recording_sampler();
        self.pty.close();
        drain_reader_and_recorder(&mut self.reader, &mut self.recorder);
    }
}

#[cfg(feature = "recording-raster")]
fn recording_ffmpeg_path(
    format: crate::api::RecordingFormat,
) -> Result<Option<PathBuf>, crate::api::TuiTestError> {
    recording_ffmpeg_path_with(format, shell::which)
}

#[cfg(feature = "recording-raster")]
fn recording_ffmpeg_path_with(
    format: crate::api::RecordingFormat,
    find_executable: impl FnOnce(&str) -> Option<String>,
) -> Result<Option<PathBuf>, crate::api::TuiTestError> {
    if format != crate::api::RecordingFormat::Mp4 {
        return Ok(None);
    }
    find_executable("ffmpeg")
        .map(PathBuf::from)
        .map(Some)
        .ok_or_else(|| {
            crate::api::TuiTestError::usage(
                "MP4 recording requires ffmpeg to be installed and available on PATH",
            )
        })
}

fn capture_error(error: CaptureError) -> crate::api::TuiTestError {
    match error {
        CaptureError::AlreadyActive => {
            crate::api::TuiTestError::usage("a recording is already active")
        }
        CaptureError::NotActive => crate::api::TuiTestError::usage("no recording is active"),
        CaptureError::WorkerStopped => {
            crate::api::TuiTestError::internal("recording worker stopped unexpectedly")
        }
        CaptureError::Io(message) => {
            crate::api::TuiTestError::internal(format!("recording capture failed: {message}"))
        }
    }
}

#[cfg(feature = "recording-raster")]
fn cleanup_sidecar(path: &std::path::Path, log: impl FnOnce(&str)) {
    if let Err(error) = std::fs::remove_file(path) {
        log(&format!(
            "recording exported but failed to remove sidecar {}: {error}",
            path.display()
        ));
    }
}

#[cfg(feature = "recording-raster")]
fn temporary_output_path(target: &std::path::Path) -> PathBuf {
    let mut name = target
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("recording"))
        .to_os_string();
    name.push(".tui-test.tmp");
    target.with_file_name(name)
}

#[cfg(all(feature = "recording-raster", not(windows)))]
fn replace_output(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(all(feature = "recording-raster", windows))]
fn replace_output(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::alacritty::AlacrittyEmu;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    #[cfg(feature = "recording-raster")]
    #[test]
    fn recording_sampler_skips_late_deadlines_without_drift() {
        let started = Instant::now();
        let interval = Duration::from_millis(100);
        for (now, next) in [(0, 100), (100, 200), (145, 200), (399, 400), (701, 800)] {
            assert_eq!(
                next_recording_deadline(started, started + Duration::from_millis(now), interval)
                    .unwrap(),
                started + Duration::from_millis(next)
            );
        }
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    fn recording_session() -> Session {
        Session::open(
            None,
            Some(vec![
                "sh".into(),
                "-c".into(),
                "stty -echo; printf READY; while read -r line; do printf '%s' \"$line\"; done"
                    .into(),
            ]),
            Backend::Alacritty,
            Profile::default(),
            crate::render::style::Style::default(),
            8,
            2,
            None,
            vec![],
            crate::api::Timeouts::default(),
            DiagnosticRetentionOptions::default(),
            Arc::new(Logger::disabled()),
            None,
            false,
        )
        .unwrap()
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    fn recording_options(path: &std::path::Path, fps: u8) -> ManualRecordingOptions {
        ManualRecordingOptions {
            path: path.to_string_lossy().into_owned(),
            format: Some(crate::api::RecordingFormat::Apng),
            fps: Some(fps),
            speed: None,
            idle_time_limit: None,
            zoom: Some(0.5),
            background: None,
        }
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    #[test]
    fn recording_state_changes_wait_for_a_sample_and_casts_have_no_timer_events() {
        let session = recording_session();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ready = session
                .state
                .lock()
                .unwrap()
                .emu
                .viewable_rows()
                .iter()
                .flatten()
                .map(|cell| cell.ch.as_str())
                .collect::<String>()
                .contains("READY");
            if ready {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let path = test_path("sampler-state").with_extension("apng");
        session
            .start_recording(recording_options(&path, 1))
            .unwrap();
        {
            let mut state = session.state.lock().unwrap();
            let revision = state.recording_frames.as_ref().unwrap().revision();
            state.emu.process(b"0\r\n1\r\n2\r\n3\r\n4");
            invalidate_emulator_view(&mut state);
            scroll_emulator_and_record(&mut state, crate::api::ScrollTarget::Top).unwrap();
            resize_emulator_and_record(&mut state, &session.recorder, 10, 3);
            assert_eq!(
                state.recording_frames.as_ref().unwrap().revision(),
                revision
            );
            assert_ne!(state.visual_revision, revision);
        }
        session.stop_recording().unwrap();
        std::fs::remove_file(&path).unwrap();

        let path = path.with_extension("cast");
        let mut options = recording_options(&path, 255);
        options.format = Some(crate::api::RecordingFormat::Cast);
        options.zoom = None;
        session.start_recording(options).unwrap();
        assert!(session.recording_sampler.lock().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(50));
        session.stop_recording().unwrap();
        let cast = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            cast.lines().count(),
            2,
            "only the header and initial seed should exist: {cast}"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    #[test]
    fn recording_stop_uses_elapsed_time_before_the_first_tick() {
        let session = recording_session();
        let path = test_path("sampler-duration").with_extension("apng");
        let mut options = recording_options(&path, 1);
        options.speed = Some(2.0);
        let before_start = Instant::now();
        session.start_recording(options).unwrap();
        let after_start = Instant::now();
        std::thread::sleep(Duration::from_millis(100));
        let before_stop = Instant::now();
        session.stop_recording().unwrap();
        let reader = png::Decoder::new(std::fs::File::open(&path).unwrap())
            .read_info()
            .unwrap();
        assert_eq!(reader.info().animation_control.unwrap().num_frames, 1);
        let frame = reader.info().frame_control.unwrap();
        let duration =
            Duration::from_secs_f64(f64::from(frame.delay_num) / f64::from(frame.delay_den));
        let tolerance = Duration::from_millis(10);
        assert!(duration + tolerance >= before_stop.duration_since(after_start).div_f64(2.0));
        assert!(duration <= before_stop.duration_since(before_start).div_f64(2.0) + tolerance);
        assert!(session.recording_sampler.lock().unwrap().is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    #[test]
    fn recording_sampler_cleans_up_failed_starts_exports_and_close() {
        let session = recording_session();
        let path = test_path("sampler-lifecycle").with_extension("apng");
        let blocked = test_path("sampler-parent");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert!(session
            .start_recording(recording_options(&blocked.join("out.apng"), 1))
            .is_err());
        assert!(session.recording_sampler.lock().unwrap().is_none());
        assert!(session.state.lock().unwrap().recording_frames.is_none());
        std::fs::remove_file(blocked).unwrap();

        session
            .start_recording(recording_options(&path, 1))
            .unwrap();
        assert!(session
            .start_recording(recording_options(&path, 1))
            .is_err());
        assert!(session.recording_sampler.lock().unwrap().is_some());
        assert!(session.state.lock().unwrap().recording_frames.is_some());
        session.stop_recording().unwrap();
        assert!(session.recording_sampler.lock().unwrap().is_none());
        assert!(session.state.lock().unwrap().recording_frames.is_none());
        assert!(session.stop_recording().is_err());
        std::fs::remove_file(&path).unwrap();

        std::fs::create_dir(&path).unwrap();
        session
            .start_recording(recording_options(&path, 1))
            .unwrap();
        assert!(session
            .stop_recording()
            .unwrap_err()
            .message
            .contains("captured cast retained"));
        assert!(session.recording_sampler.lock().unwrap().is_none());
        assert!(session.state.lock().unwrap().recording_frames.is_none());
        assert!(record::sidecar_path(&path).exists());
        std::fs::remove_file(record::sidecar_path(&path)).unwrap();
        std::fs::remove_dir(&path).unwrap();

        session
            .start_recording(recording_options(&path, 1))
            .unwrap();
        let started = Instant::now();
        session.kill();
        assert!(
            started.elapsed() < Duration::from_millis(750),
            "close waited for a one-second tick"
        );
        assert!(session.recording_sampler.lock().unwrap().is_none());
        assert!(session.state.lock().unwrap().recording_frames.is_none());
        assert!(session
            .start_recording(recording_options(&path, 1))
            .is_err());
        drop(session);
        std::fs::remove_file(record::sidecar_path(&path)).unwrap();
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    #[test]
    fn recording_stop_races_with_output_and_leaves_no_sampler() {
        let session = recording_session();
        let path = test_path("sampler-stop-race").with_extension("apng");
        session
            .start_recording(recording_options(&path, 255))
            .unwrap();
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                for _ in 0..100 {
                    session.write(b"output\n").unwrap();
                }
            });
            session.stop_recording().unwrap();
            writer.join().unwrap();
        });
        assert!(session.recording_sampler.lock().unwrap().is_none());
        assert!(session.state.lock().unwrap().recording_frames.is_none());
        drop(session);
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(all(unix, feature = "recording-raster"))]
    #[test]
    fn recording_drop_wakes_sampler_without_waiting_for_tick() {
        let session = recording_session();
        let path = test_path("sampler-drop").with_extension("apng");
        session
            .start_recording(recording_options(&path, 1))
            .unwrap();
        let state = Arc::downgrade(&session.state);
        let started = Instant::now();
        drop(session);
        while state.upgrade().is_some() && started.elapsed() < Duration::from_millis(750) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            state.upgrade().is_none(),
            "a worker retained the terminal after drop"
        );
        std::fs::remove_file(record::sidecar_path(&path)).unwrap();
    }

    #[test]
    fn resize_is_queued_after_old_output_and_starts_a_new_idle_period() {
        let path = test_path("resize-order");
        let mut recorder = Recorder::create(
            Some(path.clone()),
            1,
            1,
            &[],
            false,
            Arc::new(Logger::disabled()),
        )
        .unwrap();
        let started_at = Instant::now();
        let state = Arc::new(Mutex::new(TermState {
            emu: Box::new(AlacrittyEmu::new(1, 1, &Profile::default())),
            profile: Profile::default(),
            style: crate::render::style::Style::default(),
            #[cfg(feature = "recording-raster")]
            recording_frames: None,
            tracker: CommandTracker::new(),
            mouse_mode: MouseModeTracker::new(),
            observed_clipboard_revision: 0,
            started_at,
            visual_revision: 0,
            screen_history: ScreenHistory::new(crate::diagnostics::DEFAULT_SCREEN_HISTORY_LIMIT),
            screen_dirty: true,
            last_screen_sample: started_at,
            last_visual_change_ms: 0,
            diagnostic_error: None,
            last_change: Instant::now(),
            awaiting_start: None,
            exited: None,
            exit_signal: None,
            exit_error: None,
            highlight: None,
        }));
        let capture = recorder.capture();
        let output_state = Arc::clone(&state);
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let output = std::thread::spawn(move || {
            let _state = output_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            capture.on_data(b"old-size-output");
        });
        locked_rx.recv().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            release_tx.send(()).unwrap();
        });

        let resize_started = Instant::now();
        resize_emulator_and_record(&mut state.lock().unwrap(), &recorder, 2, 1);
        assert!(
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last_change
                >= resize_started,
            "wait idle must not reuse the quiet period from before a resize"
        );
        release.join().unwrap();
        output.join().unwrap();
        recorder.capture().on_data(b"new-size-output");
        recorder.flush().unwrap();

        let cast = std::fs::read_to_string(&path).unwrap();
        let old = cast.find("old-size-output").unwrap();
        let resize = cast.find("\"r\",\"2x1\"").unwrap();
        let new = cast.find("new-size-output").unwrap();
        assert!(old < resize && resize < new, "{cast}");

        recorder.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "recording-raster")]
    #[test]
    fn mp4_requires_ffmpeg_before_recording_starts() {
        let error = recording_ffmpeg_path_with(crate::api::RecordingFormat::Mp4, |_| None)
            .expect_err("missing ffmpeg should reject MP4 recording");
        assert_eq!(error.kind, crate::api::ErrorKind::Usage);
        assert!(error.message.contains("ffmpeg"));

        assert_eq!(
            recording_ffmpeg_path_with(crate::api::RecordingFormat::Apng, |_| {
                panic!("non-MP4 formats must not look for ffmpeg")
            })
            .unwrap(),
            None
        );
    }

    #[test]
    fn shutdown_drains_reader_tail_before_stopping_recorder() {
        let path = test_path("reader-tail");
        let mut recorder = Recorder::create(
            Some(path.clone()),
            1,
            1,
            &[],
            false,
            Arc::new(Logger::disabled()),
        )
        .unwrap();
        let capture = recorder.capture();
        let mut reader = Some(std::thread::spawn(move || {
            capture.on_data(b"reader-tail-marker");
        }));

        drain_reader_and_recorder(&mut reader, &mut recorder);

        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("reader-tail-marker"));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "recording-raster")]
    #[test]
    fn sidecar_cleanup_failure_does_not_fail_export() {
        let missing = test_path("missing-sidecar");
        let mut logged = None;
        cleanup_sidecar(&missing, |message| logged = Some(message.to_string()));
        assert!(logged
            .as_deref()
            .is_some_and(|message| message.contains("failed to remove sidecar")));
    }

    fn test_path(label: &str) -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("session-tests");
        std::fs::create_dir_all(&root).unwrap();
        root.join(format!(
            "{label}-{}-{}.cast",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
