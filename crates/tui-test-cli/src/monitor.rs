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

#[cfg(test)]
use tui_test::engine::LiveFrame as Frame;
pub(crate) use tui_test::monitoring::input::MouseRemapper;
pub(crate) use tui_test::monitoring::render::{render_frame, ModeMirror};
#[cfg(test)]
use tui_test::terminal::cell::EmuCell;
#[cfg(test)]
use tui_test::terminal::emu::{KeyboardMode, MouseMode};

use crate::ansi;
#[cfg(windows)]
use crate::console_input::ConsoleInput;
use crate::monitor_input::{InputAction, InputEvent, InputParser};
use crate::protocol::MonitorInput;

#[derive(Clone)]
enum MonitorTarget {
    Daemon(String),
    Host {
        endpoint: String,
        session: String,
        generation: u64,
        lease: Option<u64>,
    },
}

impl MonitorTarget {
    fn endpoint(&self) -> &str {
        match self {
            Self::Daemon(endpoint) | Self::Host { endpoint, .. } => endpoint,
        }
    }

    fn request(&self, request: crate::protocol::Request) -> crate::protocol::Request {
        match self {
            Self::Daemon(_) => request,
            Self::Host {
                session,
                generation,
                lease,
                ..
            } => crate::protocol::Request::Routed {
                session: session.clone(),
                generation: *generation,
                lease: *lease,
                request: Box::new(request),
            },
        }
    }
}

/// Run the interactive monitor client for `session` until the viewer quits or
/// the session owner goes away. Returns a process exit code.
pub fn run_client(
    session: Option<&str>,
    interactive: bool,
    id: Option<&str>,
    latest: bool,
    filter: &crate::cli::SessionFilter,
) -> i32 {
    let mut target = match resolve_target(session, interactive, id, latest, filter) {
        Ok(Some(target)) => target,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("{}", error.message);
            return match error.kind {
                tui_test::ErrorKind::NoSession => 3,
                tui_test::ErrorKind::Usage => 2,
                _ => 5,
            };
        }
    };
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        eprintln!("`monitor` requires an interactive terminal");
        return 2;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!("`monitor` requires terminal stdin");
        return 2;
    }
    let _lease = match LeaseStream::connect(&mut target, interactive) {
        Ok(lease) => lease,
        Err(error) => {
            eprintln!("failed to attach monitor: {error}");
            return 4;
        }
    };

    let size = crossterm::terminal::size().unwrap_or((80, 24));
    let input_stream = if interactive {
        match InputStream::connect(&target, size) {
            Ok(stream) => Some(stream),
            Err(error) => {
                eprintln!("failed to attach monitor input: {error}");
                return 4;
            }
        }
    } else {
        None
    };

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
    if let Err(error) = enter_viewer(
        &mut viewer.stdout,
        input_stream
            .as_ref()
            .map(|stream| stream.initial_frame.as_slice()),
    ) {
        eprintln!("failed to initialize monitor: {error}");
        return 5;
    }
    let input = match input_stream {
        Some(stream) => ViewerInput::Interactive(Box::new(InteractiveInput {
            stdin: spawn_stdin_reader(),
            stream,
            parser: InputParser::default(),
        })),
        None => ViewerInput::ReadOnly,
    };
    stream_loop(&target, input, size)
}

