use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ipc;
use super::protocol::{HostSession, HostSnapshot, Request};
use super::SessionId;
use crate::config;

pub const HOST_PROTOCOL: u32 = super::protocol::VERSION;
pub const MONITOR_FRAME_MS: u64 = 50;
pub const HOST_CAPABILITIES: &[&str] = &["duplex-monitor", "resize"];

pub fn host_dir() -> PathBuf {
    config::home_dir().join("hosts")
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDescriptor {
    pub protocol: u32,
    pub id: SessionId,
    pub pid: u32,
    pub started_at: u64,
    pub cwd: Option<String>,
    pub endpoint: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveredHostSession {
    pub descriptor: HostDescriptor,
    pub session: HostSession,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn new_descriptor() -> HostDescriptor {
    let pid = std::process::id();
    let started_at = now_ms();
    let id = SessionId::new_v4();
    HostDescriptor {
        protocol: HOST_PROTOCOL,
        id,
        pid,
        started_at,
        cwd: std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().into_owned()),
        endpoint: endpoint(&id),
    }
}

fn endpoint(id: &SessionId) -> String {
    if cfg!(windows) {
        format!("tui-test-host-{id}")
    } else {
        let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
        host_dir()
            .join(format!("{}.sock", &digest[..16]))
            .to_string_lossy()
            .into_owned()
    }
}

pub fn descriptor_path(id: &SessionId) -> PathBuf {
    host_dir().join(format!("{id}.json"))
}

pub fn publish(descriptor: &HostDescriptor) -> std::io::Result<PathBuf> {
    let dir = ensure_host_dir()?;
    let path = descriptor_path(&descriptor.id);
    if !valid_descriptor(&path, descriptor) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid process monitor descriptor",
        ));
    }
    let bytes = serde_json::to_vec_pretty(descriptor)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    match std::fs::read(&path) {
        Ok(existing) if existing == bytes => return Ok(path),
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "process monitor identity already registered",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temporary = dir.join(format!("{}.json.new", descriptor.id));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = file.write_all(&bytes).and_then(|_| {
        drop(file);
        std::fs::rename(&temporary, &path)
    });
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(path)
}

pub fn unpublish(descriptor: &HostDescriptor) {
    let path = descriptor_path(&descriptor.id);
    if !valid_descriptor(&path, descriptor) {
        eprintln!("[tui-test] refusing to remove an invalid process monitor descriptor");
        return;
    }
    let _ = std::fs::remove_file(path);
    if !cfg!(windows) {
        let _ = std::fs::remove_file(&descriptor.endpoint);
    }
}

pub fn discover() -> Vec<DiscoveredHostSession> {
    let Ok(entries) = std::fs::read_dir(host_dir()) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(descriptor) = serde_json::from_slice::<HostDescriptor>(&bytes) else {
            continue;
        };
        if !valid_descriptor(&path, &descriptor) || descriptor.protocol != HOST_PROTOCOL {
            continue;
        }
        let connected = ipc::connect(&descriptor.endpoint);
        let connection = match connected {
            Ok(connection) => connection,
            Err(error) => {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) {
                    cleanup_stale_descriptor(&path, &descriptor);
                }
                continue;
            }
        };
        match ipc::exchange_timeout(connection, &Request::HostSessions, Duration::from_secs(1)) {
            Ok(response) if response.ok => {
                let Some(value) = response.data else {
                    continue;
                };
                let Ok(snapshot) = serde_json::from_value::<HostSnapshot>(value) else {
                    continue;
                };
                if !compatible_snapshot(&descriptor, &snapshot) {
                    continue;
                }
                sessions.extend(snapshot.sessions.into_iter().map(|session| {
                    DiscoveredHostSession {
                        descriptor: descriptor.clone(),
                        session,
                    }
                }));
            }
            _ => {}
        }
    }
    sessions.sort_by_key(|entry| entry.session.id);
    sessions
}

