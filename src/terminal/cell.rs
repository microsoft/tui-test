//! Backend-neutral grid vocabulary.
//!
//! Every consumer of the terminal grid (render, assert, monitor, locator)
//! speaks these types and nothing else, so swapping the emulator backend
//! behind [`crate::terminal::emu::Emulator`] is invisible to them. Nothing in
//! this module may depend on a specific emulator crate.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmuCell {
    /// Empty string means a blank cell (rendered as a space).
    pub ch: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strike: bool,
}

impl Default for EmuCell {
    fn default() -> Self {
        EmuCell {
            ch: String::new(),
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            invisible: false,
            strike: false,
        }
    }
}

/// Join a grid of cells into one string per row (blank cells → spaces).
pub fn rows_to_strings(rows: &[Vec<EmuCell>]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|c| if c.ch.is_empty() { " " } else { c.ch.as_str() })
                .collect::<String>()
        })
        .collect()
}
