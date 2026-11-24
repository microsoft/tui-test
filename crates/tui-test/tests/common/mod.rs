//! Shared child fixture for the checked-input integration tests.
//!
//! The fixture child enables terminal modes on request and reports every
//! stdin byte back as `rx:<hex>` text, so each test observes exactly what
//! the child received. A `@@command@@` token in the input stream changes
//! modes or prints filler instead of being echoed.
//!
//! Each test binary keeps its own small `fixture_child` test entry point
//! (the child process is the test binary itself, re-executed with a spec in
//! its own environment variable); everything else lives here so the
//! command set cannot drift between the suites.

use std::io::{Read, Write};

pub fn emit(bytes: &[u8]) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(bytes).expect("fixture stdout write");
    stdout.flush().expect("fixture stdout flush");
}

pub fn run_command(command: &[u8]) {
    match command {
        b"mouse-off" => emit(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l"),
        b"mouse-click" => emit(b"\x1b[?1000h"),
        b"mouse-drag" => emit(b"\x1b[?1002h"),
        b"mouse-motion" => emit(b"\x1b[?1003h"),
        b"x10-on" => emit(b"\x1b[?9h"),
        b"x10-off" => emit(b"\x1b[?9l"),
        b"hl-on" => emit(b"\x1b[?1001h"),
        b"hl-off" => emit(b"\x1b[?1001l"),
        b"sgr-on" => emit(b"\x1b[?1006h"),
        b"sgr-off" => emit(b"\x1b[?1016h"),
        b"paste-on" => emit(b"\x1b[?2004h"),
        b"paste-off" => emit(b"\x1b[?2004l"),
        b"focus-on" => emit(b"\x1b[?1004h"),
        b"focus-off" => emit(b"\x1b[?1004l"),
        b"fill" => {
            for index in 0..60 {
                emit(format!("line {index:02}\r\n").as_bytes());
            }
        }
        _ => emit(b"bad-command\r\n"),
    }
}

fn first_command(buffer: &[u8]) -> Option<(usize, usize)> {
    let start = buffer.windows(2).position(|pair| pair == b"@@")?;
    let rest = &buffer[start + 2..];
    let close = rest.windows(2).position(|pair| pair == b"@@")?;
    Some((start, start + 2 + close + 2))
}

fn last_marker(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).rposition(|pair| pair == b"@@")
}

fn echo_hex(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut line = String::from("rx:");
    for byte in bytes {
        line.push_str(&format!("{byte:02x}"));
    }
    line.push_str("\r\n");
    emit(line.as_bytes());
}

/// Echo input bytes as hex, holding back a trailing partial command.
fn drain_buffer(buffer: &mut Vec<u8>) {
    loop {
        if let Some((start, end)) = first_command(buffer) {
            let head = buffer[..start].to_vec();
            let command = buffer[start + 2..end - 2].to_vec();
            buffer.drain(..end);
            echo_hex(&head);
            run_command(&command);
            continue;
        }
        if let Some(marker) = last_marker(buffer) {
            let head = buffer[..marker].to_vec();
            buffer.drain(..marker);
            echo_hex(&head);
            return;
        }
        if buffer.ends_with(b"@") {
            let keep = buffer.len() - 1;
            let head = buffer[..keep].to_vec();
            buffer.drain(..keep);
            echo_hex(&head);
            return;
        }
        let rest = std::mem::take(buffer);
        echo_hex(&rest);
        return;
    }
}

pub fn run_fixture(spec: &str) {
    #[cfg(unix)]
    assert!(std::process::Command::new("stty")
        .args(["raw", "-echo"])
        .status()
        .expect("stty runs")
        .success());
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
            fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
        }
        assert_ne!(SetConsoleMode(GetStdHandle(-10i32 as u32), 0), 0);
    }
    for command in spec.split(',') {
        if command.is_empty() {
            continue;
        }
        run_command(command.as_bytes());
    }
    emit(b"READY\r\n");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut input = std::io::stdin().lock();
    loop {
        match input.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                drain_buffer(&mut buffer);
            }
            Err(_) => break,
        }
    }
}
