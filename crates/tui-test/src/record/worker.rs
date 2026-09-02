use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Instant;

use sha2::{Digest, Sha256};

use super::{
    cast, AutomaticRecordingSnapshot, CaptureError, Message, StartRecording, StoppedRecording,
};

struct ActiveRecording {
    writer: cast::CastWriter,
    decoder: cast::IncrementalDecoder,
    request: StartRecording,
    started: Instant,
    last_at: Instant,
    error: Option<String>,
}

pub(super) fn worker_loop(
    receiver: mpsc::Receiver<Message>,
    mut primary: Option<cast::CastWriter>,
    logger: Arc<crate::logger::Logger>,
) {
    let mut primary_decoder = cast::IncrementalDecoder::default();
    let mut primary_last_at = Instant::now();
    let mut active: Option<ActiveRecording> = None;

    while let Ok(message) = receiver.recv() {
        match message {
            Message::Data { at, bytes } => {
                primary_last_at = at;
                let text = primary_decoder.push(&bytes);
                if !text.is_empty() {
                    if let Some(writer) = primary.as_mut() {
                        if let Err(error) = writer.write_output(at, &text) {
                            logger.event(&format!("automatic recording failed: {error}"));
                            primary = None;
                        }
                    }
                }
                if let Some(recording) = active.as_mut().filter(|recording| at >= recording.started)
                {
                    recording.last_at = at;
                    let text = recording.decoder.push(&bytes);
                    if text.is_empty() {
                        continue;
                    }
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
            Message::Start {
                at,
                request,
                decoder,
                reply,
            } => {
                if active.is_some() {
                    let _ = reply.send(Err(CaptureError::AlreadyActive));
                    continue;
                }
                let request = *request;
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
                    decoder: decoder.unwrap_or_else(|| primary_decoder.clone()),
                    request,
                    started: at,
                    last_at: at,
                    error: None,
                });
                let _ = reply.send(Ok(()));
            }
            Message::Stop { reply } => {
                let Some(mut recording) = active.take() else {
                    let _ = reply.send(Err(CaptureError::NotActive));
                    continue;
                };
                let boundary = recording.decoder.clone();
                let tail = recording.decoder.finish();
                if !tail.is_empty() {
                    let result = recording.writer.write_output(recording.last_at, &tail);
                    remember_error(&mut recording, result);
                }
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
                    boundary,
                    #[cfg(feature = "recording-raster")]
                    capture_path: request.capture_path,
                    format: request.format,
                    #[cfg(feature = "recording-raster")]
                    zoom: request.zoom,
                    #[cfg(feature = "recording-raster")]
                    timeline: request.timeline,
                    #[cfg(feature = "recording-raster")]
                    ffmpeg_path: request.ffmpeg_path,
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
            Message::SnapshotAutomatic {
                target_path,
                max_bytes,
                reply,
            } => {
                let result = match primary.as_mut() {
                    Some(writer) => match writer.flush() {
                        Ok(()) => copy_committed_prefix(writer, &target_path, max_bytes),
                        Err(error) => {
                            logger.event(&format!(
                                "automatic recording snapshot flush failed: {error}"
                            ));
                            primary = None;
                            Err(CaptureError::Io(error.to_string()))
                        }
                    },
                    None => Err(CaptureError::Io(
                        "automatic recording is unavailable".to_string(),
                    )),
                };
                let _ = reply.send(result);
            }
            Message::Shutdown => {
                let tail = primary_decoder.finish();
                if !tail.is_empty() {
                    if let Some(writer) = primary.as_mut() {
                        if let Err(error) = writer.write_output(primary_last_at, &tail) {
                            logger.event(&format!("automatic recording failed: {error}"));
                            primary = None;
                        }
                    }
                }
                if let Some(writer) = primary.as_mut() {
                    let _ = writer.flush();
                }
                if let Some(recording) = active.as_mut() {
                    let tail = recording.decoder.finish();
                    if !tail.is_empty() {
                        let result = recording.writer.write_output(recording.last_at, &tail);
                        remember_error(recording, result);
                    }
                    let _ = recording.writer.flush();
                }
                break;
            }
        }
    }
}

fn copy_committed_prefix(
    writer: &cast::CastWriter,
    target_path: &Path,
    max_bytes: u64,
) -> Result<AutomaticRecordingSnapshot, CaptureError> {
    let bytes = std::fs::metadata(writer.path()).map_err(io_error)?.len();
    if bytes > max_bytes {
        return Err(CaptureError::Io(format!(
            "automatic recording snapshot exceeds maximum byte limit ({bytes} > {max_bytes})"
        )));
    }

    let source = File::open(writer.path()).map_err(io_error)?;
    let sha256 = copy_exact_to_target(source, target_path, bytes).map_err(io_error)?;
    let last_committed_ms = writer
        .last_committed()
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    Ok(AutomaticRecordingSnapshot {
        bytes,
        sha256,
        last_committed_ms,
    })
}

fn copy_exact_to_target(
    mut source: impl Read,
    target_path: &Path,
    bytes: u64,
) -> std::io::Result<String> {
    let mut target = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target_path)?;
    let result = copy_exact(&mut source, &mut target, bytes);
    if result.is_err() {
        drop(target);
        let _ = std::fs::remove_file(target_path);
    }
    result
}

fn copy_exact(
    source: &mut impl Read,
    target: &mut impl Write,
    bytes: u64,
) -> std::io::Result<String> {
    let mut remaining = bytes;
    let mut buffer = [0_u8; 64 * 1024];
    let mut hasher = Sha256::new();
    while remaining > 0 {
        let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = source.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "automatic recording changed while being copied",
            ));
        }
        target.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    target.flush()?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn io_error(error: std::io::Error) -> CaptureError {
    CaptureError::Io(error.to_string())
}

fn remember_error(recording: &mut ActiveRecording, result: std::io::Result<()>) {
    if recording.error.is_none() {
        if let Err(error) = result {
            recording.error = Some(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FailingReader {
        prefix: Cursor<Vec<u8>>,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                return self.prefix.read(buffer);
            }
            Err(io::Error::other("injected read failure"))
        }
    }

    #[test]
    fn failed_copy_removes_partial_target() {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let target = std::env::temp_dir().join(format!(
            "tui-test-recorder-copy-failure-{}-{}.cast",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&target);
        let reader = FailingReader {
            prefix: Cursor::new(b"partial".to_vec()),
        };

        let error = copy_exact_to_target(reader, &target, 64).unwrap_err();

        assert_eq!(error.to_string(), "injected read failure");
        assert!(!target.exists());
    }
}
