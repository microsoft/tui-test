//! cli ↔ daemon transport over an `interprocess` local socket.
//! One JSON request line per connection, one JSON response line back.

use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions};

pub use interprocess::local_socket::Stream;

use crate::protocol::{Request, Response};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(5);

fn to_name(raw: &str) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    if cfg!(windows) {
        raw.to_ns_name::<GenericNamespaced>()
    } else {
        raw.to_fs_name::<GenericFilePath>()
    }
}

/// Bound control-plane probes, including probes of an older, blocked daemon.
pub fn send_with_timeout(
    socket: &str,
    req: &Request,
    timeout: Duration,
) -> anyhow::Result<Response> {
    exchange_with_timeout(connect(socket)?, req, timeout)
}

pub fn exchange_with_timeout(
    mut conn: Stream,
    req: &Request,
    timeout: Duration,
) -> anyhow::Result<Response> {
    let deadline = Instant::now() + timeout;
    let mut line = serde_json::to_vec(req)?;
    line.push(b'\n');
    if line.len() > MAX_REQUEST_BYTES {
        anyhow::bail!("request exceeds {MAX_REQUEST_BYTES} bytes");
    }
    conn.set_nonblocking(true)?;
    write_until(&mut conn, &line, deadline)?;
    let mut reader = BufReader::new(conn);
    let line = read_line_until(&mut NonblockingReader(&mut reader), Some(deadline))?;
    Ok(serde_json::from_slice(&line)?)
}

pub fn exchange(conn: Stream, req: &Request) -> anyhow::Result<Response> {
    let mut reader = BufReader::new(conn);
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    if line.len() > MAX_REQUEST_BYTES {
        anyhow::bail!("request exceeds {MAX_REQUEST_BYTES} bytes");
    }
    reader.get_mut().write_all(line.as_bytes())?;
    reader.get_mut().flush()?;

    let mut response = String::new();
    reader.read_line(&mut response)?;
    let resp: Response = serde_json::from_str(response.trim())?;
    Ok(resp)
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
    let listener = ListenerOptions::new()
        .name(name)
        .reclaim_name(false)
        .create_sync()?;
    Ok(listener)
}

/// Keep the reader for streaming requests so any read-ahead remains available.
#[cfg(test)]
pub fn read_request(reader: &mut impl BufRead) -> anyhow::Result<Request> {
    Ok(serde_json::from_slice(&read_line_until(reader, None)?)?)
}

pub fn read_request_with_timeout(
    reader: &mut BufReader<Stream>,
    timeout: Duration,
) -> anyhow::Result<Request> {
    reader.get_ref().set_nonblocking(true)?;
    let result = read_line_until(
        &mut NonblockingReader(reader),
        Some(Instant::now() + timeout),
    );
    reader.get_ref().set_nonblocking(false)?;
    Ok(serde_json::from_slice(&result?)?)
}

fn read_line_until(
    reader: &mut impl BufRead,
    deadline: Option<Instant>,
) -> anyhow::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            anyhow::bail!("timed out reading request");
        }
        let available = match reader.fill_buf() {
            Ok([]) => anyhow::bail!("connection closed before request"),
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && deadline.is_some() => {
                std::thread::sleep(IO_POLL_INTERVAL);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let end = available.iter().position(|byte| *byte == b'\n');
        let length = end.map_or(available.len(), |end| end + 1);
        if line.len() + length > MAX_REQUEST_BYTES {
            anyhow::bail!("request exceeds {MAX_REQUEST_BYTES} bytes");
        }
        line.extend_from_slice(&available[..length]);
        reader.consume(length);
        if end.is_some() {
            return Ok(line);
        }
    }
}

struct NonblockingReader<'a>(&'a mut BufReader<Stream>);

impl Read for NonblockingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let available = self.fill_buf()?;
        let length = available.len().min(buffer.len());
        buffer[..length].copy_from_slice(&available[..length]);
        self.consume(length);
        Ok(length)
    }
}

impl BufRead for NonblockingReader<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.0.buffer().is_empty() {
            check_readable(self.0.get_ref())?;
        }
        self.0.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.0.consume(amount);
    }
}

#[cfg(windows)]
fn check_readable(conn: &Stream) -> std::io::Result<()> {
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let Stream::NamedPipe(pipe) = conn;
    let mut available = 0;
    // PIPE_NOWAIT reads can return zero on an empty, still-connected pipe.
    // Peek distinguishes that case from EOF before interprocess maps it to 0.
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
        return Err(std::io::Error::last_os_error());
    }
    if available == 0 {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn check_readable(_: &Stream) -> std::io::Result<()> {
    Ok(())
}

/// Write one response line to an accepted connection.
pub fn write_response(conn: &mut Stream, resp: &Response) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(resp)?;
    line.push('\n');
    conn.set_nonblocking(true)?;
    let result = write_until(conn, line.as_bytes(), Instant::now() + REQUEST_TIMEOUT);
    conn.set_nonblocking(false)?;
    result?;
    Ok(())
}

fn write_until(conn: &mut Stream, mut bytes: &[u8], deadline: Instant) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        match conn.write(bytes) {
            Ok(0) if cfg!(windows) => std::thread::sleep(IO_POLL_INTERVAL),
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(IO_POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
    conn.flush()
}

/// Wait briefly for the client to read the final response.
/// Windows named pipes discard buffered data when the server exits; EOF proves
/// the reply arrived. An unresponsive client can only cost `timeout`.
pub fn drain_peer(mut conn: Stream, timeout: Duration) {
    if conn.set_nonblocking(true).is_err() {
        return;
    }
    let start = Instant::now();
    let mut buf = [0u8; 64];
    while start.elapsed() < timeout {
        match check_readable(&conn).and_then(|()| conn.read(&mut buf)) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(IO_POLL_INTERVAL);
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::MonitorInput;
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
    fn request_size_is_bounded_before_parsing() {
        let bytes = vec![b' '; MAX_REQUEST_BYTES + 1];
        let mut reader = BufReader::with_capacity(1024, Cursor::new(bytes));
        let error = read_request(&mut reader).unwrap_err();
        assert!(error.to_string().contains("exceeds"));
        assert_eq!(reader.buffer(), b" ");
    }

    #[test]
    fn request_deadline_expires_without_data() {
        struct Pending;
        impl Read for Pending {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::WouldBlock.into())
            }
        }
        let mut reader = BufReader::new(Pending);
        let error = read_line_until(
            &mut reader,
            Some(Instant::now() + Duration::from_millis(20)),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
