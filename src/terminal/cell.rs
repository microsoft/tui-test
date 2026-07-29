//! Backend-neutral grid vocabulary.
//!
//! Every consumer of the terminal grid (render, assert, monitor, locator)
//! speaks these types and nothing else, so swapping the emulator backend
//! behind [`crate::terminal::emu::Emulator`] is invisible to them. Nothing in
//! this module may depend on a specific emulator crate.

use bitflags::bitflags;
use compact_str::CompactString;

/// The 16 themeable palette slots (ANSI 0-15).
///
/// Split out from [`Color::Idx`] by *numeric range*, not by how the escape
/// sequence spelled it, so `SGR 31` and `SGR 38;5;1` both land here.
///
/// The backends do not agree on whether that spelling survives parsing:
/// alacritty keeps it (`Named` vs `Indexed`) and so does xterm.js (`CM_P16`
/// vs `CM_P256`), but ghostty flattens both into one `.palette` value. A model
/// that preserved the distinction would therefore be unimplementable on
/// ghostty. The usual reason to want it, painting bold text bright, does not
/// need it either: ghostty keys that off the index (`bold && idx < 8 =>
/// palette[idx + 8]`), which applies to `38;5;1` just as much as to `31`.
///
/// What all three do agree on is that 0-15 are the slots a theme may override,
/// which is the only distinction any consumer here acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NamedColor {
    Black = 0,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl NamedColor {
    pub const ALL: [NamedColor; 16] = [
        NamedColor::Black,
        NamedColor::Red,
        NamedColor::Green,
        NamedColor::Yellow,
        NamedColor::Blue,
        NamedColor::Magenta,
        NamedColor::Cyan,
        NamedColor::White,
        NamedColor::BrightBlack,
        NamedColor::BrightRed,
        NamedColor::BrightGreen,
        NamedColor::BrightYellow,
        NamedColor::BrightBlue,
        NamedColor::BrightMagenta,
        NamedColor::BrightCyan,
        NamedColor::BrightWhite,
    ];

    /// The palette slot this name occupies (0-15).
    pub fn index(self) -> u8 {
        self as u8
    }

    /// The name for a palette slot, or `None` outside 0-15.
    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// A themeable palette slot, ANSI 0-15.
    Named(NamedColor),
    /// A fixed 256-color palette index, 16-255.
    Idx(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    /// Build from a 256-color index, routing 0-15 to [`Color::Named`] so the
    /// same index yields the same value no matter which backend produced it.
    pub fn from_index(i: u8) -> Self {
        match NamedColor::from_index(i) {
            Some(named) => Color::Named(named),
            None => Color::Idx(i),
        }
    }

    /// The 256-color index for this color, approximating RGB.
    pub fn to_index(self) -> u8 {
        match self {
            Color::Named(n) => n.index(),
            Color::Idx(i) => i,
            Color::Rgb(r, g, b) => crate::assert::color::rgb_to_ansi256(r, g, b),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineStyle {
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Underline {
    pub style: UnderlineStyle,
    /// `None` means the underline takes the cell's foreground color.
    pub color: Option<Color>,
}

bitflags! {
    /// Boolean SGR attributes, one bit each: the render and monitor loops copy
    /// and compare a cell's style per column, so keeping it to a single byte
    /// keeps those comparisons to a single integer compare.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Attrs: u8 {
        const BOLD      = 1 << 0;
        const DIM       = 1 << 1;
        const ITALIC    = 1 << 2;
        const INVERSE   = 1 << 3;
        const INVISIBLE = 1 << 4;
        const STRIKE    = 1 << 5;
        const BLINK     = 1 << 6;
    }
}

/// The grapheme stored in the cell that follows a double-width character.
pub const CONTINUATION: &str = "";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmuCell {
    /// The cell's grapheme. A blank cell holds `" "`; [`CONTINUATION`] (the
    /// empty string) is reserved for the cell trailing a double-width char.
    pub ch: CompactString,
    /// `None` means the terminal's default foreground.
    pub fg: Option<Color>,
    /// `None` means the terminal's default background.
    pub bg: Option<Color>,
    /// `None` means not underlined.
    pub underline: Option<Underline>,
    pub attrs: Attrs,
}

impl EmuCell {
    /// A blank, unstyled cell.
    pub const fn blank() -> Self {
        EmuCell {
            ch: CompactString::const_new(" "),
            fg: None,
            bg: None,
            underline: None,
            attrs: Attrs::empty(),
        }
    }

    pub fn has(&self, attr: Attrs) -> bool {
        self.attrs.contains(attr)
    }
}

impl Default for EmuCell {
    fn default() -> Self {
        EmuCell::blank()
    }
}

/// Join a grid of cells into one string per row.
///
/// Continuation cells contribute nothing: a double-width character already
/// carries both its columns, so emitting a filler for the second one would
/// widen the row by one and shift every column after it.
pub fn rows_to_strings(rows: &[Vec<EmuCell>]) -> Vec<String> {
    rows.iter()
        .map(|row| row.iter().map(|c| c.ch.as_str()).collect::<String>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> EmuCell {
        EmuCell {
            ch: CompactString::from(s),
            ..EmuCell::blank()
        }
    }

    #[test]
    fn index_splits_named_from_palette() {
        assert_eq!(Color::from_index(0), Color::Named(NamedColor::Black));
        assert_eq!(Color::from_index(9), Color::Named(NamedColor::BrightRed));
        assert_eq!(Color::from_index(15), Color::Named(NamedColor::BrightWhite));
        assert_eq!(Color::from_index(16), Color::Idx(16));
        assert_eq!(Color::from_index(255), Color::Idx(255));
        for i in 0..=255u8 {
            assert_eq!(Color::from_index(i).to_index(), i, "roundtrip {i}");
        }
    }

    #[test]
    fn continuation_cells_do_not_widen_a_row() {
        let rows = vec![vec![cell("你"), cell(CONTINUATION), cell("a"), cell(" ")]];
        assert_eq!(rows_to_strings(&rows), vec!["你a "]);
    }

    #[test]
    fn blank_is_a_space_not_a_continuation() {
        assert_eq!(EmuCell::blank().ch, " ");
        assert_ne!(EmuCell::blank().ch, CONTINUATION);
    }
}
