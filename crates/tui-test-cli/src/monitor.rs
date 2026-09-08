//! Live session monitor: a human watches what an agent is driving.
//!
//! The daemon renders the live emulator grid into a framed, full-color ANSI
//! frame (see [`render_frame`]) and streams one every ~20fps over the session
//! socket. The client ([`run_client`]) takes over an alternate screen in raw
//! mode and blits those frames, so the viewer sees the session in real time
//! while the agent keeps driving it through the same daemon.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

#[cfg(test)]
use tui_test::engine::LiveFrame as Frame;
#[cfg(test)]
use tui_test::monitoring::input::MouseRemapper;
use tui_test::monitoring::ipc::{Connection, Incoming};
use tui_test::monitoring::protocol::{
    Attach, MonitorInput, MonitorOutput, MonitorReady, Request, Response, Route, VERSION,
};
#[cfg(test)]
use tui_test::monitoring::render::{render_frame, ModeMirror};
#[cfg(test)]
use tui_test::terminal::cell::EmuCell;
#[cfg(test)]
use tui_test::terminal::emu::{KeyboardMode, MouseMode};
use tui_test::TuiTestError;

use crate::ansi;
#[cfg(windows)]
use crate::console_input::ConsoleInput;
use crate::monitor_input::{InputAction, InputEvent, InputParser};

#[derive(Clone)]
enum MonitorTarget {
    Daemon(String),
    Host {
        endpoint: String,
        session: String,
        generation: u64,
    },
}

impl MonitorTarget {
    fn endpoint(&self) -> &str {
        match self {
            Self::Daemon(endpoint) | Self::Host { endpoint, .. } => endpoint,
        }
    }

    fn request(&self, size: (u16, u16), interactive: bool) -> Request {
        let mut attach = Attach::new(size.0, size.1, interactive);
        if let Self::Host {
            session,
            generation,
            ..
        } = self
        {
            attach.route = Some(Route {
                session: session.clone(),
                generation: *generation,
            });
        }
        Request::Monitor(attach)
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
    let target = match resolve_target(session, interactive, id, latest, filter) {
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
    let size = crossterm::terminal::size().unwrap_or((80, 24));
    let network = match ViewerConnection::connect(&target, size, interactive) {
        Ok(network) => network,
        Err(error) => {
            eprintln!("failed to attach monitor: {error}");
            return 4;
        }
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
        interactive.then_some(network.initial_frame.as_bytes()),
    ) {
        eprintln!("failed to initialize monitor: {error}");
        return 5;
    }
    if !interactive {
        if let Err(error) = viewer
            .stdout
            .write_all(network.initial_frame.as_bytes())
            .and_then(|_| viewer.stdout.flush())
        {
            eprintln!("failed to display monitor: {error}");
            return 5;
        }
    }
    let input = if interactive {
        ViewerInput::Interactive(Box::new(InteractiveInput {
            stdin: spawn_stdin_reader(),
            parser: InputParser::default(),
        }))
    } else {
        ViewerInput::ReadOnly
    };
    match stream_loop(&network, input, size) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{}", error.message);
            error.kind.exit_code()
        }
    }
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
    parser: InputParser,
}

enum ViewerAction {
    Stop,
    Resize((u16, u16)),
}

fn stream_loop(
    network: &ViewerConnection,
    mut input: ViewerInput,
    mut size: (u16, u16),
) -> Result<(), TuiTestError> {
    loop {
        let (frames, result) = {
            let mut state = network
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (std::mem::take(&mut state.frames), state.result.take())
        };
        for frame in frames {
            let mut out = std::io::stdout().lock();
            out.write_all(frame.as_bytes())
                .and_then(|_| out.flush())
                .map_err(|error| TuiTestError::internal(error.to_string()))?;
        }
        if let Some(result) = result {
            return result;
        }
        if let Ok(current) = crossterm::terminal::size() {
            if current != size {
                network.send(MonitorInput::Resize {
                    cols: current.0,
                    rows: current.1,
                })?;
                size = current;
            }
        }
        let action = match &mut input {
            ViewerInput::ReadOnly => read_only_input(),
            ViewerInput::Interactive(input) => input.poll(network)?,
        };
        match action {
            Some(ViewerAction::Stop) => {
                return network.detach();
            }
            Some(ViewerAction::Resize(current)) if current != size => {
                network.send(MonitorInput::Resize {
                    cols: current.0,
                    rows: current.1,
                })?;
                size = current;
            }
            _ => {}
        }
    }
}

