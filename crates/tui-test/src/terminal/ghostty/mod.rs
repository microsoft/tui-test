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

    fn press(key: &str) -> KeyPress {
        crate::input::keys::token_to_presses(key, crate::api::KeyAction::Down)
            .expect("valid token")
            .remove(0)
    }

    /// The whole point of routing: ghostty's encoder reads the modes off the
    /// live terminal, so `DECCKM` reaches key encoding without this backend
    /// having to report the mode separately.
    #[test]
    fn the_encoder_follows_the_terminals_cursor_key_mode() {
        let mut emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        assert_eq!(emu.encode_key(&press("up")).as_deref(), Some(&b"\x1b[A"[..]));

        emu.process(b"\x1b[?1h");
        assert_eq!(emu.encode_key(&press("up")).as_deref(), Some(&b"\x1bOA"[..]));

        emu.process(b"\x1b[?1l");
        assert_eq!(emu.encode_key(&press("up")).as_deref(), Some(&b"\x1b[A"[..]));
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

    /// A key ghostty has no code for declines too.
    #[test]
    fn an_unmapped_key_declines() {
        let emu = GhosttyEmu::new(10, 2, &Profile::default()).unwrap();
        assert_eq!(emu.encode_key(&press("\u{4f60}")), None);
    }

    /// Key tokens whose encoding the two encoders both claim to define.
    ///
    /// Deliberately not every combination. Ghostty encodes combinations the
    /// legacy scheme cannot express by upgrading them to `CSI-u` or xterm's
    /// `modifyOtherKeys` form: `Ctrl+Tab` becomes `CSI 27;5;9~` where legacy
    /// has only a bare `\t` with the Ctrl lost, and `Ctrl+Shift+0` becomes
    /// `CSI 41;5u`. That is a richer answer to a different question, not a
    /// different answer to this one, so those combinations are excluded here
    /// rather than asserted and then explained away. What is left is the set
    /// where legacy has a defined encoding and both encoders should produce it.
    const ORACLE_CASES: &[&str] = &[
        // Cursor and editing keys, which DECCKM moves between CSI and SS3.
        "Up", "Down", "Left", "Right", "Home", "End",
        "PageUp", "PageDown", "Insert", "Delete",
        "Ctrl+Up", "Ctrl+Down", "Ctrl+Left", "Ctrl+Right", "Ctrl+Home", "Ctrl+End",
        "Shift+Up", "Shift+Down", "Shift+Left", "Shift+Right",
        "Alt+Up", "Alt+Down", "Alt+Left", "Alt+Right",
        // Control keys with their classic C0 encodings.
        "Backspace", "Tab", "Enter", "Escape", "Space",
        "Alt+Backspace", "Alt+Enter", "Alt+Escape", "Shift+Tab",
        // Function keys. F3 is left out: ghostty and kitty spell it
        // `CSI 13~` where xterm spells it `SS3 R`, a naming split that
        // predates both and that neither is wrong about.
        "F1", "F2", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        // Text, shifted text, and the two modifiers legacy does define for it.
        "a", "z", "A", "Z", "0", "9", "-", "=", "[", "]", ";", "'", ",", ".", "/", "\\", "`",
        "!", "@", "#", "$", "%", "^", "&", "*", "(", ")", "_", "+", "{", "}", ":", "\"", "<",
        ">", "?", "|", "~",
        "Shift+a", "Shift+z", "Shift+0", "Shift+9", "Shift+-", "Shift+/",
        "Ctrl+a", "Ctrl+z", "Ctrl+c", "Ctrl+d", "Ctrl+l", "Ctrl+u", "Ctrl+w",
        "Alt+a", "Alt+z", "Alt+0", "Alt+9",
    ];

    /// Terminal states to compare under, as the bytes that set them.
    const ORACLE_MODES: &[(&str, &[u8])] = &[
        ("legacy", b""),
        ("application cursor keys", b"\x1b[?1h"),
        ("kitty disambiguate", b"\x1b[>1u"),
        ("kitty events", b"\x1b[>3u"),
        ("kitty report all", b"\x1b[>15u"),
    ];

    /// Drop the event-type sub-parameter when it is the default.
    ///
    /// Under `REPORT_EVENT_TYPES` the Kitty protocol writes the event as
    /// `modifiers:event`, where `1` means press. Press is the default and the
    /// spec allows omitting it: kitty itself emits `CSI A`, ghostty emits
    /// `CSI 1;1:1A`. Both decode to the same event, so normalizing here
    /// compares the two encoders at the level they actually disagree on
    /// rather than pinning one of two legal spellings.
    ///
    /// Only a `:1` immediately before the final byte is a default event type.
    /// An alternate-key sub-parameter such as the `:65` in `CSI 97:65;6u`
    /// sits before the `;` and is left alone.
    fn normalize_default_event_type(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let is_default_event = bytes[i] == b':'
                && bytes.get(i + 1) == Some(&b'1')
                && bytes
                    .get(i + 2)
                    .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'~');
            if is_default_event {
                i += 2;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
    }

    /// Drop a `1;1` parameter pair that means "no modifiers, one repeat".
    ///
    /// `CSI 1;1 A` and `CSI A` are the same event: both parameters are at
    /// their defaults, and every terminal that emits the short form expects
    /// the long one to be accepted. Ghostty writes them out once event
    /// reporting is on; kitty and xterm omit them. As with the event type,
    /// this normalizes a spelling rather than a meaning.
    fn normalize_default_params(bytes: &[u8]) -> Vec<u8> {
        let Some(rest) = bytes.strip_prefix(b"\x1b[1;1") else {
            return bytes.to_vec();
        };
        // Only when what remains is the final byte, so a longer parameter
        // list that merely starts `1;1` is left alone.
        if rest.len() == 1 && (rest[0].is_ascii_alphabetic() || rest[0] == b'~') {
            let mut out = b"\x1b[".to_vec();
            out.extend_from_slice(rest);
            return out;
        }
        bytes.to_vec()
    }

    /// Events where the two encoders genuinely disagree, with the reason.
    ///
    /// Kept as a named list rather than dropped from the matrix, so the
    /// disagreement stays visible and a change to either side still shows up
    /// as this list going stale.
    const ORACLE_KNOWN_DIVERGENCES: &[(&str, &str, &str)] = &[(
        "kitty report all",
        "Space",
        "ghostty sends the text ` `, the shared encoder sends `CSI 32u`. \
         Report-all-keys means what it says: the Kitty spec has text-producing \
         keys report as escape codes under this flag, so the shared encoder \
         looks right and this is worth raising upstream.",
    )];

    /// Hold the shared encoder against ghostty's, which is a reference
    /// implementation maintained by people who work on nothing else.
    ///
    /// This is what keeps the fallback honest. Three of the four backends
    /// have no encoder of their own and will always use `keys.rs`, so a
    /// disagreement here is `keys.rs` being wrong on three backends rather
    /// than two encoders holding different opinions.
    ///
    /// It has already paid for itself: it caught `key press A` sending `a`,
    /// `Space` encoding to nothing, and `Alt+a` losing its modifier.
    #[test]
    fn the_shared_encoder_agrees_with_ghostty() {
        use crate::api::KeyAction;
        use crate::input::keys;

        let mut disagreements = Vec::new();
        let mut compared = 0usize;
        for (mode_name, mode_bytes) in ORACLE_MODES {
            let mut emu = GhosttyEmu::new(20, 4, &Profile::default()).unwrap();
            emu.process(mode_bytes);
            let modes = keys::InputModes {
                keyboard: emu.keyboard_mode(),
                cursor_key_application: emu.cursor_key_application(),
            };
            for token in ORACLE_CASES {
                let presses = keys::token_to_presses(token, KeyAction::Down).expect("valid token");
                let Some(native) = presses
                    .iter()
                    .map(|press| emu.encode_key(press))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                compared += 1;
                let native = normalize_default_params(&normalize_default_event_type(&native.concat()));
                let shared = keys::token_to_seq_for_action_with_mode(token, KeyAction::Down, modes)
                    .expect("valid token");
                let known = ORACLE_KNOWN_DIVERGENCES
                    .iter()
                    .any(|(mode, case, _)| mode == mode_name && case == token);
                if known {
                    assert_ne!(
                        native,
                        shared.as_bytes(),
                        "{mode_name} {token} now agrees; drop it from \
                         ORACLE_KNOWN_DIVERGENCES"
                    );
                    continue;
                }
                if native != shared.as_bytes() {
                    disagreements.push(format!(
                        "  {mode_name:<24} {token:<12} ghostty {:<18?} shared {shared:?}",
                        String::from_utf8_lossy(&native)
                    ));
                }
            }
        }
        assert!(compared > 400, "only {compared} events were comparable");
        assert!(
            disagreements.is_empty(),
            "the shared encoder disagrees with ghostty on {} of {compared} events:\n{}",
            disagreements.len(),
            disagreements.join("\n")
        );
    }

    /// A profile that turns the protocol off has to reach the backend's own
    /// encoder too, not just `keyboard_mode`. Ghostty reads its Kitty flags
    /// off the live terminal, which goes around the profile unless the
    /// encoder is told otherwise.
    #[test]
    fn a_disabled_profile_stops_the_backend_encoding_kitty() {
        let profile = Profile {
            kitty_keyboard: false,
            ..Default::default()
        };
        let mut emu = GhosttyEmu::new(10, 2, &profile).unwrap();
        emu.process(b"\x1b[>1u");

        assert_eq!(emu.keyboard_mode(), KeyboardMode::empty());
        assert_eq!(
            emu.encode_key(&press("Escape")).as_deref(),
            Some(&b"\x1b"[..]),
            "a disabled profile keeps the legacy encoding"
        );
        assert_eq!(
            emu.encode_key(&press("a")).as_deref(),
            Some(&b"a"[..]),
            "and text is still text"
        );
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
