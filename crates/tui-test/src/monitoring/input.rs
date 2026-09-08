use super::ansi;
use vte::{Params, Parser, Perform};

const MAX_PENDING: usize = 4096;
const CTRL_RIGHT_BRACKET: u8 = 0x1d;

pub enum InputEvent {
    Detach,
    Mouse {
        button: u16,
        x: u16,
        y: u16,
        release: bool,
    },
}

pub enum InputAction {
    Forward,
    Replace(Vec<u8>),
    Detach,
}

#[derive(Default)]
struct InputState {
    complete: bool,
    event: Option<InputEvent>,
    paste: bool,
}

impl Perform for InputState {
    fn print(&mut self, _: char) {
        self.complete = true;
    }

    fn execute(&mut self, byte: u8) {
        self.complete = true;
        if byte == CTRL_RIGHT_BRACKET && !self.paste {
            self.event = Some(InputEvent::Detach);
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.complete = true;
        if ignore {
            return;
        }
        let mut params = params.iter();
        let key = params.next().unwrap_or(&[]);
        if intermediates.is_empty() && action == '~' && params.next().is_none() {
            match key {
                [200] => self.paste = true,
                [201] => self.paste = false,
                _ => {}
            }
            return;
        }
        if self.paste {
            return;
        }
        if intermediates.is_empty() && action == 'u' && key.first() == Some(&93) {
            let modifiers = params.next().unwrap_or(&[1]);
            let event = modifiers.get(1).copied().unwrap_or(1);
            let bits = modifiers.first().copied().unwrap_or(1).saturating_sub(1);
            const CTRL: u16 = 4;
            const LOCKS: u16 = 64 | 128;
            if matches!(event, 1 | 2) && bits & CTRL != 0 && bits & !(CTRL | LOCKS) == 0 {
                self.event = Some(InputEvent::Detach);
            }
        } else if intermediates == b"<" && matches!(action, 'M' | 'm') {
            if let ([button], Some([x]), Some([y]), None) =
                (key, params.next(), params.next(), params.next())
            {
                self.event = Some(InputEvent::Mouse {
                    button: *button,
                    x: *x,
                    y: *y,
                    release: action == 'm',
                });
            }
        }
    }
}

/// Use VTE for CSI input, preserving original bytes rather than reconstructing
/// them from callbacks (which can normalize invalid UTF-8).
#[derive(Default)]
pub struct InputParser {
    parser: Parser,
    state: InputState,
    pending: Vec<u8>,
    passthrough: bool,
}

impl InputParser {
    pub fn push(
        &mut self,
        bytes: &[u8],
        mut filter: impl FnMut(InputEvent) -> InputAction,
    ) -> (Vec<u8>, bool) {
        let mut output = Vec::new();
        for &byte in bytes {
            if byte == 0x1b || (byte == CTRL_RIGHT_BRACKET && self.pending != b"\x1b") {
                output.append(&mut self.pending);
                self.passthrough = false;
            }
            self.pending.push(byte);
            // VTE parses terminal output, where ESC ] starts an OSC. On input
            // it is also the ordinary legacy Alt+] chord. Only defer CSI input.
            if self.pending.first() == Some(&0x1b)
                && self.pending.get(1).is_some_and(|byte| *byte != b'[')
            {
                output.append(&mut self.pending);
                self.parser = Parser::new();
                self.passthrough = false;
                continue;
            }
            self.state.complete = false;
            self.state.event = None;
            self.parser.advance(&mut self.state, &[byte]);
            if byte == 0x1b {
                continue;
            }
            if self.state.complete {
                let event = self.state.event.take().filter(|_| {
                    !self.passthrough
                        && (self.pending.starts_with(b"\x1b[")
                            || self.pending == [CTRL_RIGHT_BRACKET])
                });
                match event.map(&mut filter).unwrap_or(InputAction::Forward) {
                    InputAction::Forward => output.append(&mut self.pending),
                    InputAction::Replace(bytes) => {
                        output.extend(bytes);
                        self.pending.clear();
                    }
                    InputAction::Detach => {
                        self.pending.clear();
                        return (output, true);
                    }
                }
                self.passthrough = false;
            } else if self.pending.len() >= MAX_PENDING {
                output.append(&mut self.pending);
                self.passthrough = true;
            }
        }
        (output, false)
    }

