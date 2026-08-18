use std::path::PathBuf;
use std::sync::{mpsc, Arc};
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
}

pub(crate) struct Recorder {
    sender: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
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
    pub timeline: frames::TimelineOptions,
}

pub(crate) struct StoppedRecording {
    pub target_path: PathBuf,
    #[cfg(feature = "recording-raster")]
    pub capture_path: PathBuf,
    pub format: RecordingFormat,
    #[cfg(feature = "recording-raster")]
    pub timeline: frames::TimelineOptions,
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
        path: PathBuf,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
        logger: Arc<crate::logger::Logger>,
    ) -> Self {
        let started = Instant::now();
        let writer = match cast::CastWriter::create(&path, cols, rows, env, started) {
            Ok(writer) => Some(writer),
            Err(error) => {
                logger.event(&format!(
                    "automatic recording disabled; failed to create {}: {error}",
                    path.display()
                ));
                None
            }
        };
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || worker_loop(receiver, writer, logger));
        Self {
            sender,
            worker: Some(worker),
        }
    }

    pub fn capture(&self) -> Capture {
        Capture {
            sender: self.sender.clone(),
        }
    }

    pub fn start(&self, request: StartRecording) -> Result<(), CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Start {
                at: Instant::now(),
                request,
                reply,
            })
            .map_err(|_| CaptureError::WorkerStopped)?;
        response.recv().unwrap_or(Err(CaptureError::WorkerStopped))
    }

    pub fn stop(&self) -> Result<StoppedRecording, CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Stop { reply })
            .map_err(|_| CaptureError::WorkerStopped)?;
        response.recv().unwrap_or(Err(CaptureError::WorkerStopped))
    }

    pub fn flush(&self) -> Result<(), CaptureError> {
        let (reply, response) = mpsc::sync_channel(0);
        self.sender
            .send(Message::Flush { reply })
            .map_err(|_| CaptureError::WorkerStopped)?;
        response.recv().unwrap_or(Err(CaptureError::WorkerStopped))
    }

    pub fn on_resize(&self, cols: u16, rows: u16) {
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
        request: StartRecording,
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
            path.clone(),
            80,
            30,
            &[],
            Arc::new(crate::logger::Logger::disabled()),
        );
        recorder.capture().on_data(b"flush-marker");
        recorder.flush().unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("flush-marker"));
        drop(recorder);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stopped_recording_finishes_pending_decoder_bytes() {
        let primary = temp_path("primary");
        let target = temp_path("selected");
        let recorder = Recorder::create(
            primary.clone(),
            80,
            30,
            &[],
            Arc::new(crate::logger::Logger::disabled()),
        );
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
                timeline: frames::TimelineOptions::default(),
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
            path.clone(),
            80,
            30,
            &[],
            Arc::new(crate::logger::Logger::disabled()),
        );
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
