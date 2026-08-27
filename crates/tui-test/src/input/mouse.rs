//! SGR mouse-event encoders.

use crate::api::MouseOptions;

const CSI: &str = "\u{1b}[";

/// SGR press at (x, y), 0-based.
pub fn down(x: u16, y: u16, options: MouseOptions) -> String {
    format!("{CSI}<{};{};{}M", options.sgr_code(), x + 1, y + 1)
}

/// SGR release at (x, y), 0-based.
pub fn up(x: u16, y: u16, options: MouseOptions) -> String {
    format!("{CSI}<{};{};{}m", options.sgr_code(), x + 1, y + 1)
}

/// SGR motion at (x, y), 0-based (button-motion bit set).
pub fn motion(x: u16, y: u16) -> String {
    format!("{CSI}<35;{};{}M", x + 1, y + 1)
}

/// SGR drag motion at (x, y), preserving the button and modifier bits.
pub fn drag_motion(x: u16, y: u16, options: MouseOptions) -> String {
    format!("{CSI}<{};{};{}M", options.sgr_code() | 32, x + 1, y + 1)
}

/// Scroll wheel: SGR codes 64 (up) and 65 (down).
pub fn scroll(x: u16, y: u16, up: bool) -> String {
    let code = if up { 64 } else { 65 };
    format!("{CSI}<{};{};{}M", code, x + 1, y + 1)
}

/// A full click: press then release.
pub fn click(x: u16, y: u16, options: MouseOptions) -> String {
    format!("{}{}", down(x, y, options), up(x, y, options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MouseButton;

    #[test]
    fn preserves_buttons_and_modifiers() {
        let options = MouseOptions::new(MouseButton::Right).with_ctrl();
        assert_eq!(click(2, 3, options), "\u{1b}[<18;3;4M\u{1b}[<18;3;4m");
        assert_eq!(motion(2, 3), "\u{1b}[<35;3;4M");
        assert_eq!(drag_motion(2, 3, options), "\u{1b}[<50;3;4M");
    }
}
