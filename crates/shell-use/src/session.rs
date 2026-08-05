//! A single managed terminal session: a PTY feeding an emulator and command
//! tracker, with a background reader thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::event::BellTracker;
use crate::logger::Logger;
use crate::shell::{self, Shell};
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::emu::Emulator;
use crate::terminal::integration::CommandTracker;
use crate::terminal::pty::{Pty, SpawnOptions};
use crate::trace::recorder::Recorder;

pub struct TermState {
    pub emu: Box<dyn Emulator>,
    /// Shell-integration state, derived from the raw PTY stream rather than
    /// the emulator, so it is identical across backends.
    pub tracker: CommandTracker,
    pub last_change: Instant,
    pub awaiting_start: Option<u64>,
    pub exited: Option<i32>,
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
    pub(crate) bells: BellTracker,
    recorder: Arc<Mutex<Recorder>>,
    logger: Arc<Logger>,
    _reader: JoinHandle<()>,
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
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        timeouts: crate::api::Timeouts,
        logger: Arc<Logger>,
        recording_path: PathBuf,
    ) -> anyhow::Result<Self> {
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

        let bells = BellTracker::default();
        let state = Arc::new(Mutex::new(TermState {
            emu: Box::new(AlacrittyEmu::with_bell_tracker(
                cols,
                rows,
                5_000,
                bells.clone(),
            )),
            tracker: CommandTracker::new(),
            last_change: Instant::now(),
            awaiting_start: None,
            exited: None,
        }));
        let pty = Arc::new(Mutex::new(pty));
        let cancelled = Arc::new(AtomicBool::new(false));

        let mut rec_env: Vec<(&str, String)> = vec![("TERM", "xterm-256color".to_string())];
        if let Some(sh) = shell {
            rec_env.push(("SHELL", sh.as_str().to_string()));
        }
        let recorder = Arc::new(Mutex::new(Recorder::create(
            &recording_path,
            cols,
            rows,
            &rec_env,
        )));

        let reader_state = state.clone();
        let reader_pty = pty.clone();
        let reader_logger = logger.clone();
        let reader_recorder = recorder.clone();
        let mut reader = reader;
        let handle = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        reader_logger.read(&buf[..n]);
                        reader_recorder
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .on_data(&buf[..n]);
                        let pending = {
                            let mut st = reader_state
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            st.emu.process(&buf[..n]);
                            st.tracker.feed(&buf[..n]);
                            st.last_change = Instant::now();
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
            let code = reader_pty.lock().ok().and_then(|mut p| p.try_wait());
            reader_logger.event(&format!("pty exited code={:?}", code));
            let mut st = reader_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            st.exited = Some(code.unwrap_or(0));
            st.last_change = Instant::now();
        });

        logger.event(&format!(
            "session open shell={:?} program={:?} {}x{}",
            shell, program, cols, rows
        ));

        Ok(Session {
            shell,
            cols,
            rows,
            timeouts,
            pty,
            state,
            cancelled,
            bells,
            recorder,
            logger,
            _reader: handle,
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
        self.recorder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .on_resize(cols, rows);
        self.cols = cols;
        self.rows = rows;
        let mut st = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
