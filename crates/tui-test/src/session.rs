//! A single managed terminal session: a PTY feeding an emulator and command
//! tracker, with a background reader thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::logger::Logger;
use crate::profile::Profile;
use crate::record::Recorder;
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
        let recorder = Recorder::create(
            recording_path,
            cols,
            rows,
            &rec_env,
            logger.clone(),
        );

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
}
