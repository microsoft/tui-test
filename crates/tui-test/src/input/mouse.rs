//! SGR mouse-event encoders.

const CSI: &str = "\u{1b}[";

/// SGR press at (x, y), 0-based; button 0=left, 1=middle, 2=right.
pub fn down(x: u16, y: u16, button: u8) -> String {
    format!("{CSI}<{};{};{}M", button, x + 1, y + 1)
}

/// SGR release at (x, y), 0-based.
pub fn up(x: u16, y: u16, button: u8) -> String {
    format!("{CSI}<{};{};{}m", button, x + 1, y + 1)
}

/// SGR motion at (x, y), 0-based (button-motion bit set).
pub fn motion(x: u16, y: u16) -> String {
    format!("{CSI}<35;{};{}M", x + 1, y + 1)
}

/// Scroll wheel: SGR codes 64 (up) and 65 (down).
pub fn scroll(x: u16, y: u16, up: bool) -> String {
    let code = if up { 64 } else { 65 };
    format!("{CSI}<{};{};{}M", code, x + 1, y + 1)
}

/// A full click: press then release.
pub fn click(x: u16, y: u16, button: u8) -> String {
    format!("{}{}", down(x, y, button), up(x, y, button))
}