fn resolve_target(
    session: Option<&str>,
    interactive: bool,
    id: Option<&str>,
    latest: bool,
    filter: &crate::cli::SessionFilter,
) -> Result<Option<MonitorTarget>, tui_test::TuiTestError> {
    use crate::discovery::Candidate;
    let picker = std::io::IsTerminal::is_terminal(&std::io::stdout())
        && std::io::IsTerminal::is_terminal(&std::io::stdin());
    Ok(
        crate::discovery::select(session, id, latest, filter, interactive, picker)?.map(
            |candidate| match candidate {
                Candidate::Daemon(name) => MonitorTarget::Daemon(crate::config::socket_name(&name)),
                Candidate::Process(candidate) => MonitorTarget::Host {
                    endpoint: candidate.descriptor.endpoint,
                    session: candidate.session.session,
                    generation: candidate.session.generation,
                    lease: None,
                },
            },
        ),
    )
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

fn enter_viewer(out: &mut impl Write, initial_frame: Option<&[u8]>) -> std::io::Result<()> {
    crossterm::execute!(
        out,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )?;
    if let Some(frame) = initial_frame {
        out.write_all(ansi::BRACKETED_PASTE_SAVE)?;
        out.write_all(ansi::MOUSE_SAVE)?;
        // Push saves the viewer's old flags and selects zero. Apply the target's
        // complete mode snapshot before starting the stdin reader.
        out.write_all(ansi::KITTY_KEYBOARD_SAVE_AND_RESET)?;
        out.write_all(frame)?;
        out.flush()?;
    }
    Ok(())
}

fn leave_viewer(out: &mut impl Write, interactive: bool) {
    if interactive {
        let _ = out.write_all(ansi::KITTY_KEYBOARD_POP);
        let _ = out.write_all(ansi::BRACKETED_PASTE_DISABLE.as_bytes());
        let _ = out.write_all(ansi::BRACKETED_PASTE_RESTORE);
        let _ = out.write_all(ansi::MOUSE_DISABLE.as_bytes());
        let _ = out.write_all(ansi::MOUSE_RESTORE);
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
}

#[cfg(windows)]
impl VirtualTerminalInput {
    fn enable() -> std::io::Result<Self> {
        use windows_sys::Win32::System::Console::{
            GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_INPUT,
            STD_INPUT_HANDLE,
        };

        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut original_mode = 0;
            if handle.is_null() || GetConsoleMode(handle, &mut original_mode) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if SetConsoleMode(handle, original_mode | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self {
                handle,
                original_mode,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for VirtualTerminalInput {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::SetConsoleMode;

        unsafe {
            SetConsoleMode(self.handle, self.original_mode);
        }
    }
}

/// Read viewer stdin on its own thread; the channel closes when stdin does.
fn spawn_stdin_reader() -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        #[cfg(windows)]
        let mut stdin = ConsoleInput::new();
        #[cfg(not(windows))]
        let mut stdin = std::io::stdin().lock();
        let mut buffer = [0; 4096];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) if sender.send(Ok(buffer[..read].to_vec())).is_err() => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    receiver
}

enum ViewerInput {
    ReadOnly,
    Interactive(Box<InteractiveInput>),
}

struct InteractiveInput {
    stdin: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    stream: InputStream,
    parser: InputParser,
}

enum ViewerAction {
    Stop,
    Resize((u16, u16)),
}

fn stream_loop(target: &MonitorTarget, mut input: ViewerInput, mut viewer: (u16, u16)) -> i32 {
    use crate::ipc;
    use crate::protocol::Request;

    let interactive = matches!(&input, ViewerInput::Interactive(_));
    loop {
        let (vcols, vrows) = viewer;
        let mut conn = match ipc::connect(target.endpoint()) {
            Ok(c) => c,
            Err(_) => return 4,
        };
        let mut line = match serde_json::to_string(&target.request(Request::Monitor {
            cols: vcols,
            rows: vrows,
            interactive,
        })) {
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
                            if out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
                                disconnected.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                }
            })
        };

        let action = viewer_input_loop(viewer, &mut input, &disconnected);
        stop.store(true, Ordering::Relaxed);
        let _ = reader.join();

        viewer = match action {
            Ok(ViewerAction::Stop) => return 0,
            Ok(ViewerAction::Resize(size)) => size,
            Err(error) => {
                eprintln!("monitor input failed: {error}");
                return 5;
            }
        };
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        );
        if let ViewerInput::Interactive(input) = &input {
            if let Err(error) = input.stream.resize(viewer) {
                eprintln!("failed to resize monitor input: {error}");
                return 4;
            }
        }
    }
}

fn viewer_input_loop(
    viewer: (u16, u16),
    input: &mut ViewerInput,
    disconnected: &AtomicBool,
) -> std::io::Result<ViewerAction> {
    loop {
        if disconnected.load(Ordering::Relaxed) {
            return Ok(ViewerAction::Stop);
        }
        if let Ok(size) = crossterm::terminal::size() {
            if size != viewer {
                return Ok(ViewerAction::Resize(size));
            }
        }
        let action = match input {
            ViewerInput::ReadOnly => read_only_input(),
            ViewerInput::Interactive(input) => input.poll()?,
        };
        if let Some(action) = action {
            return Ok(action);
        }
    }
}

