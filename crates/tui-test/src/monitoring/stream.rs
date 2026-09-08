//! One duplex attachment implementation for daemon and process-owned terminals.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::input::MouseRemapper;
use super::ipc::{Connection, Incoming};
use super::protocol::{Attach, MonitorInput, MonitorOutput, MonitorReady, Response, VERSION};
use super::{render, viewport::Viewport};
use crate::{engine::LiveFrame, Engine, TuiTestError};

pub trait MonitorSession: Send + Sync {
    fn frame(&self) -> Option<LiveFrame>;
    fn write(&self, bytes: &[u8]) -> Result<(), TuiTestError>;
    fn resize(&self, viewer: (u16, u16));
    fn synchronize_size(&self) -> Result<(), TuiTestError>;
    fn active(&self) -> bool {
        true
    }
}

struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub struct DaemonSession {
    engine: Arc<Engine>,
    viewport: Viewport,
    activity: Arc<Mutex<Instant>>,
}

impl DaemonSession {
    pub fn new(engine: Arc<Engine>, attach: &Attach, activity: Arc<Mutex<Instant>>) -> Self {
        let viewport = engine.monitor_viewport(
            None,
            render::content_size((attach.cols, attach.rows)),
            attach.interactive,
        );
        Self {
            engine,
            viewport,
            activity,
        }
    }
}

impl MonitorSession for DaemonSession {
    fn frame(&self) -> Option<LiveFrame> {
        *self
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
        self.engine.frame()
    }
    fn write(&self, bytes: &[u8]) -> Result<(), TuiTestError> {
        self.engine.write_monitor_input_raw(bytes)
    }
    fn resize(&self, viewer: (u16, u16)) {
        self.viewport.update(render::content_size(viewer));
    }
    fn synchronize_size(&self) -> Result<(), TuiTestError> {
        self.engine.apply_monitor_viewport(&self.viewport)
    }
}

fn mouse_size(source: &dyn MonitorSession) -> Option<(u16, u16)> {
    source
        .frame()
        .filter(|frame| frame.mouse_mode != crate::terminal::emu::MouseMode::None)
        .map(|frame| frame.size)
}

pub fn serve(
    mut connection: Connection,
    source: Arc<dyn MonitorSession>,
    session: &str,
    attach: Attach,
) -> io::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let writer = connection.writer();
    if let Err(error) = attach.validate() {
        writer.send(&Response::from_error(error), &stop)?;
        connection.drain(Duration::from_secs(2));
        return Ok(());
    }
    if !source.active() {
        writer.send(&Response::from_error(TuiTestError::no_session()), &stop)?;
        connection.drain(Duration::from_secs(2));
        return Ok(());
    }
    let viewer = Arc::new(Mutex::new((attach.cols, attach.rows)));
    let mut modes = render::ModeMirror::default();
    let frame = render::render_frame(
        source.frame().as_ref(),
        (attach.cols, attach.rows),
        session,
        attach.interactive,
        &mut modes,
    );
    writer.send(
        &Response::with(
            serde_json::to_value(MonitorReady {
                protocol: VERSION,
                frame,
            })
            .map_err(io::Error::other)?,
        ),
        &stop,
    )?;

    let reader_source = source.clone();
    let reader_stop = stop.clone();
    let reader_viewer = viewer.clone();
    let reader = std::thread::spawn(move || {
        let _stop = StopOnDrop(reader_stop.clone());
        let result = read_input(
            &mut connection,
            &*reader_source,
            &reader_stop,
            &reader_viewer,
            attach.interactive,
        );
        (connection, result)
    });

    let mut output = Ok(());
    // Input completion stops new frames, not a JSON message already being written.
    // The writer's deadline still bounds an unresponsive peer.
    let complete_write = AtomicBool::new(false);
    while !stop.load(Ordering::Acquire) && source.active() {
        let size = *viewer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = render::render_frame(
            source.frame().as_ref(),
            size,
            session,
            attach.interactive,
            &mut modes,
        );
        if let Err(error) = writer.send(&MonitorOutput::Frame { frame }, &complete_write) {
            output = Err(error);
            break;
        }
        std::thread::sleep(Duration::from_millis(super::host::MONITOR_FRAME_MS));
    }
    stop.store(true, Ordering::Release);
    let (connection, failure) = match reader.join() {
        Ok((connection, result)) => (Some(connection), result.err()),
        Err(_) => (
            None,
            Some(TuiTestError::internal("monitor input worker panicked")),
        ),
    };
    // A failed write may have sent a partial line; never append another JSON value.
    output?;
    let mut output = Ok(());
    let sent = if let Some(error) = failure {
        output = Err(io::Error::other(error.message.clone()));
        writer.send(
            &MonitorOutput::Error {
                message: error.message,
                error_kind: error.kind,
            },
            &complete_write,
        )
    } else {
        writer.send(&MonitorOutput::Closed, &complete_write)
    };
    if sent.is_ok() {
        if let Some(mut connection) = connection {
            connection.drain(Duration::from_secs(2));
        }
    }
    output
}

