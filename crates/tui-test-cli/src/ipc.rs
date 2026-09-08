//! cli ↔ daemon transport over an `interprocess` local socket.
//! One JSON request line per connection, one JSON response line back.

use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions};

pub use interprocess::local_socket::Stream;

use crate::protocol::{Request, Response};

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
}
