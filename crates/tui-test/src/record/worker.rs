use std::sync::{mpsc, Arc};
use std::time::Instant;

use super::{cast, CaptureError, Message, StartRecording, StoppedRecording};

struct ActiveRecording {
    writer: cast::CastWriter,
    request: StartRecording,
    started: Instant,
    error: Option<String>,
}

pub(super) fn worker_loop(
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
