//! cli daemon host: local socket listener, idle watchdog, monitor streaming,
//! and process state files around the reusable in-process engine.

use std::io::BufReader;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Listener;
use interprocess::local_socket::Stream;

use tui_test::engine::Engine;
use tui_test::logger::Logger;
use tui_test::Operation;

use crate::protocol::{Request, Response};
use crate::{config, ipc};

pub fn run(session_name: String, verbose: bool) -> anyhow::Result<()> {
    config::ensure_home()?;
    sweep_recordings(&session_name);
    let socket = config::socket_name(&session_name);
    let listener = ipc::listen(&socket)?;
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
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    spawn_idle_watchdog(
        Arc::clone(&engine),
        Arc::clone(&last_activity),
        session_name.clone(),
    );
    let (operations, requests) = mpsc::channel();
    let operation_worker =
        spawn_operation_worker(requests, Arc::clone(&engine), session_name.clone(), logging);

    loop {
        let Ok(conn) = listener.accept() else {
            continue;
        };
        let mut reader = BufReader::new(conn);
        let req = match ipc::read_request(&mut reader) {
            Ok(request) => request,
            Err(_) => continue,
        };
        *last_activity.lock().unwrap() = Instant::now();
        if let Request::Monitor(attach) = req {
            spawn_monitor(
                Arc::clone(&engine),
                Arc::clone(&last_activity),
                reader,
                session_name.clone(),
                attach,
            );
            continue;
        }
        if matches!(&req, Request::Close | Request::Shutdown) {
            // Stop accepting before acknowledging shutdown, not after the reply drains.
            drop(listener);
            let _ = operations.send((req, reader.into_inner()));
            break;
        }
        if operations.send((req, reader.into_inner())).is_err() {
            break;
        }
    }

    drop(operations);
    let _ = operation_worker.join();
    cleanup(&session_name);
    Ok(())
}

fn spawn_operation_worker(
    requests: mpsc::Receiver<(Request, Stream)>,
    engine: Arc<Engine>,
    session: String,
    logging: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for (req, mut conn) in requests {
            let enrich = match &req {
                Request::Open { .. } => Some(false),
                Request::Status => Some(true),
                _ => None,
            };
            let shutdown = matches!(&req, Request::Close | Request::Shutdown);
            let recording_lifecycle = matches!(
                &req,
                Request::Open { .. } | Request::Close | Request::Shutdown
            );
            if matches!(&req, Request::Open { .. }) {
                let _ = std::fs::write(config::recording_pointer_file(&session), "");
            }
            let mut response = match req {
                Request::Ping => Response::ok(),
                Request::Shutdown => {
                    crate::protocol::response_result(engine.execute(Operation::Close))
                }
                Request::Status => status_response(&engine),
                Request::FlushRecording => flush_recording_response(&engine),
                operation => operation.execute(&engine),
            };
            if recording_lifecycle {
                sync_recording_pointer(&engine, &session);
            }
            if let Some(status) = enrich {
                enrich_cli_response(&mut response, &session, logging, status);
            }
            let _ = ipc::write_response(&mut conn, &response);
            if shutdown {
                ipc::drain_peer(conn, Duration::from_millis(config::SHUTDOWN_DRAIN_MS));
                break;
            }
        }
    })
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

fn spawn_idle_watchdog(engine: Arc<Engine>, last_activity: Arc<Mutex<Instant>>, session: String) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(config::IDLE_CHECK_INTERVAL_MS));
        let idle = last_activity.lock().unwrap().elapsed();
        if idle >= Duration::from_millis(config::IDLE_TIMEOUT_MS) {
            engine.log_event(&format!(
                "idle timeout: no activity for {}s, shutting down",
                idle.as_secs()
            ));
            let _ = engine.execute(Operation::Close);
            sync_recording_pointer(&engine, &session);
            cleanup(&session);
            std::process::exit(0);
        }
    });
}

fn spawn_monitor(
    engine: Arc<Engine>,
    activity: Arc<Mutex<Instant>>,
    reader: BufReader<Stream>,
    session: String,
    attach: tui_test::monitoring::protocol::Attach,
) {
    std::thread::spawn(move || {
        let Ok(mut connection) = tui_test::monitoring::ipc::Connection::from_reader(reader) else {
            return;
        };
        if attach.route.is_some() {
            let _ = connection.writer().send(
                &Response::from_error(tui_test::TuiTestError::usage(
                    "daemon monitor does not accept a process route",
                )),
                &std::sync::atomic::AtomicBool::new(false),
            );
            connection.drain(Duration::from_secs(2));
            return;
        }
        let source = Arc::new(tui_test::monitoring::stream::DaemonSession::new(
            engine.clone(),
            &attach,
            activity,
        ));
        engine.log_event("monitor attached");
        if let Err(error) =
            tui_test::monitoring::stream::serve(connection, source, &session, attach)
        {
            engine.log_event(&format!("monitor disconnected: {error}"));
        }
        engine.log_event("monitor detached");
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