    pub fn on_idle(&mut self) -> Vec<u8> {
        // A lone legacy Escape is ambiguous. Incomplete CSI sequences, however,
        // must survive idle polls so split keyboard/mouse reports stay intact.
        if self.pending == b"\x1b" || self.pending == b"\x1b[" {
            self.parser = Parser::new();
            self.finish()
        } else {
            Vec::new()
        }
    }

    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

pub struct MouseRemapper {
    parser: InputParser,
    viewer: (u16, u16),
    pressed: u8,
    active: bool,
    mouse_seen: bool,
}

impl MouseRemapper {
    pub fn new(viewer: (u16, u16)) -> Self {
        Self {
            parser: InputParser::default(),
            viewer,
            pressed: 0,
            active: false,
            mouse_seen: false,
        }
    }

    pub fn observe(&mut self, size: Option<(u16, u16)>) {
        if size.is_some() {
            if !self.active {
                self.pressed = 0;
            }
            self.active = true;
            self.mouse_seen = true;
        } else {
            self.active = false;
        }
    }

    pub fn resize(&mut self, viewer: (u16, u16)) {
        self.viewer = viewer;
    }

    pub fn push(&mut self, bytes: &[u8], size: Option<(u16, u16)>) -> Vec<u8> {
        self.observe(size);
        let available = super::render::content_size(self.viewer);
        let size = size.map(|target| (target.0.min(available.0), target.1.min(available.1)));
        self.parser
            .push(bytes, |event| match event {
                InputEvent::Mouse {
                    button,
                    x,
                    y,
                    release,
                } if self.mouse_seen => {
                    match remap_sgr_mouse(button, x, y, release, size, &mut self.pressed) {
                        Some(remapped) => InputAction::Replace(remapped),
                        None => InputAction::Forward,
                    }
                }
                _ => InputAction::Forward,
            })
            .0
    }

    pub fn on_idle(&mut self) -> Vec<u8> {
        self.parser.on_idle()
    }

