use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub const SHUTDOWN_DRAIN_MS: u64 = 2_000;
pub const MONITOR_FRAME_MS: u64 = 50;
pub const IDLE_TIMEOUT_MS: u64 = 4 * 60 * 60 * 1_000;
pub const IDLE_CHECK_INTERVAL_MS: u64 = 5 * 60 * 1_000;

pub fn home_dir() -> PathBuf {
    tui_test::config::home_dir()
}

pub fn ensure_home() -> std::io::Result<PathBuf> {
    let dir = home_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn pid_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.pid"))
}

pub fn daemon_lock_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.pid.lock"))
}

pub fn log_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.log"))
}

fn recording_root_dir(recording: &tui_test::AutomaticRecording) -> PathBuf {
    tui_test::config::canonical_recording_root(recording).unwrap_or_else(|_| {
        let root = tui_test::config::recording_root(recording);
        if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(root)
        }
    })
}

pub fn recording_dir(recording: &tui_test::AutomaticRecording) -> PathBuf {
    recording_root_dir(recording)
        .join("cli")
        .join(runtime_home_id())
}

fn runtime_home_id() -> String {
    let home = home_dir();
    let home = if home.is_absolute() {
        home
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(home)
    };
    let digest = format!("{:x}", Sha256::digest(home.to_string_lossy().as_bytes()));
    digest[..SOCKET_DIGEST_HEX_LEN].to_string()
}

pub fn recording_relative_file(session: &str) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let generation = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    PathBuf::from("cli").join(runtime_home_id()).join(format!(
        "{session}.tui-test-{}-{generation}-{}.cast",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

pub fn recording_file(session: &str, recording: &tui_test::AutomaticRecording) -> PathBuf {
    let directory = recording_dir(recording);
    std::fs::read_dir(&directory)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (recording_session(&path).as_deref() == Some(session)).then(|| {
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (modified, path)
            })
        })
        .max_by_key(|(modified, path)| (*modified, path.clone()))
        .map(|(_, path)| path)
        .unwrap_or_else(|| recording_root_dir(recording).join(format!("{session}.cast")))
}

pub fn recording_session(path: &std::path::Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    Some(
        stem.rsplit_once(".tui-test-")
            .map(|(session, _)| session)
            .unwrap_or(stem)
            .to_string(),
    )
}

const SOCKET_PATH_MAX: usize = 100;
const SOCKET_DIGEST_HEX_LEN: usize = 16;

pub fn socket_name(session: &str) -> String {
    if cfg!(windows) {
        return format!("tui-test-{session}.sock");
    }
    socket_path_in(&home_dir(), session)
        .to_string_lossy()
        .into_owned()
}

fn socket_path_in(dir: &std::path::Path, session: &str) -> PathBuf {
    let path = dir.join(format!("{session}.sock"));
    if path.as_os_str().len() <= SOCKET_PATH_MAX {
        return path;
    }
    let digest = format!("{:x}", Sha256::digest(session.as_bytes()));
    dir.join(format!("{}.sock", &digest[..SOCKET_DIGEST_HEX_LEN]))
}

pub fn session_name_from_env(explicit: Option<String>) -> String {
    explicit
        .or_else(|| std::env::var("TUI_TEST_SESSION").ok())
        .unwrap_or_else(|| "default".to_string())
}

pub fn session_was_specified(explicit: &Option<String>) -> bool {
    explicit.is_some() || std::env::var("TUI_TEST_SESSION").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_socket_path_keeps_the_session_name() {
        let dir = PathBuf::from("/tmp/tui-test");
        assert_eq!(
            socket_path_in(&dir, "work"),
            PathBuf::from("/tmp/tui-test/work.sock")
        );
    }

    #[test]
    fn a_long_socket_path_stays_within_sun_path() {
        let dir = PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/tui-test-Ab12Cd");
        let session = format!("tui-test-{}", "x".repeat(50));
        let path = socket_path_in(&dir, &session);
        assert!(path.as_os_str().len() <= SOCKET_PATH_MAX);
        assert_eq!(path, socket_path_in(&dir, &session));
    }

    #[test]
    fn long_socket_path_matches_the_binding_digest() {
        let dir =
            PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/tui-test-Ab12Cd34");
        assert_eq!(
            socket_path_in(&dir, "helpers-track-54321-9f8e7d6c-1"),
            dir.join("9ba800cbf25eaece.sock")
        );
    }

    #[test]
    fn shortened_socket_names_stay_distinct_per_session() {
        let dir = PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/tui-test-Ab12Cd");
        let long = "y".repeat(60);
        assert_ne!(
            socket_path_in(&dir, &format!("a{long}")),
            socket_path_in(&dir, &format!("b{long}")),
        );
    }

    #[test]
    fn generated_recording_names_round_trip_the_session() {
        let path = recording_relative_file("work.with-dots");
        assert_eq!(recording_session(&path).as_deref(), Some("work.with-dots"));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("cast")
        );
    }

    #[test]
    fn daemon_generations_get_distinct_recording_names() {
        assert_ne!(
            recording_relative_file("work"),
            recording_relative_file("work")
        );
    }

    #[test]
    fn legacy_recording_names_still_resolve() {
        assert_eq!(
            recording_session(&PathBuf::from("legacy.cast")).as_deref(),
            Some("legacy")
        );
    }

    #[test]
    fn recording_lookup_falls_back_to_the_legacy_root_path() {
        let root =
            std::env::temp_dir().join(format!("tui-test-legacy-recording-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let legacy = root.join("work.cast");
        std::fs::write(&legacy, "legacy").unwrap();
        let recording = tui_test::AutomaticRecording {
            directory: Some(root.clone()),
            ..tui_test::AutomaticRecording::default()
        };
        assert_eq!(recording_file("work", &recording), legacy);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_storage_is_namespaced_by_runtime_home() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("TUI_TEST_HOME");
        let recording = tui_test::AutomaticRecording {
            directory: Some(std::env::temp_dir().join("shared-recordings")),
            ..tui_test::AutomaticRecording::default()
        };

        std::env::set_var("TUI_TEST_HOME", "runtime-a");
        let first = recording_dir(&recording);
        std::env::set_var("TUI_TEST_HOME", "runtime-b");
        let second = recording_dir(&recording);

        if let Some(previous) = previous {
            std::env::set_var("TUI_TEST_HOME", previous);
        } else {
            std::env::remove_var("TUI_TEST_HOME");
        }
        assert_ne!(first, second);
    }
}
