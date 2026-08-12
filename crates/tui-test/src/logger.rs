//! Optional verbose logging of everything a terminal session reads and writes
//! the PTY, plus lifecycle events. Modeled on inshellisense's data log: every
//! record is timestamped and byte-escaped so control sequences are visible.
//!
//! Tags:
//!   READ   bytes read from the PTY (what the app draws on screen)
//!   WRITE  bytes written to the PTY (keystrokes / input)
//!   REPLY  bytes we wrote back to the PTY answering a capability query
//!   EVENT  lifecycle notes (open, resize, exit, requests)

use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Logger {
    sink: Option<Mutex<BufWriter<std::fs::File>>>,
}

impl Logger {
    /// A no-op logger.
    pub fn disabled() -> Self {
        Logger { sink: None }
    }

    /// Create a logger that truncates and writes to `path`.
    pub fn to_file(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        Ok(Logger {
            sink: Some(Mutex::new(BufWriter::new(file))),
        })
    }

    pub fn enabled(&self) -> bool {
        self.sink.is_some()
    }

    pub fn event(&self, msg: &str) {
        self.write_line("EVENT", msg);
    }

    pub fn read(&self, bytes: &[u8]) {
        if self.sink.is_some() {
            self.write_line("READ ", &escape(bytes));
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        if self.sink.is_some() {
            self.write_line("WRITE", &escape(bytes));
        }
    }

    pub fn reply(&self, bytes: &[u8]) {
        if self.sink.is_some() {
            self.write_line("REPLY", &escape(bytes));
        }
    }

    fn write_line(&self, tag: &str, body: &str) {
        let Some(sink) = &self.sink else { return };
        if let Ok(mut w) = sink.lock() {
            let _ = writeln!(w, "{} {tag} {body}", timestamp());
            let _ = w.flush();
        }
    }
}

/// UTC time-of-day as `HH:MM:SS.mmm`.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Render bytes with control characters made visible.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            0x1b => out.push_str("\\e"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}
