use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::api::RecordingFormat;

pub(crate) mod cast;
#[cfg(feature = "recording-raster")]
pub mod frames;

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
    pub cols: u16,
    #[cfg(feature = "recording-raster")]
    pub rows: u16,
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
        let _ = self.sender.send(Message::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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

struct ActiveRecording {
    writer: cast::CastWriter,
    request: StartRecording,
    started: Instant,
    error: Option<String>,
}

fn worker_loop(
    receiver: mpsc::Receiver<Message>,
    mut primary: Option<cast::CastWriter>,
    logger: Arc<crate::logger::Logger>,
) {
    let mut decoder = cast::IncrementalDecoder::default();
    let mut active: Option<ActiveRecording> = None;

    while let Ok(message) = receiver.recv() {
        match message {
            Message::Data { at, bytes } => {
                let text = decoder.push(&bytes);
                if text.is_empty() {
                    continue;
                }
                if let Some(writer) = primary.as_mut() {
                    if let Err(error) = writer.write_output(at, &text) {
                        logger.event(&format!("automatic recording failed: {error}"));
                        primary = None;
                    }
                }
                if let Some(recording) = active.as_mut().filter(|recording| at >= recording.started)
                {
                    let result = recording.writer.write_output(at, &text);
                    remember_error(recording, result);
                }
            }
            Message::Resize { at, cols, rows } => {
                if let Some(writer) = primary.as_mut() {
                    if let Err(error) = writer.write_resize(at, cols, rows) {
                        logger.event(&format!("automatic recording failed: {error}"));
                        primary = None;
                    }
                }
                if let Some(recording) = active.as_mut().filter(|recording| at >= recording.started)
                {
                    let result = recording.writer.write_resize(at, cols, rows);
                    remember_error(recording, result);
                }
            }
            Message::Start { at, request, reply } => {
                if active.is_some() {
                    let _ = reply.send(Err(CaptureError::AlreadyActive));
                    continue;
                }
                let writer = cast::CastWriter::create(
                    &request.capture_path,
                    request.cols,
                    request.rows,
                    &request.env,
                    at,
                );
                let mut writer = match writer {
                    Ok(writer) => writer,
                    Err(error) => {
                        let _ = reply.send(Err(CaptureError::Io(error.to_string())));
                        continue;
                    }
                };
                if let Err(error) = writer.write_output(at, &request.initial_output) {
                    let _ = reply.send(Err(CaptureError::Io(error.to_string())));
                    continue;
                }
                active = Some(ActiveRecording {
                    writer,
                    request,
                    started: at,
                    error: None,
                });
                let _ = reply.send(Ok(()));
            }
            Message::Stop { reply } => {
                let Some(mut recording) = active.take() else {
                    let _ = reply.send(Err(CaptureError::NotActive));
                    continue;
                };
                if recording.error.is_none() {
                    if let Err(error) = recording.writer.flush() {
                        recording.error = Some(error.to_string());
                    }
                }
                if let Some(error) = recording.error {
                    let _ = reply.send(Err(CaptureError::Io(error)));
                    continue;
                }
                let request = recording.request;
                let _ = reply.send(Ok(StoppedRecording {
                    target_path: request.target_path,
                    #[cfg(feature = "recording-raster")]
                    capture_path: request.capture_path,
                    format: request.format,
                    #[cfg(feature = "recording-raster")]
                    cols: request.cols,
                    #[cfg(feature = "recording-raster")]
                    rows: request.rows,
                    #[cfg(feature = "recording-raster")]
                    timeline: request.timeline,
                }));
            }
            Message::Flush { reply } => {
                let result = match primary.as_mut() {
                    Some(writer) => writer
                        .flush()
                        .map_err(|error| CaptureError::Io(error.to_string())),
                    None => Err(CaptureError::Io(
                        "automatic recording is unavailable".to_string(),
                    )),
                };
                if let Err(CaptureError::Io(error)) = &result {
                    logger.event(&format!("automatic recording flush failed: {error}"));
                    primary = None;
                }
                let _ = reply.send(result);
            }
            Message::Shutdown => {
                if let Some(writer) = primary.as_mut() {
                    let _ = writer.flush();
                }
                if let Some(recording) = active.as_mut() {
                    let _ = recording.writer.flush();
                }
                break;
            }
        }
    }
}

fn remember_error(recording: &mut ActiveRecording, result: std::io::Result<()>) {
    if recording.error.is_none() {
        if let Err(error) = result {
            recording.error = Some(error.to_string());
        }
    }
}

pub(crate) fn sidecar_path(target: &std::path::Path) -> PathBuf {
    let mut name = target
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("recording"))
        .to_os_string();
    name.push(".shell-use.cast");
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_acknowledges_all_prior_capture_messages() {
        let path = std::env::temp_dir().join(format!(
            "shell-use-recorder-flush-{}.cast",
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
}
