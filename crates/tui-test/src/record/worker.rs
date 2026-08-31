use std::sync::{mpsc, Arc};
use std::time::Instant;

use super::{cast, CaptureError, Message, StartRecording, StoppedRecording};

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

fn remember_error(recording: &mut ActiveRecording, result: std::io::Result<()>) {
    if recording.error.is_none() {
        if let Err(error) = result {
            recording.error = Some(error.to_string());
        }
    }
}
