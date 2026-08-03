use std::path::PathBuf;

use sha2::{Digest, Sha256};

pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 30;
pub const POLL_DELAY_MS: u64 = 50;
/// Cap for `open`'s implicit prompt wait when no `ready` budget is configured.
pub const OPEN_READY_CAP_MS: u64 = 8_000;
/// How long shutdown waits for the client to read its final response.
pub const SHUTDOWN_DRAIN_MS: u64 = 2_000;
/// Monitor refresh interval (~20fps).
pub const MONITOR_FRAME_MS: u64 = 50;
/// Idle timeout: the daemon shuts itself down after this long without
/// servicing a request.
pub const IDLE_TIMEOUT_MS: u64 = 4 * 60 * 60 * 1_000;
/// How often the idle watchdog checks for inactivity.
pub const IDLE_CHECK_INTERVAL_MS: u64 = 5 * 60 * 1_000;

/// The kind of thing a wait or assertion is blocking on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutClass {
    /// Text appearing on (or leaving) the screen: `expect text`, `wait text`.
    Text,
    /// The screen going quiet: `wait idle`.
    Idle,
    /// A foreground command finishing: `wait command`, `expect exit-code`.
    Command,
    /// The session's program exiting: `wait exit`.
    Exit,
    /// The shell reporting a prompt: `wait ready`, and `open`'s implicit wait.
    Ready,
}

impl TimeoutClass {
    /// The built-in budget when nothing else is configured.
    pub fn built_in_ms(self) -> u64 {
        match self {
            TimeoutClass::Text | TimeoutClass::Idle => 5_000,
            TimeoutClass::Command | TimeoutClass::Exit | TimeoutClass::Ready => 30_000,
        }
    }

    /// The environment variable consulted for this class.
    pub fn env_var(self) -> &'static str {
        match self {
            TimeoutClass::Text => "SHELL_USE_TIMEOUT_TEXT_MS",
            TimeoutClass::Idle => "SHELL_USE_TIMEOUT_IDLE_MS",
            TimeoutClass::Command => "SHELL_USE_TIMEOUT_COMMAND_MS",
            TimeoutClass::Exit => "SHELL_USE_TIMEOUT_EXIT_MS",
            TimeoutClass::Ready => "SHELL_USE_TIMEOUT_READY_MS",
        }
    }

    /// The environment override, read at call time so tests can vary it.
    pub fn env_ms(self) -> Option<u64> {
        env_timeout_ms(self.env_var())
    }

    /// Resolve this class's default without a session.
    pub fn default_ms(self) -> u64 {
        self.env_ms().unwrap_or_else(|| self.built_in_ms())
    }
}

fn env_timeout_ms(key: &str) -> Option<u64> {
    parse_timeout_ms(&std::env::var(key).ok()?)
}

/// Parse a positive millisecond duration.
fn parse_timeout_ms(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok().filter(|ms| *ms > 0)
}

/// Root directory for all daemon state (sockets, pids, logs).
/// Override with `SHELL_USE_HOME`.
pub fn home_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SHELL_USE_HOME") {
        return PathBuf::from(dir);
    }
    let base = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    base.join(".shell-use")
}

pub fn ensure_home() -> std::io::Result<PathBuf> {
    let dir = home_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn pid_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.pid"))
}

pub fn log_file(session: &str) -> PathBuf {
    home_dir().join(format!("{session}.log"))
}

/// Directory for session recordings, in the user's XDG cache. Recordings
/// persist after a session ends (so they remain retrievable) and are cleared
/// per-session when that session's daemon next starts.
///
/// Honors `SHELL_USE_HOME` for test isolation.
pub fn recording_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SHELL_USE_HOME") {
        return PathBuf::from(dir).join("recordings");
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("shell-use")
}

/// Always-on session recording in asciinema v2 cast format, stored in the XDG
/// cache by session name (e.g. `<cache>/shell-use/<session>.cast`).
pub fn recording_file(session: &str) -> PathBuf {
    recording_dir().join(format!("{session}.cast"))
}

const SOCKET_PATH_MAX: usize = 100;
const SOCKET_DIGEST_HEX_LEN: usize = 16;

