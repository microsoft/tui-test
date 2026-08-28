use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, Weak};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::api::{
    AutomaticRecording, OpenOptions, OpenResult, Operation, OperationResult, RunOptions,
    TuiTestError,
};
use crate::engine::Engine;
use crate::logger::Logger;

#[derive(Clone)]
pub struct Session {
    name: Arc<str>,
    engine: Arc<Engine>,
}

impl Session {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let recording_path = native_recording_relative_path(&name);
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

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, TuiTestError> {
        self.engine.execute(operation)
    }

    pub fn open(&self, options: OpenOptions) -> Result<OpenResult, TuiTestError> {
        match self.execute(Operation::Open(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(TuiTestError::internal(
                "open returned an unexpected result type",
            )),
        }
    }

    pub fn run(&self, options: RunOptions) -> Result<OpenResult, TuiTestError> {
        match self.execute(Operation::Run(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(TuiTestError::internal(
                "run returned an unexpected result type",
            )),
        }
    }

    pub fn close(&self) -> Result<(), TuiTestError> {
        self.execute(Operation::Close).map(|_| ())
    }

    pub fn interrupt(&self) {
        self.engine.interrupt();
    }

    pub fn is_open(&self) -> bool {
        self.engine.is_open()
    }

    pub fn recording_path(&self) -> Option<PathBuf> {
        self.engine.recording_path()
    }

    pub fn recording(&self) -> std::io::Result<String> {
        let path = self.recording_path().ok_or_else(no_recording)?;
        self.engine
            .flush_recording()
            .map_err(tui_test_error_to_io_error)?;
        std::fs::read_to_string(path)
    }

    pub fn retain_recording(&self) {
        self.engine.retain_recording();
    }

    fn retained_recording(&self) -> Option<(PathBuf, AutomaticRecording)> {
        self.engine.retained_recording()
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

    pub fn execute(&self, operation: Operation) -> Result<OperationResult, TuiTestError> {
        self.registry.execute(&self.name, operation)
    }

    pub fn open(&self, options: OpenOptions) -> Result<OpenResult, TuiTestError> {
        match self.execute(Operation::Open(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(TuiTestError::internal(
                "open returned an unexpected result type",
            )),
        }
    }

    pub fn run(&self, options: RunOptions) -> Result<OpenResult, TuiTestError> {
        match self.execute(Operation::Run(options))? {
            OperationResult::Open(result) => Ok(result),
            _ => Err(TuiTestError::internal(
                "run returned an unexpected result type",
            )),
        }
    }

    pub fn close(&self) -> Result<(), TuiTestError> {
        self.registry.close(&self.name)
    }

    pub fn recording(&self) -> std::io::Result<String> {
        self.registry.recording(&self.name)
    }

    pub fn retain_recording(&self) -> Result<(), TuiTestError> {
        self.registry.retain_recording(&self.name)
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
    entries: HashMap<String, CompletedRecording>,
    policies: HashMap<PathBuf, AutomaticRecording>,
    next_sequence: u64,
}

struct CompletedRecording {
    path: PathBuf,
    scope: PathBuf,
    modified: SystemTime,
    size: u64,
    sequence: u64,
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
    ) -> Result<OperationResult, TuiTestError> {
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
                session.ok_or_else(TuiTestError::no_session)?.execute(other)
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

    pub fn close(&self, name: &str) -> Result<(), TuiTestError> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.close_locked(name)
    }

    pub fn close_all(&self) {
        let mut removed = Vec::new();
        {
            let _lifecycle = self
                .inner
                .lifecycle
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let sessions = std::mem::take(&mut *self.lock_sessions());
            for session in sessions.values() {
                session.interrupt();
            }
            for (name, session) in sessions {
                let _ = session.close();
                removed
                    .extend(self.replace_completed_recording(name, session.retained_recording()));
            }
        }
        Self::remove_recording_files(removed);
    }

    pub fn recording(&self, name: &str) -> std::io::Result<String> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (session, completed, removed) = {
            let _lifecycle = self
                .inner
                .lifecycle
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut recordings = self.lock_recordings();
            Self::remove_missing_recordings(&mut recordings);
            let removed = Self::prune_expired(&mut recordings);
            let session = self.lock_sessions().get(name).cloned();
            let completed = recordings
                .entries
                .get(name)
                .map(|recording| recording.path.clone());
            (session, completed, removed)
        };
        Self::remove_recording_files(removed);
        if let Some(session) = session {
            return session.recording();
        }
        let path = completed.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown native session")
        })?;
        std::fs::read_to_string(path)
    }

    pub fn retain_recording(&self, name: &str) -> Result<(), TuiTestError> {
        let generation = self.generation(name);
        let _generation = generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _lifecycle = self
            .inner
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = self
            .lock_sessions()
            .get(name)
            .cloned()
            .ok_or_else(TuiTestError::no_session)?;
        session.retain_recording();
        Ok(())
    }

    fn close_locked(&self, name: &str) -> Result<(), TuiTestError> {
        let _lifecycle = self
            .inner
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = self.lock_sessions().remove(name) else {
            return Ok(());
        };
        let result = session.close();
        let removed =
            self.replace_completed_recording(name.to_string(), session.retained_recording());
        Self::remove_recording_files(removed);
        result
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
        self.remember_recording_with_options(name, path, AutomaticRecording::default());
    }

    #[cfg(test)]
    fn remember_recording_with_options(
        &self,
        name: String,
        path: PathBuf,
        options: AutomaticRecording,
    ) {
        let removed = Self::cache_recording(&mut self.lock_recordings(), name, path, options);
        Self::remove_recording_files(removed);
    }

    fn replace_completed_recording(
        &self,
        name: String,
        recording: Option<(PathBuf, AutomaticRecording)>,
    ) -> Vec<PathBuf> {
        let mut recordings = self.lock_recordings();
        Self::remove_missing_recordings(&mut recordings);
        if let Some((path, options)) = recording {
            Self::cache_recording(&mut recordings, name, path, options)
        } else {
            let Some(recording) = recordings.entries.remove(&name) else {
                return Vec::new();
            };
            if !recordings
                .entries
                .values()
                .any(|candidate| candidate.scope == recording.scope)
            {
                recordings.policies.remove(&recording.scope);
            }
            vec![recording.path]
        }
    }

    fn cache_recording(
        recordings: &mut CompletedRecordings,
        name: String,
        path: PathBuf,
        options: AutomaticRecording,
    ) -> Vec<PathBuf> {
        let mut removed = Vec::new();
        if let Some(previous) = recordings.entries.remove(&name) {
            let previous_scope = previous.scope.clone();
            if previous.path != path {
                removed.push(previous.path);
            }
            if !recordings
                .entries
                .values()
                .any(|recording| recording.scope == previous_scope)
            {
                recordings.policies.remove(&previous_scope);
            }
        }
        let metadata = std::fs::metadata(&path).ok();
        let scope = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        recordings.policies.insert(scope.clone(), options);
        let sequence = recordings.next_sequence;
        recordings.next_sequence = recordings.next_sequence.wrapping_add(1);
        recordings.entries.insert(
            name,
            CompletedRecording {
                path,
                scope: scope.clone(),
                modified: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .unwrap_or_else(SystemTime::now),
                size: metadata.map_or(0, |metadata| metadata.len()),
                sequence,
            },
        );
        removed.extend(Self::prune_scope(recordings, &scope));
        removed
    }

    fn prune_expired(recordings: &mut CompletedRecordings) -> Vec<PathBuf> {
        let scopes = recordings.policies.keys().cloned().collect::<Vec<_>>();
        let mut removed = Vec::new();
        for scope in scopes {
            removed.extend(Self::prune_scope(recordings, &scope));
        }
        removed
    }

    fn remove_missing_recordings(recordings: &mut CompletedRecordings) {
        recordings
            .entries
            .retain(|_, recording| recording.path.is_file());
        let active_scopes = recordings
            .entries
            .values()
            .map(|recording| recording.scope.clone())
            .collect::<std::collections::HashSet<_>>();
        recordings
            .policies
            .retain(|scope, _| active_scopes.contains(scope));
    }

    fn prune_scope(recordings: &mut CompletedRecordings, scope: &Path) -> Vec<PathBuf> {
        let Some(policy) = recordings.policies.get(scope).cloned() else {
            return Vec::new();
        };
        let now = SystemTime::now();
        let mut removed = Vec::new();

        if let Some(max_age) = policy.retention_age_seconds {
            let expired = recordings
                .entries
                .iter()
                .filter(|(_, recording)| recording.scope == scope)
                .filter(|(_, recording)| {
                    now.duration_since(recording.modified)
                        .is_ok_and(|age| age.as_secs() >= max_age)
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            for name in expired {
                if let Some(recording) = recordings.entries.remove(&name) {
                    removed.push(recording.path);
                }
            }
        }

        loop {
            let mut candidates = recordings
                .entries
                .iter()
                .filter(|(_, recording)| recording.scope == scope)
                .map(|(name, recording)| {
                    (
                        name.clone(),
                        recording.modified,
                        recording.sequence,
                        recording.size,
                    )
                })
                .collect::<Vec<_>>();
            candidates.sort_by_key(|(_, modified, sequence, _)| (*modified, *sequence));
            let total_size = candidates
                .iter()
                .fold(0u64, |total, (_, _, _, size)| total.saturating_add(*size));
            let count_exceeded = policy
                .retention_count
                .is_some_and(|limit| candidates.len() > limit);
            let size_exceeded = policy
                .retention_size_bytes
                .is_some_and(|limit| total_size > limit);
            if !count_exceeded && !size_exceeded {
                break;
            }
            let Some((oldest, _, _, _)) = candidates.first() else {
                break;
            };
            if let Some(recording) = recordings.entries.remove(oldest) {
                removed.push(recording.path);
            }
        }

        if !recordings
            .entries
            .values()
            .any(|recording| recording.scope == scope)
        {
            recordings.policies.remove(scope);
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

fn native_recording_relative_path(name: &str) -> PathBuf {
    static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
    let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    PathBuf::from("native")
        .join(std::process::id().to_string())
        .join(format!("{}-{sequence}.cast", &digest[..16]))
}

fn no_recording() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "automatic recording is disabled",
    )
}

fn tui_test_error_to_io_error(error: TuiTestError) -> std::io::Error {
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
    use crate::api::{ErrorKind, Operation, DEFAULT_RECORDING_RETENTION_COUNT};

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
            std::env::temp_dir().join(format!("tui-test-recording-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let options = AutomaticRecording {
            retention_count: Some(2),
            ..AutomaticRecording::default()
        };

        for index in 0..=2 {
            let name = format!("session-{index}");
            let path = root.join(format!("{index}.cast"));
            std::fs::write(&path, index.to_string()).unwrap();
            registry.remember_recording_with_options(name, path, options.clone());
        }

        assert_eq!(registry.lock_recordings().entries.len(), 2);
        assert!(registry.recording("session-0").is_err());
        assert_eq!(registry.recording("session-2").unwrap(), "2");
        assert!(!root.join("0.cast").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completed_recordings_expire_by_age() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "tui-test-aged-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "expired").unwrap();
        registry.remember_recording_with_options(
            "expired".to_string(),
            path.clone(),
            AutomaticRecording {
                retention_age_seconds: Some(0),
                ..AutomaticRecording::default()
            },
        );

        assert!(registry.recording("expired").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn completed_recordings_are_bounded_by_total_size() {
        let registry = SessionRegistry::default();
        let root = std::env::temp_dir().join(format!(
            "tui-test-recording-size-cache-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let options = AutomaticRecording {
            retention_count: None,
            retention_size_bytes: Some(6),
            ..AutomaticRecording::default()
        };
        for (name, contents) in [("first", "1234"), ("second", "5678")] {
            let path = root.join(format!("{name}.cast"));
            std::fs::write(&path, contents).unwrap();
            registry.remember_recording_with_options(name.to_string(), path, options.clone());
        }

        assert!(registry.recording("first").is_err());
        assert_eq!(registry.recording("second").unwrap(), "5678");
        assert!(!root.join("first.cast").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn default_retention_matches_the_legacy_bound() {
        assert_eq!(
            AutomaticRecording::default().retention_count,
            Some(DEFAULT_RECORDING_RETENTION_COUNT)
        );
    }

    #[test]
    fn missing_operations_do_not_hide_completed_recordings() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "tui-test-retained-recording-{}.cast",
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
            "tui-test-valid-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "valid").unwrap();
        registry.remember_recording("valid".to_string(), path.clone());

        for index in 0..=DEFAULT_RECORDING_RETENTION_COUNT {
            registry.close(&format!("empty-{index}")).unwrap();
        }

        assert_eq!(registry.recording("valid").unwrap(), "valid");
        assert_eq!(registry.lock_recordings().entries.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn active_session_does_not_fall_back_to_prior_recording() {
        let registry = SessionRegistry::default();
        let path = std::env::temp_dir().join(format!(
            "tui-test-prior-recording-{}.cast",
            std::process::id()
        ));
        std::fs::write(&path, "prior").unwrap();
        registry.remember_recording("same".to_string(), path.clone());
        registry.get_or_create_locked("same".to_string());

        assert!(registry.recording("same").is_err());
        let _ = std::fs::remove_file(path);
    }
}