fn read_input(
    connection: &mut Connection,
    source: &dyn MonitorSession,
    stop: &AtomicBool,
    viewer: &Mutex<(u16, u16)>,
    interactive: bool,
) -> Result<(), TuiTestError> {
    let mut mouse = MouseRemapper::new(
        *viewer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let mut last_input = Instant::now();
    source.synchronize_size()?;
    while !stop.load(Ordering::Acquire) && source.active() {
        match connection
            .try_receive::<MonitorInput>()
            .map_err(|error| TuiTestError::internal(error.to_string()))?
        {
            Incoming::Closed | Incoming::Message(MonitorInput::Detach) => break,
            Incoming::Message(MonitorInput::Write { data }) => {
                if !interactive {
                    return Err(TuiTestError::usage("read-only monitor cannot send input"));
                }
                last_input = Instant::now();
                let input = mouse.push(&data, mouse_size(source));
                source.write(&input)?;
            }
            Incoming::Message(MonitorInput::Resize { cols, rows }) => {
                if cols == 0 || rows == 0 {
                    return Err(TuiTestError::usage(
                        "monitor dimensions must be greater than zero",
                    ));
                }
                source.resize((cols, rows));
                *viewer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = (cols, rows);
                mouse.resize((cols, rows));
                source.synchronize_size()?;
            }
            Incoming::Pending => {
                source.synchronize_size()?;
                if interactive {
                    mouse.observe(mouse_size(source));
                    if last_input.elapsed() >= Duration::from_millis(50) {
                        source.write(&mouse.on_idle())?;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    if interactive && source.active() {
        source.write(&mouse.finish())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ipc;
    use super::*;
    use interprocess::local_socket::traits::Listener;

    struct EmptySession(std::sync::mpsc::Sender<()>);

    impl MonitorSession for EmptySession {
        fn frame(&self) -> Option<LiveFrame> {
            let _ = self.0.send(());
            None
        }
        fn write(&self, _: &[u8]) -> Result<(), TuiTestError> {
            Ok(())
        }
        fn resize(&self, _: (u16, u16)) {}
        fn synchronize_size(&self) -> Result<(), TuiTestError> {
            Ok(())
        }
    }

    #[test]
    fn input_failure_finishes_the_in_flight_frame_before_its_error() {
        let endpoint = std::env::temp_dir()
            .join(format!("tt-stream-error-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let listener = ipc::listen(&endpoint).unwrap();
        let (rendered, frames) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                serve(
                    Connection::new(listener.accept().unwrap()).unwrap(),
                    Arc::new(EmptySession(rendered)),
                    "large-frame",
                    Attach::new(200, 100, false),
                )
            });
            let mut connection = Connection::new(ipc::connect(&endpoint).unwrap()).unwrap();
            let stop = AtomicBool::new(false);
            let ready: Response = connection
                .receive(Some(Duration::from_secs(5)), &stop)
                .unwrap();
            assert!(ready.ok);
            // Stop reading until the socket fills and frame production stalls.
            // Keep individual frames small enough to finish within the write deadline.
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match frames.recv_timeout(Duration::from_millis(200)) {
                    Ok(()) => assert!(Instant::now() < deadline, "frame output did not back up"),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(error) => panic!("frame output stopped before input failed: {error}"),
                }
            }
            connection
                .writer()
                .send(&MonitorInput::Resize { cols: 0, rows: 24 }, &stop)
                .unwrap();
            std::thread::sleep(Duration::from_millis(100));
            loop {
                match connection
                    .receive::<MonitorOutput>(Some(Duration::from_secs(5)), &stop)
                    .unwrap()
                {
                    MonitorOutput::Frame { .. } => {}
                    MonitorOutput::Error {
                        message,
                        error_kind,
                    } => {
                        assert_eq!(error_kind, crate::ErrorKind::Usage);
                        assert_eq!(message, "monitor dimensions must be greater than zero");
                        break;
                    }
                    MonitorOutput::Closed => panic!("input failure was lost"),
                }
            }
            drop(connection);
            assert!(server.join().unwrap().is_err());
        });
    }
}
