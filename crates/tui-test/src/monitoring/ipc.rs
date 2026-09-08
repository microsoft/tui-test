//! cli ↔ daemon transport over an `interprocess` local socket.
//! One JSON request line per connection, one JSON response line back.

use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions};

pub use interprocess::local_socket::Stream;

use super::protocol::{Request, Response};

fn to_name(raw: &str) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    if cfg!(windows) {
        raw.to_ns_name::<GenericNamespaced>()
    } else {
        raw.to_fs_name::<GenericFilePath>()
    }
}

/// Connect to a running daemon and exchange a single request/response.
pub fn send(socket: &str, req: &Request) -> anyhow::Result<Response> {
    exchange(connect(socket)?, req)
}

pub fn exchange(conn: Stream, req: &Request) -> anyhow::Result<Response> {
    exchange_on(conn, req)
}

pub fn exchange_timeout(
    conn: Stream,
    req: &Request,
    timeout: Duration,
) -> anyhow::Result<Response> {
    #[cfg(not(windows))]
    conn.set_nonblocking(true)?;
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "discovery timeout is too large",
        )
    })?;
    exchange_on(
        DeadlineStream {
            stream: conn,
            deadline,
        },
        req,
    )
}

fn exchange_on(conn: impl Read + Write, req: &Request) -> anyhow::Result<Response> {
    let mut reader = BufReader::new(conn);
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    reader.get_mut().write_all(line.as_bytes())?;
    reader.get_mut().flush()?;

    let mut response = String::new();
    reader.read_line(&mut response)?;
    let resp: Response = serde_json::from_str(response.trim())?;
    Ok(resp)
}

struct DeadlineStream {
    stream: Stream,
    deadline: Instant,
}

impl DeadlineStream {
    fn retry<T>(
        &mut self,
        mut operation: impl FnMut(&mut Stream) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        loop {
            let Some(remaining) = self.deadline.checked_duration_since(Instant::now()) else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "process monitor discovery timed out",
                ));
            };
            match operation(&mut self.stream) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(remaining.min(Duration::from_millis(5)));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.retry(|stream| read_available(stream, buffer))
    }
}

/// Read without waiting for bytes. Unix callers must enable nonblocking mode.
/// Peeking distinguishes an idle Windows pipe from an actual peer disconnect.
pub fn read_available(stream: &mut Stream, buffer: &mut [u8]) -> std::io::Result<usize> {
    if buffer.is_empty() {
        return Ok(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsHandle, AsRawHandle};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let Stream::NamedPipe(pipe) = &*stream;
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
            let error = std::io::Error::last_os_error();
            return if matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::NotConnected
            ) {
                Ok(0)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let length = buffer.len().min(available as usize);
        stream.read(&mut buffer[..length])
    }
    #[cfg(not(windows))]
    stream.read(buffer)
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.retry(|stream| match stream.write(buffer) {
            Ok(0) => Err(std::io::ErrorKind::WouldBlock.into()),
            result => result,
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.retry(Write::flush)
    }
}

/// Is a daemon currently accepting connections on this socket?
pub fn is_running(socket: &str) -> bool {
    match to_name(socket) {
        Ok(name) => Stream::connect(name).is_ok(),
        Err(_) => false,
    }
}

/// Open a raw connection to a running daemon (for streaming, e.g. the monitor).
pub fn connect(socket: &str) -> std::io::Result<Stream> {
    let name = to_name(socket)?;
    Stream::connect(name)
}

/// Bind the daemon listener, removing any stale Unix socket file first.
pub fn listen(socket: &str) -> anyhow::Result<interprocess::local_socket::Listener> {
    if !cfg!(windows) {
        let _ = std::fs::remove_file(socket);
    }
    let name = to_name(socket)?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    Ok(listener)
}

/// Keep the reader for streaming requests so any read-ahead remains available.
pub fn read_request(reader: &mut impl BufRead) -> anyhow::Result<Request> {
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    if line.last() != Some(&b'\n') {
        anyhow::bail!("connection closed before request");
    }
    let req: Request = serde_json::from_slice(&line)?;
    Ok(req)
}

/// Write one response line to an accepted connection.
pub fn write_response(conn: &mut Stream, resp: &Response) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(resp)?;
    line.push('\n');
    conn.write_all(line.as_bytes())?;
    conn.flush()?;
    Ok(())
}

