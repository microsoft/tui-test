use std::path::PathBuf;

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

pub fn host_dir() -> PathBuf {
    home_dir().join("hosts")
}

pub fn ensure_host_dir() -> std::io::Result<PathBuf> {
    let dir = host_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
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

pub fn recording_pointer_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.recording"))
}

pub fn recording_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TUI_TEST_HOME") {
        return PathBuf::from(dir).join("recordings");
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("tui-test")
}

pub fn recording_file(session: &str) -> PathBuf {
    recording_dir().join(format!("{session}.cast"))
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
}
