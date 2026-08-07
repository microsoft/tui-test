use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, Weak};

use sha2::{Digest, Sha256};

use crate::api::{OpenOptions, OpenResult, Operation, OperationResult, RunOptions, ShellUseError};
use crate::engine::Engine;
use crate::logger::Logger;

const MAX_COMPLETED_RECORDINGS: usize = 1024;

#[derive(Clone)]
pub struct Session {
    name: Arc<str>,
    engine: Arc<Engine>,
}

impl Session {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let recording_path = native_recording_path(&name);
        Self {
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

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, ShellUseError> {
        self.engine.execute(operation)
    }

    pub fn open(&self, options: OpenOptions) -> Result<OpenResult, ShellUseError> {
        match self.execute(Operation::Open(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(ShellUseError::internal(
                "open returned an unexpected result type",
            )),
        }
    }

    pub fn run(&self, options: RunOptions) -> Result<OpenResult, ShellUseError> {
        match self.execute(Operation::Run(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(ShellUseError::internal(
                "run returned an unexpected result type",
            )),
        }
    }

    pub fn close(&self) -> Result<(), ShellUseError> {
        self.execute(Operation::Close).map(|_| ())
    }

    pub fn interrupt(&self) {
        self.engine.interrupt();
    }

    pub fn is_open(&self) -> bool {
        self.engine.is_open()
    }

    pub fn recording_path(&self) -> &Path {
        self.engine.recording_path()
    }

    pub fn recording(&self) -> std::io::Result<String> {
        self.engine
            .flush_recording()
            .map_err(shell_error_to_io_error)?;
        std::fs::read_to_string(self.recording_path())
    }
}

#[derive(Clone)]
pub struct SessionHandle {
    name: Arc<str>,
    registry: SessionRegistry,
}

impl SessionHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, ShellUseError> {
        self.registry.execute(&self.name, operation)
    }

    pub fn open(&self, options: OpenOptions) -> Result<OpenResult, ShellUseError> {
        match self.execute(Operation::Open(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(ShellUseError::internal(
                "open returned an unexpected result type",
            )),
        }
    }

    pub fn run(&self, options: RunOptions) -> Result<OpenResult, ShellUseError> {
        match self.execute(Operation::Run(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(ShellUseError::internal(
                "run returned an unexpected result type",
            )),
        }
    }

    pub fn close(&self) -> Result<(), ShellUseError> {
        self.registry.close(&self.name)
    }

    pub fn recording(&self) -> std::io::Result<String> {
        self.registry.recording(&self.name)
    }
}

#[derive(Clone)]
pub struct SessionRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    sessions: Mutex<HashMap<String, Session>>,
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
        Self {
            inner: Arc::new(RegistryInner {
                sessions: Mutex::new(HashMap::new()),
                recordings: Mutex::new(CompletedRecordings::default()),
                generations: Mutex::new(HashMap::new()),
                lifecycle: RwLock::new(()),
            }),
        }
    }
}

impl SessionRegistry {
    pub fn session(&self, name: impl Into<String>) -> SessionHandle {
        let name = name.into();
        SessionHandle {
            name: Arc::from(name),
            registry: self.clone(),
        }
    }

    fn get_or_create_locked(&self, name: String) -> Session {
        let mut sessions = self.lock_sessions();
        sessions
            .entry(name.clone())
            .or_insert_with(|| Session::new(name))
            .clone()
    }

    pub fn execute(
        &self,
        name: &str,
        operation: Operation,
    ) -> Result<OperationResult, ShellUseError> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match operation {
            Operation::Open(_) | Operation::Run(_) => {
                let _lifecycle = self
                    .inner
                    .lifecycle
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                self.get_or_create_locked(name.to_string())
                    .execute(operation)
            }
            Operation::Close => self.close_locked(name).map(|_| OperationResult::Unit),
            other => {
                let session = {
                    let _lifecycle = self
                        .inner
                        .lifecycle
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    self.lock_sessions().get(name).cloned()
                };
                session
                    .ok_or_else(ShellUseError::no_session)?
                    .execute(other)
            }
        }
    }

    pub fn sessions(&self) -> Vec<String> {
        let sessions = self
            .lock_sessions()
            .iter()
            .map(|(name, session)| (name.clone(), session.clone()))
            .collect::<Vec<_>>();
        let mut names = sessions
            .into_iter()
            .filter_map(|(name, session)| session.is_open().then_some(name))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn close(&self, name: &str) -> Result<(), ShellUseError> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.close_locked(name)
    }

    pub fn close_all(&self) {
        let (sessions, removed) = {
            let _lifecycle = self
                .inner
                .lifecycle
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut recordings = self.lock_recordings();
            let sessions = std::mem::take(&mut *self.lock_sessions());
            let mut removed = Vec::new();
            for (name, session) in &sessions {
                let path = session.recording_path();
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
        for session in sessions.values() {
            session.interrupt();
        }
        for session in sessions.into_values() {
            let _ = session.close();
        }
    }

    pub fn recording(&self, name: &str) -> std::io::Result<String> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (session, completed) = {
            let _lifecycle = self
                .inner
                .lifecycle
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let recordings = self.lock_recordings();
            let session = self.lock_sessions().get(name).cloned();
            let completed = recordings.paths.get(name).cloned();
            (session, completed)
        };
        if let Some(session) = session {
            return session.recording();
        }
        let path = completed.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown native session")
        })?;
        std::fs::read_to_string(path)
    }

    fn close_locked(&self, name: &str) -> Result<(), ShellUseError> {
        let (session, removed) = {
            let _lifecycle = self
                .inner
                .lifecycle
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut recordings = self.lock_recordings();
            let Some(session) = self.lock_sessions().remove(name) else {
                return Ok(());
            };
            let path = session.recording_path();
            let removed = if path.is_file() {
                Self::cache_recording(&mut recordings, name.to_string(), path.to_path_buf())
            } else {
                Vec::new()
            };
            (session, removed)
        };
        Self::remove_recording_files(removed);
        session.close()
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, Session>> {
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_recordings(&self) -> MutexGuard<'_, CompletedRecordings> {
        self.inner
            .recordings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn generation(&self, name: &str) -> Arc<Mutex<()>> {
        let mut generations = self
            .inner
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

fn shell_error_to_io_error(error: ShellUseError) -> std::io::Error {
    let kind = if error.kind == crate::api::ErrorKind::NoSession {
        std::io::ErrorKind::NotFound
    } else {
        std::io::ErrorKind::Other
    };
    std::io::Error::new(kind, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ErrorKind, Operation};

    #[test]
    fn registry_reuses_names_and_lists_only_open_sessions() {
        let registry = SessionRegistry::default();
        let first = registry.get_or_create_locked("same".to_string());
        let second = registry.get_or_create_locked("same".to_string());
        assert!(Arc::ptr_eq(&first.engine, &second.engine));
        assert!(registry.sessions().is_empty());
    }

    #[test]
    fn closed_session_operations_report_no_session() {
        let registry = SessionRegistry::default();
        let error = registry.execute("missing", Operation::State).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NoSession);
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
    fn missing_operations_do_not_hide_completed_recordings() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "shell-use-retained-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "retained").unwrap();
        registry.remember_recording("retained".to_string(), path.clone());

        assert_eq!(
            registry
                .execute("retained", Operation::State)
                .unwrap_err()
                .kind,
            ErrorKind::NoSession
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
    fn active_session_does_not_fall_back_to_prior_recording() {
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
