use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::profile::ColorSlot;
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

const WIN32_INPUT_MODE: &[u8] = b"\x1b[?9001h";

#[cfg(feature = "recording-raster")]
mod reader;
#[cfg(feature = "recording-raster")]
pub(crate) use reader::{read, CastEventKind, CastReader};

pub(crate) struct CastWriter {
    start: Instant,
    path: PathBuf,
    sink: BufWriter<File>,
    last_committed: Option<Duration>,
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
            path: path.to_path_buf(),
            sink: BufWriter::new(sink),
            last_committed: None,
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn last_committed(&self) -> Option<Duration> {
        self.last_committed
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
        let elapsed = at.saturating_duration_since(self.start);
        serde_json::to_writer(&mut self.sink, &(elapsed.as_secs_f64(), code, data))
            .map_err(json_error)?;
        self.sink.write_all(b"\n")?;
        self.sink.flush()?;
        self.last_committed = Some(elapsed);
        Ok(())
    }
}

pub(crate) fn snapshot_to_ansi(emulator: &dyn Emulator) -> String {
    let rows = emulator.viewable_rows();
    let (cols, _) = emulator.size();
    let cursor = emulator.cursor();
    let blank = EmuCell::blank();
    let mut output = String::from("\x1b[0m\x1b[?7l\x1b[2J\x1b[H");
    for index in 0..=u8::MAX {
        write_osc_color(
            &mut output,
            &format!("4;{index}"),
            emulator.color(ColorSlot::Indexed(index)),
        );
    }
    write_osc_color(&mut output, "10", emulator.color(ColorSlot::Foreground));
    write_osc_color(&mut output, "11", emulator.color(ColorSlot::Background));
    write_osc_color(&mut output, "12", emulator.color(ColorSlot::Cursor));
    let _ = write!(
        output,
        "\x1b]2;{}\x1b\\",
        emulator.title().unwrap_or_default()
    );
    for (y, row) in rows.iter().enumerate() {
        for x in 0..usize::from(cols) {
            let cell = row.get(x).unwrap_or(&blank);
            if cell == &blank || cell.ch.as_str() == CONTINUATION {
                continue;
            }
            let _ = write!(output, "\x1b[{};{}H", y + 1, x + 1);
            write_style(&mut output, cell);
            output.push_str(&cell.ch);
        }
    }
    let _ = write!(
        output,
        "\x1b[0m\x1b[{};{}H\x1b[{} q\x1b[?7h{}",
        cursor.1 + 1,
        cursor.0 + 1,
        match emulator.cursor_shape() {
            CursorShape::Block => 2,
            CursorShape::Underline => 4,
            CursorShape::Bar => 6,
        },
        if emulator.cursor_visible() {
            "\x1b[?25h"
        } else {
            "\x1b[?25l"
        }
    );
    output
}

fn write_osc_color(output: &mut String, selector: &str, color: crate::profile::Rgb) {
    let _ = write!(
        output,
        "\x1b]{selector};#{:02x}{:02x}{:02x}\x1b\\",
        color.r, color.g, color.b
    );
}

fn write_style(output: &mut String, cell: &EmuCell) {
    let mut codes = vec!["0".to_string()];
    for (attr, code) in [
        (Attrs::BOLD, "1"),
        (Attrs::DIM, "2"),
        (Attrs::ITALIC, "3"),
        (Attrs::BLINK, "5"),
        (Attrs::INVERSE, "7"),
        (Attrs::INVISIBLE, "8"),
        (Attrs::STRIKE, "9"),
    ] {
        if cell.has(attr) {
            codes.push(code.to_string());
        }
    }
    let underline = match cell.underline {
        UnderlineStyle::None => None,
        UnderlineStyle::Single => Some("4"),
        UnderlineStyle::Double => Some("4:2"),
        UnderlineStyle::Curly => Some("4:3"),
        UnderlineStyle::Dotted => Some("4:4"),
        UnderlineStyle::Dashed => Some("4:5"),
    };
    if let Some(underline) = underline {
        codes.push(underline.to_string());
    }
    push_color(&mut codes, cell.fg, true, "");
    push_color(&mut codes, cell.bg, false, "");
    if cell.underline.is_underlined() {
        push_color(&mut codes, cell.underline_color, true, "5");
    }
    let _ = write!(output, "\x1b[{}m", codes.join(";"));
}

fn push_color(codes: &mut Vec<String>, color: Option<Color>, foreground: bool, prefix: &str) {
    let Some(color) = color else {
        return;
    };
    let base = if prefix.is_empty() {
        if foreground {
            "38"
        } else {
            "48"
        }
    } else {
        "58"
    };
    match color {
        Color::Named(named) if prefix.is_empty() => {
            let index = named.index();
            let value = if foreground {
                if index < 8 {
                    30 + index
                } else {
                    90 + index - 8
                }
            } else if index < 8 {
                40 + index
            } else {
                100 + index - 8
            };
            codes.push(value.to_string());
        }
        Color::Named(named) => codes.push(format!("{base};5;{}", named.index())),
        Color::Idx(index) => codes.push(format!("{base};5;{index}")),
        Color::Rgb(red, green, blue) => {
            codes.push(format!("{base};2;{red};{green};{blue}"));
        }
    }
}

#[derive(Clone, Default)]
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

    pub fn finish(&mut self) -> String {
        self.utf8_pending.append(&mut self.filter_pending);
        let output = String::from_utf8_lossy(&self.utf8_pending).into_owned();
        self.utf8_pending.clear();
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

    #[test]
    fn incremental_decode_finishes_trailing_prefixes_and_utf8() {
        let mut decoder = IncrementalDecoder::default();
        assert_eq!(decoder.push(b"a\x1b[?"), "a");
        assert_eq!(decoder.finish(), "\x1b[?");
        assert_eq!(decoder.finish(), "");

        assert_eq!(decoder.push(&[0xe2]), "");
        assert_eq!(decoder.finish(), "\u{fffd}");
    }

    #[test]
    fn snapshot_captures_existing_visual_state() {
        use crate::profile::{ColorSlot, Profile, Rgb};
        use crate::terminal::alacritty::AlacrittyEmu;
        use crate::terminal::emu::{CursorShape, Emulator};

        let mut source = AlacrittyEmu::new(2, 1, &Profile::default());
        source.process(
            b"\x1b]2;before recording\x07\x1b]4;1;#010203\x07\
              \x1b]10;#040506\x07\x1b]11;#070809\x07\x1b]12;#0a0b0c\x07\
              \x1b[1;2H\x1b[6 q\x1b[?25l",
        );

        let mut replay = AlacrittyEmu::new(2, 1, &Profile::default());
        replay.process(snapshot_to_ansi(&source).as_bytes());

        assert_eq!(replay.title().as_deref(), Some("before recording"));
        assert_eq!(replay.color(ColorSlot::Indexed(1)), Rgb::new(1, 2, 3));
        assert_eq!(replay.color(ColorSlot::Foreground), Rgb::new(4, 5, 6));
        assert_eq!(replay.color(ColorSlot::Background), Rgb::new(7, 8, 9));
        assert_eq!(replay.color(ColorSlot::Cursor), Rgb::new(10, 11, 12));
        assert_eq!(replay.cursor(), (1, 0));
        assert_eq!(replay.cursor_shape(), CursorShape::Bar);
        assert!(!replay.cursor_visible());
    }
}
