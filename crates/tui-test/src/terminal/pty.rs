//! PTY spawning and control via `portable-pty`.

use std::io::{Read, Write};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::shell::Launch;

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub struct SpawnOptions {
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

impl Pty {
    /// Spawn a program in a fresh PTY, returning the controller and a reader
    /// for its output.
    pub fn spawn(
        target: &str,
        args: &[String],
        opts: &SpawnOptions,
    ) -> anyhow::Result<(Pty, Box<dyn Read + Send>)> {
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
        if let Some(cwd) = &opts.cwd {
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
                master: pair.master,
                writer,
                child,
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
        let opts = SpawnOptions {
            cols,
            rows,
            cwd,
            env: launch.env.clone(),
        };
        Pty::spawn(&launch.target, &launch.args, &opts)
    }

    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// Send a named signal. Cross-platform support is limited: INT delivers a
    /// Ctrl-C to the foreground app; TERM/KILL terminate the child.
    pub fn signal(&mut self, name: &str) -> anyhow::Result<()> {
        let upper = name.trim_start_matches("SIG").to_uppercase();
        match upper.as_str() {
            "INT" => self.write(b"\x03")?,
            "TERM" | "KILL" | "QUIT" => self.kill(),
            other => anyhow::bail!("unsupported signal: {other}"),
        }
        Ok(())
    }

    /// Return the exit code if the child has exited.
    pub fn try_wait(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.exit_code() as i32),
            _ => None,
        }
    }
}
