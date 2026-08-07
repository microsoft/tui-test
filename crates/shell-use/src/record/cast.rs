use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
#[cfg(feature = "recording-raster")]
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "recording-raster")]
use serde::Deserialize;

use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};

const WIN32_INPUT_MODE: &[u8] = b"\x1b[?9001h";

#[cfg(feature = "recording-raster")]
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct CastHeader {
    pub version: u8,
    pub width: u16,
    pub height: u16,
}

#[cfg(feature = "recording-raster")]
pub(crate) struct CastReader<R> {
    pub header: CastHeader,
    reader: R,
    line: String,
    done: bool,
}

#[cfg(feature = "recording-raster")]
#[derive(Debug)]
pub(crate) struct CastEvent {
    pub time: f64,
    pub kind: CastEventKind,
}

#[cfg(feature = "recording-raster")]
#[derive(Debug)]
pub(crate) enum CastEventKind {
    Output(String),
    Resize(u16, u16),
}

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

#[cfg(feature = "recording-raster")]
impl<R: BufRead> CastReader<R> {
    fn new(mut reader: R) -> anyhow::Result<Self> {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            anyhow::bail!("cast file is empty");
        }
        let header: CastHeader = serde_json::from_str(line.trim_end())?;
        if header.version != 2 {
            anyhow::bail!("unsupported asciicast version {}", header.version);
        }
        if header.width == 0 || header.height == 0 {
            anyhow::bail!("cast dimensions must be non-zero");
        }
        Ok(Self {
            header,
            reader,
            line,
            done: false,
        })
    }
}

#[cfg(feature = "recording-raster")]
impl<R: BufRead> Iterator for CastReader<R> {
    type Item = anyhow::Result<CastEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            self.line.clear();
            let count = match self.reader.read_line(&mut self.line) {
                Ok(count) => count,
                Err(error) => {
                    self.done = true;
                    return Some(Err(error.into()));
                }
            };
            if count == 0 {
                self.done = true;
                return None;
            }
            let complete = self.line.ends_with('\n');
            let trimmed = self.line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }
            let event = serde_json::from_str::<(f64, String, String)>(trimmed);
            let (time, code, data) = match event {
                Ok(event) => event,
                Err(_) if !complete => {
                    self.done = true;
                    return None;
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error.into()));
                }
            };
            if !time.is_finite() || time < 0.0 {
                self.done = true;
                return Some(Err(anyhow::anyhow!(
                    "cast event timestamps must be finite and non-negative"
                )));
            }
            let kind = match code.as_str() {
                "o" => CastEventKind::Output(data),
                "r" => {
                    let Some((cols, rows)) = data.split_once('x') else {
                        self.done = true;
                        return Some(Err(anyhow::anyhow!("invalid cast resize event '{data}'")));
                    };
                    let cols = match cols.parse::<u16>() {
                        Ok(cols) => cols,
                        Err(error) => {
                            self.done = true;
                            return Some(Err(error.into()));
                        }
                    };
                    let rows = match rows.parse::<u16>() {
                        Ok(rows) => rows,
                        Err(error) => {
                            self.done = true;
                            return Some(Err(error.into()));
                        }
                    };
                    CastEventKind::Resize(cols, rows)
                }
                _ => continue,
            };
            return Some(Ok(CastEvent { time, kind }));
        }
    }
}

#[cfg(feature = "recording-raster")]
pub(crate) fn read(path: &Path) -> anyhow::Result<CastReader<BufReader<File>>> {
    CastReader::new(BufReader::new(File::open(path)?))
}

#[cfg(all(test, feature = "recording-raster"))]
fn read_bytes(bytes: &[u8]) -> anyhow::Result<CastReader<BufReader<&[u8]>>> {
    CastReader::new(BufReader::new(bytes))
}

pub(crate) fn snapshot_to_ansi(rows: &[Vec<EmuCell>], cols: u16, cursor: (u16, u16)) -> String {
    let blank = EmuCell::blank();
    let mut output = String::from("\x1b[0m\x1b[?7l\x1b[2J\x1b[H");
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
        "\x1b[0m\x1b[{};{}H\x1b[?7h",
        cursor.1 + 1,
        cursor.0 + 1
    );
    output
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

#[derive(Default)]
pub(crate) struct IncrementalDecoder {
    pending: Vec<u8>,
}

impl IncrementalDecoder {
    pub fn push(&mut self, data: &[u8]) -> String {
        let mut cleaned = Vec::with_capacity(data.len());
        strip_subsequence(data, WIN32_INPUT_MODE, &mut cleaned);
        self.pending.extend_from_slice(&cleaned);

        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if let Ok(text) = std::str::from_utf8(&self.pending[..valid]) {
                        output.push_str(text);
                    }
                    match error.error_len() {
                        Some(length) => {
                            output.push('\u{FFFD}');
                            self.pending.drain(..valid + length);
                        }
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        output
    }
}

fn strip_subsequence(data: &[u8], needle: &[u8], output: &mut Vec<u8>) {
    let mut index = 0;
    while index < data.len() {
        if data[index..].starts_with(needle) {
            index += needle.len();
        } else {
            output.push(data[index]);
            index += 1;
        }
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
    fn snapshot_is_standard_ansi_output() {
        let grid = vec![vec![EmuCell {
            ch: "x".into(),
            fg: Some(Color::Rgb(1, 2, 3)),
            ..EmuCell::blank()
        }]];
        let ansi = snapshot_to_ansi(&grid, 1, (0, 0));
        assert!(ansi.contains("\x1b[1;1H"));
        assert!(ansi.contains("38;2;1;2;3"));
        assert!(ansi.contains('x'));
    }

    #[test]
    fn snapshot_restores_the_live_cursor_before_future_output() {
        use crate::terminal::alacritty::AlacrittyEmu;
        use crate::terminal::emu::Emulator;

        let grid = vec![vec![EmuCell::blank(); 4]; 2];
        let mut emulator = AlacrittyEmu::new(4, 2, 0);
        emulator.process(snapshot_to_ansi(&grid, 4, (2, 1)).as_bytes());
        emulator.process(b"X");
        assert_eq!(emulator.viewable_rows()[1][2].ch, "X");
    }

    #[test]
    #[cfg(feature = "recording-raster")]
    fn reader_ignores_an_incomplete_trailing_event() {
        let bytes = b"{\"version\":2,\"width\":1,\"height\":1}\n[0.0,\"o\",\"x\"]\n[1.0";
        let mut reader = read_bytes(bytes).unwrap();
        assert!(matches!(
            reader.next().unwrap().unwrap().kind,
            CastEventKind::Output(ref output) if output == "x"
        ));
        assert!(reader.next().is_none());
    }
}