    pub fn finish(&mut self) -> Vec<u8> {
        self.parser.finish()
    }
}

fn remap_sgr_mouse(
    button: u16,
    x: u16,
    y: u16,
    release: bool,
    size: Option<(u16, u16)>,
    pressed: &mut u8,
) -> Option<Vec<u8>> {
    let mut x = x.checked_sub(1)?;
    let mut y = y.checked_sub(1)?;
    let base_button = (button & 0b11) as u8;
    let button_bit = (base_button < 3).then(|| 1 << base_button);
    let Some(size) = size else {
        if release {
            if let Some(bit) = button_bit {
                *pressed &= !bit;
            }
        }
        return Some(Vec::new());
    };
    let outside = x == 0 || y == 0 || x > size.0 || y > size.1;
    if release {
        let bit = button_bit?;
        if *pressed & bit == 0 {
            return Some(Vec::new());
        }
        *pressed &= !bit;
        if outside {
            if size.0 == 0 || size.1 == 0 {
                return Some(Vec::new());
            }
            x = x.clamp(1, size.0);
            y = y.clamp(1, size.1);
        }
    } else {
        if outside {
            return Some(Vec::new());
        }
        let motion = button & 32 != 0;
        let wheel = button & 64 != 0;
        if motion && button_bit.is_some_and(|bit| *pressed & bit == 0) {
            return Some(Vec::new());
        }
        if !motion && !wheel {
            if let Some(bit) = button_bit {
                *pressed |= bit;
            }
        }
    }
    Some(ansi::sgr_mouse(
        button,
        x,
        y,
        if release { b'm' } else { b'M' },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detach(parser: &mut InputParser, bytes: &[u8]) -> (Vec<u8>, bool) {
        parser.push(bytes, |event| match event {
            InputEvent::Detach => InputAction::Detach,
            _ => InputAction::Forward,
        })
    }

    #[test]
    fn monitor_input_preserves_bytes_except_the_detach_chord() {
        let raw = b"\x03text \xff\x1a\x1b[200~paste\n\x1b[201~";
        let mut parser = InputParser::default();
        assert_eq!(detach(&mut parser, raw), (raw.to_vec(), false));
        assert_eq!(
            detach(&mut parser, b"before\x1dafter"),
            (b"before".to_vec(), true)
        );
    }

    #[test]
    fn monitor_unrecognized_escape_sequences_are_forwarded_unchanged() {
        for input in [
            b"\x1b]0;title\x07".as_slice(),
            b"\x1bP1;2qpayload\x1b\\",
            b"\x1b[38;2;1;2;3m",
        ] {
            let mut parser = InputParser::default();
            let mut output = Vec::new();
            for byte in input {
                let (forwarded, detached) = detach(&mut parser, &[*byte]);
                assert!(!detached);
                output.extend(forwarded);
            }
            output.extend(parser.finish());
            assert_eq!(output, input);
        }
    }
    #[test]
    fn monitor_detach_accepts_kitty_subparameters_across_every_split() {
        for chord in [
            b"\x1b[93;5u".as_slice(),
            b"\x1b[93::93;5u",
            b"\x1b[93;69:1u",
            b"\x1b[93;197:2u",
            b"\x1b[93;5:1;93u",
        ] {
            for split in 1..chord.len() {
                let mut parser = InputParser::default();
                assert_eq!(detach(&mut parser, &chord[..split]), (Vec::new(), false));
                if split > 2 {
                    assert!(parser.on_idle().is_empty());
                }
                assert_eq!(detach(&mut parser, &chord[split..]), (Vec::new(), true));
            }
        }
    }

    #[test]
    fn monitor_detach_leaves_other_modifiers_and_releases_alone() {
        for input in [
            b"\x1b[93;6u".as_slice(),
            b"\x1b[93;7u",
            b"\x1b[93;5:3u",
            b"\x1b[93u",
            b"\x1b[93;0u",
            b"\x1b[?93;5u",
            b"\x1b[93;5~",
            b"\x1b[93;5 u",
        ] {
            assert_eq!(
                detach(&mut InputParser::default(), input),
                (input.to_vec(), false)
            );
        }
    }

    #[test]
    fn monitor_detach_does_not_interpret_bracketed_paste() {
        for input in [
            b"\x1b[200~\x1d\x1b[93::93;5u\x1b[<0;2;2M\x1b[201~".as_slice(),
            b"\x1b[200~\x1b]paste\x1d\x1b[201~",
        ] {
            let mut parser = InputParser::default();
            let mut forwarded = Vec::new();
            for byte in input {
                let (bytes, detached) = detach(&mut parser, &[*byte]);
                assert!(!detached);
                forwarded.extend(bytes);
            }
            forwarded.extend(parser.finish());
            assert_eq!(forwarded, input);
            assert_eq!(detach(&mut parser, b"\x1d"), (Vec::new(), true));
        }
    }

    #[test]
    fn monitor_legacy_alt_keys_are_forwarded_without_another_keystroke() {
        for input in [
            b"\x1b\x7f".as_slice(),
            "\x1b\u{e9}".as_bytes(),
            "\x1b\u{1f680}".as_bytes(),
            b"\x1b]",
            b"\x1bP",
            b"\x1b[",
        ] {
            let mut parser = InputParser::default();
            let (mut output, detached) = detach(&mut parser, input);
            assert!(!detached);
            output.extend(parser.on_idle());
            assert_eq!(output, input);
            assert_eq!(detach(&mut parser, b"\x1d"), (Vec::new(), true));
        }
    }

    #[test]
    fn monitor_legacy_alt_ctrl_right_bracket_does_not_detach() {
        let mut parser = InputParser::default();
        assert_eq!(
            detach(&mut parser, b"\x1b\x1d"),
            (b"\x1b\x1d".to_vec(), false)
        );
        assert_eq!(detach(&mut parser, b"\x1d"), (Vec::new(), true));

        let mut parser = InputParser::default();
        assert_eq!(detach(&mut parser, b"\x1b"), (Vec::new(), false));
        assert_eq!(detach(&mut parser, b"\x1d"), (b"\x1b\x1d".to_vec(), false));
        assert_eq!(detach(&mut parser, b"\x1d"), (Vec::new(), true));
    }

    #[test]
    fn monitor_input_flushes_lone_escape_and_preserves_long_sequences() {
        let mut parser = InputParser::default();
        assert_eq!(detach(&mut parser, b"\x1b"), (Vec::new(), false));
        assert_eq!(parser.on_idle(), b"\x1b");
        assert_eq!(detach(&mut parser, b"[93;5u"), (b"[93;5u".to_vec(), false));
        let mut long = b"\x1b[".to_vec();
        long.extend(std::iter::repeat_n(b'9', MAX_PENDING * 2));
        long.push(b'u');
        let (mut output, detached) = detach(&mut parser, &long);
        assert!(!detached);
        output.extend(parser.finish());
        assert_eq!(output, long);
    }
}
