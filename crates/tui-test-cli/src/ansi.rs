pub const BORDER: &str = "\x1b[38;5;240m";
pub const RESET: &str = "\x1b[0m";
pub const HOME: &str = "\x1b[H";
pub const ERASE_DISPLAY: &str = "\x1b[J";
pub const ERASE_LINE: &str = "\x1b[K";
pub const SGR_START: &str = "\x1b[0";

pub const BRACKETED_PASTE_ENABLE: &str = "\x1b[?2004h";
pub const BRACKETED_PASTE_DISABLE: &str = "\x1b[?2004l";
pub const BRACKETED_PASTE_SAVE: &[u8] = b"\x1b[?2004s";
pub const BRACKETED_PASTE_RESTORE: &[u8] = b"\x1b[?2004r";

pub const KITTY_KEYBOARD_PUSH: &[u8] = b"\x1b[>0u";
pub const KITTY_KEYBOARD_POP: &[u8] = b"\x1b[<u";
pub const KITTY_CTRL_RIGHT_BRACKET: &[u8] = b"\x1b[93;";

pub fn kitty_keyboard_mode(flags: u8) -> String {
    format!("\x1b[={flags}u")
}
