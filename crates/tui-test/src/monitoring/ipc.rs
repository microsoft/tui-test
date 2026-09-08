//! Shared local IPC and bounded, cancellation-aware JSON streaming.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions};
use serde::{de::DeserializeOwned, Serialize};

use super::protocol::Response;
pub use interprocess::local_socket::Stream;

const MAX_MESSAGE: usize = 16 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(5);

fn to_name(raw: &str) -> io::Result<interprocess::local_socket::Name<'_>> {
    if cfg!(windows) {
        raw.to_ns_name::<GenericNamespaced>()
    } else {
        raw.to_fs_name::<GenericFilePath>()
    }
}

pub fn connect(socket: &str) -> io::Result<Stream> {
    Stream::connect(to_name(socket)?)
}

pub fn is_running(socket: &str) -> bool {
    connect(socket).is_ok()
}

pub fn listen(socket: &str) -> anyhow::Result<interprocess::local_socket::Listener> {
    if !cfg!(windows) {
        let _ = std::fs::remove_file(socket);
    }
    Ok(ListenerOptions::new()
        .name(to_name(socket)?)
        .create_sync()?)
}

pub fn read_request<T: DeserializeOwned>(reader: &mut impl BufRead) -> anyhow::Result<T> {
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    if line.last() != Some(&b'\n') {
        anyhow::bail!("connection closed before request");
    }
    Ok(serde_json::from_slice(&line)?)
}

pub fn write_response(conn: &mut Stream, response: &Response) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *conn, response)?;
    conn.write_all(b"\n")?;
    conn.flush()?;
    Ok(())
}

pub fn send(socket: &str, request: &impl Serialize) -> anyhow::Result<Response> {
    exchange(connect(socket)?, request)
}

pub fn exchange(conn: Stream, request: &impl Serialize) -> anyhow::Result<Response> {
    let mut conn = Connection::new(conn)?;
    let stop = AtomicBool::new(false);
    conn.writer().send(request, &stop)?;
    Ok(conn.receive(None, &stop)?)
}

pub fn exchange_timeout(
    conn: Stream,
    request: &impl Serialize,
    timeout: Duration,
) -> anyhow::Result<Response> {
    let mut conn = Connection::new(conn)?;
    let stop = AtomicBool::new(false);
    conn.writer().send(request, &stop)?;
    Ok(conn.receive(Some(timeout), &stop)?)
}

pub enum Incoming<T> {
    Message(T),
    Pending,
    Closed,
}

/// One socket, with independent read/write handles sharing its lifetime.
pub struct Connection {
    stream: Arc<Stream>,
    pending: Vec<u8>,
    searched: usize,
}

impl Connection {
    pub fn new(stream: Stream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream: Arc::new(stream),
            pending: Vec::new(),
            searched: 0,
        })
    }

    pub fn from_reader(reader: BufReader<Stream>) -> io::Result<Self> {
        let pending = reader.buffer().to_vec();
        let mut connection = Self::new(reader.into_inner())?;
        connection.pending = pending;
        Ok(connection)
    }

    pub fn writer(&self) -> Writer {
        Writer(self.stream.clone())
    }

    pub fn try_receive<T: DeserializeOwned>(&mut self) -> io::Result<Incoming<T>> {
        loop {
            if let Some(end) = self.pending[self.searched..]
                .iter()
                .position(|byte| *byte == b'\n')
            {
                let end = self.searched + end;
                if end + 1 > MAX_MESSAGE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "IPC message is too large",
                    ));
                }
                let message =
                    serde_json::from_slice(&self.pending[..end]).map_err(io::Error::other)?;
                self.pending.drain(..=end);
                self.searched = 0;
                return Ok(Incoming::Message(message));
            }
            self.searched = self.pending.len();
            if self.pending.len() >= MAX_MESSAGE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "IPC message is too large",
                ));
            }
            let mut buffer = [0u8; 16 * 1024];
            match read_available(&self.stream, &mut buffer) {
                Ok(0) if self.pending.is_empty() => return Ok(Incoming::Closed),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "incomplete IPC message",
                    ))
                }
                Ok(read) => self.pending.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(Incoming::Pending)
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn receive<T: DeserializeOwned>(
        &mut self,
        timeout: Option<Duration>,
        stop: &AtomicBool,
    ) -> io::Result<T> {
        let deadline = timeout
            .map(|timeout| {
                Instant::now().checked_add(timeout).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "IPC timeout is too large")
                })
            })
            .transpose()?;
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            match self.try_receive()? {
                Incoming::Message(value) => return Ok(value),
                Incoming::Closed => return Err(io::ErrorKind::UnexpectedEof.into()),
                Incoming::Pending => {}
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC response timed out",
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Keep named-pipe replies alive until the peer consumes them and closes.
    pub fn drain(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.try_receive::<serde_json::Value>() {
                Ok(Incoming::Closed) | Err(_) => break,
                _ => std::thread::sleep(POLL),
            }
        }
    }
}

pub struct Writer(Arc<Stream>);

impl Writer {
    pub fn send(&self, value: &impl Serialize, stop: &AtomicBool) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_MESSAGE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "IPC message is too large",
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut remaining = bytes.as_slice();
        let mut chunk = 4096;
        let mut stream = &*self.0;
        while !remaining.is_empty() {
            if stop.load(Ordering::Acquire) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            match stream.write(&remaining[..remaining.len().min(chunk)]) {
                // PIPE_NOWAIT refuses oversized writes even with an empty buffer.
                Ok(0) if chunk > 256 => chunk /= 2,
                Ok(0) => std::thread::sleep(POLL),
                Ok(written) => remaining = &remaining[written..],
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(POLL)
                }
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC write timed out",
                ));
            }
        }
        Ok(())
    }
}

