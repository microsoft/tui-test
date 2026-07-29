//! Live session monitor: a human watches what an agent is driving.
//!
//! The daemon renders the live emulator grid into a framed, full-color ANSI
//! frame (see [`render_frame`]) and streams one every ~20fps over the session
//! socket. The client ([`run_client`]) takes over an alternate screen in raw
//! mode and blits those frames, so the viewer sees the session in real time
//! while the agent keeps driving it through the same daemon.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::terminal::cell::{Color, EmuCell};

const BORDER: &str = "\x1b[38;5;240m";
const RESET: &str = "\x1b[0m";

/// A snapshot of a live session, rendered into one monitor frame.
pub struct Frame {
    pub grid: Vec<Vec<EmuCell>>,
    pub cursor: (u16, u16),
    pub size: (u16, u16),
    pub exited: Option<i32>,
    pub shell: Option<&'static str>,
}

/// Render a framed, full-color view of `frame` clipped to the `viewer` size.
///
/// `None` renders a "no active session" placeholder. The output positions
/// itself from the home cell and clears trailing cells/rows, so successive
/// frames repaint in place without flicker (no full screen clear).
pub fn render_frame(frame: Option<&Frame>, viewer: (u16, u16), session: &str) -> Vec<u8> {
    let vcols = viewer.0.max(8);
    let vrows = viewer.1.max(4);
    let inner_w = match frame {
        Some(f) => f.size.0.min(vcols - 2),
        None => vcols - 2,
    } as usize;
    let inner_h = match frame {
        Some(f) => f.size.1.min(vrows - 2),
        None => vrows - 2,
    } as usize;

    let mut out = String::with_capacity(inner_w * inner_h * 4);
    out.push_str("\x1b[H");
    header(&mut out, frame, session, inner_w);
    if let Some(f) = frame {
        content(&mut out, f, inner_w, inner_h);
    } else {
        placeholder(&mut out, inner_w, inner_h);
    }
    border_line(&mut out, '└', '┘', "┤ q quit ├", inner_w, false);
    out.push_str("\x1b[J");
    out.into_bytes()
}

fn header(out: &mut String, frame: Option<&Frame>, session: &str, inner_w: usize) {
    let title = match frame {
        Some(f) => {
            let shell = f.shell.map(|s| format!("{s} · ")).unwrap_or_default();
            let status = match f.exited {
                Some(code) => format!("exited {code}"),
                None => "live".to_string(),
            };
            format!("┤ {shell}{}×{} · {status} ├", f.size.0, f.size.1)
        }
        None => format!("┤ {session} · no session ├"),
    };
    border_line(out, '┌', '┐', &title, inner_w, true);
}

fn content(out: &mut String, f: &Frame, inner_w: usize, inner_h: usize) {
    let (cx, cy) = f.cursor;
    let show_cursor = f.exited.is_none();
    for y in 0..inner_h {
        out.push_str(BORDER);
        out.push('│');
        out.push_str(RESET);
        let row = f.grid.get(y);
        let mut last: Option<Style> = None;
        for x in 0..inner_w {
            let mut cell = row
                .and_then(|r| r.get(x))
                .cloned()
                .unwrap_or_else(blank_cell);
            if show_cursor && x as u16 == cx && y as u16 == cy {
                cell.inverse = !cell.inverse;
            }
            let style = Style::from(&cell);
            if last.as_ref() != Some(&style) {
                out.push_str(&style.sgr());
                last = Some(style);
            }
            if cell.ch.is_empty() {
                out.push(' ');
            } else {
                out.push_str(&cell.ch);
            }
        }
        out.push_str(RESET);
        out.push_str(BORDER);
        out.push('│');
        out.push_str(RESET);
        out.push_str("\x1b[K\r\n");
    }
}

fn placeholder(out: &mut String, inner_w: usize, inner_h: usize) {
    let msg = "no active session, run `shell-use open`";
    for y in 0..inner_h {
        out.push_str(BORDER);
        out.push('│');
        out.push_str(RESET);
        if y == inner_h / 2 {
            let shown: String = msg.chars().take(inner_w).collect();
            let count = shown.chars().count();
            let pad = inner_w.saturating_sub(count) / 2;
            out.push_str(&" ".repeat(pad));
            out.push_str(&shown);
            out.push_str(&" ".repeat(inner_w.saturating_sub(pad + count)));
        } else {
            out.push_str(&" ".repeat(inner_w));
        }
        out.push_str(BORDER);
        out.push('│');
        out.push_str(RESET);
        out.push_str("\x1b[K\r\n");
    }
}

fn border_line(out: &mut String, left: char, right: char, title: &str, inner_w: usize, nl: bool) {
    out.push_str(BORDER);
    out.push(left);
    let tlen = title.chars().count();
    if tlen + 1 >= inner_w {
        out.extend(title.chars().take(inner_w));
    } else {
        out.push('─');
        out.push_str(title);
        for _ in 0..(inner_w - 1 - tlen) {
            out.push('─');
        }
    }
    out.push(right);
    out.push_str(RESET);
    out.push_str("\x1b[K");
    if nl {
        out.push_str("\r\n");
    }
}

#[derive(PartialEq, Clone)]
struct Style {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    invisible: bool,
    strike: bool,
}

