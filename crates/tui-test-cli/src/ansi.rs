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

pub const MOUSE_DISABLE: &str = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l";
pub const MOUSE_CLICK_ENABLE: &str = "\x1b[?1006h\x1b[?1000h";
pub const MOUSE_DRAG_ENABLE: &str = "\x1b[?1006h\x1b[?1002h";
pub const MOUSE_MOTION_ENABLE: &str = "\x1b[?1006h\x1b[?1003h";
pub const SGR_MOUSE_PREFIX: &[u8] = b"\x1b[<";

pub fn kitty_keyboard_mode(flags: u8) -> String {
    format!("\x1b[={flags}u")
}

pub fn sgr_mouse(button: u16, x: u16, y: u16, final_byte: u8) -> Vec<u8> {
    format!("\x1b[<{button};{x};{y}{}", final_byte as char).into_bytes()
}
