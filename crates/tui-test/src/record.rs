use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::api::RecordingFormat;

pub(crate) mod cast;
#[cfg(feature = "recording-raster")]
pub mod frames;
mod path;
mod seed;
mod worker;

use worker::worker_loop;

#[derive(Clone)]
pub(crate) struct Capture {
    sender: mpsc::Sender<Message>,
}

pub(crate) struct Recorder {
    sender: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
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
    pub background: Option<crate::api::CaptureBackground>,
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
    pub background: Option<crate::api::CaptureBackground>,
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
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || worker_loop(receiver, writer, logger, cols, rows));
        Ok(Self {
            sender,
            worker: Some(worker),
            automatic,
        })
    }

    pub fn automatic_enabled(&self) -> bool {
        self.automatic
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
                request: Box::new(request),
                reply,
            })
            .map_err(|_| CaptureError::WorkerStopped)
            .and_then(|()| response.recv().unwrap_or(Err(CaptureError::WorkerStopped)))
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
        // Even without a sink, the worker needs the bounded terminal state and
        // decoder boundary to seed a later manual recording correctly.
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
                background: None,
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
            background: None,
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
                background: None,
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

    #[test]
    fn manual_capture_and_export_targets_cannot_alias_the_automatic_recording() {
        let directory = temp_path("alias-directory");
        std::fs::create_dir_all(directory.join("child")).unwrap();
        let automatic = directory.join("automatic.cast");
        let other = directory.join("selected.cast");
        let recorder = Recorder::create(
            Some(automatic.clone()),
            5,
            4,
            &[],
            false,
            Arc::new(crate::logger::Logger::disabled()),
        )
        .unwrap();
        recorder.capture().on_data(b"preserved-before");
        recorder.flush().unwrap();
        let before = std::fs::read(&automatic).unwrap();
        let hard_link = directory.join("hard-link.cast");
        std::fs::hard_link(&automatic, &hard_link).unwrap();
        let mut aliases = vec![
            automatic.clone(),
            directory.join("child").join("..").join("automatic.cast"),
            hard_link.clone(),
        ];
        let symlink = directory.join("symlink.cast");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&automatic, &symlink);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&automatic, &symlink);
        #[cfg(any(unix, windows))]
        if linked.is_ok() {
            aliases.push(symlink.clone());
        }

        for alias in &aliases {
            for capture_alias in [false, true] {
                let mut request = manual_request(&other);
                if capture_alias {
                    request.capture_path = alias.clone();
                } else {
                    request.target_path = alias.clone();
                }
                let error = recorder.start(request).unwrap_err();
                assert!(
                    matches!(error, CaptureError::Io(message) if message.contains("automatic recording")),
                    "{}",
                    alias.display()
                );
                assert_eq!(std::fs::read(&automatic).unwrap(), before);
                assert!(!other.exists());
            }
        }
        recorder.capture().on_data(b"preserved-after");
        recorder.flush().unwrap();
        let after = std::fs::read_to_string(&automatic).unwrap();
        assert!(after.contains("preserved-before"));
        assert!(after.contains("preserved-after"));
        assert_eq!(
            after.lines().filter(|line| line.starts_with('{')).count(),
            1
        );
        drop(recorder);
        std::fs::remove_file(hard_link).unwrap();
        if symlink.exists() {
            std::fs::remove_file(symlink).unwrap();
        }
        std::fs::remove_file(automatic).unwrap();
        std::fs::remove_dir(directory.join("child")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn manual_seeds_preserve_rendition_wrap_and_margins_without_automatic_capture() {
        use crate::profile::Profile;
        use crate::terminal::alacritty::AlacrittyEmu;
        use crate::terminal::emu::Emulator;

        for automatic_enabled in [false, true] {
            let automatic = temp_path("seed-automatic");
            let target = temp_path("seed-manual");
            let recorder = Recorder::create(
                automatic_enabled.then(|| automatic.clone()),
                5,
                4,
                &[],
                false,
                Arc::new(crate::logger::Logger::disabled()),
            )
            .unwrap();
            let capture = recorder.capture();
            let mut source = AlacrittyEmu::new(5, 4, &Profile::default());
            let prefix = b"\x1b[2;3r\x1b[31m\x1b[3;1Habcde";
            capture.on_data(prefix);
            source.process(prefix);
            let mut request = manual_request(&target);
            request.initial_output = cast::snapshot_to_ansi(&source);
            recorder.start(request).unwrap();
            let suffix = b"X\nY";
            capture.on_data(suffix);
            source.process(suffix);
            recorder.stop().unwrap();

            let mut replay = AlacrittyEmu::new(5, 4, &Profile::default());
            let cast = std::fs::read_to_string(&target).unwrap();
            for event in cast.lines().skip(1) {
                let event: serde_json::Value = serde_json::from_str(event).unwrap();
                assert_eq!(event[1], "o");
                replay.process(event[2].as_str().unwrap().as_bytes());
            }
            assert_eq!(replay.viewable_rows(), source.viewable_rows());
            assert_eq!(replay.cursor(), source.cursor());
            drop(recorder);
            std::fs::remove_file(target).unwrap();
            if automatic_enabled {
                std::fs::remove_file(automatic).unwrap();
            }
        }
    }

    fn manual_request(path: &std::path::Path) -> StartRecording {
        StartRecording {
            target_path: path.to_path_buf(),
            capture_path: path.to_path_buf(),
            format: RecordingFormat::Cast,
            cols: 5,
            rows: 4,
            env: Vec::new(),
            initial_output: String::new(),
            #[cfg(feature = "recording-raster")]
            zoom: 1.0,
            #[cfg(feature = "recording-raster")]
            background: None,
            #[cfg(feature = "recording-raster")]
            timeline: frames::TimelineOptions::default(),
            #[cfg(feature = "recording-raster")]
            ffmpeg_path: None,
        }
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
