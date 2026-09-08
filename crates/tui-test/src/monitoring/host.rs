use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::ipc;
use super::protocol::{HostSession, HostSnapshot, Request};
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
    pub owner: String,
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

pub fn new_descriptor() -> std::io::Result<HostDescriptor> {
    let pid = std::process::id();
    let started_at = now_ms();
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(std::io::Error::other)?;
    let nonce = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let owner = format!("{pid}-{nonce}");
    let endpoint = if cfg!(windows) {
        format!("tui-test-host-{owner}")
    } else {
        host_dir()
            .join(format!("{owner}.sock"))
            .to_string_lossy()
            .into_owned()
    };
    Ok(HostDescriptor {
        protocol: HOST_PROTOCOL,
        owner,
        pid,
        started_at,
        cwd: std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().into_owned()),
        endpoint,
    })
}

pub fn descriptor_path(owner: &str) -> PathBuf {
    host_dir().join(format!("{owner}.json"))
}

pub fn publish(descriptor: &HostDescriptor) -> std::io::Result<PathBuf> {
    let dir = ensure_host_dir()?;
    let path = descriptor_path(&descriptor.owner);
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
    let temporary = dir.join(format!("{}.json.new", descriptor.owner));
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
    let path = descriptor_path(&descriptor.owner);
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
    sessions.sort_by(|a, b| a.session.id.cmp(&b.session.id));
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
            current.owner == descriptor.owner
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
    let prefix = format!("{}-", descriptor.pid);
    let Some(nonce) = descriptor.owner.strip_prefix(&prefix) else {
        return false;
    };
    if nonce.len() != 16 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    if path != descriptor_path(&descriptor.owner) {
        return false;
    }
    let expected = if cfg!(windows) {
        format!("tui-test-host-{}", descriptor.owner)
    } else {
        host_dir()
            .join(format!("{}.sock", descriptor.owner))
            .to_string_lossy()
            .into_owned()
    };
    descriptor.endpoint == expected
}

fn compatible_snapshot(descriptor: &HostDescriptor, snapshot: &HostSnapshot) -> bool {
    snapshot.protocol == HOST_PROTOCOL
        && snapshot.owner == descriptor.owner
        && HOST_CAPABILITIES.iter().all(|capability| {
            snapshot
                .capabilities
                .iter()
                .any(|value| value.as_str() == *capability)
        })
        && snapshot.sessions.iter().all(|session| {
            session.owner == descriptor.owner
                && session.pid == descriptor.pid
                && session.id == format!("{}/{}", descriptor.owner, session.session)
        })
}

pub fn monitor_command(id: &str, interactive: bool) -> String {
    let quoted = if id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_./".contains(&byte))
    {
        id.to_string()
    } else if cfg!(windows) {
        format!("'{}'", id.replace('\'', "''"))
    } else {
        format!("'{}'", id.replace('\'', "'\\''"))
    };
    format!(
        "tui-test monitor{} --id {quoted}",
        if interactive { " --interactive" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_descriptors_bind_identity_to_the_expected_endpoint() {
        let mut descriptor = new_descriptor().unwrap();
        let path = descriptor_path(&descriptor.owner);
        assert!(valid_descriptor(&path, &descriptor));
        descriptor.endpoint = "unrelated-file".into();
        assert!(!valid_descriptor(&path, &descriptor));
        descriptor.owner = "../unrelated".into();
        assert!(!valid_descriptor(&path, &descriptor));
    }

    #[test]
    fn monitor_discovery_requires_matching_identity_and_capabilities() {
        let descriptor = new_descriptor().unwrap();
        let mut snapshot = HostSnapshot {
            protocol: HOST_PROTOCOL,
            owner: descriptor.owner.clone(),
            capabilities: HOST_CAPABILITIES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            sessions: Vec::new(),
        };
        assert!(compatible_snapshot(&descriptor, &snapshot));
        snapshot.owner.push('x');
        assert!(!compatible_snapshot(&descriptor, &snapshot));
        snapshot.owner = descriptor.owner.clone();
        snapshot.capabilities.clear();
        assert!(!compatible_snapshot(&descriptor, &snapshot));
    }

    #[test]
    fn monitor_commands_quote_nontrivial_session_ids() {
        assert_eq!(
            monitor_command("123-abc/login", true),
            "tui-test monitor --interactive --id 123-abc/login"
        );
        assert!(
            monitor_command("123-abc/a test's name", false).starts_with("tui-test monitor --id '")
        );
    }
}
