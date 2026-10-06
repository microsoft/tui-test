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
    socket_name_in(&home_dir(), session)
}

pub fn socket_name_in(home: &std::path::Path, session: &str) -> String {
    #[cfg(windows)]
    {
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").unwrap_or_default(),
        )
        .to_lowercase();
        pipe_name_in(home, &user, session)
    }
    #[cfg(not(windows))]
    {
        socket_path_in(home, session).to_string_lossy().into_owned()
    }
}

#[cfg(windows)]
fn pipe_name_in(home: &std::path::Path, user: &str, session: &str) -> String {
    use std::os::windows::ffi::OsStrExt;

    let absolute = std::path::absolute(home).unwrap_or_else(|_| home.to_path_buf());
    // Canonicalize the existing ancestor too, so the endpoint is unchanged
    // between the first client (before mkdir) and the newly started daemon.
    let mut ancestor = absolute.as_path();
    let canonical = loop {
        if let Ok(canonical) = std::fs::canonicalize(ancestor) {
            let suffix = absolute.strip_prefix(ancestor).expect("path ancestor");
            break if suffix.as_os_str().is_empty() {
                canonical
            } else {
                canonical.join(suffix)
            };
        }
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            None => break absolute.clone(),
        }
    };
    let mut path: Vec<u16> = canonical.as_os_str().encode_wide().collect();
    let verbatim: Vec<_> = "\\\\?\\".encode_utf16().collect();
    let unc: Vec<_> = "\\\\?\\UNC\\".encode_utf16().collect();
    if path.starts_with(&unc) {
        path.splice(..unc.len(), "\\\\".encode_utf16());
    } else if path.starts_with(&verbatim) {
        path.drain(..verbatim.len());
    }
    let mut digest = Sha256::new();
    for component in [
        path.into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        user.as_bytes().to_vec(),
        session.as_bytes().to_vec(),
    ] {
        digest.update((component.len() as u64).to_le_bytes());
        digest.update(component);
    }
    format!("tui-test-{:x}.sock", digest.finalize())
}

#[cfg_attr(windows, allow(dead_code))]
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

    #[cfg(windows)]
    #[test]
    fn pipe_names_are_scoped_to_home_user_and_session() {
        let root = std::env::current_dir().unwrap();
        let a = root.join("pipe-home-a");
        let b = root.join("pipe-home-b");
        let expected = pipe_name_in(&a, "domain\\alice", "work");
        assert_eq!(
            expected,
            pipe_name_in(&a.join("."), "domain\\alice", "work")
        );
        assert_ne!(expected, pipe_name_in(&b, "domain\\alice", "work"));
        assert_ne!(expected, pipe_name_in(&a, "domain\\bob", "work"));
        assert_ne!(expected, pipe_name_in(&a, "domain\\alice", "other"));
    }

    #[cfg(windows)]
    #[test]
    fn pipe_names_are_stable_before_and_after_home_creation() {
        let home = std::env::temp_dir().join(format!("tui-test-pipe-home-{}", std::process::id()));
        assert!(!home.exists());
        let before = pipe_name_in(&home, "user", "work");
        std::fs::create_dir_all(&home).unwrap();
        let after = pipe_name_in(&home, "user", "work");
        std::fs::remove_dir(&home).unwrap();
        assert_eq!(before, after);
    }
}
