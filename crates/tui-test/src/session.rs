//! A single managed terminal session: a PTY feeding an emulator and command
//! tracker, with a background reader thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::diagnostics::{elapsed_ms, DiagnosticRetentionOptions, ScreenHistory};
use crate::event::BellTracker;
use crate::input::mouse;
use crate::logger::Logger;
use crate::profile::Profile;
use crate::record::{self, CaptureError, Recorder, StartRecording};
#[cfg(feature = "recording-raster")]
use crate::render::raster::GridRenderer;
use crate::shell::{self, Shell};
use crate::terminal::backend::Backend;
use crate::terminal::emu::{Emulator, MouseMode, MouseModeTracker, TerminalMode};
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
    logger: Arc<Logger>,
    reader: Option<JoinHandle<()>>,
    _process_watcher: JoinHandle<()>,
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
                            #[cfg(feature = "recording-raster")]
                            capture_recording_frame(&mut st);
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
        let idle_time_limit = idle_time_limit.unwrap_or(5.0);
        if !idle_time_limit.is_finite() || idle_time_limit < 0.0 {
            return Err(crate::api::TuiTestError::usage(
                "idle time limit must be a finite, non-negative number of seconds",
            ));
        }
        let idle_time_limit = std::time::Duration::try_from_secs_f64(idle_time_limit)
            .map_err(|_| crate::api::TuiTestError::usage("idle time limit is too large"))?;
        std::time::Duration::try_from_secs_f64(idle_time_limit.as_secs_f64() / speed)
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
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                ..record::frames::TimelineOptions::default()
            },
            #[cfg(feature = "recording-raster")]
            ffmpeg_path,
        };
        #[cfg(feature = "recording-raster")]
        let frames = if format == crate::api::RecordingFormat::Cast {
            None
        } else {
            Some(
                record::frames::FrameCapture::new(
                    state.emu.as_ref(),
                    request.timeline.clone(),
                    Instant::now(),
                )
                .map_err(|error| crate::api::TuiTestError::internal(error.to_string()))?,
            )
        };
        self.recorder.start(request).map_err(capture_error)?;
        #[cfg(feature = "recording-raster")]
        {
            let mut state = state;
            state.recording_frames = frames;
        }
        Ok(())
    }

    pub fn stop_recording(&self) -> Result<String, crate::api::TuiTestError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(feature = "recording-raster")]
        let mut state = state;
        #[cfg(feature = "recording-raster")]
        let frames = state.recording_frames.take();
        let stopped = self.recorder.stop().map_err(capture_error)?;
        drop(state);
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
                let frames = frames
                    .ok_or_else(|| anyhow::anyhow!("recording frames are unavailable"))?
                    .finish()?;
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
        self.pty.close();
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

    /// Send one mouse report after checks against the terminal modes.
    ///
    /// The method takes the viewport/input gate before sampling and holds it
    /// through the completed write, then reads the tracking level, the
    /// encoding flag, the viewport, and the screen size from one terminal
    /// state lock. The state lock is released before the blocking PTY write,
    /// but the gate stays held, so a concurrent scroll either waits for this
    /// report or runs first and makes the check below observe history. It
    /// validates the coordinates and the mode before it writes any byte. A
    /// press or a release needs click tracking or better, including X10
    /// (mode 9) tracking, which reports presses only and refuses releases.
    /// A move with a pressed button needs drag tracking or motion tracking.
    /// A move with no pressed button needs motion tracking. A tracking
    /// selector this crate cannot send reports for, such as highlight
    /// tracking (mode 1001), behaves as if tracking were off. Tracking
    /// without SGR encoding (mode 1006)
    /// cannot receive the SGR bytes this method sends, so the method returns
    /// `Unsupported` and writes nothing in that case.
    ///
    /// Mouse input that targets a history viewport fails. The caller scrolls
    /// to the bottom first and sends again. The raw input and legacy mouse
    /// methods keep their behavior; this method only adds the checked path.
    pub fn mouse_checked(&self, action: CheckedMouseAction) -> Result<(), CheckedInputError> {
        let _input = self
            .viewport_input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (cols, rows, viewport_offset, tracking, relayable, reports_releases) = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (cols, rows) = state.emu.size();
            (
                cols,
                rows,
                state.emu.viewport().offset,
                state.mouse_mode.mode(),
                state.mouse_mode.relayable(),
                state.mouse_mode.reports_releases(),
            )
        };
        let (x, y, minimum) = match action {
            CheckedMouseAction::Press { x, y, .. } => (x, y, MouseMode::Click),
            CheckedMouseAction::Release { x, y, .. } => (x, y, MouseMode::Click),
            CheckedMouseAction::Drag { x, y, .. } => (x, y, MouseMode::Drag),
            CheckedMouseAction::Motion { x, y } => (x, y, MouseMode::Motion),
        };
        if viewport_offset != 0 {
            return Err(CheckedInputError::Rejected(
                "mouse input cannot target a history viewport; scroll to the bottom first"
                    .to_string(),
            ));
        }
        if x >= cols || y >= rows {
            return Err(CheckedInputError::Rejected(format!(
                "mouse coordinates ({x}, {y}) are outside the active screen ({cols}x{rows})"
            )));
        }
        if tracking == MouseMode::None {
            return Err(CheckedInputError::Unsupported(
                "the application did not enable mouse tracking".to_string(),
            ));
        }
        if matches!(action, CheckedMouseAction::Release { .. }) && !reports_releases {
            return Err(CheckedInputError::Unsupported(
                "X10 (mode 9) mouse tracking reports button presses only; releases are never reported"
                    .to_string(),
            ));
        }
        let sufficient = match minimum {
            MouseMode::Click => true,
            MouseMode::Drag => matches!(tracking, MouseMode::Drag | MouseMode::Motion),
            MouseMode::Motion => tracking == MouseMode::Motion,
            // Unreachable in practice: `minimum` derives from the action and
            // is never `None`. The arm stays so the match is exhaustive over
            // `MouseMode`.
            MouseMode::None => false,
        };
        if !sufficient {
            return Err(CheckedInputError::Unsupported(format!(
                "the application enabled {} tracking, which does not accept this report",
                tracking.name()
            )));
        }
        if relayable != tracking {
            return Err(CheckedInputError::Unsupported(
                "the application did not select SGR (mode 1006) mouse encoding".to_string(),
            ));
        }
        let bytes = match action {
            CheckedMouseAction::Press { x, y, options } => mouse::down(x, y, options),
            CheckedMouseAction::Release { x, y, options } => mouse::up(x, y, options),
            CheckedMouseAction::Drag { x, y, options } => mouse::drag_motion(x, y, options),
            CheckedMouseAction::Motion { x, y } => mouse::motion(x, y),
        };
        self.write(bytes.as_bytes())
            .map_err(|error| CheckedInputError::Failed(error.to_string()))?;
        Ok(())
    }

    /// Send text as a paste through the ordered input write path.
    ///
    /// The method takes the viewport/input gate before sampling and holds it
    /// through the completed write, then scrolls to the bottom like other
    /// keyboard input before writing anything, so the paste always targets
    /// the live screen. The state lock is released before the blocking PTY
    /// write. When the application enabled bracketed-paste mode, the method
    /// wraps the text in `ESC [ 200 ~` and `ESC [ 201 ~`. When the mode is
    /// off, the method sends the text without markers. A text that contains
    /// the terminator `ESC [ 201 ~` would end a bracketed payload before its
    /// last character, so the method rejects such a text while markers are
    /// on and writes nothing. With markers off, the terminator has no
    /// framing effect and travels unchanged. Raw bytes stay available through
    /// [`Session::write`] for deliberate protocol tests.
    ///
    /// An empty text with markers on still sends the empty framing pair
    /// (`ESC [ 200 ~` followed by `ESC [ 201 ~`); with markers off an empty
    /// text writes nothing and scrolls nothing.
    pub fn paste(&self, text: &str) -> Result<(), CheckedInputError> {
        let _input = self
            .viewport_input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bracketed = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.emu.mode(TerminalMode::BracketedPaste)
        };
        if bracketed {
            if text.contains(BRACKETED_PASTE_END) {
                return Err(CheckedInputError::Rejected(
                    "paste text contains the bracketed-paste terminator".to_string(),
                ));
            }
            let mut framed = String::with_capacity(
                BRACKETED_PASTE_START.len() + text.len() + BRACKETED_PASTE_END.len(),
            );
            framed.push_str(BRACKETED_PASTE_START);
            framed.push_str(text);
            framed.push_str(BRACKETED_PASTE_END);
            scroll_to_bottom(self)?;
            self.write(framed.as_bytes())
                .map_err(|error| CheckedInputError::Failed(error.to_string()))?;
        } else if !text.is_empty() {
            scroll_to_bottom(self)?;
            self.write(text.as_bytes())
                .map_err(|error| CheckedInputError::Failed(error.to_string()))?;
        }
        Ok(())
    }

    /// Send a focus report through the ordered input write path.
    ///
    /// The method takes the viewport/input gate before sampling and holds it
    /// through the completed write. When the application enabled focus
    /// events (`CSI ? 1004 h`), the method sends `ESC [ I` for gained focus
    /// and `ESC [ O` for lost focus. When the application did not enable
    /// focus events, the method returns `Unsupported` and writes nothing.
    pub fn focus(&self, focused: bool) -> Result<(), CheckedInputError> {
        let _input = self
            .viewport_input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let enabled = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.emu.mode(TerminalMode::FocusEvents)
        };
        if !enabled {
            return Err(CheckedInputError::Unsupported(
                "the application did not enable focus events (CSI ? 1004 h)".to_string(),
            ));
        }
        let report = if focused { FOCUS_IN } else { FOCUS_OUT };
        self.write(report.as_bytes())
            .map_err(|error| CheckedInputError::Failed(error.to_string()))?;
        Ok(())
    }
}

