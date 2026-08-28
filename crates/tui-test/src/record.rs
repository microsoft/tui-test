use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime};

use crate::api::{AutomaticRecording, RecordingFormat};

pub(crate) mod cast;
#[cfg(feature = "recording-raster")]
pub mod frames;
mod worker;

use worker::worker_loop;

#[derive(Default)]
struct AutomaticRecordingState {
    active: HashSet<PathBuf>,
}

pub(crate) struct AutomaticRecordingGuard {
    path: PathBuf,
}

impl AutomaticRecordingGuard {
    pub fn register(path: PathBuf) -> Self {
        automatic_recording_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .insert(path.clone());
        Self { path }
    }
}

impl Drop for AutomaticRecordingGuard {
    fn drop(&mut self) {
        automatic_recording_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .remove(&self.path);
    }
}

fn automatic_recording_state() -> &'static Mutex<AutomaticRecordingState> {
    static STATE: OnceLock<Mutex<AutomaticRecordingState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AutomaticRecordingState::default()))
}

pub(crate) fn prune_automatic_recordings(scope: &Path, policy: &AutomaticRecording) {
    let state = automatic_recording_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(entries) = std::fs::read_dir(scope) else {
        return;
    };
    let mut candidates = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if state.active.contains(&path)
                || path.extension().and_then(|extension| extension.to_str()) != Some("cast")
            {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            Some((
                path,
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            ))
        })
        .collect::<Vec<_>>();

    let now = SystemTime::now();
    if let Some(max_age) = policy.retention_age_seconds {
        candidates.retain(|(path, modified, _)| {
            let expired = now
                .duration_since(*modified)
                .is_ok_and(|age| age.as_secs() >= max_age);
            if expired {
                let _ = std::fs::remove_file(path);
            }
            !expired
        });
    }

    candidates.sort_by_key(|(_, modified, _)| *modified);
    loop {
        let total_size = candidates
            .iter()
            .fold(0u64, |total, (_, _, size)| total.saturating_add(*size));
        let count_exceeded = policy
            .retention_count
            .is_some_and(|limit| candidates.len() > limit);
        let size_exceeded = policy
            .retention_size_bytes
            .is_some_and(|limit| total_size > limit);
        if !count_exceeded && !size_exceeded {
            break;
        }
        let (path, _, _) = candidates.remove(0);
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Clone)]
pub(crate) struct Capture {
    sender: mpsc::Sender<Message>,
    active_sources: Arc<AtomicUsize>,
    boundary: Arc<Mutex<cast::IncrementalDecoder>>,
}

pub(crate) struct Recorder {
    sender: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
    active_sources: Arc<AtomicUsize>,
    boundary: Arc<Mutex<cast::IncrementalDecoder>>,
}

pub(crate) struct StartRecording {
    pub target_path: PathBuf,
    pub capture_path: PathBuf,
    pub format: RecordingFormat,
    pub cols: u16,
    pub rows: u16,
    pub env: Vec<(String, String)>,
    pub initial_output: String,
    #[cfg(feature = "recording-raster")]
    pub zoom: f64,
    #[cfg(feature = "recording-raster")]
    pub timeline: frames::TimelineOptions,
    #[cfg(feature = "recording-raster")]
    pub ffmpeg_path: Option<PathBuf>,
}

pub(crate) struct StoppedRecording {
    pub target_path: PathBuf,
    #[cfg(feature = "recording-raster")]
    pub capture_path: PathBuf,
    pub format: RecordingFormat,
    #[cfg(feature = "recording-raster")]
    pub zoom: f64,
    #[cfg(feature = "recording-raster")]
    pub timeline: frames::TimelineOptions,
    #[cfg(feature = "recording-raster")]
    pub ffmpeg_path: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) enum CaptureError {
    AlreadyActive,
    NotActive,
    WorkerStopped,
    Io(String),
}

