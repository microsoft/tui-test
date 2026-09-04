//! Live session monitor: a human watches what an agent is driving.
//!
//! The daemon renders the live emulator grid into a framed, full-color ANSI
//! frame (see [`render_frame`]) and streams one every ~20fps over the session
//! socket. The client ([`run_client`]) takes over an alternate screen in raw
//! mode and blits those frames, so the viewer sees the session in real time
//! while the agent keeps driving it through the same daemon.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use tui_test::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle};
use tui_test::terminal::emu::{KeyboardMode, MouseMode};

use crate::ansi;

#[cfg(windows)]
const UTF8_CONSOLE_CODE_PAGE: u32 = 65001;

/// A snapshot of a live session, rendered into one monitor frame.
pub struct Frame {
    pub grid: Vec<Vec<EmuCell>>,
    pub cursor: (u16, u16),
    pub size: (u16, u16),
    pub keyboard_mode: KeyboardMode,
    pub bracketed_paste: bool,
    pub mouse_mode: MouseMode,
    pub exited: Option<i32>,
    pub shell: Option<&'static str>,
}

/// The target's input modes as last announced to the viewer's terminal, so a
/// mode is only re-applied when the target changes it.
#[derive(Default)]
pub(crate) struct ModeMirror {
    applied: Option<(KeyboardMode, bool, MouseMode)>,
}

/// Render a framed, full-color view of `frame` clipped to the `viewer` size.
///
/// `None` renders a "no active session" placeholder. The output positions
/// itself from the home cell and clears trailing cells/rows, so successive
/// frames repaint in place without flicker (no full screen clear).
pub fn render_frame(
    frame: Option<&Frame>,
    viewer: (u16, u16),
    session: &str,
    interactive: bool,
    modes: &mut ModeMirror,
) -> Vec<u8> {
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
    if interactive {
        let keyboard = frame.map_or_else(KeyboardMode::empty, |f| f.keyboard_mode);
        let paste = frame.is_some_and(|f| f.bracketed_paste);
        let mouse = frame.map_or(MouseMode::None, |f| f.mouse_mode);
        if modes.applied.map(|(mode, _, _)| mode) != Some(keyboard) {
            out.push_str(&ansi::kitty_keyboard_mode(keyboard.bits()));
        }
        if modes.applied.map(|(_, paste, _)| paste) != Some(paste) {
            out.push_str(if paste {
                ansi::BRACKETED_PASTE_ENABLE
            } else {
                ansi::BRACKETED_PASTE_DISABLE
            });
        }
        if modes.applied.map(|(_, _, mouse)| mouse) != Some(mouse) {
            out.push_str(ansi::MOUSE_DISABLE);
            out.push_str(match mouse {
                MouseMode::None => "",
                MouseMode::Click => ansi::MOUSE_CLICK_ENABLE,
                MouseMode::Drag => ansi::MOUSE_DRAG_ENABLE,
                MouseMode::Motion => ansi::MOUSE_MOTION_ENABLE,
            });
        }
        modes.applied = Some((keyboard, paste, mouse));
    }
    out.push_str(ansi::HOME);
    header(&mut out, frame, session, inner_w);
    if let Some(f) = frame {
        content(&mut out, f, inner_w, inner_h);
    } else {
        placeholder(&mut out, inner_w, inner_h);
    }
    let detach_hint = if interactive {
        "┤ Ctrl+] detach ├"
    } else {
        "┤ q quit ├"
    };
    border_line(&mut out, '└', '┘', detach_hint, inner_w, false);
    out.push_str(ansi::ERASE_DISPLAY);
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
        out.push_str(ansi::BORDER);
        out.push('│');
        out.push_str(ansi::RESET);
        let row = f.grid.get(y);
        let mut last: Option<Style> = None;
        for x in 0..inner_w {
            let mut cell = row.and_then(|r| r.get(x)).cloned().unwrap_or_default();
            if show_cursor && x as u16 == cx && y as u16 == cy {
                cell.attrs.toggle(Attrs::INVERSE);
            }
            let style = Style::from(&cell);
            if last.as_ref() != Some(&style) {
                out.push_str(&style.sgr());
                last = Some(style);
            }
            out.push_str(&cell.ch);
        }
        out.push_str(ansi::RESET);
        out.push_str(ansi::BORDER);
        out.push('│');
        out.push_str(ansi::RESET);
        out.push_str(ansi::ERASE_LINE);
        out.push_str("\r\n");
    }
}