/// One mouse report for [`Session::mouse_checked`].
///
/// Coordinates are 0-based cells relative to the active screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckedMouseAction {
    /// A button press.
    Press {
        x: u16,
        y: u16,
        options: crate::api::MouseOptions,
    },
    /// A button release.
    Release {
        x: u16,
        y: u16,
        options: crate::api::MouseOptions,
    },
    /// Movement with a pressed button.
    Drag {
        x: u16,
        y: u16,
        options: crate::api::MouseOptions,
    },
    /// Movement with no pressed button.
    Motion { x: u16, y: u16 },
}

/// Failure from [`Session::mouse_checked`], [`Session::paste`], or
/// [`Session::focus`].
///
/// `Unsupported` and `Rejected` mean the method wrote nothing.
/// `Failed` covers everything outside validation: no open session, a
/// recorded emulator fault, or a PTY write error after the checks passed.
/// Each message preserves the source error it maps from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckedInputError {
    /// The application did not enable the mode the input needs, or it
    /// selected an encoding this method cannot send.
    Unsupported(String),
    /// The input is invalid: coordinates outside the active screen, a
    /// history viewport target, or a payload that would corrupt its framing.
    Rejected(String),
    /// No session is open, the emulator recorded a fault, or the PTY write
    /// failed after the mode checks passed.
    Failed(String),
}

