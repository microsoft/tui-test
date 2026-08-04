use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, Weak};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::engine::Engine;
use crate::logger::Logger;
use crate::protocol::{ErrorKind, Request, Response};

const MAX_COMPLETED_RECORDINGS: usize = 1024;

#[derive(Debug, Clone)]
pub struct ShellUseError {
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for ShellUseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ShellUseError {}

#[derive(Clone)]
pub struct Runtime {
    name: Arc<str>,
    engine: Arc<Engine>,
}

impl Runtime {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let recording_path = native_recording_path(&name);
        Runtime {
            name: Arc::from(name.as_str()),
            engine: Arc::new(Engine::new(
                name,
                Arc::new(Logger::disabled()),
                recording_path,
            )),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn response(&self, request: Request) -> Response {
        if matches!(
            request,
            Request::Ping | Request::Status | Request::Monitor { .. } | Request::Shutdown
        ) {
            return Response::usage("request is only available through the cli daemon");
        }
        catch_unwind(AssertUnwindSafe(|| self.engine.handle(request).0)).unwrap_or_else(|payload| {
            Response::internal(format!(
                "native terminal operation panicked: {}",
                panic_message(payload.as_ref())
            ))
        })
    }

    pub fn response_value(&self, request: Value) -> Response {
        match serde_json::from_value(request) {
            Ok(request) => self.response(request),
            Err(error) => Response::usage(format!("invalid request: {error}")),
        }
    }

    pub fn request(&self, request: Request) -> Result<Value, ShellUseError> {
        unwrap_response(self.response(request))
    }

    pub fn request_value(&self, request: Value) -> Result<Value, ShellUseError> {
        unwrap_response(self.response_value(request))
    }

    pub fn is_open(&self) -> bool {
        self.engine.is_open()
    }

    pub fn close(&self) -> Result<(), ShellUseError> {
        self.request(Request::Close).map(|_| ())
    }

    pub fn recording_path(&self) -> &Path {
        self.engine.recording_path()
    }

    pub fn recording(&self) -> std::io::Result<String> {
        std::fs::read_to_string(self.recording_path())
    }
}

pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, Runtime>>,
    recordings: Mutex<CompletedRecordings>,
    generations: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    lifecycle: RwLock<()>,
}

#[derive(Default)]
struct CompletedRecordings {
    paths: HashMap<String, PathBuf>,
    order: VecDeque<String>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        SessionRegistry {
            sessions: Mutex::new(HashMap::new()),
            recordings: Mutex::new(CompletedRecordings::default()),
            generations: Mutex::new(HashMap::new()),
            lifecycle: RwLock::new(()),
        }
    }
}

impl SessionRegistry {
    fn get_or_create_locked(&self, name: String) -> Runtime {
        let mut sessions = self.lock_sessions();
        sessions
            .entry(name.clone())
            .or_insert_with(|| Runtime::new(name))
            .clone()
    }

    pub fn response_value(&self, name: &str, request: Value) -> Response {
        match serde_json::from_value(request) {
            Ok(request) => self.response(name, request),
            Err(error) => Response::usage(format!("invalid request: {error}")),
        }
    }

    pub fn response(&self, name: &str, request: Request) -> Response {
        if matches!(
            request,
            Request::Ping | Request::Status | Request::Monitor { .. } | Request::Shutdown
        ) {
            return Response::usage("request is only available through the cli daemon");
        }
        let _lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match request {
            Request::Open { .. } => self
                .get_or_create_locked(name.to_string())
                .response(request),
            Request::Close => self.close_response_locked(name),
            other => {
                let runtime = self.lock_sessions().get(name).cloned();
                runtime
                    .map(|runtime| runtime.response(other))
                    .unwrap_or_else(Response::no_session)
            }
        }
    }

    pub fn sessions(&self) -> Vec<String> {
        let sessions = self
            .lock_sessions()
            .iter()
            .map(|(name, runtime)| (name.clone(), runtime.clone()))
            .collect::<Vec<_>>();
        let mut names = sessions
            .into_iter()
            .filter_map(|(name, runtime)| runtime.is_open().then_some(name))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn close(&self, name: &str) -> Result<(), ShellUseError> {
        let _lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unwrap_response(self.close_response_locked(name)).map(|_| ())
    }

    pub fn close_all(&self) {
        let _lifecycle = self
            .lifecycle
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (sessions, removed) = {
            let mut recordings = self.lock_recordings();
            let sessions = std::mem::take(&mut *self.lock_sessions());
            let mut removed = Vec::new();
            for (name, runtime) in &sessions {
                let path = runtime.recording_path();
                if path.is_file() {
                    removed.extend(Self::cache_recording(
                        &mut recordings,
                        name.clone(),
                        path.to_path_buf(),
                    ));
                }
            }
            (sessions, removed)
        };
        Self::remove_recording_files(removed);
        for runtime in sessions.into_values() {
            let _ = runtime.close();
        }
    }

    pub fn recording(&self, name: &str) -> std::io::Result<String> {
        let _lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recordings = self.lock_recordings();
        let runtime = self.lock_sessions().get(name).cloned();
        if let Some(runtime) = runtime {
            let result = runtime.recording();
            drop(recordings);
            return result;
        }
        let path = recordings.paths.get(name).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown native session")
        })?;
        std::fs::read_to_string(path)
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, Runtime>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_recordings(&self) -> MutexGuard<'_, CompletedRecordings> {
        self.recordings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn generation(&self, name: &str) -> Arc<Mutex<()>> {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        generations.retain(|_, generation| generation.strong_count() > 0);
        if let Some(generation) = generations.get(name).and_then(Weak::upgrade) {
            return generation;
        }
        let generation = Arc::new(Mutex::new(()));
        generations.insert(name.to_string(), Arc::downgrade(&generation));
        generation
    }

