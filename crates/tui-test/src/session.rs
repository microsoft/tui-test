//! A single managed terminal session: a PTY feeding an emulator and command
//! tracker, with a background reader thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::logger::Logger;
use crate::profile::Profile;
use crate::record::{self, CaptureError, Recorder, StartRecording};
#[cfg(feature = "recording-raster")]
use crate::render::raster::GridRenderer;
use crate::shell::{self, Shell};
use crate::terminal::backend::Backend;
use crate::terminal::emu::Emulator;
use crate::terminal::integration::CommandTracker;
use crate::terminal::pty::{Pty, SpawnOptions};

pub struct TermState {
    pub emu: Box<dyn Emulator>,
    /// Shell-integration state, derived from the raw PTY stream rather than
    /// the emulator, so it is identical across backends.
    pub tracker: CommandTracker,
    pub last_change: Instant,
    pub awaiting_start: Option<u64>,
    pub exited: Option<i32>,
    pub exit_error: Option<String>,
}

pub struct Session {
    pub shell: Option<Shell>,
    pub cols: u16,
    pub rows: u16,
    /// Per-class timeout defaults for the lifetime of this session.
    pub timeouts: crate::api::Timeouts,
    pub pty: Arc<Mutex<Pty>>,
    pub state: Arc<Mutex<TermState>>,
    pub cancelled: Arc<AtomicBool>,
    recorder: Recorder,
    logger: Arc<Logger>,
    _reader: JoinHandle<()>,
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
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        timeouts: crate::api::Timeouts,
        logger: Arc<Logger>,
        recording_path: PathBuf,
    ) -> anyhow::Result<Self> {
        let emu = backend.build(cols, rows, &profile)?;

        let (pty, reader) = if let Some(program) = &program {
            let (target, args) = program
                .split_first()
                .ok_or_else(|| anyhow::anyhow!("empty program"))?;
            let opts = SpawnOptions {
                cols,
                rows,
                cwd,
                env,
            };
            Pty::spawn(target, args, &opts)?
        } else {
            let sh = shell.unwrap_or_else(shell::default_shell);
            let mut launch = shell::shell_launch(sh)?;
            launch.env.extend(env);
            Pty::spawn_launch(&launch, cols, rows, cwd)?
        };

        let state = Arc::new(Mutex::new(TermState {
            emu,
            tracker: CommandTracker::new(),
            last_change: Instant::now(),
            awaiting_start: None,
            exited: None,
            exit_error: None,
        }));
        let pty = Arc::new(Mutex::new(pty));
        let cancelled = Arc::new(AtomicBool::new(false));

        let mut rec_env = vec![("TERM".to_string(), "xterm-256color".to_string())];
        if let Some(sh) = shell {
            rec_env.push(("SHELL".to_string(), sh.as_str().to_string()));
        }
        let recorder = Recorder::create(recording_path, cols, rows, &rec_env, logger.clone());

        let reader_state = state.clone();
        let reader_pty = pty.clone();
        let reader_logger = logger.clone();
        let reader_recorder = recorder.capture();
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
                            st.last_change = Instant::now();
                            reader_recorder.on_data(&buf[..n]);
                            st.emu.take_pending_writes()
                        };
                        if !pending.is_empty() {
                            reader_logger.reply(&pending);
                            if let Ok(mut p) = reader_pty.lock() {
                                let _ = p.write(&pending);
                            }
                        }
                    }
                }
            }
        });

        let watcher_state = state.clone();
        let watcher_pty = pty.clone();
        let watcher_logger = logger.clone();
        let process_watcher = std::thread::spawn(move || loop {
            let status = {
                let mut pty = watcher_pty
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pty.try_wait()
            };
            match status {
                Ok(Some(code)) => {
                    watcher_logger.event(&format!("process exited code={code}"));
                    let mut st = watcher_state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    st.exited = Some(code);
                    st.last_change = Instant::now();
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
                    st.last_change = Instant::now();
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
            cols,
            rows,
            timeouts,
            pty,
            state,
            cancelled,
            recorder,
            logger,
            _reader: reader_handle,
            _process_watcher: process_watcher,
        })
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
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
        self.pty
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write(data)?;
        Ok(())
    }

    pub fn submit(&self, data: &str) -> anyhow::Result<()> {
        let mut bytes = data.as_bytes().to_vec();
        let ret = self.shell.map(|s| s.return_char()).unwrap_or("\r");
        bytes.extend_from_slice(ret.as_bytes());
        self.write(&bytes)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.logger.event(&format!("resize {cols}x{rows}"));
        self.cols = cols;
        self.rows = rows;
        let mut st = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.recorder.on_resize(cols, rows);
        st.emu.resize(cols, rows);
        drop(st);
        self.pty
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .resize(cols, rows)?;
        Ok(())
    }

    pub fn start_recording(
        &self,
        path: String,
        format: Option<crate::api::RecordingFormat>,
        fps: Option<u8>,
        speed: Option<f64>,
        idle_time_limit: Option<f64>,
    ) -> Result<(), crate::api::ShellUseError> {
        if path.trim().is_empty() {
            return Err(crate::api::ShellUseError::usage(
                "recording path must not be empty",
            ));
        }
        let format = format
            .or_else(|| crate::api::RecordingFormat::infer(&path))
            .ok_or_else(|| {
                crate::api::ShellUseError::usage(
                    "cannot infer recording format; use .png, .apng, .gif, or .cast",
                )
            })?;
        #[cfg(not(feature = "recording-raster"))]
        if format != crate::api::RecordingFormat::Cast {
            return Err(crate::api::ShellUseError::usage(
                "APNG and GIF recording require the shell-use 'recording-raster' feature",
            ));
        }
        let fps = fps.unwrap_or(30);
        if fps == 0 {
            return Err(crate::api::ShellUseError::usage(
                "recording fps must be greater than zero",
            ));
        }
        let speed = speed.unwrap_or(1.0);
        if !speed.is_finite() || speed <= 0.0 {
            return Err(crate::api::ShellUseError::usage(
                "recording speed must be finite and greater than zero",
            ));
        }
        let idle_time_limit = idle_time_limit.unwrap_or(5.0);
        if !idle_time_limit.is_finite() || idle_time_limit < 0.0 {
            return Err(crate::api::ShellUseError::usage(
                "idle time limit must be a finite, non-negative number of seconds",
            ));
        }
        let idle_time_limit = std::time::Duration::try_from_secs_f64(idle_time_limit)
            .map_err(|_| crate::api::ShellUseError::usage("idle time limit is too large"))?;
        std::time::Duration::try_from_secs_f64(idle_time_limit.as_secs_f64() / speed)
            .map_err(|_| crate::api::ShellUseError::usage("recording speed is too small"))?;

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
        let initial_output =
            record::cast::snapshot_to_ansi(&state.emu.viewable_rows(), cols, state.emu.cursor());
        let result = self.recorder.start(StartRecording {
            target_path,
            capture_path,
            format,
            cols,
            rows,
            env,
            initial_output,
            #[cfg(feature = "recording-raster")]
            timeline: record::frames::TimelineOptions {
                fps,
                speed,
                idle_time_limit,
                ..record::frames::TimelineOptions::default()
            },
        });
        drop(state);
        result.map_err(capture_error)
    }

    pub fn stop_recording(&self) -> Result<String, crate::api::ShellUseError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stopped = self.recorder.stop().map_err(capture_error)?;
        drop(state);
        if stopped.format == crate::api::RecordingFormat::Cast {
            return Ok(stopped.target_path.to_string_lossy().into_owned());
        }

        #[cfg(not(feature = "recording-raster"))]
        return Err(crate::api::ShellUseError::internal(
            "raster recording was started without the 'recording-raster' feature",
        ));

        #[cfg(feature = "recording-raster")]
        {
            let temporary_path = temporary_output_path(&stopped.target_path);
            let result = (|| -> anyhow::Result<()> {
                let cast = record::cast::read(&stopped.capture_path)?;
                let frames = record::frames::from_cast(cast, &stopped.timeline)?;
                let scale = match stopped.format {
                    crate::api::RecordingFormat::Apng | crate::api::RecordingFormat::Gif => 2,
                    crate::api::RecordingFormat::Cast => unreachable!(),
                };
                let mut renderer =
                    GridRenderer::with_scale(stopped.cols, usize::from(stopped.rows), scale);
                crate::render::encode::encode(
                    &temporary_path,
                    stopped.format,
                    &frames,
                    &mut renderer,
                    stopped.cols,
                )?;
                replace_output(&temporary_path, &stopped.target_path)?;
                std::fs::remove_file(&stopped.capture_path)?;
                Ok(())
            })();

            match result {
                Ok(()) => Ok(stopped.target_path.to_string_lossy().into_owned()),
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary_path);
                    Err(crate::api::ShellUseError::internal(format!(
                        "failed to export recording; captured cast retained at {}: {error}",
                        stopped.capture_path.display()
                    )))
                }
            }
        }
    }

    pub fn kill(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.pty
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .kill();
    }

    pub fn pid(&self) -> Option<u32> {
        self.pty
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pid()
    }

    pub fn flush_recording(&self) -> Result<(), crate::api::ShellUseError> {
        self.recorder.flush().map_err(capture_error)
    }
}

fn capture_error(error: CaptureError) -> crate::api::ShellUseError {
    match error {
        CaptureError::AlreadyActive => {
            crate::api::ShellUseError::usage("a recording is already active")
        }
        CaptureError::NotActive => crate::api::ShellUseError::usage("no recording is active"),
        CaptureError::WorkerStopped => {
            crate::api::ShellUseError::internal("recording worker stopped unexpectedly")
        }
        CaptureError::Io(message) => {
            crate::api::ShellUseError::internal(format!("recording capture failed: {message}"))
        }
    }
}

#[cfg(feature = "recording-raster")]
fn temporary_output_path(target: &std::path::Path) -> PathBuf {
    let mut name = target
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("recording"))
        .to_os_string();
    name.push(".shell-use.tmp");
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
    // Safety: both buffers are NUL-terminated and remain alive for the call.
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

#[cfg(all(test, feature = "recording-raster"))]
mod tests {
    use super::*;

    #[test]
    fn failed_replacement_preserves_the_existing_output() {
        let root =
            std::env::temp_dir().join(format!("shell-use-replace-output-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("recording.gif");
        let missing = root.join("missing.tmp");
        std::fs::write(&target, b"previous recording").unwrap();

        assert!(replace_output(&missing, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous recording");

        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
