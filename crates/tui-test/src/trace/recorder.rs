//! Always-on session recording in the asciinema v2 cast format.
//!
//! Output (and resize) events are streamed to a `.cast` file as they arrive, so
//! the recording is always durable and readable while the session is alive. The
//! file is removed when the session is killed. Format reference:
//! <https://docs.asciinema.org/manual/asciicast/v2/>.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const WIN32_INPUT_MODE: &[u8] = b"\x1b[?9001h";

pub struct Recorder {
    start: Instant,
    sink: Option<BufWriter<File>>,
    /// Trailing bytes of an incomplete UTF-8 sequence, carried to the next chunk
    /// so multi-byte glyphs split across PTY reads aren't corrupted.
    pending: Vec<u8>,
}

impl Recorder {
    /// A recorder that writes nowhere.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Recorder {
            start: Instant::now(),
            sink: None,
            pending: Vec::new(),
        }
    }

    /// Create (truncating) a cast file and write the asciinema v2 header.
    pub fn create(path: &Path, cols: u16, rows: u16, env: &[(&str, String)]) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let sink = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .ok()
            .map(BufWriter::new);
        let mut rec = Recorder {
            start: Instant::now(),
            sink,
            pending: Vec::new(),
        };
        rec.write_header(cols, rows, env);
        rec
    }

    fn write_header(&mut self, cols: u16, rows: u16, env: &[(&str, String)]) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let env_obj: serde_json::Map<String, serde_json::Value> = env
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::Value::String(v.clone())))
            .collect();
        let header = serde_json::json!({
            "version": 2,
            "width": cols,
            "height": rows,
            "timestamp": timestamp,
            "env": env_obj,
        });
        let _ = writeln!(sink, "{header}");
        let _ = sink.flush();
    }

    /// Record a chunk of terminal output as an `"o"` event.
    pub fn on_data(&mut self, data: &[u8]) {
        let mut cleaned = Vec::with_capacity(data.len());
        strip_subsequence(data, WIN32_INPUT_MODE, &mut cleaned);
        if cleaned.is_empty() {
            return;
        }
        let text = self.decode_incremental(&cleaned);
        if !text.is_empty() {
            self.write_event("o", &text);
        }
    }

    /// Record a terminal resize as an `"r"` event (`<cols>x<rows>`).
    pub fn on_resize(&mut self, cols: u16, rows: u16) {
        self.write_event("r", &format!("{cols}x{rows}"));
    }

    /// Decode appended bytes as UTF-8, retaining a trailing incomplete sequence
    /// (≤3 bytes) for the next call. Genuinely invalid bytes become U+FFFD.
    fn decode_incremental(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    if let Ok(s) = std::str::from_utf8(&self.pending[..valid]) {
                        out.push_str(s);
                    }
                    match e.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + len);
                        }
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    fn write_event(&mut self, code: &str, data: &str) {
        let time = self.start.elapsed().as_secs_f64();
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        let line = serde_json::to_string(&(time, code, data)).unwrap_or_default();
        let _ = writeln!(sink, "{line}");
        let _ = sink.flush();
    }
}

fn strip_subsequence(data: &[u8], needle: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(needle) {
            i += needle.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_decode_handles_split_multibyte() {
        let mut rec = Recorder::disabled();
        assert_eq!(rec.decode_incremental(&[0xE2, 0x94]), "");
        assert_eq!(rec.decode_incremental(&[0x80, b'x']), "─x");
    }

    #[test]
    fn incremental_decode_emits_replacement_for_invalid() {
        let mut rec = Recorder::disabled();
        assert_eq!(rec.decode_incremental(&[b'a', 0xFF, b'b']), "a\u{FFFD}b");
    }

    #[test]
    fn strip_removes_win32_input_mode() {
        let mut out = Vec::new();
        strip_subsequence(b"a\x1b[?9001hb", WIN32_INPUT_MODE, &mut out);
        assert_eq!(out, b"ab");
    }
}