impl std::fmt::Display for CheckedInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Rejected(message) | Self::Failed(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for CheckedInputError {}

const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";
const FOCUS_IN: &str = "\x1b[I";
const FOCUS_OUT: &str = "\x1b[O";

fn resize_emulator_and_record(state: &mut TermState, recorder: &Recorder, cols: u16, rows: u16) {
    recorder.on_resize(cols, rows);
    state.emu.resize(cols, rows);
    invalidate_emulator_view(state);
    let _ = try_capture_visual_state(state, true);
}

/// Scroll checked keyboard-style input back to the live screen.
///
/// The caller must already hold `viewport_input`: this only takes the
/// terminal-state lock, so the gate stays held across the scroll and the
/// write that follows. Errors preserve the source [`crate::api::TuiTestError`]
/// message.
fn scroll_to_bottom(session: &Session) -> Result<(), CheckedInputError> {
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    scroll_emulator_and_record(&mut state, crate::api::ScrollTarget::Bottom)
        .map_err(|error| CheckedInputError::Failed(error.to_string()))
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
    #[cfg(feature = "recording-raster")]
    capture_recording_frame(state);
}

#[cfg(feature = "recording-raster")]
fn capture_recording_frame(state: &mut TermState) {
    if let Some(frames) = state.recording_frames.as_mut() {
        frames.capture(state.emu.as_ref(), Instant::now());
    }
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
