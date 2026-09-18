//! PTY spawning and control via `portable-pty`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};

use crate::api::TuiTestError;
use crate::shell::Launch;

pub struct Pty {
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    // Process control must never wait for a blocked input write.
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

pub(crate) fn validate_size(cols: u16, rows: u16) -> Result<(), TuiTestError> {
    if cols == 0 || rows == 0 {
        return Err(TuiTestError::usage(
            "terminal columns and rows must be greater than zero",
        ));
    }
    #[cfg(windows)]
    if cols > i16::MAX as u16 || rows > i16::MAX as u16 {
        return Err(TuiTestError::usage(
            "terminal columns and rows must not exceed 32767 on Windows",
        ));
    }
    Ok(())
}

pub(crate) fn validate_cwd(cwd: &Path) -> Result<(), TuiTestError> {
    let metadata = cwd.metadata().map_err(|error| {
        TuiTestError::usage(format!("invalid session cwd {}: {error}", cwd.display()))
    })?;
    if !metadata.is_dir() {
        return Err(TuiTestError::usage(format!(
            "session cwd is not a directory: {}",
            cwd.display()
        )));
    }
    Ok(())
}

pub struct SpawnOptions {
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessExit {
    pub(crate) code: i32,
    pub(crate) signal: Option<String>,
}

impl From<ExitStatus> for ProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.exit_code() as i32,
            signal: status.signal().map(str::to_string),
        }
    }
}

impl Pty {
    /// Spawn a program in a fresh PTY, returning the controller and a reader
    /// for its output.
    pub fn spawn(
        target: &str,
        args: &[String],
        opts: &SpawnOptions,
    ) -> anyhow::Result<(Pty, Box<dyn Read + Send>)> {
        Self::spawn_with_cwd(target, args, opts, opts.cwd.as_deref().map(Path::new))
    }