/// Platform-appropriate socket name for a session.
///
/// On Windows this is a namespaced pipe name; on Unix it is a filesystem path
/// inside the home directory.
pub fn socket_name(session: &str) -> String {
    if cfg!(windows) {
        return format!("shell-use-{session}.sock");
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
        .or_else(|| std::env::var("SHELL_USE_SESSION").ok())
        .unwrap_or_else(|| "default".to_string())
}

/// Whether the user actually named a session.
pub fn session_was_specified(explicit: &Option<String>) -> bool {
    explicit.is_some() || std::env::var("SHELL_USE_SESSION").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_overrides_accept_positive_integers() {
        assert_eq!(parse_timeout_ms("1"), Some(1));
        assert_eq!(parse_timeout_ms("30000"), Some(30_000));
        assert_eq!(parse_timeout_ms("  2500  "), Some(2_500));
    }

    #[test]
    fn timeout_overrides_reject_junk_so_the_default_wins() {
        for raw in ["", "0", "-1", "abc", "1.5", "5s"] {
            assert_eq!(
                parse_timeout_ms(raw),
                None,
                "expected {raw:?} to be ignored"
            );
        }
    }

    #[test]
    fn built_in_defaults_split_screen_from_process() {
        assert_eq!(TimeoutClass::Text.built_in_ms(), 5_000);
        assert_eq!(TimeoutClass::Idle.built_in_ms(), 5_000);
        assert_eq!(TimeoutClass::Command.built_in_ms(), 30_000);
        assert_eq!(TimeoutClass::Exit.built_in_ms(), 30_000);
        assert_eq!(TimeoutClass::Ready.built_in_ms(), 30_000);
    }

    #[test]
    fn a_short_socket_path_keeps_the_session_name() {
        let dir = PathBuf::from("/tmp/shell-use");
        assert_eq!(
            socket_path_in(&dir, "work"),
            PathBuf::from("/tmp/shell-use/work.sock")
        );
    }

    #[test]
    fn a_long_socket_path_stays_within_sun_path() {
        let dir =
            PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/shell-use-Ab12Cd");
        let session = format!("shell-use-{}", "x".repeat(50));
        let path = socket_path_in(&dir, &session);
        assert!(
            path.as_os_str().len() <= SOCKET_PATH_MAX,
            "{} is {} bytes, too long for sun_path",
            path.display(),
            path.as_os_str().len()
        );
        assert_eq!(
            path,
            socket_path_in(&dir, &session),
            "the shortened name must be stable so the CLI and daemon agree"
        );
    }

    #[test]
    fn long_socket_path_matches_the_binding_digest() {
        let dir =
            PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/shell-use-Ab12Cd34");
        assert_eq!(
            socket_path_in(&dir, "helpers-track-54321-9f8e7d6c-1"),
            dir.join("9ba800cbf25eaece.sock")
        );
    }

    #[test]
    fn shortened_socket_names_stay_distinct_per_session() {
        let dir =
            PathBuf::from("/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/shell-use-Ab12Cd");
        let long = "y".repeat(60);
        assert_ne!(
            socket_path_in(&dir, &format!("a{long}")),
            socket_path_in(&dir, &format!("b{long}")),
        );
    }

    #[test]
    fn each_class_has_a_distinct_env_var() {
        let classes = [
            TimeoutClass::Text,
            TimeoutClass::Idle,
            TimeoutClass::Command,
            TimeoutClass::Exit,
            TimeoutClass::Ready,
        ];
        let mut seen = std::collections::HashSet::new();
        for class in classes {
            let name = class.env_var();
            assert!(name.starts_with("SHELL_USE_TIMEOUT_"));
            assert!(name.ends_with("_MS"));
            assert!(seen.insert(name), "duplicate env var {name}");
        }
    }

    #[test]
    fn class_default_falls_back_to_the_built_in() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        for class in [TimeoutClass::Text, TimeoutClass::Command] {
            let key = class.env_var();
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            let result = std::panic::catch_unwind(|| {
                assert_eq!(class.default_ms(), class.built_in_ms());
            });
            if let Some(value) = old {
                std::env::set_var(key, value);
            }
            result.unwrap();
        }
    }
}
