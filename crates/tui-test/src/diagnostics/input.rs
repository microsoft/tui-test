use serde::{Deserialize, Serialize};

use crate::api::{KeyAction, MouseOptions, TextPosition};

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