fn placeholder(out: &mut String, inner_w: usize, inner_h: usize) {
    let msg = "no active session, run `tui-test open`";
    for y in 0..inner_h {
        out.push_str(ansi::BORDER);
        out.push('│');
        out.push_str(ansi::RESET);
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
        out.push_str(ansi::BORDER);
        out.push('│');
        out.push_str(ansi::RESET);
        out.push_str(ansi::ERASE_LINE);
        out.push_str("\r\n");
    }
}

fn border_line(out: &mut String, left: char, right: char, title: &str, inner_w: usize, nl: bool) {
    out.push_str(ansi::BORDER);
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
    out.push_str(ansi::RESET);
    out.push_str(ansi::ERASE_LINE);
    if nl {
        out.push_str("\r\n");
    }
}

#[derive(PartialEq, Clone)]
struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    underline: UnderlineStyle,
    underline_color: Option<Color>,
    attrs: Attrs,
}

impl Style {
    fn from(c: &EmuCell) -> Self {
        Style {
            fg: c.fg,
            bg: c.bg,
            underline: c.underline,
            underline_color: c.underline_color,
            attrs: c.attrs,
        }
    }

    fn sgr(&self) -> String {
        let mut s = String::from(ansi::SGR_START);
        for (attr, code) in [
            (Attrs::BOLD, "1"),
            (Attrs::DIM, "2"),
            (Attrs::ITALIC, "3"),
            (Attrs::BLINK, "5"),
            (Attrs::INVERSE, "7"),
            (Attrs::INVISIBLE, "8"),
            (Attrs::STRIKE, "9"),
        ] {
            if self.attrs.contains(attr) {
                s.push(';');
                s.push_str(code);
            }
        }
        let sub = match self.underline {
            UnderlineStyle::None => 0,
            UnderlineStyle::Single => 1,
            UnderlineStyle::Double => 2,
            UnderlineStyle::Curly => 3,
            UnderlineStyle::Dotted => 4,
            UnderlineStyle::Dashed => 5,
        };
        if sub != 0 {
            s.push_str(&format!(";4:{sub}"));
            // SGR 58 takes its arguments as colon-joined subparameters. Mixing
            // in a `;` would end the parameter early and the terminal would
            // read whatever follows as the underline's color instead.
            match self.underline_color {
                Some(Color::Rgb(r, g, b)) => s.push_str(&format!(";58:2::{r}:{g}:{b}")),
                Some(c) => s.push_str(&format!(";58:5:{}", c.to_index())),
                None => {}
            }
        }
        push_color(&mut s, self.fg, true);
        push_color(&mut s, self.bg, false);
        s.push('m');
        s
    }
}

fn push_color(s: &mut String, color: Option<Color>, fg: bool) {
    let base = if fg { 38 } else { 48 };
    match color {
        None => {}
        Some(Color::Rgb(r, g, b)) => s.push_str(&format!(";{base};2;{r};{g};{b}")),
        Some(c) => s.push_str(&format!(";{base};5;{}", c.to_index())),
    }
}

/// Run the interactive monitor client for `session` until the viewer quits or
/// the session/daemon goes away. Returns a process exit code.
pub fn run_client(session: &str, interactive: bool) -> i32 {
    use crate::{config, ipc};

    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        eprintln!("no active session '{session}'; run `tui-test open` first");
        return 3;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        eprintln!("`monitor` requires an interactive terminal");
        return 2;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!("`monitor` requires terminal stdin");
        return 2;
    }

    if crossterm::terminal::enable_raw_mode().is_err() {
        eprintln!("failed to enter raw mode");
        return 5;
    }

    #[cfg(windows)]
    let vt_input = if interactive {
        match VirtualTerminalInput::enable() {
            Ok(mode) => Some(mode),
            Err(error) => {
                let _ = crossterm::terminal::disable_raw_mode();
                eprintln!("failed to enable virtual terminal input: {error}");
                return 5;
            }
        }
    } else {
        None
    };

    let mut viewer = ViewerGuard {
        stdout: std::io::stdout(),
        interactive,
        #[cfg(windows)]
        vt_input,
    };
    enter_viewer(&mut viewer.stdout, interactive);
    stream_loop(&socket, interactive.then(spawn_stdin_reader), interactive)
}

