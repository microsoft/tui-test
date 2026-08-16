//! [`Emulator`] backend built on `libghostty-vt`.
//!
//! Ghostty handles are deliberately `!Send`, so the native terminal lives
//! on one worker thread. [`GhosttyEmu`] is the `Send` channel handle used by
//! the rest of tui-test.

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use alacritty_terminal::vte::{Params, Parser, Perform};
use anyhow::{anyhow, Context, Result};

use crate::profile::{ColorSlot, Profile, Rgb};
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::{CursorShape, Emulator};

use self::core::GhosttyCore;

mod core;

#[derive(Default)]
struct TitleState {
    current: Option<String>,
    stack: Vec<Option<String>>,
}

impl Perform for TitleState {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if !matches!(params.first().copied(), Some(b"0" | b"2")) {
            return;
        }
        let title = params[1..]
            .iter()
            .map(|part| String::from_utf8_lossy(part))
            .collect::<Vec<_>>()
            .join(";");
        self.current = (!title.is_empty()).then_some(title);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || action != 't' || !intermediates.is_empty() {
            return;
        }
        let operation = params
            .iter()
            .next()
            .and_then(|param| param.first())
            .copied()
            .unwrap_or_default();
        match operation {
            22 => self.stack.push(self.current.clone()),
            23 => {
                if let Some(title) = self.stack.pop() {
                    self.current = title;
                }
            }
            _ => {}
        }
    }
}

struct TitleTracker {
    parser: Parser,
    state: TitleState,
}

impl TitleTracker {
    fn new() -> Self {
        Self {
            parser: Parser::new(),
            state: TitleState::default(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }
}

type Job = Box<dyn FnOnce(&mut GhosttyCore) + Send + 'static>;

pub struct GhosttyEmu {
    jobs: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    title: TitleTracker,
}

impl GhosttyEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Result<Self> {
        let (jobs, receiver) = mpsc::channel::<Job>();
        let (ready, started) = mpsc::sync_channel(1);
        let profile = *profile;
        let worker = thread::Builder::new()
            .name("tui-test-ghostty".to_string())
            .spawn(move || match GhosttyCore::new(cols, rows, profile) {
                Ok(mut core) => {
                    let _ = ready.send(Ok(()));
                    while let Ok(job) = receiver.recv() {
                        job(&mut core);
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(format!("{error:#}")));
                }
            })
            .context("spawning Ghostty worker")?;

        match started.recv().context("starting Ghostty worker")? {
            Ok(()) => Ok(Self {
                jobs: Some(jobs),
                worker: Some(worker),
                title: TitleTracker::new(),
            }),
            Err(message) => {
                let _ = worker.join();
                Err(anyhow!(message))
            }
        }
    }

    fn call<T, F>(&self, operation: &'static str, job: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&mut GhosttyCore) -> T + Send + 'static,
    {
        let (reply, response) = mpsc::sync_channel(1);
        self.jobs
            .as_ref()
            .expect("Ghostty worker is shutting down")
            .send(Box::new(move |core| {
                let _ = reply.send(job(core));
            }))
            .unwrap_or_else(|_| panic!("Ghostty worker stopped during {operation}"));
        response
            .recv()
            .unwrap_or_else(|_| panic!("Ghostty worker stopped during {operation}"))
    }

    fn call_result<T, F>(&self, operation: &'static str, job: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&mut GhosttyCore) -> Result<T> + Send + 'static,
    {
        self.call(operation, job)
            .unwrap_or_else(|error| panic!("Ghostty {operation} failed: {error:#}"))
    }
}

impl Drop for GhosttyEmu {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Emulator for GhosttyEmu {
    fn process(&mut self, bytes: &[u8]) {
        let owned = bytes.to_vec();
        self.call("processing output", move |core| core.process(&owned));
        self.title.feed(bytes);
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        self.call("draining replies", GhosttyCore::take_pending_writes)
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.call_result("resize", move |core| core.resize(cols, rows));
    }

    fn size(&self) -> (u16, u16) {
        self.call_result("reading size", |core| core.size())
    }

    fn cursor(&self) -> (u16, u16) {
        self.call_result("reading cursor", |core| Ok(core.frame()?.cursor))
    }

    fn title(&self) -> Option<String> {
        self.title.state.current.clone()
    }

    fn cursor_visible(&self) -> bool {
        self.call_result("reading cursor visibility", |core| {
            Ok(core.frame()?.cursor_visible)
        })
    }

    fn cursor_shape(&self) -> CursorShape {
        self.call_result("reading cursor shape", |core| {
            Ok(core.frame()?.cursor_shape)
        })
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.call_result("reading viewport", |core| Ok(core.frame()?.rows.clone()))
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        self.call_result("reading scrollback", GhosttyCore::full_rows)
    }

    fn color(&self, slot: ColorSlot) -> Rgb {
        self.call_result("reading color", move |core| core.color(slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::emulator_conformance_tests!(|cols, rows, profile| {
        Box::new(GhosttyEmu::new(cols, rows, profile).expect("create Ghostty emulator"))
    });

    #[test]
    fn title_sequences_can_span_process_calls() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        emu.process(b"\x1b]2;split");
        emu.process(b" title\x1b\\");
        assert_eq!(emu.title().as_deref(), Some("split title"));
    }
}
