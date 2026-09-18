//! cli daemon host: local socket listener, idle watchdog, monitor streaming,
//! and process state files around the reusable in-process engine.

use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerNonblockingMode, Stream};

use tui_test::engine::Engine;
use tui_test::logger::Logger;
use tui_test::Operation;

use crate::protocol::{Request, Response};
use crate::{config, ipc, monitor};

const MAX_PENDING_REQUESTS: usize = 64;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

struct Host {
    engine: Arc<Engine>,
    session: String,
    logging: bool,
    status: Mutex<Response>,
    last_activity: Arc<Mutex<Instant>>,
    stopping: AtomicBool,
}

struct ConnectionPermit(Arc<AtomicUsize>);

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn run(session_name: String, verbose: bool) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        // A caller may itself have disabled Ctrl+C. Do not pass that process
        // attribute on to the shells we create in ConPTY.
        unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0) };
    }
    config::ensure_home()?;
    sweep_recordings(&session_name);
    let socket = config::socket_name(&session_name);
    let listener = ipc::listen(&socket)?;
    listener.set_nonblocking(ListenerNonblockingMode::Accept)?;
    std::fs::write(
        config::pid_file(&session_name),
        std::process::id().to_string(),
    )
    .ok();

    let logger = if verbose {
        match Logger::to_file(&config::log_file(&session_name)) {
            Ok(logger) => Arc::new(logger),
            Err(_) => Arc::new(Logger::disabled()),
        }
    } else {
        Arc::new(Logger::disabled())
    };
    let logging = logger.enabled();
    let engine = Arc::new(Engine::new(
        session_name.clone(),
        logger,
        config::recording_file(&session_name),
    ));
    engine.log_event(&format!(
        "daemon start session={session_name} pid={}",
        std::process::id()
    ));
    let host = Arc::new(Host {
        status: Mutex::new(status_response(&engine)),
        engine,
        session: session_name.clone(),
        logging,
        last_activity: Arc::new(Mutex::new(Instant::now())),
        stopping: AtomicBool::new(false),
    });
    let (shutdown, shutdown_requests) = mpsc::channel();
    spawn_idle_watchdog(Arc::clone(&host), shutdown.clone());
    let (operations, requests) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
    spawn_operation_worker(requests, Arc::clone(&host));
    let readers = Arc::new(AtomicUsize::new(0));

    let mut listener_error = None;
    let closing_client = loop {
        if let Ok(client) = shutdown_requests.try_recv() {
            break client;
        }
        match listener.accept() {
            Ok(conn) => {
                if readers
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        (count < MAX_PENDING_REQUESTS).then_some(count + 1)
                    })
                    .is_err()
                {
                    continue;
                }
                let permit = ConnectionPermit(Arc::clone(&readers));
                let host = Arc::clone(&host);
                let operations = operations.clone();
                let shutdown = shutdown.clone();
                std::thread::spawn(move || {
                    let _permit = permit;
                    handle_connection(conn, &host, &operations, &shutdown);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                ) => {}
            Err(error) => {
                listener_error = Some(error);
                break None;
            }
        }
    };
    host.stopping.store(true, Ordering::Release);
    drop(operations);
    let response = shutdown_engine(Arc::clone(&host));
    if let Some(mut conn) = closing_client {
        let _ = ipc::write_response(&mut conn, &response);
        ipc::drain_peer(conn, Duration::from_millis(config::SHUTDOWN_DRAIN_MS));
    }
    cleanup(&session_name);
    listener_error.map_or(Ok(()), |error| Err(error.into()))
}