struct ViewerGuard {
    stdout: std::io::Stdout,
    interactive: bool,
    #[cfg(windows)]
    vt_input: Option<VirtualTerminalInput>,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        leave_viewer(&mut self.stdout, self.interactive);
        #[cfg(windows)]
        drop(self.vt_input.take());
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn enter_viewer(out: &mut impl Write, interactive: bool) {
    let _ = crossterm::execute!(
        out,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    );
    if interactive {
        let _ = out.write_all(ansi::BRACKETED_PASTE_SAVE);
        let _ = out.write_all(ansi::KITTY_KEYBOARD_PUSH);
        let _ = out.write_all(ansi::MOUSE_DISABLE.as_bytes());
        let _ = out.flush();
    }
}

fn leave_viewer(out: &mut impl Write, interactive: bool) {
    if interactive {
        let _ = out.write_all(ansi::KITTY_KEYBOARD_POP);
        let _ = out.write_all(ansi::BRACKETED_PASTE_DISABLE.as_bytes());
        let _ = out.write_all(ansi::BRACKETED_PASTE_RESTORE);
        let _ = out.write_all(ansi::MOUSE_DISABLE.as_bytes());
        let _ = out.flush();
    }
    let _ = crossterm::execute!(
        out,
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
}

#[cfg(windows)]
struct VirtualTerminalInput {
    handle: *mut core::ffi::c_void,
    original_mode: u32,
    original_code_page: u32,
}

#[cfg(windows)]
impl VirtualTerminalInput {
    fn enable() -> std::io::Result<Self> {
        use windows_sys::Win32::System::Console::{
            GetConsoleCP, GetConsoleMode, GetStdHandle, SetConsoleCP, SetConsoleMode,
            ENABLE_VIRTUAL_TERMINAL_INPUT, STD_INPUT_HANDLE,
        };

        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut original_mode = 0;
            if handle.is_null() || GetConsoleMode(handle, &mut original_mode) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let original_code_page = GetConsoleCP();
            if original_code_page == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if SetConsoleMode(handle, original_mode | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if SetConsoleCP(UTF8_CONSOLE_CODE_PAGE) == 0 {
                let error = std::io::Error::last_os_error();
                SetConsoleMode(handle, original_mode);
                return Err(error);
            }
            Ok(Self {
                handle,
                original_mode,
                original_code_page,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for VirtualTerminalInput {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleMode};

        unsafe {
            SetConsoleCP(self.original_code_page);
            SetConsoleMode(self.handle, self.original_mode);
        }
    }
}

/// Read viewer stdin on its own thread; the channel closes when stdin does.
fn spawn_stdin_reader() -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let mut buffer = [0; 4096];
        while let Ok(read) = stdin.read(&mut buffer) {
            if read == 0 || sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}

fn stream_loop(socket: &str, input: Option<mpsc::Receiver<Vec<u8>>>, interactive: bool) -> i32 {
    use crate::ipc;
    use crate::protocol::Request;

    let mut detach = DetachParser::default();
    loop {
        let (vcols, vrows) = crossterm::terminal::size().unwrap_or((80, 24));
        let viewer = (vcols, vrows);
        let input_stream = if interactive {
            match InputStream::connect(socket, viewer) {
                Ok(stream) => Some(stream),
                Err(_) => return 4,
            }
        } else {
            None
        };
        let mut conn = match ipc::connect(socket) {
            Ok(c) => c,
            Err(_) => return 4,
        };
        let mut line = match serde_json::to_string(&Request::Monitor {
            cols: vcols,
            rows: vrows,
            interactive,
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

        let reconnect = match (&input, &input_stream) {
            (Some(input), Some(input_stream)) => {
                interactive_input_loop(viewer, input, &disconnected, &mut detach, input_stream)
            }
            _ => read_only_input_loop(viewer, &disconnected),
        };
        stop.store(true, Ordering::Relaxed);
        let _ = reader.join();

        if !reconnect {
            return 0;
        }
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        );
    }
}

/// Pump viewer input until the viewer detaches or the frame stream has to be
/// reopened at a new size (`true`).
fn interactive_input_loop(
    viewer: (u16, u16),
    input: &mpsc::Receiver<Vec<u8>>,
    disconnected: &AtomicBool,
    detach: &mut DetachParser,
    input_stream: &InputStream,
) -> bool {
    loop {
        if disconnected.load(Ordering::Relaxed) {
            return false;
        }
        if crossterm::terminal::size().is_ok_and(|size| size != viewer) {
            return true;
        }

        match input.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                let (forward, detached) = detach.push(&bytes);
                if !input_stream.send(forward) {
                    eprintln!("monitor input disconnected");
                    return false;
                }
                if detached {
                    return false;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(bytes) = detach.on_idle() {
                    if !input_stream.send(bytes) {
                        eprintln!("monitor input disconnected");
                        return false;
                    }
                }
            }
        }
    }
}

fn read_only_input_loop(viewer: (u16, u16), disconnected: &AtomicBool) -> bool {
    use crossterm::event::{Event, KeyCode, KeyModifiers};

    loop {
        if disconnected.load(Ordering::Relaxed) {
            return false;
        }
        if crossterm::terminal::size().is_ok_and(|size| size != viewer) {
            return true;
        }
        if !crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false) {
            continue;
        }
        match crossterm::event::read() {
            Ok(Event::Key(key)) => {
                let ctrl_c =
                    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl_c || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    return false;
                }
            }
            Ok(Event::Resize(_, _)) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

/// Kitty encodes `Ctrl+]` as `CSI 93 ; <modifiers> u`; every other terminal
/// sends the single byte 0x1d.
const CTRL_RIGHT_BRACKET: u8 = 0x1d;

/// Splits viewer stdin into bytes for the target and the detach chord, holding
/// back a partial kitty chord until the rest of it arrives.
#[derive(Default)]
struct DetachParser {
    pending: Vec<u8>,
}

impl DetachParser {
    /// Returns the bytes to forward and whether the viewer asked to detach.
    fn push(&mut self, bytes: &[u8]) -> (Vec<u8>, bool) {
        self.pending.extend_from_slice(bytes);
        if let Some(at) = self
            .pending
            .iter()
            .position(|byte| *byte == CTRL_RIGHT_BRACKET)
        {
            let forwarded = self.pending.drain(..at).collect();
            self.pending.clear();
            return (forwarded, true);
        }
        let mut forwarded = Vec::new();
        loop {
            let Some(start) = self
                .pending
                .windows(ansi::KITTY_CTRL_RIGHT_BRACKET.len())
                .position(|window| window == ansi::KITTY_CTRL_RIGHT_BRACKET)
            else {
                let keep = (1..ansi::KITTY_CTRL_RIGHT_BRACKET
                    .len()
                    .min(self.pending.len() + 1))
                    .rev()
                    .find(|length| {
                        self.pending
                            .ends_with(&ansi::KITTY_CTRL_RIGHT_BRACKET[..*length])
                    })
                    .unwrap_or(0);
                let ready = self.pending.len() - keep;
                forwarded.extend(self.pending.drain(..ready));
                return (forwarded, false);
            };
            forwarded.extend(self.pending.drain(..start));
            let Some(end) = self.pending[ansi::KITTY_CTRL_RIGHT_BRACKET.len()..]
                .iter()
                .position(|byte| (0x40..=0x7e).contains(byte))
            else {
                return (forwarded, false);
            };
            let sequence: Vec<u8> = self
                .pending
                .drain(..=end + ansi::KITTY_CTRL_RIGHT_BRACKET.len())
                .collect();
            if is_detach_chord(&sequence) {
                self.pending.clear();
                return (forwarded, true);
            }
            forwarded.extend(sequence);
        }
    }

    /// Nothing followed the held-back bytes, so they were not a detach chord.
    fn on_idle(&mut self) -> Option<Vec<u8>> {
        (!self.pending.is_empty()).then(|| std::mem::take(&mut self.pending))
    }
}

/// True for `Ctrl+]` held with no modifier that would make it another chord
/// the target is entitled to see.
fn is_detach_chord(sequence: &[u8]) -> bool {
    const CTRL: u16 = 1 << 2;
    const LOCKS: u16 = (1 << 6) | (1 << 7);

    let Some(modifiers) = sequence
        .strip_prefix(ansi::KITTY_CTRL_RIGHT_BRACKET)
        .and_then(|rest| rest.strip_suffix(b"u"))
        .and_then(|rest| rest.split(|byte| matches!(byte, b';' | b':')).next())
        .and_then(|digits| std::str::from_utf8(digits).ok()?.parse::<u16>().ok())
        .and_then(|value| value.checked_sub(1))
    else {
        return false;
    };
    modifiers & CTRL != 0 && modifiers & !(CTRL | LOCKS) == 0
}

pub(crate) struct MouseRemapper {
    pending: Vec<u8>,
    viewer: (u16, u16),
    pressed: u8,
    active: bool,
    mouse_seen: bool,
}

impl MouseRemapper {
    pub(crate) fn new(viewer: (u16, u16)) -> Self {
        Self {
            pending: Vec::new(),
            viewer,
            pressed: 0,
            active: false,
            mouse_seen: false,
        }
    }

    pub(crate) fn observe(&mut self, size: Option<(u16, u16)>) {
        if size.is_some() {
            if !self.active {
                self.pressed = 0;
            }
            self.active = true;
            self.mouse_seen = true;
        } else {
            self.active = false;
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8], size: Option<(u16, u16)>) -> Vec<u8> {
        self.observe(size);
        if size.is_none() && !self.mouse_seen {
            let mut forwarded = std::mem::take(&mut self.pending);
            forwarded.extend_from_slice(bytes);
            return forwarded;
        }
        let size = size.map(|target| {
            (
                target.0.min(self.viewer.0.saturating_sub(2)),
                target.1.min(self.viewer.1.saturating_sub(2)),
            )
        });
        self.pending.extend_from_slice(bytes);
        let mut forwarded = Vec::new();
        loop {
            let Some(start) = self
                .pending
                .windows(ansi::SGR_MOUSE_PREFIX.len())
                .position(|window| window == ansi::SGR_MOUSE_PREFIX)
            else {
                let keep = (1..ansi::SGR_MOUSE_PREFIX.len().min(self.pending.len() + 1))
                    .rev()
                    .find(|length| self.pending.ends_with(&ansi::SGR_MOUSE_PREFIX[..*length]))
                    .unwrap_or(0);
                let ready = self.pending.len() - keep;
                forwarded.extend(self.pending.drain(..ready));
                return forwarded;
            };
            forwarded.extend(self.pending.drain(..start));
            let Some(end) = self.pending[ansi::SGR_MOUSE_PREFIX.len()..]
                .iter()
                .position(|byte| (0x40..=0x7e).contains(byte))
            else {
                if self.pending.len() > 64 {
                    forwarded.push(self.pending.remove(0));
                    continue;
                }
                return forwarded;
            };
            let sequence: Vec<u8> = self
                .pending
                .drain(..=end + ansi::SGR_MOUSE_PREFIX.len())
                .collect();
            match remap_sgr_mouse(&sequence, size, &mut self.pressed) {
                Some(Some(remapped)) => forwarded.extend(remapped),
                Some(None) => {}
                None => forwarded.extend(sequence),
            }
        }
    }

    pub(crate) fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

fn remap_sgr_mouse(
    sequence: &[u8],
    size: Option<(u16, u16)>,
    pressed: &mut u8,
) -> Option<Option<Vec<u8>>> {
    let (&final_byte, params) = sequence.split_last()?;
    if !matches!(final_byte, b'M' | b'm') {
        return None;
    }
    let mut params = params
        .strip_prefix(ansi::SGR_MOUSE_PREFIX)?
        .split(|byte| *byte == b';');
    let button = parse_u16(params.next()?)?;
    let x = parse_u16(params.next()?)?;
    let y = parse_u16(params.next()?)?;
    if params.next().is_some() {
        return None;
    }
    let mut x = x.checked_sub(1)?;
    let mut y = y.checked_sub(1)?;
    let base_button = (button & 0b11) as u8;
    let button_bit = (base_button < 3).then(|| 1 << base_button);
    let Some(size) = size else {
        if final_byte == b'm' {
            if let Some(bit) = button_bit {
                *pressed &= !bit;
            }
        }
        return Some(None);
    };
    let outside = x == 0 || y == 0 || x > size.0 || y > size.1;
    if final_byte == b'm' {
        let bit = button_bit?;
        if *pressed & bit == 0 {
            return Some(None);
        }
        *pressed &= !bit;
        if outside {
            if size.0 == 0 || size.1 == 0 {
                return Some(None);
            }
            x = x.clamp(1, size.0);
            y = y.clamp(1, size.1);
        }
    } else {
        if outside {
            return Some(None);
        }
        let motion = button & 32 != 0;
        let wheel = button & 64 != 0;
        if motion && button_bit.is_some_and(|bit| *pressed & bit == 0) {
            return Some(None);
        }
        if !motion && !wheel {
            if let Some(bit) = button_bit {
                *pressed |= bit;
            }
        }
    }
    Some(Some(ansi::sgr_mouse(button, x, y, final_byte)))
}

fn parse_u16(bytes: &[u8]) -> Option<u16> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

struct InputStream {
    sender: mpsc::Sender<Vec<u8>>,
    connected: Arc<AtomicBool>,
}

impl InputStream {
    fn connect(socket: &str, viewer: (u16, u16)) -> std::io::Result<Self> {
        let conn = crate::ipc::connect(socket)?;
        let mut conn = BufReader::new(conn);
        let mut request = serde_json::to_vec(&crate::protocol::Request::MonitorInputStream {
            cols: viewer.0,
            rows: viewer.1,
        })
        .map_err(std::io::Error::other)?;
        request.push(b'\n');
        conn.get_mut().write_all(&request)?;
        conn.get_mut().flush()?;
        let mut response = String::new();
        conn.read_line(&mut response)?;
        let response: crate::protocol::Response =
            serde_json::from_str(response.trim()).map_err(std::io::Error::other)?;
        if !response.ok {
            return Err(std::io::Error::other("monitor input stream rejected"));
        }
        let mut conn = conn.into_inner();

        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        let connected = Arc::new(AtomicBool::new(true));
        let writer_connected = Arc::clone(&connected);
        std::thread::spawn(move || {
            for bytes in receiver {
                if conn.write_all(&bytes).is_err() || conn.flush().is_err() {
                    break;
                }
            }
            writer_connected.store(false, Ordering::Relaxed);
        });
        Ok(Self { sender, connected })
    }

    fn send(&self, bytes: Vec<u8>) -> bool {
        bytes.is_empty()
            || (self.connected.load(Ordering::Relaxed) && self.sender.send(bytes).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(ch: &str) -> EmuCell {
        EmuCell {
            ch: ch.into(),
            ..EmuCell::blank()
        }
    }

    /// `sgr` writes to the viewer's real terminal, so the only honest check is
    /// to feed it back through an emulator and see the same style come out.
    /// A malformed escape does not fail loudly; it silently reassigns the
    /// parameters that follow it, which is how `58` once swallowed the
    /// foreground color.
    #[test]
    fn sgr_survives_a_round_trip_through_the_emulator() {
        use tui_test::terminal::{alacritty::AlacrittyEmu, cell::NamedColor, emu::Emulator};

        let styles = [
            Style {
                fg: Some(Color::Named(NamedColor::Red)),
                bg: Some(Color::Idx(196)),
                underline: UnderlineStyle::Curly,
                underline_color: Some(Color::Rgb(1, 2, 3)),
                attrs: Attrs::BOLD | Attrs::ITALIC | Attrs::STRIKE,
            },
            Style {
                fg: Some(Color::Rgb(9, 8, 7)),
                bg: None,
                underline: UnderlineStyle::Dotted,
                underline_color: Some(Color::Idx(33)),
                attrs: Attrs::DIM,
            },
            Style {
                fg: None,
                bg: Some(Color::Named(NamedColor::BrightWhite)),
                underline: UnderlineStyle::Single,
                underline_color: None,
                attrs: Attrs::empty(),
            },
        ];

        for want in styles {
            let mut emu = AlacrittyEmu::new(10, 2, &tui_test::profile::Profile::default());
            emu.process(want.sgr().as_bytes());
            emu.process(b"x");
            let got = Style::from(&emu.viewable_rows()[0][0]);
            assert!(
                got == want,
                "{:?} round-tripped to fg={:?} bg={:?} underline={:?}/{:?} attrs={:?}",
                want.sgr(),
                got.fg,
                got.bg,
                got.underline,
                got.underline_color,
                got.attrs,
            );
        }
    }

    #[test]
    fn render_includes_frame_and_content() {
        let frame = Frame {
            grid: vec![vec![cell("h"), cell("i")]],
            cursor: (0, 0),
            size: (40, 1),
            keyboard_mode: KeyboardMode::empty(),
            bracketed_paste: false,
            mouse_mode: MouseMode::None,
            exited: None,
            shell: Some("bash"),
        };
        let bytes = render_frame(
            Some(&frame),
            (50, 6),
            "default",
            false,
            &mut ModeMirror::default(),
        );
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains('┌') && text.contains('┘'));
        assert!(text.contains("bash"));
        assert!(text.contains('h') && text.contains('i'));
        assert!(text.starts_with("\x1b[H"));
    }

    #[test]
    fn render_placeholder_without_session() {
        let bytes = render_frame(None, (40, 6), "work", false, &mut ModeMirror::default());
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
            keyboard_mode: KeyboardMode::empty(),
            bracketed_paste: false,
            mouse_mode: MouseMode::None,
            exited: None,
            shell: None,
        };
        let text = String::from_utf8(render_frame(
            Some(&frame),
            (10, 5),
            "s",
            false,
            &mut ModeMirror::default(),
        ))
        .unwrap();
        assert!(text.contains(";7") || text.contains("[7"));
    }

    /// The viewer's terminal mirrors target input modes only when they change.
    #[test]
    fn interactive_render_mirrors_target_modes_when_they_change() {
        let mut frame = Frame {
            grid: vec![vec![cell("x")]],
            cursor: (0, 0),
            size: (1, 1),
            keyboard_mode: KeyboardMode::empty(),
            bracketed_paste: false,
            mouse_mode: MouseMode::None,
            exited: None,
            shell: None,
        };
        let mut modes = ModeMirror::default();
        let render = |frame: Option<&Frame>, modes: &mut ModeMirror| {
            render_frame(frame, (10, 5), "s", true, modes)
        };

        assert!(render(Some(&frame), &mut modes)
            .starts_with(b"\x1b[=0u\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[H"));
        assert!(render(Some(&frame), &mut modes).starts_with(b"\x1b[H"));

        frame.keyboard_mode =
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ASSOCIATED_TEXT;
        frame.bracketed_paste = true;
        frame.mouse_mode = MouseMode::Drag;
        assert!(render(Some(&frame), &mut modes).starts_with(
            b"\x1b[=17u\x1b[?2004h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1006h\x1b[?1002h\x1b[H"
        ));
        assert!(render(None, &mut modes)
            .starts_with(b"\x1b[=0u\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[H"));
        assert!(render_frame(
            Some(&frame),
            (10, 5),
            "s",
            false,
            &mut ModeMirror::default()
        )
        .starts_with(b"\x1b[H"));
    }

    /// Interactive mode restores viewer input modes; read-only mode leaves them
    /// untouched.
    #[test]
    fn viewer_saves_and_restores_terminal_modes() {
        let mut output = Vec::new();
        enter_viewer(&mut output, true);
        leave_viewer(&mut output, true);
        let text = String::from_utf8(output).unwrap();
        let push = std::str::from_utf8(ansi::KITTY_KEYBOARD_PUSH).unwrap();
        let pop = std::str::from_utf8(ansi::KITTY_KEYBOARD_POP).unwrap();
        let order = [
            "\x1b[?2004s",
            push,
            pop,
            "\x1b[?2004l",
            "\x1b[?2004r",
            "\x1b[?1049l",
        ];
        let found: Vec<_> = order.iter().map(|sequence| text.find(sequence)).collect();
        assert!(
            found.windows(2).all(|at| at[0].is_some() && at[0] < at[1]),
            "out of order: {found:?} in {text:?}"
        );

        let mut read_only = Vec::new();
        enter_viewer(&mut read_only, false);
        leave_viewer(&mut read_only, false);
        assert!(!read_only
            .windows(b"\x1b[?2004".len())
            .any(|window| window == b"\x1b[?2004"));
    }

    /// Viewer keystrokes reach the target untouched; only the detach chord is
    /// swallowed, including a kitty encoding split across reads.
    #[test]
    fn interactive_input_is_raw_except_for_the_detach_chord() {
        let raw = b"\x03text \xff\x1b[200~paste\n\x1b[201~";
        let mut input = DetachParser::default();
        assert_eq!(input.push(raw), (raw.to_vec(), false));
        assert_eq!(input.push(b"before\x1dafter"), (b"before".to_vec(), true));

        for chord in [
            b"\x1b[93;5u".as_slice(),
            b"\x1b[93;69:1u",
            b"\x1b[93;197:1;29u",
        ] {
            let mut input = DetachParser::default();
            let split = chord.len() - 2;
            assert_eq!(input.push(&chord[..split]), (Vec::new(), false));
            assert_eq!(input.push(&chord[split..]), (Vec::new(), true));
        }

        // Ctrl+Shift+] is a different chord, so it belongs to the target.
        let mut input = DetachParser::default();
        assert_eq!(input.push(b"\x1b[93;6u"), (b"\x1b[93;6u".to_vec(), false));
    }

    #[test]
    fn monitor_mouse_reports_map_to_the_target_grid() {
        let mut mouse = MouseRemapper::new((12, 7));
        assert_eq!(
            mouse.push(b"text\x1b[<0;2", Some((10, 5))),
            b"text".to_vec()
        );
        assert_eq!(
            mouse.push(b";2Mtail", Some((10, 5))),
            b"\x1b[<0;1;1Mtail".to_vec()
        );
        assert_eq!(
            mouse.push(b"\x1b[<64;11;6M", Some((10, 5))),
            b"\x1b[<64;10;5M".to_vec()
        );
        assert!(mouse
            .push(b"\x1b[<0;1;2M\x1b[<0;12;2M", Some((10, 5)))
            .is_empty());
        assert_eq!(mouse.push(b"\x1b[31m", Some((10, 5))), b"\x1b[31m".to_vec());

        let mut clipped = MouseRemapper::new((8, 4));
        assert!(clipped.push(b"\x1b[<0;7;4M", Some((10, 5))).is_empty());
        assert!(clipped.push(b"\x1b[<0;7;4m", Some((10, 5))).is_empty());
        assert_eq!(
            clipped.push(b"\x1b[<0;2;2M", Some((10, 5))),
            b"\x1b[<0;1;1M".to_vec()
        );
        assert_eq!(
            clipped.push(b"\x1b[<0;7;4m", Some((10, 5))),
            b"\x1b[<0;6;2m".to_vec()
        );

        let mut idle = MouseRemapper::new((12, 7));
        assert!(idle.push(b"\x1b", Some((10, 5))).is_empty());
        assert_eq!(idle.finish(), b"\x1b");

        let mut disabled = MouseRemapper::new((12, 7));
        assert_eq!(
            disabled.push(b"\x1b[<0;2;2M", None),
            b"\x1b[<0;2;2M".to_vec()
        );

        let mut observed = MouseRemapper::new((12, 7));
        observed.observe(Some((10, 5)));
        observed.observe(None);
        assert!(observed.push(b"\x1b[<0;2;2M", None).is_empty());

        let mut turning_off = MouseRemapper::new((12, 7));
        assert_eq!(
            turning_off.push(b"\x1b[<0;2;2M", Some((10, 5))),
            b"\x1b[<0;1;1M".to_vec()
        );
        assert!(turning_off.push(b"\x1b[<0;2;2m", None).is_empty());
        assert!(turning_off.push(b"\x1b[<64;2;2M", None).is_empty());
        assert_eq!(turning_off.push(b"a", None), b"a".to_vec());
    }
}
