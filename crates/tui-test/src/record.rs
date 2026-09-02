use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::api::RecordingFormat;

pub(crate) mod cast;
#[cfg(feature = "recording-raster")]
pub mod frames;
mod worker;

use worker::worker_loop;

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
    automatic: bool,
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
    boundary: cast::IncrementalDecoder,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomaticRecordingSnapshot {
    pub bytes: u64,
    pub sha256: String,
    pub last_committed_ms: Option<u64>,
}

#[derive(Debug)]
pub(crate) enum CaptureError {
    AlreadyActive,
    NotActive,
    WorkerStopped,
    Io(String),
}

impl Recorder {
    #[cfg(test)]
    pub fn create(
        path: Option<PathBuf>,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
        required: bool,
        logger: Arc<crate::logger::Logger>,
    ) -> std::io::Result<Self> {
        Self::create_at(path, cols, rows, env, required, logger, Instant::now())
    }

    pub fn create_at(
        path: Option<PathBuf>,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
        required: bool,
        logger: Arc<crate::logger::Logger>,
        started: Instant,
    ) -> std::io::Result<Self> {
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
        let automatic = writer.is_some();
        let active_sources = Arc::new(AtomicUsize::new(usize::from(automatic)));
        let boundary = Arc::new(Mutex::new(cast::IncrementalDecoder::default()));
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || worker_loop(receiver, writer, logger));
        Ok(Self {
            sender,
            worker: Some(worker),
            active_sources,
            boundary,
            automatic,
        })
    }

    pub fn automatic_enabled(&self) -> bool {
        self.automatic
    }

    pub fn capture(&self) -> Capture {
        Capture {
            sender: self.sender.clone(),
            active_sources: Arc::clone(&self.active_sources),
            boundary: Arc::clone(&self.boundary),
        }
    }

    pub fn start(&self, request: StartRecording) -> Result<(), CaptureError> {
        let decoder = (self.active_sources.fetch_add(1, Ordering::AcqRel) == 0).then(|| {
            self.boundary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });
        let (reply, response) = mpsc::sync_channel(0);
        let result = self
            .sender
            .send(Message::Start {
                at: Instant::now(),
                request: Box::new(request),
                decoder,
                reply,
            })
            .map_err(|_| CaptureError::WorkerStopped)
            .and_then(|()| response.recv().unwrap_or(Err(CaptureError::WorkerStopped)));
        if result.is_err() {
            self.active_sources.fetch_sub(1, Ordering::AcqRel);
        }
        result
    }

    pub fn stop(&self) -> Result<StoppedRecording, CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Stop { reply })
            .map_err(|_| CaptureError::WorkerStopped)?;
        let result = response.recv().unwrap_or(Err(CaptureError::WorkerStopped));
        if matches!(&result, Ok(_) | Err(CaptureError::Io(_)))
            && self.active_sources.fetch_sub(1, Ordering::AcqRel) == 1
        {
            *self
                .boundary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = result
                .as_ref()
                .map(|stopped| stopped.boundary.clone())
                .unwrap_or_default();
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

    pub fn snapshot_automatic(
        &self,
        target_path: PathBuf,
        max_bytes: u64,
    ) -> Result<AutomaticRecordingSnapshot, CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::SnapshotAutomatic {
                target_path,
                max_bytes,
                reply,
            })
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
    SnapshotAutomatic {
        target_path: PathBuf,
        max_bytes: u64,
        reply: mpsc::SyncSender<Result<AutomaticRecordingSnapshot, CaptureError>>,
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
    use sha2::{Digest, Sha256};
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
    fn automatic_snapshot_observes_queue_boundary_and_recording_continues() {
        let source = temp_path("snapshot-source");
        let target = temp_path("snapshot-target");
        remove_if_present(&source);
        remove_if_present(&target);
        let recorder = Recorder::create(
            Some(source.clone()),
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        let capture = recorder.capture();
        capture.on_data(b"before-snapshot");

        let (reply, response) = mpsc::sync_channel(0);
        recorder
            .sender
            .send(Message::SnapshotAutomatic {
                target_path: target.clone(),
                max_bytes: u64::MAX,
                reply,
            })
            .unwrap();
        capture.on_data(b"after-snapshot");

        let snapshot = response.recv().unwrap().unwrap();
        let copied = std::fs::read(&target).unwrap();
        let copied_text = String::from_utf8_lossy(&copied);
        assert!(copied_text.contains("before-snapshot"));
        assert!(!copied_text.contains("after-snapshot"));
        assert_eq!(snapshot.bytes, copied.len() as u64);
        assert_eq!(
            snapshot.sha256,
            format!("sha256:{:x}", Sha256::digest(&copied))
        );
        assert!(snapshot.last_committed_ms.is_some());

        recorder.flush().unwrap();
        let source_text = std::fs::read_to_string(&source).unwrap();
        assert!(source_text.contains("before-snapshot"));
        assert!(source_text.contains("after-snapshot"));

        drop(recorder);
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn automatic_snapshot_rejects_oversize_without_target() {
        let source = temp_path("snapshot-oversize-source");
        let target = temp_path("snapshot-oversize-target");
        remove_if_present(&source);
        remove_if_present(&target);
        let recorder = Recorder::create(
            Some(source.clone()),
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder.capture().on_data(b"oversize");

        let error = recorder.snapshot_automatic(target.clone(), 1).unwrap_err();
        assert!(
            matches!(error, CaptureError::Io(message) if message.contains("exceeds maximum byte limit"))
        );
        assert!(!target.exists());

        drop(recorder);
        std::fs::remove_file(source).unwrap();
    }

    #[test]
    fn automatic_snapshot_reports_unavailable_without_target() {
        let target = temp_path("snapshot-unavailable-target");
        remove_if_present(&target);
        let recorder = Recorder::create(
            None,
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();

        let error = recorder
            .snapshot_automatic(target.clone(), u64::MAX)
            .unwrap_err();
        assert!(
            matches!(error, CaptureError::Io(message) if message == "automatic recording is unavailable")
        );
        assert!(!target.exists());
    }

    #[test]
    fn manual_recording_preserves_a_disabled_capture_boundary() {
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

        assert!(std::fs::read_to_string(&target).unwrap().contains('é'));
        drop(recorder);
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn manual_recording_preserves_boundary_across_stop_and_start() {
        let first = temp_path("manual-first");
        let second = temp_path("manual-second");
        let recorder = Recorder::create(
            None,
            80,
            30,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        let request = |path: &PathBuf| StartRecording {
            target_path: path.clone(),
            capture_path: path.clone(),
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
        };
        let capture = recorder.capture();
        recorder.start(request(&first)).unwrap();
        capture.on_data(&[0xc3]);
        recorder.stop().unwrap();
        recorder.start(request(&second)).unwrap();
        capture.on_data(&[0xa9]);
        recorder.stop().unwrap();

        assert!(std::fs::read_to_string(&second).unwrap().contains('é'));
        drop(recorder);
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
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

    fn remove_if_present(path: &std::path::Path) {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }
}
