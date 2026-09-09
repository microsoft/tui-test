//! [`Emulator`] backend built on the `ghostty-vt` dependency.
//!
//! Ghostty handles are deliberately `!Send`, so the native terminal lives
//! on one worker thread. [`GhosttyEmu`] is the `Send` channel handle used by
//! the rest of tui-test.

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use alacritty_terminal::vte::{Params, Parser, Perform};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::event::BellTracker;
use crate::input::keys::KeyPress;
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::{
    ClipboardType, ClipboardValidator, CursorShape, Emulator, KeyboardMode,
};

use self::core::GhosttyCore;

mod core;

#[derive(Debug)]
enum ClipboardOperation {
    Store {
        clipboard: ClipboardType,
        text: String,
    },
    Query {
        clipboard: ClipboardType,
        selector: u8,
        bell_terminated: bool,
    },
}

#[derive(Default)]
struct SequenceState {
    current: Option<String>,
    stack: Vec<Option<String>>,
    clipboard_operations: Vec<ClipboardOperation>,
}

impl Perform for SequenceState {
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        match params.first().copied() {
            Some(b"0" | b"2") => {
                let title = params[1..]
                    .iter()
                    .map(|part| String::from_utf8_lossy(part))
                    .collect::<Vec<_>>()
                    .join(";");
                self.current = (!title.is_empty()).then_some(title);
            }
            Some(b"52") => {
                let (clipboard, selector) = match params.get(1).copied() {
                    Some(b"c") => (ClipboardType::Clipboard, b'c'),
                    Some(b"p") => (ClipboardType::Selection, b'p'),
                    Some(b"s") => (ClipboardType::Selection, b's'),
                    _ => return,
                };
                match params.get(2).copied() {
                    Some(b"?") => self.clipboard_operations.push(ClipboardOperation::Query {
                        clipboard,
                        selector,
                        bell_terminated,
                    }),
                    Some(encoded) => {
                        let Ok(bytes) = BASE64.decode(encoded) else {
                            return;
                        };
                        let Ok(text) = String::from_utf8(bytes) else {
                            return;
                        };
                        self.clipboard_operations
                            .push(ClipboardOperation::Store { clipboard, text });
                    }
                    None => {}
                }
            }
            _ => {}
        }
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

struct SequenceTracker {
    parser: Parser,
    state: SequenceState,
}

impl SequenceTracker {
    fn new() -> Self {
        Self {
            parser: Parser::new(),
            state: SequenceState::default(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    fn take_clipboard_operations(&mut self) -> Vec<ClipboardOperation> {
        std::mem::take(&mut self.state.clipboard_operations)
    }
}

type Job = Box<dyn FnOnce(&mut GhosttyCore) + Send + 'static>;

pub struct GhosttyEmu {
    jobs: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    sequences: SequenceTracker,
    clipboard_validator: ClipboardValidator,
}

impl GhosttyEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Result<Self> {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> Result<Self> {
        let (jobs, receiver) = mpsc::channel::<Job>();
        let (ready, started) = mpsc::sync_channel(1);
        let profile = *profile;
        let worker = thread::Builder::new()
            .name("tui-test-ghostty".to_string())
            .spawn(move || match GhosttyCore::new(cols, rows, profile, bells) {
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
                sequences: SequenceTracker::new(),
                clipboard_validator: ClipboardValidator::new(),
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
        self.clipboard_validator.process(bytes);
        let mut start = 0;
        for index in 0..bytes.len() {
            self.sequences.feed(&bytes[index..=index]);
            let operations = self.sequences.take_clipboard_operations();
            if operations.is_empty() {
                continue;
            }

            let owned = bytes[start..=index].to_vec();
            self.call("processing output", move |core| {
                core.process(&owned);
                for operation in operations {
                    match operation {
                        ClipboardOperation::Store { clipboard, text } => {
                            core.set_clipboard(clipboard, text);
                        }
                        ClipboardOperation::Query {
                            clipboard,
                            selector,
                            bell_terminated,
                        } => {
                            core.answer_clipboard_query(clipboard, selector, bell_terminated);
                        }
                    }
                }
            });
            start = index + 1;
        }

        if start < bytes.len() {
            let owned = bytes[start..].to_vec();
            self.call("processing output", move |core| core.process(&owned));
        }
    }

    fn fault(&self) -> Option<String> {
        self.clipboard_validator.fault()
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        self.call("draining replies", GhosttyCore::take_pending_writes)
    }

    fn encode_key(&self, press: &KeyPress) -> Option<Vec<u8>> {
        // The worker owns the terminal, so the event has to be moved across
        // the channel rather than borrowed.
        let press = press.clone();
        self.call_result("encoding key", move |core| core.encode_key(&press))
    }

    fn cursor_key_application(&self) -> bool {
        self.call_result("reading cursor key mode", |core| {
            core.cursor_key_application()
        })
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        self.call_result("reading keyboard mode", |core| core.keyboard_mode())
    }

    fn clipboard(&self, clipboard: ClipboardType) -> anyhow::Result<String> {
        Ok(self.call("reading clipboard", move |core| core.clipboard(clipboard)))
    }

    fn clipboard_revision(&self, clipboard: ClipboardType) -> anyhow::Result<u64> {
        Ok(self.call("reading clipboard state", move |core| {
            core.clipboard_revision(clipboard)
        }))
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
        self.sequences.state.current.clone()
    }

    fn bracketed_paste_mode(&self) -> bool {
        self.call_result("reading bracketed paste mode", |core| {
            core.bracketed_paste_mode()
        })
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
    use crate::api::KeyAction;

    crate::emulator_conformance_tests!(
        |cols, rows, profile| {
            Box::new(GhosttyEmu::new(cols, rows, profile).expect("create Ghostty emulator"))
        },
        &[
            // libghostty-vt exposes a cell's hyperlink through
            // `ghostty_grid_ref_hyperlink_uri`, which returns the URI and
            // nothing else. There is no accessor for the `id=` parameter, so
            // it is not that the mapping drops it: it never crosses the FFI.
            crate::terminal::conformance::Divergence::HyperlinkHasNoId,
        ]
    );

    fn press(key: &str) -> KeyPress {
        crate::input::keys::token_to_presses(key, KeyAction::Down)
            .expect("valid token")
            .remove(0)
    }

    /// The whole point of routing: ghostty's encoder reads the modes off the
    /// live terminal, so `DECCKM` reaches key encoding without this backend
    /// having to report the mode separately.
    #[test]
    fn the_encoder_follows_the_terminals_cursor_key_mode() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        assert_eq!(
            emu.encode_key(&press("up")).as_deref(),
            Some(&b"\x1b[A"[..])
        );

        emu.process(b"\x1b[?1h");
        assert_eq!(
            emu.encode_key(&press("up")).as_deref(),
            Some(&b"\x1bOA"[..])
        );

        emu.process(b"\x1b[?1l");
        assert_eq!(
            emu.encode_key(&press("up")).as_deref(),
            Some(&b"\x1b[A"[..])
        );
    }

    /// Kitty flags reach the encoder from the same terminal state.
    #[test]
    fn the_encoder_follows_the_negotiated_kitty_flags() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        assert_eq!(emu.encode_key(&press("a")).as_deref(), Some(&b"a"[..]));

        emu.process(b"\x1b[>1u");
        assert_eq!(
            emu.encode_key(&press("Escape")).as_deref(),
            Some(&b"\x1b[27u"[..]),
            "disambiguation is what mode 1 asks for"
        );
    }

    /// A modifier ghostty's bitmask cannot represent has to decline rather
    /// than encode without it, so the caller falls back instead of silently
    /// sending a key with the modifier dropped.
    #[test]
    fn kitty_only_modifiers_decline_rather_than_lose_the_modifier() {
        let emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        for modifier in ["hyper", "meta"] {
            let event = press(&format!("{modifier}+a"));
            assert_eq!(emu.encode_key(&event), None, "{modifier} is not encodable");
        }
    }

    /// A named key still sits on a codepoint. Without one ghostty encodes the
    /// key as its bare text and the modifiers vanish, so `Ctrl+Space` arrives
    /// as a plain space instead of `CSI 32;5u` and a release sends nothing at
    /// all. Legacy mode never showed it because there the text *is* the answer.
    #[test]
    fn a_named_key_carries_its_codepoint_into_kitty_mode() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        emu.process(b"\x1b[>15u");

        for (token, down, up) in [
            ("space", &b"\x1b[32u"[..], &b"\x1b[32;1:3u"[..]),
            ("ctrl+space", &b"\x1b[32;5u"[..], &b"\x1b[32;5:3u"[..]),
            ("alt+space", &b"\x1b[32;3u"[..], &b"\x1b[32;3:3u"[..]),
        ] {
            for (action, want) in [(KeyAction::Down, down), (KeyAction::Up, up)] {
                let presses =
                    crate::input::keys::token_to_presses(token, action).expect("valid token");
                let got: Option<Vec<u8>> = presses
                    .iter()
                    .map(|p| emu.encode_key(p))
                    .collect::<Option<Vec<_>>>()
                    .map(|parts| parts.concat());
                assert_eq!(got.as_deref(), Some(want), "{token} {action:?}");
            }
        }
    }

    /// The text a key produces belongs to the key and the layout, not to the
    /// press, so it is handed over for a release too: ghostty derives the
    /// shifted alternate key from it, and withholding it made `Shift+a`
    /// release as `CSI 97;2:3u` after its press had reported `CSI 97:65;2u`.
    #[test]
    fn a_release_still_reports_the_shifted_alternate_key() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        emu.process(b"\x1b[>15u");
        for (token, down, up) in [
            ("Shift+a", &b"\x1b[97:65;2u"[..], &b"\x1b[97:65;2:3u"[..]),
            ("Shift+1", &b"\x1b[49:33;2u"[..], &b"\x1b[49:33;2:3u"[..]),
        ] {
            for (action, want) in [(KeyAction::Down, down), (KeyAction::Up, up)] {
                let presses =
                    crate::input::keys::token_to_presses(token, action).expect("valid token");
                let got: Option<Vec<u8>> = presses
                    .iter()
                    .map(|p| emu.encode_key(p))
                    .collect::<Option<Vec<_>>>()
                    .map(|parts| parts.concat());
                assert_eq!(got.as_deref(), Some(want), "{token} {action:?}");
            }
        }
    }

    /// Super is a modifier ghostty can carry, but only in a Kitty mode.
    ///
    /// The bitmask has Ctrl, Alt, Shift and Super, so a Kitty encoding reports
    /// it. The legacy encoding has no form for it and drops it silently:
    /// `super+a` came back empty and `ctrl+super+a` as `CSI 97;5u`, which is
    /// `ctrl+a` with the Super gone. Declining there sends it to the shared
    /// encoder, which carries it.
    #[test]
    fn super_encodes_in_a_kitty_mode_and_declines_in_legacy() {
        let legacy = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        for token in ["super+a", "ctrl+super+a"] {
            assert_eq!(
                legacy.encode_key(&press(token)),
                None,
                "{token} has no legacy form that keeps Super"
            );
        }

        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        emu.process(b"\x1b[>1u");
        for (token, want) in [
            ("super+a", &b"\x1b[97;9u"[..]),
            ("ctrl+super+a", &b"\x1b[97;13u"[..]),
        ] {
            assert_eq!(
                emu.encode_key(&press(token)).as_deref(),
                Some(want),
                "{token} reports Super"
            );
        }
    }

    /// A key that produces text sends the text until keys are reported as
    /// escape codes, however it is modified.
    ///
    /// `Shift+Space` looks like it should be `CSI 32;2u` everywhere, but kitty
    /// returns text for any press that carries some unless the report-all-keys
    /// flag is set (`send_text_standalone = !report_text` in `key_encoding.c`),
    /// so a space is right until then. The release has no text and is an
    /// escape code as soon as event types are reported.
    #[test]
    fn shift_space_sends_text_until_keys_are_escape_codes() {
        for (flags, down, up) in [
            (0u8, &b" "[..], &b""[..]),
            (1, b" ", b""),
            (3, b" ", b"\x1b[32;2:3u"),
            (15, b"\x1b[32;2u", b"\x1b[32;2:3u"),
        ] {
            let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
            if flags > 0 {
                emu.process(format!("\x1b[>{flags}u").as_bytes());
            }
            for (action, want) in [(KeyAction::Down, down), (KeyAction::Up, up)] {
                let presses = crate::input::keys::token_to_presses("shift+space", action)
                    .expect("valid token");
                let got: Option<Vec<u8>> = presses
                    .iter()
                    .map(|p| emu.encode_key(p))
                    .collect::<Option<Vec<_>>>()
                    .map(|parts| parts.concat());
                assert_eq!(got.as_deref(), Some(want), "flags {flags} {action:?}");
            }
        }
    }

    /// Legacy mode has no codepoint form, so the same keys stay bytes.
    #[test]
    fn a_named_key_keeps_its_legacy_bytes() {
        let emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        for (token, want) in [
            ("space", &b" "[..]),
            ("ctrl+space", &b"\x00"[..]),
            ("alt+space", &b"\x1b "[..]),
        ] {
            assert_eq!(
                emu.encode_key(&press(token)).as_deref(),
                Some(want),
                "{token}"
            );
        }
    }

    /// A key ghostty has no code for declines too.
    #[test]
    fn an_unmapped_key_declines() {
        let emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        assert_eq!(emu.encode_key(&press("\u{4f60}")), None);
    }

    #[test]
    fn bells_are_counted_without_counting_osc_terminators() {
        let bells = BellTracker::default();
        let mut emulator =
            GhosttyEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone())
                .expect("create emulator");

        emulator.process(b"\x07\x1b]0;window title\x07\x07");

        assert_eq!(bells.count(), 2);
        assert_eq!(bells.sequence(), 2);
    }

    #[test]
    fn title_sequences_can_span_process_calls() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        emu.process(b"\x1b]2;split");
        emu.process(b" title\x1b\\");
        assert_eq!(emu.title().as_deref(), Some("split title"));
    }
}
