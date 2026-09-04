use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol::{HostSession, Request};
use crate::{config, ipc};

pub const HOST_PROTOCOL: u32 = 1;

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
    let seed = format!("{pid}-{started_at}-{:?}", std::thread::current().id());
    let digest = format!("{:x}", Sha256::digest(seed.as_bytes()));
    let owner = format!("{pid}-{}", &digest[..8]);
    let endpoint = if cfg!(windows) {
        format!("tui-test-host-{owner}")
    } else {
        config::host_dir()
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
    config::host_dir().join(format!("{owner}.json"))
}

pub fn publish(descriptor: &HostDescriptor) -> std::io::Result<PathBuf> {
    let dir = config::ensure_host_dir()?;
    let path = descriptor_path(&descriptor.owner);
    let temporary = dir.join(format!("{}.json.new", descriptor.owner));
    let bytes = serde_json::to_vec_pretty(descriptor)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(&temporary, bytes)?;
    #[cfg(windows)]
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

pub fn unpublish(descriptor: &HostDescriptor) {
    let _ = std::fs::remove_file(descriptor_path(&descriptor.owner));
    if !cfg!(windows) {
        let _ = std::fs::remove_file(&descriptor.endpoint);
    }
}

pub fn discover() -> Vec<DiscoveredHostSession> {
    let Ok(entries) = std::fs::read_dir(config::host_dir()) else {
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
        if descriptor.protocol != HOST_PROTOCOL {
            continue;
        }
        match ipc::send(&descriptor.endpoint, &Request::HostSessions) {
            Ok(response) if response.ok => {
                let Some(value) = response.data else {
                    continue;
                };
                let Ok(host_sessions) = serde_json::from_value::<Vec<HostSession>>(value) else {
                    continue;
                };
                sessions.extend(
                    host_sessions
                        .into_iter()
                        .map(|session| DiscoveredHostSession {
                            descriptor: descriptor.clone(),
                            session,
                        }),
                );
            }
            _ => cleanup_stale_descriptor(&path, &descriptor),
        }
    }
    sessions.sort_by(|a, b| a.session.id.cmp(&b.session.id));
    sessions
}

fn cleanup_stale_descriptor(path: &Path, descriptor: &HostDescriptor) {
    let Ok(current) = std::fs::read(path) else {
        return;
    };
    if serde_json::from_slice::<HostDescriptor>(&current)
        .ok()
        .is_some_and(|current| {
            current.owner == descriptor.owner && current.endpoint == descriptor.endpoint
        })
    {
        let _ = std::fs::remove_file(path);
        if !cfg!(windows) {
            let _ = std::fs::remove_file(&descriptor.endpoint);
        }
    }
}

pub fn routed(session: impl Into<String>, generation: u64, request: Request) -> Request {
    Request::Routed {
        session: session.into(),
        generation,
        request: Box::new(request),
    }
}
