use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct CastHeader {
    pub version: u8,
    pub width: u16,
    pub height: u16,
}

pub(crate) struct CastReader<R> {
    pub header: CastHeader,
    reader: R,
    line: String,
    done: bool,
}

#[derive(Debug)]
pub(crate) struct CastEvent {
    pub time: f64,
    pub kind: CastEventKind,
}

#[derive(Debug)]
pub(crate) enum CastEventKind {
    Output(String),
    Resize(u16, u16),
}

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

pub(crate) fn read(path: &Path) -> anyhow::Result<CastReader<BufReader<File>>> {
    CastReader::new(BufReader::new(File::open(path)?))
}

#[cfg(test)]
fn read_bytes(bytes: &[u8]) -> anyhow::Result<CastReader<BufReader<&[u8]>>> {
    CastReader::new(BufReader::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
