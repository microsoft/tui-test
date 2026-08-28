//! cli daemon host: local socket listener, idle watchdog, monitor streaming,
//! and process state files around the reusable in-process engine.

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use fs2::FileExt;
use interprocess::local_socket::traits::ListenerExt;
use interprocess::local_socket::Stream;

use tui_test::engine::Engine;
use tui_test::logger::Logger;
use tui_test::Operation;

use crate::protocol::{Request, Response};
use crate::{config, ipc, monitor};

pub fn run(session_name: String, verbose: bool) -> anyhow::Result<()> {
    config::ensure_home()?;
    let socket = config::socket_name(&session_name);
    let listener = ipc::listen(&socket)?;
    std::fs::write(
        config::pid_file(&session_name),
        std::process::id().to_string(),
    )?;

    let logger = if verbose {
        match Logger::to_file(&config::log_file(&session_name)) {
            Ok(logger) => Arc::new(logger),
            Err(_) => Arc::new(Logger::disabled()),
        }
    } else {
        Arc::new(Logger::disabled())
    };
    let logging = logger.enabled();
    let engine = Arc::new(Engine::new_with_external_recording_retention(
        session_name.clone(),
        logger,
        config::recording_relative_file(&session_name),
    ));
    let recording_policy = Arc::new(Mutex::new(tui_test::AutomaticRecording::default()));
    engine.log_event(&format!(
        "daemon start session={session_name} pid={}",
        std::process::id()
    ));
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    spawn_idle_watchdog(
        Arc::clone(&engine),
        Arc::clone(&last_activity),
        Arc::clone(&recording_policy),
        session_name.clone(),
    );

    let mut cleaned_up = false;
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
        let opening = matches!(&req, Request::Open { .. });
        if let Request::Open { recording, .. } = &req {
            sweep_recordings(recording, None, None);
        }
        let mut response = match req {
            Request::Ping => Response::ok(),
            Request::Shutdown => Response::from_result(engine.execute(Operation::Close)),
            Request::Status => status_response(&engine),
            Request::FlushRecording => flush_recording_response(&engine),
            operation => operation.execute(&engine),
        };
        if opening {
            *recording_policy.lock().unwrap() = engine.recording_options();
        }
        let completed_recording = shutdown.then(|| engine.recording_path()).flatten();

        fn flush_recording_response(engine: &Engine) -> Response {
            if !engine.recording_configured() {
                return Response::with(serde_json::json!({
                    "recording": null,
                    "disabled": false,
                }));
            }
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
                        "recording": path.to_string_lossy(),
                        "disabled": false,
                    }))
                }
                Err(error) => Response::from_error(error),
            }
        }
        if shutdown {
            remove_pid_file(&session_name);
        }
        if let Some(status) = enrich {
            enrich_cli_response(&mut response, &session_name, logging, status);
        }
        let _ = ipc::write_response(&mut conn, &response);
        if shutdown {
            ipc::drain_peer(conn, Duration::from_millis(config::SHUTDOWN_DRAIN_MS));
            sweep_recordings(
                &recording_policy.lock().unwrap(),
                completed_recording.as_deref(),
                Some(&session_name),
            );
            remove_socket(&session_name);
            cleaned_up = true;
            break;
        }
    }

    if !cleaned_up {
        cleanup(&session_name);
    }
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
    remove_pid_file(session);
    remove_socket(session);
}

fn remove_pid_file(session: &str) {
    let _ = std::fs::remove_file(config::pid_file(session));
}

fn remove_socket(session: &str) {
    if !cfg!(windows) {
        let _ = std::fs::remove_file(config::socket_name(session));
    }
}