impl InteractiveInput {
    fn poll(&mut self) -> std::io::Result<Option<ViewerAction>> {
        match self.stdin.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                let (forward, detached) = self.parser.push(&bytes?, |event| match event {
                    InputEvent::Detach => InputAction::Detach,
                    _ => InputAction::Forward,
                });
                self.stream.send(forward)?;
                Ok(detached.then_some(ViewerAction::Stop))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.stream.send(self.parser.finish())?;
                Ok(Some(ViewerAction::Stop))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.stream.send(self.parser.on_idle())?;
                Ok(None)
            }
        }
    }
}

fn read_only_input() -> Option<ViewerAction> {
    use crossterm::event::{Event, KeyCode, KeyModifiers};

    if !crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false) {
        return None;
    }
    match crossterm::event::read() {
        Ok(Event::Key(key)) => {
            let ctrl_c =
                key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
            (ctrl_c || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc))
                .then_some(ViewerAction::Stop)
        }
        Ok(Event::Resize(cols, rows)) => Some(ViewerAction::Resize((cols, rows))),
        Ok(_) => None,
        Err(_) => Some(ViewerAction::Stop),
    }
}

struct InputStream {
    sender: mpsc::Sender<MonitorInput>,
    connected: Arc<AtomicBool>,
    initial_frame: Vec<u8>,
}

struct LeaseStream {
    _connection: crate::ipc::Stream,
}

impl LeaseStream {
    fn connect(target: &mut MonitorTarget, interactive: bool) -> std::io::Result<Option<Self>> {
        if matches!(target, MonitorTarget::Daemon(_)) {
            return Ok(None);
        }
        let (connection, response) = connect_stream(
            target,
            crate::protocol::Request::MonitorLeaseStream { interactive },
        )?;
        let ready = serde_json::from_value::<crate::protocol::MonitorLeaseReady>(
            response
                .data
                .ok_or_else(|| std::io::Error::other("missing monitor lease"))?,
        )
        .map_err(std::io::Error::other)?;
        if let MonitorTarget::Host { lease, .. } = target {
            *lease = Some(ready.lease);
        }
        Ok(Some(Self {
            _connection: connection,
        }))
    }
}

impl InputStream {
    fn connect(target: &MonitorTarget, viewer: (u16, u16)) -> std::io::Result<Self> {
        let (mut conn, response) = connect_stream(
            target,
            crate::protocol::Request::MonitorInputStream {
                cols: viewer.0,
                rows: viewer.1,
            },
        )?;
        let data = response
            .data
            .ok_or_else(|| std::io::Error::other("missing monitor input handshake"))?;
        let initial_frame = serde_json::from_value::<crate::protocol::MonitorInputReady>(data)
            .map_err(std::io::Error::other)?
            .initial_frame;

        let (sender, receiver) = mpsc::channel::<MonitorInput>();
        let connected = Arc::new(AtomicBool::new(true));
        let writer_connected = Arc::clone(&connected);
        std::thread::spawn(move || {
            for message in receiver {
                if serde_json::to_writer(&mut conn, &message).is_err()
                    || conn.write_all(b"\n").is_err()
                    || conn.flush().is_err()
                {
                    break;
                }
            }
            writer_connected.store(false, Ordering::Relaxed);
        });
        Ok(Self {
            sender,
            connected,
            initial_frame,
        })
    }

    fn send(&self, bytes: Vec<u8>) -> std::io::Result<()> {
        if bytes.is_empty() {
            self.check_connected()
        } else {
            self.enqueue(MonitorInput::Write { data: bytes })
        }
    }

    fn resize(&self, viewer: (u16, u16)) -> std::io::Result<()> {
        self.enqueue(MonitorInput::Resize {
            cols: viewer.0,
            rows: viewer.1,
        })
    }