    pub(crate) fn spawn_with_cwd(
        target: &str,
        args: &[String],
        opts: &SpawnOptions,
        cwd: Option<&Path>,
    ) -> anyhow::Result<(Pty, Box<dyn Read + Send>)> {
        validate_size(opts.cols, opts.rows)?;
        if let Some(cwd) = cwd {
            validate_cwd(cwd)?;
        }
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: opts.rows,
            cols: opts.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(target);
        for arg in args {
            cmd.arg(arg);
        }
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        } else if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        Ok((
            Pty {
                master: Mutex::new(Some(pair.master)),
                writer: Mutex::new(Some(writer)),
                child: Mutex::new(child),
            },
            reader,
        ))
    }

    /// Spawn a shell using its computed launch configuration.
    pub fn spawn_launch(
        launch: &Launch,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
    ) -> anyhow::Result<(Pty, Box<dyn Read + Send>)> {
        Self::spawn_launch_with_cwd(launch, cols, rows, cwd.map(PathBuf::from))
    }

    pub(crate) fn spawn_launch_with_cwd(
        launch: &Launch,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> anyhow::Result<(Pty, Box<dyn Read + Send>)> {
        let opts = SpawnOptions {
            cols,
            rows,
            cwd: None,
            env: launch.env.clone(),
        };
        Self::spawn_with_cwd(&launch.target, &launch.args, &opts, cwd.as_deref())
    }

    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let mut guard = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let writer = guard
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "PTY is closed"))?;
        writer.write_all(data)?;
        writer.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        validate_size(cols, rows)?;
        let guard = self
            .master
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let master = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("PTY is closed"))?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .process_id()
    }

    pub fn kill(&self) {
        {
            let mut child = self
                .child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
            }
        }
        // ConPTY can keep a full input pipe open after its child exits. Release
        // the master before waiting for a writer to finish or take its lock.
        let master = self
            .master
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(master);
    }

    pub fn close(&self) {
        self.kill();
        self.writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    /// Send a named signal. Cross-platform support is limited: INT delivers a
    /// Ctrl-C to the foreground app; TERM/KILL terminate the child.
    pub fn signal(&self, name: &str) -> anyhow::Result<()> {
        let upper = name.trim_start_matches("SIG").to_uppercase();
        match upper.as_str() {
            "INT" => self.write(b"\x03")?,
            "TERM" | "KILL" | "QUIT" => self.kill(),
            other => anyhow::bail!("unsupported signal: {other}"),
        }
        Ok(())
    }

    /// Return the process status if the child has exited.
    pub(crate) fn try_wait(&self) -> std::io::Result<Option<ProcessExit>> {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_wait()
            .map(|status| status.map(Into::into))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::ChildKiller;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct TestChild(Arc<AtomicBool>);

    impl ChildKiller for TestChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.0.store(true, Ordering::Release);
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    impl Child for TestChild {
        fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
            Ok(self
                .0
                .load(Ordering::Acquire)
                .then(|| ExitStatus::with_exit_code(1)))
        }

        fn wait(&mut self) -> std::io::Result<ExitStatus> {
            self.try_wait()?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, "test child is running")
            })
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    struct BlockingWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.entered.send(()).unwrap();
            let _ = self.release.recv_timeout(Duration::from_secs(5));
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test write released",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn process_control_does_not_wait_for_a_blocked_pty_write() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let pty = Arc::new(Pty {
            master: Mutex::new(None),
            writer: Mutex::new(Some(Box::new(BlockingWriter {
                entered: entered_tx,
                release: release_rx,
            }))),
            child: Mutex::new(Box::new(TestChild(Arc::new(AtomicBool::new(false))))),
        });
        let writer_pty = pty.clone();
        let writer = std::thread::spawn(move || writer_pty.write(b"blocked input"));
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let control_pty = pty.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let controller = std::thread::spawn(move || {
            assert_eq!(control_pty.try_wait().unwrap(), None);
            control_pty.signal("KILL").unwrap();
            finished_tx.send(control_pty.try_wait().unwrap()).unwrap();
        });
        let status = finished_rx.recv_timeout(Duration::from_secs(1));
        let _ = release_tx.send(());
        assert!(writer.join().unwrap().is_err());
        controller.join().unwrap();
        assert_eq!(
            status.expect("process status and kill must not acquire the writer lock"),
            Some(ProcessExit {
                code: 1,
                signal: None,
            })
        );
        pty.close();
    }

    #[test]
    fn unread_input_fixture() {
        if std::env::var_os("TUI_TEST_UNREAD_PTY_FIXTURE").is_none() {
            return;
        }
        #[cfg(windows)]
        unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
                fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
            }
            assert_ne!(SetConsoleMode(GetStdHandle(-10i32 as u32), 0), 0);
        }
        #[cfg(unix)]
        assert!(std::process::Command::new("stty")
            .args(["raw", "-echo"])
            .status()
            .unwrap()
            .success());
        let mut stdout = std::io::stdout();
        stdout.write_all(b"unread-input-ready\r\n").unwrap();
        stdout.flush().unwrap();
        std::thread::sleep(Duration::from_secs(15));
    }

    #[test]
    fn killing_a_native_child_unblocks_a_full_input_pipe() {
        let executable = std::env::current_exe().unwrap();
        let (pty, mut reader) = Pty::spawn(
            executable.to_str().unwrap(),
            &[
                "--exact".into(),
                "terminal::pty::tests::unread_input_fixture".into(),
                "--nocapture".into(),
            ],
            &SpawnOptions {
                cols: 80,
                rows: 24,
                cwd: None,
                env: vec![("TUI_TEST_UNREAD_PTY_FIXTURE".into(), "1".into())],
            },
        )
        .unwrap();
        let pty = Arc::new(pty);
        let (ready_tx, ready_rx) = mpsc::channel();
        let reader_pty = pty.clone();
        let reader_thread = std::thread::spawn(move || {
            use crate::terminal::emu::Emulator;

            let mut emulator = crate::terminal::alacritty::AlacrittyEmu::new(
                80,
                24,
                &crate::profile::Profile::default(),
            );
            let mut output = Vec::new();
            let mut buffer = [0; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                emulator.process(&buffer[..count]);
                let replies = emulator.take_pending_writes();
                if !replies.is_empty() {
                    let _ = reader_pty.write(&replies);
                }
                output.extend_from_slice(&buffer[..count]);
                if output
                    .windows(b"unread-input-ready".len())
                    .any(|bytes| bytes == b"unread-input-ready")
                {
                    let _ = ready_tx.send(());
                    output.clear();
                }
            }
            output
        });
        let ready = ready_rx.recv_timeout(Duration::from_secs(5));
        if let Err(error) = ready {
            pty.close();
            let output = reader_thread.join().unwrap();
            panic!(
                "native fixture did not initialize: {error}; output={:?}",
                String::from_utf8_lossy(&output)
            );
        }
        let writing = pty.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let writer_thread = std::thread::spawn(move || {
            let result = writing.write(&vec![b'x'; 8 * 1024 * 1024]);
            let _ = finished_tx.send(result);
        });
        std::thread::sleep(Duration::from_millis(100));
        let blocked = matches!(finished_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
        let started = std::time::Instant::now();
        pty.kill();
        let finished = finished_rx.recv_timeout(Duration::from_secs(2));
        let elapsed = started.elapsed();
        pty.close();
        writer_thread.join().unwrap();
        reader_thread.join().unwrap();
        assert!(blocked, "fixture failed to fill the PTY input pipe");
        assert!(
            elapsed < Duration::from_secs(2),
            "kill waited for PTY input"
        );
        assert!(finished.expect("kill did not unblock PTY input").is_err());
    }

    #[test]
    fn invalid_sizes_are_usage_errors() {
        for (cols, rows) in [(0, 1), (1, 0), (0, 0)] {
            assert_eq!(
                validate_size(cols, rows).unwrap_err().kind,
                crate::api::ErrorKind::Usage
            );
        }
        assert!(validate_size(1, 1).is_ok());
        #[cfg(windows)]
        for (cols, rows) in [(32768, 1), (1, 32768), (u16::MAX, u16::MAX)] {
            assert_eq!(
                validate_size(cols, rows).unwrap_err().kind,
                crate::api::ErrorKind::Usage
            );
        }
    }

    #[test]
    fn process_exit_preserves_exit_code() {
        assert_eq!(
            ProcessExit::from(ExitStatus::with_exit_code(7)),
            ProcessExit {
                code: 7,
                signal: None,
            }
        );
    }

    #[test]
    fn process_exit_preserves_signal_and_fallback_code() {
        assert_eq!(
            ProcessExit::from(ExitStatus::with_signal("Terminated")),
            ProcessExit {
                code: 1,
                signal: Some("Terminated".to_string()),
            }
        );
    }
}