impl Recorder {
    pub fn create(
        path: Option<PathBuf>,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
        required: bool,
        logger: Arc<crate::logger::Logger>,
    ) -> std::io::Result<Self> {
        let started = Instant::now();
        let writer = match path {
            Some(path) => match cast::CastWriter::create(&path, cols, rows, env, started) {
                Ok(writer) => Some(writer),
                Err(error) if required => return Err(error),
                Err(error) => {
                    logger.event(&format!(
                        "automatic recording disabled; failed to create {}: {error}",
                        path.display()
                    ));
                    None
                }
            },
            None => None,
        };
        let active_sources = Arc::new(AtomicUsize::new(usize::from(writer.is_some())));
        let boundary = Arc::new(Mutex::new(cast::IncrementalDecoder::default()));
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || worker_loop(receiver, writer, logger));
        Ok(Self {
            sender,
            worker: Some(worker),
            active_sources,
            boundary,
        })
    }

    fn add_source(&self) -> usize {
        self.active_sources.fetch_add(1, Ordering::AcqRel)
    }

    fn remove_source(&self) -> usize {
        let result =
            self.active_sources
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    count.checked_sub(1)
                });
        debug_assert!(result.is_ok(), "recording source count underflowed");
        result.unwrap_or(1) - 1
    }

    pub fn capture(&self) -> Capture {
        Capture {
            sender: self.sender.clone(),
            active_sources: Arc::clone(&self.active_sources),
            boundary: Arc::clone(&self.boundary),
        }
    }

    pub fn start(&self, request: StartRecording) -> Result<(), CaptureError> {
        let decoder = if self.add_source() == 0 {
            Some(
                self.boundary
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            )
        } else {
            None
        };
        let result = (|| {
            let (reply, response) = mpsc::sync_channel(0);
            self.sender
                .send(Message::Start {
                    at: Instant::now(),
                    request: Box::new(request),
                    decoder,
                    reply,
                })
                .map_err(|_| CaptureError::WorkerStopped)?;
            response.recv().unwrap_or(Err(CaptureError::WorkerStopped))
        })();
        if result.is_err() {
            self.remove_source();
        }
        result
    }

    pub fn stop(&self) -> Result<StoppedRecording, CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Stop { reply })
            .map_err(|_| CaptureError::WorkerStopped)?;
        let result = response.recv().unwrap_or(Err(CaptureError::WorkerStopped));
        if matches!(&result, Ok(_) | Err(CaptureError::Io(_))) && self.remove_source() == 0 {
            *self
                .boundary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                cast::IncrementalDecoder::default();
        }
        result
    }

    pub fn flush(&self) -> Result<(), CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Flush { reply })
            .map_err(|_| CaptureError::WorkerStopped)?;
        response.recv().unwrap_or(Err(CaptureError::WorkerStopped))
    }

    pub fn on_resize(&self, cols: u16, rows: u16) {
        if self.active_sources.load(Ordering::Acquire) == 0 {
            return;
        }
        let _ = self.sender.send(Message::Resize {
            at: Instant::now(),
            cols,
            rows,
        });
    }

    pub fn shutdown(&mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.active_sources.store(0, Ordering::Release);
    }
}