    fn check_connected(&self) -> std::io::Result<()> {
        if self.connected.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "monitor input disconnected",
            ))
        }
    }

    fn enqueue(&self, message: MonitorInput) -> std::io::Result<()> {
        self.check_connected()?;
        self.sender.send(message).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "monitor input disconnected")
        })
    }
}

fn connect_stream(
    target: &MonitorTarget,
    request: crate::protocol::Request,
) -> std::io::Result<(crate::ipc::Stream, crate::protocol::Response)> {
    let conn = crate::ipc::connect(target.endpoint())?;
    let mut conn = BufReader::new(conn);
    let mut request =
        serde_json::to_vec(&target.request(request)).map_err(std::io::Error::other)?;
    request.push(b'\n');
    conn.get_mut().write_all(&request)?;
    conn.get_mut().flush()?;
    let mut response = String::new();
    conn.read_line(&mut response)?;
    let response: crate::protocol::Response =
        serde_json::from_str(response.trim()).map_err(std::io::Error::other)?;
    if !response.ok {
        return Err(std::io::Error::other(
            response
                .message
                .unwrap_or_else(|| "monitor stream rejected".into()),
        ));
    }
    Ok((conn.into_inner(), response))
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
        enter_viewer(&mut output, Some(b"\x1b[=17u")).unwrap();
        leave_viewer(&mut output, true);
        let text = String::from_utf8(output).unwrap();
        let push = std::str::from_utf8(ansi::KITTY_KEYBOARD_SAVE_AND_RESET).unwrap();
        let pop = std::str::from_utf8(ansi::KITTY_KEYBOARD_POP).unwrap();
        let order = [
            "\x1b[?2004s",
            push,
            "\x1b[=17u",
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
        enter_viewer(&mut read_only, None).unwrap();
        leave_viewer(&mut read_only, false);
        assert!(!read_only
            .windows(b"\x1b[?2004".len())
            .any(|window| window == b"\x1b[?2004"));
    }

    #[test]
    fn viewer_applies_target_keyboard_mode_before_input_and_restores_previous_mode() {
        use tui_test::terminal::{alacritty::AlacrittyEmu, emu::Emulator};
        let mut emu = AlacrittyEmu::new(20, 5, &tui_test::profile::Profile::default());
        emu.process(b"\x1b[>3u");
        let mut output = Vec::new();
        enter_viewer(&mut output, Some(b"\x1b[=17u")).unwrap();
        emu.process(&output);
        assert_eq!(emu.keyboard_mode().bits(), 17);
        output.clear();
        leave_viewer(&mut output, true);
        emu.process(&output);
        assert_eq!(emu.keyboard_mode().bits(), 3);
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

    #[test]
    fn monitor_mouse_reports_survive_idle_and_leave_paste_untouched() {
        let mut mouse = MouseRemapper::new((12, 7));
        assert!(mouse.push(b"\x1b[<0;2", Some((10, 5))).is_empty());
        assert!(mouse.on_idle().is_empty());
        assert_eq!(mouse.push(b";2M", Some((10, 5))), b"\x1b[<0;1;1M");
        let paste = b"\x1b[200~\x1b[<0;2;2M\x1d\x1b[93;5u\x1b[201~";
        let mut forwarded = Vec::new();
        for byte in paste {
            forwarded.extend(mouse.push(&[*byte], Some((10, 5))));
        }
        assert_eq!(forwarded, paste);
        let mut mouse = MouseRemapper::new((12, 7));
        assert_eq!(mouse.push(paste, Some((10, 5))), paste);
    }

    #[test]
    fn monitor_resize_preserves_mouse_buttons_and_paste_state() {
        let mut mouse = MouseRemapper::new((12, 7));
        assert_eq!(mouse.push(b"\x1b[<0;2;2M", Some((10, 5))), b"\x1b[<0;1;1M");
        mouse.resize((8, 4));
        assert_eq!(mouse.push(b"\x1b[<0;9;6m", Some((10, 5))), b"\x1b[<0;6;2m");

        assert_eq!(mouse.push(b"\x1b[200~", Some((10, 5))), b"\x1b[200~");
        mouse.resize((15, 10));
        let payload = b"\x1b[<0;2;2M\x1b[201~";
        assert_eq!(mouse.push(payload, Some((10, 5))), payload);
    }
}