impl Style {
    fn from(c: &EmuCell) -> Self {
        Style {
            fg: c.fg,
            bg: c.bg,
            bold: c.bold,
            dim: c.dim,
            italic: c.italic,
            underline: c.underline,
            inverse: c.inverse,
            invisible: c.invisible,
            strike: c.strike,
        }
    }

    fn sgr(&self) -> String {
        let mut s = String::from("\x1b[0");
        for (on, code) in [
            (self.bold, "1"),
            (self.dim, "2"),
            (self.italic, "3"),
            (self.underline, "4"),
            (self.inverse, "7"),
            (self.invisible, "8"),
            (self.strike, "9"),
        ] {
            if on {
                s.push(';');
                s.push_str(code);
            }
        }
        push_color(&mut s, self.fg, true);
        push_color(&mut s, self.bg, false);
        s.push('m');
        s
    }
}

fn push_color(s: &mut String, color: Color, fg: bool) {
    let base = if fg { 38 } else { 48 };
    match color {
        Color::Default => {}
        Color::Idx(i) => s.push_str(&format!(";{base};5;{i}")),
        Color::Rgb(r, g, b) => s.push_str(&format!(";{base};2;{r};{g};{b}")),
    }
}

fn blank_cell() -> EmuCell {
    EmuCell {
        ch: String::new(),
        fg: Color::Default,
        bg: Color::Default,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        inverse: false,
        invisible: false,
        strike: false,
    }
}

/// Run the interactive monitor client for `session` until the viewer quits or
/// the session/daemon goes away. Returns a process exit code.
pub fn run_client(session: &str) -> i32 {
    use crate::{config, ipc};

    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        eprintln!("no active session '{session}'; run `shell-use open` first");
        return 3;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        eprintln!("`monitor` requires an interactive terminal");
        return 2;
    }

    if crossterm::terminal::enable_raw_mode().is_err() {
        eprintln!("failed to enter raw mode");
        return 5;
    }
    let mut stdout = std::io::stdout();
    let _ = crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    );

    let code = stream_loop(&socket);

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
    code
}

enum Action {
    Quit,
    Reconnect,
    Disconnected,
}

fn stream_loop(socket: &str) -> i32 {
    use crate::ipc;
    use crate::protocol::Request;

    loop {
        let (vcols, vrows) = crossterm::terminal::size().unwrap_or((80, 24));
        let mut conn = match ipc::connect(socket) {
            Ok(c) => c,
            Err(_) => return 4,
        };
        let mut line = match serde_json::to_string(&Request::Monitor {
            cols: vcols,
            rows: vrows,
        }) {
            Ok(l) => l,
            Err(_) => return 5,
        };
        line.push('\n');
        if conn.write_all(line.as_bytes()).is_err() || conn.flush().is_err() {
            return 4;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let disconnected = Arc::new(AtomicBool::new(false));
        let reader = {
            let stop = stop.clone();
            let disconnected = disconnected.clone();
            std::thread::spawn(move || {
                let mut src = &conn;
                let mut buf = [0u8; 16384];
                let mut out = std::io::stdout();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    match src.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            disconnected.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(n) => {
                            let _ = out.write_all(&buf[..n]);
                            let _ = out.flush();
                        }
                    }
                }
            })
        };

        let action = input_loop(&disconnected);
        stop.store(true, Ordering::Relaxed);
        let _ = reader.join();

        match action {
            Action::Quit => return 0,
            Action::Disconnected => {
                eprintln!("session ended");
                return 0;
            }
            Action::Reconnect => {
                let _ = crossterm::execute!(
                    std::io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
                );
            }
        }
    }
}

fn input_loop(disconnected: &AtomicBool) -> Action {
    use crossterm::event::{Event, KeyCode, KeyModifiers};

    loop {
        if disconnected.load(Ordering::Relaxed) {
            return Action::Disconnected;
        }
        if crossterm::event::poll(Duration::from_millis(100)).unwrap_or(false) {
            match crossterm::event::read() {
                Ok(Event::Key(k)) => {
                    let ctrl_c =
                        k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL);
                    if ctrl_c || matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                        return Action::Quit;
                    }
                }
                Ok(Event::Resize(_, _)) => return Action::Reconnect,
                Ok(_) => {}
                Err(_) => return Action::Quit,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(ch: &str) -> EmuCell {
        EmuCell {
            ch: ch.to_string(),
            ..blank_cell()
        }
    }

    #[test]
    fn render_includes_frame_and_content() {
        let frame = Frame {
            grid: vec![vec![cell("h"), cell("i")]],
            cursor: (0, 0),
            size: (40, 1),
            exited: None,
            shell: Some("bash"),
        };
        let bytes = render_frame(Some(&frame), (50, 6), "default");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains('┌') && text.contains('┘'));
        assert!(text.contains("bash"));
        assert!(text.contains('h') && text.contains('i'));
        assert!(text.starts_with("\x1b[H"));
    }

    #[test]
    fn render_placeholder_without_session() {
        let bytes = render_frame(None, (40, 6), "work");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("no session"));
        assert!(text.contains("no active session"));
    }

    #[test]
    fn cursor_cell_is_inverted() {
        let frame = Frame {
            grid: vec![vec![cell("x")]],
            cursor: (0, 0),
            size: (1, 1),
            exited: None,
            shell: None,
        };
        let text = String::from_utf8(render_frame(Some(&frame), (10, 5), "s")).unwrap();
        assert!(text.contains(";7") || text.contains("[7"));
    }
}