/// Unix callers enable nonblocking mode; peeking avoids Windows' idle-as-EOF behavior.
pub fn read_available(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    if buffer.is_empty() {
        return Ok(0);
    }
    let mut source = stream;
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsHandle, AsRawHandle};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;
        let Stream::NamedPipe(pipe) = stream;
        let mut available = 0;
        let success = unsafe {
            PeekNamedPipe(
                pipe.as_handle().as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            return if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::NotConnected
            ) {
                Ok(0)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let length = buffer.len().min(available as usize);
        source.read(&mut buffer[..length])
    }
    #[cfg(not(windows))]
    source.read(buffer)
}

pub fn drain_peer(conn: Stream, timeout: Duration) {
    if let Ok(mut connection) = Connection::new(conn) {
        connection.drain(timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{Attach, MonitorInput, Request};
    use super::*;

    fn pair() -> (Stream, Stream) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let socket = std::env::temp_dir().join(format!(
            "tt-ipc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = socket.to_str().unwrap();
        let listener = listen(socket).unwrap();
        let client = connect(socket).unwrap();
        (client, listener.accept().unwrap())
    }

    #[test]
    fn monitor_handshake_retains_pipelined_input() {
        let (mut client, server) = pair();
        let wire = format!(
            "{}\n{}\n",
            serde_json::to_string(&Request::Monitor(Attach::new(80, 24, true))).unwrap(),
            serde_json::to_string(&MonitorInput::Resize {
                cols: 100,
                rows: 30
            })
            .unwrap()
        );
        client.write_all(wire.as_bytes()).unwrap();
        let mut reader = BufReader::new(server);
        assert!(matches!(
            read_request::<Request>(&mut reader).unwrap(),
            Request::Monitor(_)
        ));
        let mut connection = Connection::from_reader(reader).unwrap();
        assert!(matches!(
            connection
                .receive::<MonitorInput>(Some(Duration::from_secs(2)), &AtomicBool::new(false))
                .unwrap(),
            MonitorInput::Resize {
                cols: 100,
                rows: 30
            }
        ));
    }

    #[test]
    fn idle_connection_times_out_and_cancellation_interrupts_receiving() {
        let (client, _server) = pair();
        let mut connection = Connection::new(client).unwrap();
        assert!(matches!(
            connection.try_receive::<Response>().unwrap(),
            Incoming::Pending
        ));
        let stop = AtomicBool::new(false);
        assert_eq!(
            connection
                .receive::<Response>(Some(Duration::from_millis(30)), &stop)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(30));
                stop.store(true, Ordering::Release);
            });
            assert_eq!(
                connection
                    .receive::<Response>(None, &stop)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::Interrupted
            );
        });
    }

    #[test]
    fn exchange_receives_large_responses_without_losing_bytes() {
        let (client, server) = pair();
        let payload = "frame".repeat(30_000);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut connection = Connection::new(server).unwrap();
                let stop = AtomicBool::new(false);
                let request: Request = connection
                    .receive(Some(Duration::from_secs(2)), &stop)
                    .unwrap();
                assert!(matches!(request, Request::Ping));
                connection
                    .writer()
                    .send(&Response::with(payload.clone().into()), &stop)
                    .unwrap();
                connection.drain(Duration::from_secs(2));
            });
            let response =
                exchange_timeout(client, &Request::Ping, Duration::from_secs(2)).unwrap();
            assert_eq!(response.data.unwrap(), payload);
        });
    }

    #[test]
    fn disconnect_distinguishes_complete_and_truncated_messages() {
        for truncated in [false, true] {
            let (client, mut server) = pair();
            let mut connection = Connection::new(client).unwrap();
            if truncated {
                server.write_all(b"{\"ok\":").unwrap();
                assert!(matches!(
                    connection.try_receive::<Response>().unwrap(),
                    Incoming::Pending
                ));
            }
            drop(server);
            let error = connection
                .receive::<Response>(Some(Duration::from_secs(2)), &AtomicBool::new(false))
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        }
    }

    #[test]
    fn blocked_writer_has_a_deadline_and_honors_cancellation() {
        let payload = "x".repeat(4 * 1024 * 1024);
        for cancelled in [false, true] {
            let (client, _server) = pair();
            let connection = Connection::new(client).unwrap();
            let writer = connection.writer();
            let stop = AtomicBool::new(false);
            let started = Instant::now();
            std::thread::scope(|scope| {
                if cancelled {
                    scope.spawn(|| {
                        std::thread::sleep(Duration::from_millis(30));
                        stop.store(true, Ordering::Release);
                    });
                }
                let error = writer.send(&payload, &stop).unwrap_err();
                assert_eq!(
                    error.kind(),
                    if cancelled {
                        io::ErrorKind::Interrupted
                    } else {
                        io::ErrorKind::TimedOut
                    }
                );
            });
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }

    #[test]
    fn oversized_messages_are_rejected_with_or_without_a_newline() {
        for newline in [false, true] {
            let (client, _server) = pair();
            let mut connection = Connection::new(client).unwrap();
            connection.pending = vec![b' '; MAX_MESSAGE];
            if newline {
                connection.pending.push(b'\n');
            }
            assert_eq!(
                connection.try_receive::<Response>().err().unwrap().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}