fn handle_connection(
    conn: Stream,
    host: &Arc<Host>,
    operations: &mpsc::SyncSender<(Request, Stream)>,
    shutdown: &mpsc::Sender<Option<Stream>>,
) {
    let mut reader = BufReader::new(conn);
    let req = match ipc::read_request_with_timeout(&mut reader, ipc::REQUEST_TIMEOUT) {
        Ok(request) => request,
        Err(_) => return,
    };
    *host.last_activity.lock().unwrap() = Instant::now();
    if req.is_close() || req.is_shutdown() {
        if !host.stopping.swap(true, Ordering::AcqRel) {
            let _ = shutdown.send(Some(reader.into_inner()));
        } else {
            let _ = ipc::write_response(reader.get_mut(), &Response::ok());
        }
        return;
    }
    if host.stopping.load(Ordering::Acquire) {
        return;
    }
    match req {
        Request::Ping => {
            let _ = ipc::write_response(reader.get_mut(), &Response::ok());
        }
        Request::Status => {
            // Engine::status takes the session lock, which a wait can hold.
            // Publish status after each operation instead of blocking probes.
            let mut response = host.status.lock().unwrap().clone();
            if let Ok(Some(frame)) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.engine.frame()))
            {
                if let Some(data) = response.data.as_mut() {
                    data["cols"] = serde_json::json!(frame.size.0);
                    data["rows"] = serde_json::json!(frame.size.1);
                    data["exited"] = serde_json::json!(frame.exited);
                }
            }
            enrich_cli_response(&mut response, &host.session, host.logging, true);
            let _ = ipc::write_response(reader.get_mut(), &response);
        }
        Request::Monitor {
            cols,
            rows,
            interactive,
        } => {
            spawn_monitor(
                Arc::clone(&host.engine),
                reader.into_inner(),
                (cols, rows),
                host.session.clone(),
                interactive,
            );
        }
        Request::MonitorInputStream { cols, rows } => {
            let frame = host.engine.frame();
            let initial_frame = monitor::render_frame(
                frame.as_ref(),
                (cols, rows),
                &host.session,
                true,
                &mut monitor::ModeMirror::default(),
            );
            let response = Response::with(serde_json::json!({ "initial_frame": initial_frame }));
            if ipc::write_response(reader.get_mut(), &response).is_ok() {
                spawn_monitor_input(
                    Arc::clone(&host.engine),
                    Arc::clone(&host.last_activity),
                    reader,
                    (cols, rows),
                );
            }
        }
        request => {
            if request_signal(&request).is_some_and(|name| {
                matches!(
                    name.trim_start_matches("SIG").to_uppercase().as_str(),
                    "KILL" | "TERM" | "QUIT"
                )
            }) {
                host.engine.interrupt();
            }
            if let Err(error) = operations.try_send((request, reader.into_inner())) {
                let (mpsc::TrySendError::Full((_, mut conn))
                | mpsc::TrySendError::Disconnected((_, mut conn))) = error;
                let response = Response::from_error(tui_test::TuiTestError::internal(
                    "daemon request queue is full or stopping",
                ));
                let _ = ipc::write_response(&mut conn, &response);
            }
        }
    }
}

fn request_signal(request: &Request) -> Option<&str> {
    match request {
        Request::WithContext { request, .. } => request_signal(request),
        Request::Signal { name } => Some(name),
        _ => None,
    }
}

fn spawn_operation_worker(
    requests: mpsc::Receiver<(Request, Stream)>,
    host: Arc<Host>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for (req, mut conn) in requests {
            if host.stopping.load(Ordering::Acquire) {
                break;
            }
            let recording_lifecycle = req.is_open() || req.is_restart();
            if recording_lifecycle {
                let _ = std::fs::write(config::recording_pointer_file(&host.session), "");
            }
            let mut response = match req {
                Request::FlushRecording => flush_recording_response(&host.engine),
                operation => operation.execute(&host.engine),
            };
            if recording_lifecycle {
                sync_recording_pointer(&host.engine, &host.session);
                enrich_cli_response(&mut response, &host.session, host.logging, false);
            }
            *host.status.lock().unwrap() = status_response(&host.engine);
            let _ = ipc::write_response(&mut conn, &response);
        }
    })
}

