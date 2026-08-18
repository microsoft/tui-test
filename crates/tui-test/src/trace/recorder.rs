use std::path::Path;
use std::time::Instant;

use crate::record::cast::{CastWriter, IncrementalDecoder};

pub struct Recorder {
    writer: Option<CastWriter>,
    decoder: IncrementalDecoder,
}

impl Recorder {
    /// Create (truncating) a cast file and write the asciinema v2 header.
    pub fn create(path: &Path, cols: u16, rows: u16, env: &[(&str, String)]) -> Self {
        let env = env
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect::<Vec<_>>();
        Self {
            writer: CastWriter::create(path, cols, rows, &env, Instant::now()).ok(),
            decoder: IncrementalDecoder::default(),
        }
    }

    /// Record a chunk of terminal output as an `"o"` event.
    pub fn on_data(&mut self, data: &[u8]) {
        let text = self.decoder.push(data);
        if let Some(writer) = self.writer.as_mut().filter(|_| !text.is_empty()) {
            let _ = writer.write_output(Instant::now(), &text);
        }
    }

    /// Record a terminal resize as an `"r"` event (`<cols>x<rows>`).
    pub fn on_resize(&mut self, cols: u16, rows: u16) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.write_resize(Instant::now(), cols, rows);
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_recorder_uses_the_shared_cast_writer() {
        let path = std::env::temp_dir().join(format!(
            "tui-test-shared-cast-writer-{}.cast",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut recorder = Recorder::create(&path, 2, 1, &[("TERM", "xterm".to_string())]);
        recorder.on_data(b"ok");
        recorder.on_resize(3, 2);
        drop(recorder);

        let cast = std::fs::read_to_string(&path).unwrap();
        assert!(cast.contains(r#""width":2"#));
        assert!(cast.contains(r#""ok""#));
        assert!(cast.contains(r#""3x2""#));
        std::fs::remove_file(path).unwrap();
    }
}