/// Wait briefly for the client to read the final response.
/// Windows named pipes discard buffered data when the server exits; EOF proves
/// the reply arrived. An unresponsive client can only cost `timeout`.
pub fn drain_peer(conn: Stream, timeout: Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut conn = conn;
        let mut buf = [0u8; 64];
        while let Ok(n) = conn.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(timeout);
}

#[cfg(test)]
mod tests {
    use super::super::protocol::MonitorInput;
    use super::*;
    use std::io::Cursor;

    #[test]
    fn monitor_handshake_retains_pipelined_input() {
        let wire = b"{\"kind\":\"monitor_input_stream\",\"cols\":80,\"rows\":24}\n\
            {\"kind\":\"write\",\"data\":[97,98]}\n\
            {\"kind\":\"resize\",\"cols\":100,\"rows\":30}\n";
        for capacity in [1, 16, wire.len()] {
            let mut reader = BufReader::with_capacity(capacity, Cursor::new(wire));
            assert!(matches!(
                read_request(&mut reader).unwrap(),
                Request::MonitorInputStream { cols: 80, rows: 24 }
            ));
            if capacity == wire.len() {
                assert!(!reader.buffer().is_empty(), "exercise actual read-ahead");
            }
            let messages = serde_json::Deserializer::from_reader(reader)
                .into_iter::<MonitorInput>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(matches!(
                messages.as_slice(),
                [MonitorInput::Write { data }, MonitorInput::Resize { cols: 100, rows: 30 }]
                    if data == b"ab"
            ));
        }
    }

    #[test]
    fn monitor_handshake_requires_a_complete_request_line() {
        for wire in [b"".as_slice(), b"{\"kind\":\"ping\"}"] {
            assert!(read_request(&mut Cursor::new(wire)).is_err());
        }
    }

    #[test]
    fn monitor_discovery_timeout_works_with_an_unresponsive_peer() {
        use interprocess::local_socket::traits::ListenerExt;
        let descriptor = super::super::host::new_descriptor().unwrap();
        super::super::host::ensure_host_dir().unwrap();
        let listener = listen(&descriptor.endpoint).unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let _stream = listener.incoming().next().unwrap().unwrap();
            let _ = released.recv();
        });
        let result = exchange_timeout(
            connect(&descriptor.endpoint).unwrap(),
            &Request::HostSessions,
            Duration::from_millis(50),
        );
        let _ = release.send(());
        server.join().unwrap();
        if !cfg!(windows) {
            let _ = std::fs::remove_file(&descriptor.endpoint);
        }
        assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn monitor_discovery_timeout_reads_a_live_peer_response() {
        use interprocess::local_socket::traits::ListenerExt;
        let descriptor = super::super::host::new_descriptor().unwrap();
        super::super::host::ensure_host_dir().unwrap();
        let listener = listen(&descriptor.endpoint).unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let stream = listener.incoming().next().unwrap().unwrap();
            let mut reader = BufReader::new(stream);
            assert!(matches!(
                read_request(&mut reader).unwrap(),
                Request::HostSessions
            ));
            write_response(
                reader.get_mut(),
                &Response::with(serde_json::json!({"marker":"ready"})),
            )
            .unwrap();
            let _ = released.recv();
        });
        let result = exchange_timeout(
            connect(&descriptor.endpoint).unwrap(),
            &Request::HostSessions,
            Duration::from_secs(2),
        );
        let _ = release.send(());
        server.join().unwrap();
        if !cfg!(windows) {
            let _ = std::fs::remove_file(&descriptor.endpoint);
        }
        assert_eq!(result.unwrap().data.unwrap()["marker"], "ready");
    }
}