impl Capture {
    pub fn on_data(&self, data: &[u8]) {
        if self.active_sources.load(Ordering::Acquire) == 0 {
            let _ = self
                .boundary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(data);
            return;
        }
        let _ = self.sender.send(Message::Data {
            at: Instant::now(),
            bytes: data.to_vec(),
        });
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum Message {
    Data {
        at: Instant,
        bytes: Vec<u8>,
    },
    Resize {
        at: Instant,
        cols: u16,
        rows: u16,
    },
    Start {
        at: Instant,
        request: Box<StartRecording>,
        decoder: Option<cast::IncrementalDecoder>,
        reply: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    Stop {
        reply: mpsc::SyncSender<Result<StoppedRecording, CaptureError>>,
    },
    Flush {
        reply: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    Shutdown,
}

pub(crate) fn sidecar_path(target: &std::path::Path) -> PathBuf {
    let mut name = target
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("recording"))
        .to_os_string();
    name.push(".tui-test.cast");
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn flush_acknowledges_all_prior_capture_messages() {
        let path = std::env::temp_dir().join(format!(
            "tui-test-recorder-flush-{}.cast",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let recorder = Recorder::create(
            Some(path.clone()),
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder.capture().on_data(b"flush-marker");
        recorder.flush().unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("flush-marker"));
        drop(recorder);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn disabled_automatic_recording_still_allows_manual_recording() {
        let target = temp_path("manual-only");
        let recorder = Recorder::create(
            None,
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder.capture().on_data(b"discarded-before-start");
        recorder
            .start(StartRecording {
                target_path: target.clone(),
                capture_path: target.clone(),
                format: RecordingFormat::Cast,
                cols: 80,
                rows: 30,
                env: Vec::new(),
                initial_output: "initial".to_string(),
                #[cfg(feature = "recording-raster")]
                zoom: 1.0,
                #[cfg(feature = "recording-raster")]
                timeline: frames::TimelineOptions::default(),
                #[cfg(feature = "recording-raster")]
                ffmpeg_path: None,
            })
            .unwrap();
        recorder.capture().on_data(b"manual-output");
        recorder.stop().unwrap();

        let cast = std::fs::read_to_string(&target).unwrap();
        assert!(cast.contains("initial"));
        assert!(cast.contains("manual-output"));
        assert!(!cast.contains("discarded-before-start"));

        drop(recorder);
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn manual_recording_keeps_a_decoder_boundary_from_disabled_capture() {
        let target = temp_path("manual-boundary");
        let recorder = Recorder::create(
            None,
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        let capture = recorder.capture();
        capture.on_data(&[0xc3]);
        recorder
            .start(StartRecording {
                target_path: target.clone(),
                capture_path: target.clone(),
                format: RecordingFormat::Cast,
                cols: 80,
                rows: 30,
                env: Vec::new(),
                initial_output: String::new(),
                #[cfg(feature = "recording-raster")]
                zoom: 1.0,
                #[cfg(feature = "recording-raster")]
                timeline: frames::TimelineOptions::default(),
                #[cfg(feature = "recording-raster")]
                ffmpeg_path: None,
            })
            .unwrap();
        capture.on_data(&[0xa9]);
        recorder.stop().unwrap();

        let cast = std::fs::read_to_string(&target).unwrap();
        assert!(cast.contains('é'));
        assert!(!cast.contains('\u{fffd}'));

        drop(recorder);
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn stopped_recording_finishes_pending_decoder_bytes() {
        let primary = temp_path("primary");
        let target = temp_path("selected");
        let recorder = Recorder::create(
            Some(primary.clone()),
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder
            .start(StartRecording {
                target_path: target.clone(),
                capture_path: target.clone(),
                format: RecordingFormat::Cast,
                cols: 80,
                rows: 30,
                env: Vec::new(),
                initial_output: String::new(),
                #[cfg(feature = "recording-raster")]
                zoom: 1.0,
                #[cfg(feature = "recording-raster")]
                timeline: frames::TimelineOptions::default(),
                #[cfg(feature = "recording-raster")]
                ffmpeg_path: None,
            })
            .unwrap();
        recorder.capture().on_data(b"tail\x1b[?");
        recorder.stop().unwrap();

        let cast = std::fs::read_to_string(&target).unwrap();
        assert!(cast.contains("tail"));
        assert!(cast.contains(r#"\u001b[?"#));

        drop(recorder);
        std::fs::remove_file(primary).unwrap();
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn shutdown_finishes_pending_primary_decoder_bytes() {
        let path = temp_path("shutdown");
        let recorder = Recorder::create(
            Some(path.clone()),
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder.capture().on_data(b"tail\x1b[?");
        drop(recorder);

        let cast = std::fs::read_to_string(&path).unwrap();
        assert!(cast.contains("tail"));
        assert!(cast.contains(r#"\u001b[?"#));
        std::fs::remove_file(path).unwrap();
    }

    fn temp_path(label: &str) -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tui-test-recorder-{label}-{}-{}.cast",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
