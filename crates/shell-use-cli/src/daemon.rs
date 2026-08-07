//! cli daemon host: local socket listener, idle watchdog, monitor streaming,
//! and process state files around the reusable in-process engine.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt;
use interprocess::local_socket::Stream;

use shell_use::engine::Engine;
use shell_use::logger::Logger;
use shell_use::Operation;

use crate::protocol::{Request, Response};
use crate::{config, ipc, monitor};

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

    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        let req = match ipc::read_request(&conn) {
            Ok(request) => request,
            Err(_) => continue,
        };
        *last_activity.lock().unwrap() = Instant::now();
        if let Request::Monitor { cols, rows } = req {
            spawn_monitor(
                Arc::clone(&engine),
                conn,
                (cols, rows),
                session_name.clone(),
            );
            continue;
        }
        let enrich = match &req {
            Request::Open { .. } => Some(false),
            Request::Status => Some(true),
            _ => None,
        };
        let shutdown = matches!(&req, Request::Close | Request::Shutdown);
        let mut response = match req {
            Request::Ping | Request::Shutdown => Response::ok(),
            Request::Status => status_response(&engine),
            Request::FlushRecording => match engine.flush_recording() {
                Ok(()) => Response::ok(),
                Err(error) => Response::from_error(error),
            },
            operation => operation.execute(&engine),
        };
        if let Some(status) = enrich {
            enrich_cli_response(&mut response, &session_name, logging, status);
        }
        let _ = ipc::write_response(&mut conn, &response);
        if shutdown {
            ipc::drain_peer(conn, Duration::from_millis(config::SHUTDOWN_DRAIN_MS));
            break;
        }
    }

    cleanup(&session_name);
    Ok(())
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
            cleanup(&session);
            std::process::exit(0);
        }
    });
}

fn spawn_monitor(engine: Arc<Engine>, mut conn: Stream, viewer: (u16, u16), session: String) {
    std::thread::spawn(move || {
        engine.log_event("monitor attached");
        loop {
            let frame = engine.frame().map(|frame| monitor::Frame {
                grid: frame.grid,
                cursor: frame.cursor,
                size: frame.size,
                exited: frame.exited,
                shell: frame.shell,
            });
            let bytes = monitor::render_frame(frame.as_ref(), viewer, &session);
            if conn.write_all(&bytes).is_err() || conn.flush().is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(config::MONITOR_FRAME_MS));
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