fn cleanup_stale_descriptor(path: &Path, descriptor: &HostDescriptor) {
    if !valid_descriptor(path, descriptor) {
        return;
    }
    let Ok(current) = std::fs::read(path) else {
        return;
    };
    if serde_json::from_slice::<HostDescriptor>(&current)
        .ok()
        .is_some_and(|current| {
            current.id == descriptor.id
                && current.endpoint == descriptor.endpoint
                && current.pid == descriptor.pid
                && current.started_at == descriptor.started_at
                && current.protocol == descriptor.protocol
        })
    {
        let _ = std::fs::remove_file(path);
        if !cfg!(windows) {
            let _ = std::fs::remove_file(&descriptor.endpoint);
        }
    }
}

fn valid_descriptor(path: &Path, descriptor: &HostDescriptor) -> bool {
    path == descriptor_path(&descriptor.id) && descriptor.endpoint == endpoint(&descriptor.id)
}

fn compatible_snapshot(descriptor: &HostDescriptor, snapshot: &HostSnapshot) -> bool {
    snapshot.protocol == HOST_PROTOCOL
        && snapshot.id == descriptor.id
        && HOST_CAPABILITIES.iter().all(|capability| {
            snapshot
                .capabilities
                .iter()
                .any(|value| value.as_str() == *capability)
        })
        && snapshot
            .sessions
            .iter()
            .all(|session| session.pid == descriptor.pid)
}

pub fn monitor_command(session: &str, interactive: bool) -> String {
    let session = if !session.is_empty()
        && session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        session.to_string()
    } else if cfg!(windows) {
        format!("'{}'", session.replace('\'', "''"))
    } else {
        format!("'{}'", session.replace('\'', "'\\''"))
    };
    format!(
        "tui-test --session={session} monitor{}",
        if interactive { " --interactive" } else { "" }
    )
}

pub fn monitor_command_by_id(id: &SessionId, interactive: bool) -> String {
    format!(
        "tui-test monitor{} --id {id}",
        if interactive { " --interactive" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_descriptors_bind_identity_to_the_expected_endpoint() {
        let mut descriptor = new_descriptor();
        let path = descriptor_path(&descriptor.id);
        assert!(valid_descriptor(&path, &descriptor));
        descriptor.endpoint = "unrelated-file".into();
        assert!(!valid_descriptor(&path, &descriptor));
        assert!(!valid_descriptor(
            &path.with_extension("other"),
            &descriptor
        ));
    }

    #[test]
    fn monitor_discovery_requires_matching_identity_and_capabilities() {
        let descriptor = new_descriptor();
        let mut snapshot = HostSnapshot {
            protocol: HOST_PROTOCOL,
            id: descriptor.id,
            capabilities: HOST_CAPABILITIES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            sessions: Vec::new(),
        };
        assert!(compatible_snapshot(&descriptor, &snapshot));
        snapshot.id = SessionId::new_v4();
        assert!(!compatible_snapshot(&descriptor, &snapshot));
        snapshot.id = descriptor.id;
        snapshot.capabilities.clear();
        assert!(!compatible_snapshot(&descriptor, &snapshot));
    }

    #[test]
    fn monitor_commands_use_session_uuids() {
        let id: SessionId = "a78b8b34-8116-4a80-952e-97139848ac65".parse().unwrap();
        assert_eq!(
            monitor_command_by_id(&id, true),
            "tui-test monitor --interactive --id a78b8b34-8116-4a80-952e-97139848ac65"
        );
        assert_eq!(
            monitor_command("login", true),
            "tui-test --session=login monitor --interactive"
        );
        assert_eq!(
            monitor_command("a test's name", false),
            if cfg!(windows) {
                "tui-test --session='a test''s name' monitor"
            } else {
                "tui-test --session='a test'\\''s name' monitor"
            }
        );
        assert_eq!(
            monitor_command("-debug", true),
            "tui-test --session=-debug monitor --interactive"
        );
    }
}
