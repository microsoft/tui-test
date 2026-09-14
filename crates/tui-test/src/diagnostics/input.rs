use serde::{Deserialize, Serialize};

use crate::api::{KeyAction, MouseAction, MouseOptions, Operation, TextPosition};

const MAX_INPUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MouseTarget {
    Position { x: u16, y: u16 },
    Text { text: String },
    Locator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputArguments {
    Write {
        data: String,
    },
    Submit {
        data: String,
    },
    Key {
        keys: Vec<String>,
        action: KeyAction,
    },
    MouseClick {
        target: MouseTarget,
        options: MouseOptions,
        clicks: u8,
    },
    MouseMove {
        x: u16,
        y: u16,
    },
    MouseDown {
        x: u16,
        y: u16,
        options: MouseOptions,
    },
    MouseUp {
        x: u16,
        y: u16,
        options: MouseOptions,
    },
    MouseDrag {
        x1: u16,
        y1: u16,
        x2: u16,
        y2: u16,
        options: MouseOptions,
    },
    MouseScroll {
        direction: String,
        amount: u16,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDetails {
    pub arguments: InputArguments,
    /// Bytes accepted by the PTY writer. Empty means the key protocol emitted
    /// no bytes; absent means the write did not complete or was not retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_position: Option<TextPosition>,
}

impl InputDetails {
    pub(crate) fn capture(operation: &Operation) -> Option<Self> {
        let size = match operation {
            Operation::Write { data } => data.len(),
            Operation::Submit { data } => data.as_ref().map_or(0, String::len),
            Operation::Key { keys, .. } => keys.iter().fold(0usize, |size, key| {
                size.saturating_add(key.len().saturating_add(1))
            }),
            Operation::Mouse {
                action: MouseAction::Click { on_text, .. },
            } => on_text.as_ref().map_or(0, String::len),
            Operation::Mouse {
                action: MouseAction::Scroll { direction, .. },
            } => direction.len(),
            _ => 0,
        };
        if size > MAX_INPUT_BYTES {
            return Some(Self::unavailable());
        }
        let arguments = match operation {
            Operation::Write { data } => InputArguments::Write { data: data.clone() },
            Operation::Submit { data } => InputArguments::Submit {
                data: data.clone().unwrap_or_default(),
            },
            Operation::Key { keys, action } => InputArguments::Key {
                keys: keys.clone(),
                action: *action,
            },
            Operation::ClickLocator {
                options, clicks, ..
            } => InputArguments::MouseClick {
                target: MouseTarget::Locator,
                options: *options,
                clicks: *clicks,
            },
            Operation::Mouse { action } => match action {
                MouseAction::Click {
                    x,
                    y,
                    on_text,
                    options,
                    clicks,
                } => InputArguments::MouseClick {
                    target: match on_text {
                        Some(text) => MouseTarget::Text { text: text.clone() },
                        None => MouseTarget::Position {
                            x: x.unwrap_or(0),
                            y: y.unwrap_or(0),
                        },
                    },
                    options: *options,
                    clicks: *clicks,
                },
                MouseAction::Move { x, y } => InputArguments::MouseMove { x: *x, y: *y },
                MouseAction::Down { x, y, options } => InputArguments::MouseDown {
                    x: *x,
                    y: *y,
                    options: *options,
                },
                MouseAction::Up { x, y, options } => InputArguments::MouseUp {
                    x: *x,
                    y: *y,
                    options: *options,
                },
                MouseAction::Drag {
                    x1,
                    y1,
                    x2,
                    y2,
                    options,
                } => InputArguments::MouseDrag {
                    x1: *x1,
                    y1: *y1,
                    x2: *x2,
                    y2: *y2,
                    options: *options,
                },
                MouseAction::Scroll { direction, amount } => InputArguments::MouseScroll {
                    direction: direction.clone(),
                    amount: *amount,
                },
            },
            _ => return None,
        };
        let mut details = Self {
            arguments,
            sent_bytes: None,
            mouse_position: None,
        };
        details.enforce_limit();
        Some(details)
    }

    pub(crate) fn record_sent(&mut self, bytes: &[u8], position: Option<(u16, u16)>) {
        if self.is_unavailable() {
            return;
        }
        if bytes.len() > MAX_INPUT_BYTES {
            *self = Self::unavailable();
            return;
        }
        self.sent_bytes = Some(bytes.to_vec());
        self.mouse_position = position.map(|(x, y)| TextPosition {
            column: x,
            row: u32::from(y),
        });
        self.enforce_limit();
    }

    pub(crate) fn is_unavailable(&self) -> bool {
        matches!(self.arguments, InputArguments::Unavailable { .. })
    }

    fn unavailable() -> Self {
        Self {
            arguments: InputArguments::Unavailable {
                reason: format!("Input exceeded the {MAX_INPUT_BYTES}-byte retention limit"),
            },
            sent_bytes: None,
            mouse_position: None,
        }
    }

    fn enforce_limit(&mut self) {
        match serde_json::to_vec(self) {
            Ok(bytes) if bytes.len() <= MAX_INPUT_BYTES => {}
            Ok(_) => *self = Self::unavailable(),
            Err(error) => {
                *self = Self {
                    arguments: InputArguments::Unavailable {
                        reason: format!("Input could not be serialized: {error}"),
                    },
                    sent_bytes: None,
                    mouse_position: None,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_retains_exact_text_key_phases_and_mouse_targets() {
        let mut input = InputDetails::capture(&Operation::Write {
            data: "a\n\u{e9}\0".into(),
        })
        .unwrap();
        input.record_sent("a\n\u{e9}\0".as_bytes(), None);
        assert_eq!(input.sent_bytes.as_deref(), Some(&b"a\n\xc3\xa9\0"[..]));
        for action in [
            KeyAction::Down,
            KeyAction::Up,
            KeyAction::Press,
            KeyAction::Repeat,
        ] {
            let input = InputDetails::capture(&Operation::Key {
                keys: vec!["Ctrl+A".into()],
                action,
            })
            .unwrap();
            assert_eq!(
                input.arguments,
                InputArguments::Key {
                    keys: vec!["Ctrl+A".into()],
                    action
                }
            );
        }
        let mut input = InputDetails::capture(&Operation::Mouse {
            action: MouseAction::Click {
                x: None,
                y: None,
                on_text: Some("Save".into()),
                options: MouseOptions::default().with_ctrl(),
                clicks: 2,
            },
        })
        .unwrap();
        input.record_sent(b"mouse", Some((7, 3)));
        assert!(
            matches!(input.arguments, InputArguments::MouseClick { target: MouseTarget::Text { .. }, clicks: 2, options } if options.ctrl)
        );
        assert_eq!(
            input.mouse_position,
            Some(TextPosition { column: 7, row: 3 })
        );
    }

    #[test]
    fn input_distinguishes_unsent_and_empty_protocol_output() {
        let mut input = InputDetails::capture(&Operation::Key {
            keys: vec!["a".into()],
            action: KeyAction::Up,
        })
        .unwrap();
        assert_eq!(input.sent_bytes, None);
        input.record_sent(&[], None);
        assert_eq!(input.sent_bytes, Some(Vec::new()));
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(serde_json::from_str::<InputDetails>(&json).unwrap(), input);
    }

    #[test]
    fn input_budget_includes_json_escaping_and_encoded_bytes() {
        for data in [
            "x".repeat(MAX_INPUT_BYTES + 1),
            "\0".repeat(MAX_INPUT_BYTES / 2),
        ] {
            let input = InputDetails::capture(&Operation::Write { data }).unwrap();
            assert!(input.is_unavailable());
            assert!(serde_json::to_vec(&input).unwrap().len() < MAX_INPUT_BYTES);
        }
        let mut input = InputDetails::capture(&Operation::Write {
            data: "x".repeat(MAX_INPUT_BYTES / 2),
        })
        .unwrap();
        input.record_sent(&vec![255; MAX_INPUT_BYTES / 2], None);
        assert!(input.is_unavailable());
        assert!(input.sent_bytes.is_none());
        assert!(InputDetails::capture(&Operation::Open(Default::default())).is_none());
    }
}