fn shutdown_engine(host: Arc<Host>) -> Response {
    let (sender, receiver) = mpsc::channel();
    let closed = Arc::new(AtomicBool::new(false));
    let cancel_host = Arc::clone(&host);
    let cancel_done = Arc::clone(&closed);
    std::thread::spawn(move || {
        while !cancel_done.load(Ordering::Acquire) {
            cancel_host.engine.interrupt();
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    std::thread::spawn(move || {
        let response = Response::from_result(host.engine.execute(Operation::Close));
        sync_recording_pointer(&host.engine, &host.session);
        let _ = sender.send(response);
    });
    let response = receiver
        .recv_timeout(SHUTDOWN_TIMEOUT)
        .unwrap_or_else(|error| {
            Response::from_error(tui_test::TuiTestError::internal(match error {
                mpsc::RecvTimeoutError::Timeout => "session teardown timed out; daemon is stopping",
                mpsc::RecvTimeoutError::Disconnected => {
                    "session teardown failed; daemon is stopping"
                }
            }))
        });
    closed.store(true, Ordering::Release);
    response
}

fn flush_recording_response(engine: &Engine) -> Response {
    let Some(path) = engine.recording_path() else {
        return Response::with(serde_json::json!({
            "recording": null,
            "disabled": true,
        }));
    };
    match engine.flush_recording() {
        Ok(()) => Response::with(serde_json::json!({
            "recording": path.to_string_lossy(),
            "disabled": false,
        })),
        Err(error) if error.kind == tui_test::ErrorKind::NoSession => {
            Response::with(serde_json::json!({
                "recording": null,
                "disabled": false,
            }))
        }
        Err(error) => Response::from_error(error),
    }
}

fn sync_recording_pointer(engine: &Engine, session: &str) {
    let value = engine
        .recording_path()
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = std::fs::write(config::recording_pointer_file(session), value);
}

fn status_response(engine: &Engine) -> Response {
    let status = engine.status();
    let mut data = serde_json::json!({
        "session": status.session,
        "shell_pid": status.shell_pid,
        "protocol_version": crate::protocol::PROTOCOL_VERSION,
    });
    if status.cols.is_some() {
        let object = data
            .as_object_mut()
            .expect("daemon status is always a JSON object");
        object.insert("cols".to_string(), serde_json::json!(status.cols));
        object.insert("rows".to_string(), serde_json::json!(status.rows));
        object.insert("shell".to_string(), serde_json::json!(status.shell));
        object.insert("exited".to_string(), serde_json::json!(status.exited));
        object.insert("timeouts".to_string(), serde_json::json!(status.timeouts));
    }
    Response::with(data)
}

fn enrich_cli_response(response: &mut Response, session: &str, logging: bool, status: bool) {
    let Some(data) = response
        .data
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    data.insert("pid".to_string(), serde_json::json!(std::process::id()));
    if status {
        data.insert(
            "log".to_string(),
            if logging {
                serde_json::json!(config::log_file(session).to_string_lossy())
            } else {
                serde_json::Value::Null
            },
        );
        data.insert(
            "version".to_string(),
            serde_json::json!(env!("CARGO_PKG_VERSION")),
        );
    }
}

fn cleanup(session: &str) {
    let _ = std::fs::remove_file(config::pid_file(session));
    if !cfg!(windows) {
        let _ = std::fs::remove_file(config::socket_name(session));
    }
}

fn spawn_idle_watchdog(host: Arc<Host>, shutdown: mpsc::Sender<Option<Stream>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(config::IDLE_CHECK_INTERVAL_MS));
        let idle = host.last_activity.lock().unwrap().elapsed();
        if idle >= Duration::from_millis(config::IDLE_TIMEOUT_MS) {
            host.engine.log_event(&format!(
                "idle timeout: no activity for {}s, shutting down",
                idle.as_secs()
            ));
            if !host.stopping.swap(true, Ordering::AcqRel) {
                let _ = shutdown.send(None);
            }
            break;
        }
    });
}

fn spawn_monitor(
    engine: Arc<Engine>,
    mut conn: Stream,
    viewer: (u16, u16),
    session: String,
    interactive: bool,
) {
    std::thread::spawn(move || {
        engine.log_event("monitor attached");
        let mut modes = monitor::ModeMirror::default();
        loop {
            let frame = engine.frame();
            let bytes =
                monitor::render_frame(frame.as_ref(), viewer, &session, interactive, &mut modes);
            if conn.write_all(&bytes).is_err() || conn.flush().is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(config::MONITOR_FRAME_MS));
        }
        engine.log_event("monitor detached");
    });
}

/// Forward viewer input, translating SGR mouse coordinates past the frame border.
fn spawn_monitor_input(
    engine: Arc<Engine>,
    last_activity: Arc<Mutex<Instant>>,
    reader: BufReader<Stream>,
    viewer: (u16, u16),
) {
    std::thread::spawn(move || {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let messages = serde_json::Deserializer::from_reader(reader)
                .into_iter::<crate::protocol::MonitorInput>();
            for message in messages {
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let mut mouse = monitor::MouseRemapper::new(viewer);
        loop {
            let input = match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(crate::protocol::MonitorInput::Write { data })) => {
                    *last_activity.lock().unwrap() = Instant::now();
                    mouse.push(&data, engine.monitor_mouse_size())
                }
                Ok(Ok(crate::protocol::MonitorInput::Resize { cols, rows })) => {
                    mouse.resize((cols, rows));
                    Vec::new()
                }
                Ok(Err(error)) => {
                    engine.log_event(&format!("monitor input message failed: {error}"));
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    mouse.observe(engine.monitor_mouse_size());
                    mouse.on_idle()
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if !input.is_empty() {
                if let Err(error) = engine.write_monitor_input_raw(&input) {
                    engine.log_event(&format!("monitor input write failed: {}", error.message));
                }
            }
        }
        let pending = mouse.finish();
        if !pending.is_empty() {
            if let Err(error) = engine.write_monitor_input_raw(&pending) {
                engine.log_event(&format!("monitor input write failed: {}", error.message));
            }
        }
    });
}

fn sweep_recordings(current: &str) {
    let Ok(entries) = std::fs::read_dir(config::recording_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("cast") {
            continue;
        }
        let Some(session) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if session != current && ipc::is_running(&config::socket_name(session)) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}
