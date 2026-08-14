use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const WIN32_INPUT_MODE: &[u8] = b"\x1b[?9001h";

pub(crate) struct CastWriter {
    start: Instant,
    sink: BufWriter<File>,
}

impl CastWriter {
    pub fn create(
        path: &Path,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
        start: Instant,
    ) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let sink = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        let mut writer = Self {
            start,
            sink: BufWriter::new(sink),
        };
        writer.write_header(cols, rows, env)?;
        Ok(writer)
    }

    pub fn write_output(&mut self, at: Instant, data: &str) -> io::Result<()> {
        self.write_event(at, "o", data)
    }

    pub fn write_resize(&mut self, at: Instant, cols: u16, rows: u16) -> io::Result<()> {
        self.write_event(at, "r", &format!("{cols}x{rows}"))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }

    fn write_header(&mut self, cols: u16, rows: u16, env: &[(String, String)]) -> io::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let env = env
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
            .collect::<serde_json::Map<String, serde_json::Value>>();
        serde_json::to_writer(
            &mut self.sink,
            &serde_json::json!({
                "version": 2,
                "width": cols,
                "height": rows,
                "timestamp": timestamp,
                "env": env,
            }),
        )
        .map_err(json_error)?;
        self.sink.write_all(b"\n")?;
        self.sink.flush()
    }

    fn write_event(&mut self, at: Instant, code: &str, data: &str) -> io::Result<()> {
        let elapsed = at.saturating_duration_since(self.start).as_secs_f64();
        serde_json::to_writer(&mut self.sink, &(elapsed, code, data)).map_err(json_error)?;
        self.sink.write_all(b"\n")?;
        self.sink.flush()
    }
}

#[derive(Default)]
pub(crate) struct IncrementalDecoder {
    filter_pending: Vec<u8>,
    utf8_pending: Vec<u8>,
}

impl IncrementalDecoder {
    pub fn push(&mut self, data: &[u8]) -> String {
        let mut cleaned = Vec::with_capacity(data.len());
        self.filter_pending.extend_from_slice(data);
        let mut consumed = 0;
        while consumed < self.filter_pending.len() {
            let remaining = &self.filter_pending[consumed..];
            if remaining.starts_with(WIN32_INPUT_MODE) {
                consumed += WIN32_INPUT_MODE.len();
            } else if WIN32_INPUT_MODE.starts_with(remaining) {
                break;
            } else {
                cleaned.push(self.filter_pending[consumed]);
                consumed += 1;
            }
        }
        self.filter_pending.drain(..consumed);
        self.utf8_pending.extend_from_slice(&cleaned);

        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.utf8_pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.utf8_pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if let Ok(text) = std::str::from_utf8(&self.utf8_pending[..valid]) {
                        output.push_str(text);
                    }
                    match error.error_len() {
                        Some(length) => {
                            output.push('\u{FFFD}');
                            self.utf8_pending.drain(..valid + length);
                        }
                        None => {
                            self.utf8_pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        output
    }
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_decode_handles_split_multibyte() {
        let mut decoder = IncrementalDecoder::default();
        assert_eq!(decoder.push(&[0xE2, 0x94]), "");
        assert_eq!(decoder.push(&[0x80, b'x']), "─x");
    }

    #[test]
    fn incremental_decode_emits_replacement_for_invalid() {
        let mut decoder = IncrementalDecoder::default();
        assert_eq!(decoder.push(&[b'a', 0xFF, b'b']), "a\u{FFFD}b");
    }

    #[test]
    fn incremental_decode_removes_win32_input_mode() {
        let mut decoder = IncrementalDecoder::default();
        assert_eq!(decoder.push(b"a\x1b[?9001hb"), "ab");
    }

    #[test]
    fn incremental_decode_removes_split_win32_input_mode() {
        for split in 1..WIN32_INPUT_MODE.len() {
            let mut decoder = IncrementalDecoder::default();
            let mut first = b"a".to_vec();
            first.extend_from_slice(&WIN32_INPUT_MODE[..split]);
            assert_eq!(decoder.push(&first), "a", "split at {split}");

            let mut second = WIN32_INPUT_MODE[split..].to_vec();
            second.push(b'b');
            assert_eq!(decoder.push(&second), "b", "split at {split}");
        }
    }
}