fn spawn_idle_watchdog(
    engine: Arc<Engine>,
    last_activity: Arc<Mutex<Instant>>,
    recording_policy: Arc<Mutex<tui_test::AutomaticRecording>>,
    session: String,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(config::IDLE_CHECK_INTERVAL_MS));
        let idle = last_activity.lock().unwrap().elapsed();
        if idle >= Duration::from_millis(config::IDLE_TIMEOUT_MS) {
            engine.log_event(&format!(
                "idle timeout: no activity for {}s, shutting down",
                idle.as_secs()
            ));
            let _lifecycle = match crate::DaemonLock::acquire(&session) {
                Ok(lock) => lock,
                Err(error) => {
                    engine.log_event(&format!(
                        "idle shutdown failed to lock daemon lifecycle: {error}"
                    ));
                    continue;
                }
            };
            let _ = engine.execute(Operation::Close);
            let completed_recording = engine.recording_path();
            remove_pid_file(&session);
            sweep_recordings(
                &recording_policy.lock().unwrap(),
                completed_recording.as_deref(),
                Some(&session),
            );
            remove_socket(&session);
            drop(_lifecycle);
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

struct RecordingCandidate {
    session: String,
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

struct RecordingSweepLock {
    _file: File,
}

impl RecordingSweepLock {
    fn acquire(directory: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(".retention.lock"))?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}

fn sweep_recordings(
    recording: &tui_test::AutomaticRecording,
    completed_recording: Option<&std::path::Path>,
    owned_session: Option<&str>,
) {
    if recording.mode == tui_test::AutomaticRecordingMode::Disabled {
        return;
    }
    let directory = config::recording_dir(recording);
    let Ok(_sweep_lock) = RecordingSweepLock::acquire(&directory) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let active = sessions_with_pid_files();
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("cast") {
            continue;
        }
        let Some(session) = config::recording_session(&path) else {
            continue;
        };
        if active.contains(&session)
            && completed_recording != Some(path.as_path())
            && owned_session != Some(session.as_str())
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        candidates.push(RecordingCandidate {
            session,
            path,
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            size: metadata.len(),
        });
    }

    let now = SystemTime::now();
    if let Some(max_age) = recording.retention_age_seconds {
        candidates.retain(|candidate| {
            let expired = now
                .duration_since(candidate.modified)
                .is_ok_and(|age| age.as_secs() >= max_age);
            if expired {
                remove_recording_if_inactive(candidate, completed_recording, owned_session);
            }
            !expired
        });
    }

    candidates.sort_by_key(|candidate| candidate.modified);
    loop {
        let total_size = candidates.iter().fold(0u64, |total, candidate| {
            total.saturating_add(candidate.size)
        });
        let count_exceeded = recording
            .retention_count
            .is_some_and(|limit| candidates.len() > limit);
        let size_exceeded = recording
            .retention_size_bytes
            .is_some_and(|limit| total_size > limit);
        if !count_exceeded && !size_exceeded {
            break;
        }
        let oldest = candidates.remove(0);
        remove_recording_if_inactive(&oldest, completed_recording, owned_session);
    }
}

fn remove_recording_if_inactive(
    candidate: &RecordingCandidate,
    completed_recording: Option<&std::path::Path>,
    owned_session: Option<&str>,
) {
    if completed_recording == Some(candidate.path.as_path())
        || owned_session == Some(candidate.session.as_str())
    {
        let _ = std::fs::remove_file(&candidate.path);
        return;
    }
    if config::pid_file(&candidate.session).is_file() {
        return;
    }
    let _ = std::fs::remove_file(&candidate.path);
}

fn sessions_with_pid_files() -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(config::home_dir()) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let session = name.strip_suffix(".pid")?;
            Some(session.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn recording_sweep_only_prunes_the_owned_cli_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous_home = std::env::var_os("TUI_TEST_HOME");
        let root = std::env::temp_dir().join(format!(
            "tui-test-cli-recording-sweep-{}",
            std::process::id()
        ));
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("TUI_TEST_HOME", &home);
        let recording = tui_test::AutomaticRecording {
            directory: Some(root.clone()),
            retention_count: Some(1),
            ..tui_test::AutomaticRecording::default()
        };
        let cli = config::recording_dir(&recording);
        std::fs::create_dir_all(&cli).unwrap();
        let first = cli.join("first.cast");
        let second = cli.join("second.cast");
        let unrelated = root.join("demo.cast");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        std::fs::write(&unrelated, "manual").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&first)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&second)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();

        sweep_recordings(&recording, None, None);

        assert!(!first.exists());
        assert!(second.exists());
        assert!(unrelated.exists());

        if let Some(previous_home) = previous_home {
            std::env::set_var("TUI_TEST_HOME", previous_home);
        } else {
            std::env::remove_var("TUI_TEST_HOME");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