impl InteractiveInput {
    fn poll(&mut self, network: &ViewerConnection) -> Result<Option<ViewerAction>, TuiTestError> {
        let (bytes, detached) = match self.stdin.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => self.parser.push(
                &bytes.map_err(|error| TuiTestError::internal(error.to_string()))?,
                |event| match event {
                    InputEvent::Detach => InputAction::Detach,
                    _ => InputAction::Forward,
                },
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => (self.parser.on_idle(), false),
            Err(mpsc::RecvTimeoutError::Disconnected) => (self.parser.finish(), true),
        };
        if !bytes.is_empty() {
            network.send(MonitorInput::Write { data: bytes })?;
        }
        Ok(detached.then_some(ViewerAction::Stop))
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

#[derive(Default)]
struct NetworkState {
    frames: VecDeque<String>,
    result: Option<Result<(), TuiTestError>>,
}

struct ViewerConnection {
    initial_frame: String,
    sender: mpsc::Sender<MonitorInput>,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<NetworkState>>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl ViewerConnection {
    fn connect(
        target: &MonitorTarget,
        size: (u16, u16),
        interactive: bool,
    ) -> std::io::Result<Self> {
        let mut connection = Connection::new(crate::ipc::connect(target.endpoint())?)?;
        let writer = connection.writer();
        let stop = Arc::new(AtomicBool::new(false));
        writer.send(&target.request(size, interactive), &stop)?;
        let response: Response = connection.receive(Some(Duration::from_secs(5)), &stop)?;
        if !response.ok {
            return Err(std::io::Error::other(
                response
                    .message
                    .unwrap_or_else(|| "monitor attachment rejected".into()),
            ));
        }
        let ready: MonitorReady = serde_json::from_value(
            response
                .data
                .ok_or_else(|| std::io::Error::other("missing monitor handshake"))?,
        )
        .map_err(std::io::Error::other)?;
        if ready.protocol != VERSION {
            return Err(std::io::Error::other("incompatible monitor protocol"));
        }
        let state = Arc::new(Mutex::new(NetworkState::default()));
        let reader_state = state.clone();
        let reader_stop = stop.clone();
        let reader = std::thread::spawn(move || {
            while !reader_stop.load(Ordering::Acquire) {
                let result = match connection.try_receive::<MonitorOutput>() {
                    Ok(Incoming::Message(MonitorOutput::Frame { frame })) => {
                        // Mode changes are embedded in frames: preserve order rather
                        // than dropping a frame whose modes later frames depend on.
                        loop {
                            let mut state = reader_state
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.frames.len() < 2 {
                                state.frames.push_back(frame);
                                break;
                            }
                            drop(state);
                            if reader_stop.load(Ordering::Acquire) {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        continue;
                    }
                    Ok(Incoming::Pending) => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Ok(Incoming::Closed | Incoming::Message(MonitorOutput::Closed)) => Ok(()),
                    Ok(Incoming::Message(MonitorOutput::Error {
                        message,
                        error_kind,
                    })) => Err(TuiTestError::new(error_kind, message)),
                    Err(error) => Err(TuiTestError::internal(error.to_string())),
                };
                Self::finish(&reader_state, result);
                reader_stop.store(true, Ordering::Release);
                break;
            }
        });
        let (sender, receiver) = mpsc::channel::<MonitorInput>();
        let writer_state = state.clone();
        let writer_stop = stop.clone();
        let writer = std::thread::spawn(move || {
            while !writer_stop.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(20)) {
                    Ok(message) => {
                        if let Err(error) = writer.send(&message, &writer_stop) {
                            if !writer_stop.load(Ordering::Acquire) {
                                Self::finish(
                                    &writer_state,
                                    Err(TuiTestError::internal(error.to_string())),
                                );
                                writer_stop.store(true, Ordering::Release);
                            }
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Ok(Self {
            initial_frame: ready.frame,
            sender,
            stop,
            state,
            workers: vec![reader, writer],
        })
    }

    fn finish(state: &Mutex<NetworkState>, result: Result<(), TuiTestError>) {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.result.is_none() {
            state.result = Some(result);
        }
    }

    fn send(&self, message: MonitorInput) -> Result<(), TuiTestError> {
        self.sender
            .send(message)
            .map_err(|_| TuiTestError::internal("monitor connection closed"))
    }

    fn detach(&self) -> Result<(), TuiTestError> {
        self.send(MonitorInput::Detach)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Keep receiving until the server acknowledges all input preceding Detach.
            // No further frames need painting while the viewer is leaving.
            state.frames.clear();
            if let Some(result) = state.result.take() {
                return result;
            }
            drop(state);
            if std::time::Instant::now() >= deadline {
                return Err(TuiTestError::internal("monitor detach timed out"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for ViewerConnection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_viewer(server: impl FnOnce(Connection) + Send, client: impl FnOnce(ViewerConnection)) {
        use interprocess::local_socket::traits::Listener;
        use tui_test::monitoring::ipc;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let endpoint = std::env::temp_dir()
            .join(format!(
                "tt-viewer-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned();
        let listener = ipc::listen(&endpoint).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut connection = Connection::new(listener.accept().unwrap()).unwrap();
                let stop = AtomicBool::new(false);
                let _: Request = connection
                    .receive(Some(Duration::from_secs(2)), &stop)
                    .unwrap();
                let writer = connection.writer();
                writer
                    .send(
                        &Response::with(
                            serde_json::to_value(MonitorReady {
                                protocol: VERSION,
                                frame: String::new(),
                            })
                            .unwrap(),
                        ),
                        &stop,
                    )
                    .unwrap();
                server(connection);
            });
            client(
                ViewerConnection::connect(&MonitorTarget::Daemon(endpoint), (80, 24), true)
                    .unwrap(),
            );
        });
    }

    #[test]
    fn detach_delivers_input_from_the_same_stdin_chunk_before_closing() {
        let mut bytes = vec![b'x'; 32 * 1024];
        bytes.extend_from_slice(b"\r\x1d");
        with_viewer(
            |mut connection| {
                let stop = AtomicBool::new(false);
                std::thread::sleep(Duration::from_millis(100));
                let message = connection
                    .receive::<MonitorInput>(Some(Duration::from_secs(2)), &stop)
                    .unwrap();
                let MonitorInput::Write { data } = message else {
                    panic!("expected input before detach")
                };
                assert_eq!(data, bytes[..bytes.len() - 1]);
                assert!(matches!(
                    connection
                        .receive::<MonitorInput>(Some(Duration::from_secs(2)), &stop)
                        .unwrap(),
                    MonitorInput::Detach
                ));
                connection
                    .writer()
                    .send(&MonitorOutput::Closed, &stop)
                    .unwrap();
                connection.drain(Duration::from_secs(2));
            },
            |network| {
                let (sender, stdin) = mpsc::channel();
                sender.send(Ok(bytes.clone())).unwrap();
                let mut input = InteractiveInput {
                    stdin,
                    parser: InputParser::default(),
                };
                assert!(matches!(
                    input.poll(&network).unwrap(),
                    Some(ViewerAction::Stop)
                ));
                network.detach().unwrap();
            },
        );
    }

    #[test]
    fn slow_viewer_preserves_every_keyboard_mode_transition() {
        let expected: Vec<_> = (0..8)
            .map(|index| ansi::kitty_keyboard_mode(index % 2))
            .collect();
        with_viewer(
            |mut connection| {
                let stop = AtomicBool::new(false);
                let writer = connection.writer();
                for frame in &expected {
                    writer
                        .send(
                            &MonitorOutput::Frame {
                                frame: frame.clone(),
                            },
                            &stop,
                        )
                        .unwrap();
                }
                writer.send(&MonitorOutput::Closed, &stop).unwrap();
                connection.drain(Duration::from_secs(2));
            },
            |network| {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while network.state.lock().unwrap().frames.len() < 2 {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
                std::thread::sleep(Duration::from_millis(100));
                let mut received = Vec::new();
                loop {
                    let mut state = network.state.lock().unwrap();
                    received.extend(state.frames.drain(..));
                    if let Some(result) = state.result.take() {
                        result.unwrap();
                        break;
                    }
                    drop(state);
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(received, expected);
            },
        );
    }

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
        let text = bytes;
        assert!(text.contains('┌') && text.contains('┘'));
        assert!(text.contains("bash"));
        assert!(text.contains('h') && text.contains('i'));
        assert!(text.starts_with("\x1b[H"));
    }

    #[test]
    fn render_placeholder_without_session() {
        let bytes = render_frame(None, (40, 6), "work", false, &mut ModeMirror::default());
        let text = bytes;
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
        let text = render_frame(
            Some(&frame),
            (10, 5),
            "s",
            false,
            &mut ModeMirror::default(),
        );
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
            .starts_with("\x1b[=0u\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[H"));
        assert!(render(Some(&frame), &mut modes).starts_with("\x1b[H"));

        frame.keyboard_mode =
            KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ASSOCIATED_TEXT;
        frame.bracketed_paste = true;
        frame.mouse_mode = MouseMode::Drag;
        assert!(render(Some(&frame), &mut modes).starts_with(
            "\x1b[=17u\x1b[?2004h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1006h\x1b[?1002h\x1b[H"
        ));
        assert!(render(None, &mut modes)
            .starts_with("\x1b[=0u\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[H"));
        assert!(render_frame(
            Some(&frame),
            (10, 5),
            "s",
            false,
            &mut ModeMirror::default()
        )
        .starts_with("\x1b[H"));
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