    fn close_response_locked(&self, name: &str) -> Response {
        let (runtime, removed) = {
            let mut recordings = self.lock_recordings();
            let Some(runtime) = self.lock_sessions().remove(name) else {
                return Response::ok();
            };
            let path = runtime.recording_path();
            let removed = if path.is_file() {
                Self::cache_recording(&mut recordings, name.to_string(), path.to_path_buf())
            } else {
                Vec::new()
            };
            (runtime, removed)
        };
        Self::remove_recording_files(removed);
        runtime.response(Request::Close)
    }

    #[cfg(test)]
    fn remember_recording(&self, name: String, path: PathBuf) {
        let removed = Self::cache_recording(&mut self.lock_recordings(), name, path);
        Self::remove_recording_files(removed);
    }

    fn cache_recording(
        recordings: &mut CompletedRecordings,
        name: String,
        path: PathBuf,
    ) -> Vec<PathBuf> {
        let mut removed = Vec::new();
        if let Some(previous) = recordings.paths.insert(name.clone(), path.clone()) {
            if previous != path {
                removed.push(previous);
            }
            recordings.order.retain(|entry| entry != &name);
        }
        recordings.order.push_back(name);
        while recordings.paths.len() > MAX_COMPLETED_RECORDINGS {
            let Some(oldest) = recordings.order.pop_front() else {
                break;
            };
            if let Some(path) = recordings.paths.remove(&oldest) {
                removed.push(path);
            }
        }
        removed
    }

    fn remove_recording_files(paths: Vec<PathBuf>) {
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn global_registry() -> &'static SessionRegistry {
    static REGISTRY: OnceLock<SessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SessionRegistry::default)
}

fn native_recording_path(name: &str) -> PathBuf {
    static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
    let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("shell-use")
        .join("native")
        .join(std::process::id().to_string())
        .join(format!("{}-{sequence}.cast", &digest[..16]))
}

fn unwrap_response(response: Response) -> Result<Value, ShellUseError> {
    if response.ok {
        return Ok(response.data.unwrap_or(Value::Null));
    }
    Err(ShellUseError {
        kind: response.kind.unwrap_or(ErrorKind::Internal),
        message: response
            .message
            .unwrap_or_else(|| "shell-use operation failed".to_string()),
    })
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_reuses_names_and_lists_only_open_sessions() {
        let registry = SessionRegistry::default();
        let first = registry.get_or_create_locked("same".to_string());
        let second = registry.get_or_create_locked("same".to_string());
        assert!(Arc::ptr_eq(&first.engine, &second.engine));
        assert!(registry.sessions().is_empty());
    }

    #[test]
    fn invalid_request_is_a_usage_error() {
        let runtime = Runtime::new("invalid-request");
        let response = runtime.response_value(json!({"kind": "missing"}));
        assert_eq!(response.kind, Some(ErrorKind::Usage));
    }

    #[test]
    fn cli_control_requests_are_rejected() {
        let runtime = Runtime::new("cli-control");
        for request in [
            Request::Ping,
            Request::Status,
            Request::Monitor { cols: 80, rows: 24 },
            Request::Shutdown,
        ] {
            let response = runtime.response(request);
            assert_eq!(response.kind, Some(ErrorKind::Usage));
        }
    }

    #[test]
    fn completed_recordings_are_bounded() {
        let registry = SessionRegistry::default();
        let root =
            std::env::temp_dir().join(format!("shell-use-recording-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        for index in 0..=MAX_COMPLETED_RECORDINGS {
            let name = format!("session-{index}");
            let path = root.join(format!("{index}.cast"));
            std::fs::write(&path, index.to_string()).unwrap();
            registry.remember_recording(name, path);
        }

        assert_eq!(
            registry.lock_recordings().paths.len(),
            MAX_COMPLETED_RECORDINGS
        );
        assert!(registry.recording("session-0").is_err());
        assert_eq!(
            registry
                .recording(&format!("session-{MAX_COMPLETED_RECORDINGS}"))
                .unwrap(),
            MAX_COMPLETED_RECORDINGS.to_string()
        );
        assert!(!root.join("0.cast").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn non_open_requests_do_not_hide_completed_recordings() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "shell-use-retained-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "retained").unwrap();
        registry.remember_recording("retained".to_string(), path.clone());

        assert_eq!(
            registry.response("retained", Request::State).kind,
            Some(ErrorKind::NoSession)
        );
        assert_eq!(
            registry.response("retained", Request::Shutdown).kind,
            Some(ErrorKind::Usage)
        );
        assert_eq!(registry.recording("retained").unwrap(), "retained");
        assert!(registry.sessions().is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn closing_never_opened_names_does_not_evict_recordings() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "shell-use-valid-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "valid").unwrap();
        registry.remember_recording("valid".to_string(), path.clone());

        for index in 0..=MAX_COMPLETED_RECORDINGS {
            registry.close(&format!("empty-{index}")).unwrap();
        }

        assert_eq!(registry.recording("valid").unwrap(), "valid");
        assert_eq!(registry.lock_recordings().paths.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn active_runtime_does_not_fall_back_to_prior_recording() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "shell-use-prior-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "prior").unwrap();
        registry.remember_recording("same".to_string(), path.clone());
        registry.get_or_create_locked("same".to_string());

        assert!(registry.recording("same").is_err());
        let _ = std::fs::remove_file(path);
    }
}
