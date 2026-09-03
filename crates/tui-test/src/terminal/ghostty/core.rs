//! Single-threaded Ghostty terminal and neutral-grid translation.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use compact_str::CompactString;
use ghostty_vt::error::Error as GhosttyError;
use ghostty_vt::key::KittyKeyFlags;
use ghostty_vt::render::{CellIterator, CursorVisualStyle, RowIterator};
use ghostty_vt::screen::{Cell as GhosttyCell, CellContentTag, CellWide, GridRef};
use ghostty_vt::style::{Palette, PaletteIndex, RgbColor, Style, StyleColor, Underline};
use ghostty_vt::terminal::{
    ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType, Point, PointCoordinate,
    PrimaryDeviceAttributes, SecondaryDeviceAttributes, TertiaryDeviceAttributes,
};
use ghostty_vt::{RenderState, Terminal};

use crate::event::BellTracker;
use crate::profile::{xterm_color, ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{Clipboard, ClipboardType, CursorShape, KeyboardMode};

fn to_ghostty_rgb(color: Rgb) -> RgbColor {
    RgbColor {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

fn from_ghostty_rgb(color: RgbColor) -> Rgb {
    Rgb::new(color.r, color.g, color.b)
}

fn cell_color(color: StyleColor) -> Option<Color> {
    match color {
        StyleColor::None => None,
        StyleColor::Palette(index) => Some(Color::from_index(index.0)),
        StyleColor::Rgb(color) => Some(Color::Rgb(color.r, color.g, color.b)),
    }
}

fn underline(style: Underline) -> UnderlineStyle {
    match style {
        Underline::None => UnderlineStyle::None,
        Underline::Single => UnderlineStyle::Single,
        Underline::Double => UnderlineStyle::Double,
        Underline::Curly => UnderlineStyle::Curly,
        Underline::Dotted => UnderlineStyle::Dotted,
        Underline::Dashed => UnderlineStyle::Dashed,
        _ => UnderlineStyle::Single,
    }
}

fn cell_from_ghostty(cell: GhosttyCell, style: Style, graphemes: &[char]) -> Result<EmuCell> {
    let ch = match cell.wide().context("reading cell width")? {
        CellWide::SpacerTail => CompactString::const_new(CONTINUATION),
        CellWide::SpacerHead => CompactString::const_new(" "),
        CellWide::Narrow | CellWide::Wide if graphemes.is_empty() => CompactString::const_new(" "),
        CellWide::Narrow | CellWide::Wide => graphemes.iter().collect::<String>().into(),
    };

    let bg = match cell.content_tag().context("reading cell content")? {
        CellContentTag::BgColorPalette => Some(Color::from_index(
            cell.bg_color_palette()
                .context("reading cell palette background")?
                .0,
        )),
        CellContentTag::BgColorRgb => {
            let color = cell.bg_color_rgb().context("reading cell RGB background")?;
            Some(Color::Rgb(color.r, color.g, color.b))
        }
        CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => cell_color(style.bg_color),
    };

    let mut attrs = Attrs::empty();
    for (enabled, attr) in [
        (style.bold, Attrs::BOLD),
        (style.faint, Attrs::DIM),
        (style.italic, Attrs::ITALIC),
        (style.inverse, Attrs::INVERSE),
        (style.invisible, Attrs::INVISIBLE),
        (style.strikethrough, Attrs::STRIKE),
        (style.blink, Attrs::BLINK),
    ] {
        attrs.set(attr, enabled);
    }

    Ok(EmuCell {
        ch,
        fg: cell_color(style.fg_color),
        bg,
        underline: underline(style.underline),
        underline_color: cell_color(style.underline_color),
        attrs,
    })
}

fn grid_graphemes(grid: &GridRef<'_>) -> Result<Vec<char>> {
    let mut inline = ['\0'; 8];
    match grid.graphemes(&mut inline) {
        Ok(len) => Ok(inline[..len].to_vec()),
        Err(GhosttyError::OutOfSpace { required }) => {
            let mut chars = vec!['\0'; required];
            let len = grid
                .graphemes(&mut chars)
                .context("reading cell graphemes")?;
            chars.truncate(len);
            Ok(chars)
        }
        Err(error) => Err(error).context("reading cell graphemes"),
    }
}

fn cell_from_grid(grid: &GridRef<'_>) -> Result<EmuCell> {
    let cell = grid.cell().context("reading scrollback cell value")?;
    let graphemes = if matches!(
        cell.wide().context("reading scrollback cell width")?,
        CellWide::SpacerHead | CellWide::SpacerTail
    ) {
        Vec::new()
    } else {
        grid_graphemes(grid)?
    };
    cell_from_ghostty(
        cell,
        grid.style().context("reading scrollback cell style")?,
        &graphemes,
    )
}

#[derive(Clone)]
pub(super) struct Frame {
    pub(super) rows: Vec<Vec<EmuCell>>,
    pub(super) cursor: (u16, u16),
    pub(super) cursor_visible: bool,
    pub(super) cursor_shape: CursorShape,
}

pub(super) struct GhosttyCore {
    terminal: Terminal<'static, 'static>,
    render: RenderState<'static>,
    row_iter: RowIterator<'static>,
    cell_iter: CellIterator<'static>,
    pending: Rc<RefCell<Vec<u8>>>,
    clipboard: Clipboard,
    profile: Profile,
    frame: Option<Frame>,
}

impl GhosttyCore {
    pub(super) fn new(cols: u16, rows: u16, profile: Profile, bells: BellTracker) -> Result<Self> {
        let mut terminal = Terminal::new(cols.max(1), rows.max(1)).context("creating terminal")?;
        terminal
            .resize(cols.max(1), rows.max(1), 1, 1)
            .context("initializing terminal cell size")?;
        terminal
            .set_scrollback_max_lines(Some(profile.scrollback))
            .context("setting scrollback limit")?;

        terminal
            .set_default_fg_color(Some(to_ghostty_rgb(profile.colors.foreground)))
            .context("setting foreground")?
            .set_default_bg_color(Some(to_ghostty_rgb(profile.colors.background)))
            .context("setting background")?
            .set_default_cursor_color(Some(to_ghostty_rgb(profile.colors.cursor)))
            .context("setting cursor color")?;

        let ansi = profile.colors.ansi();
        let mut palette = Palette::default();
        for index in 0u8..=255 {
            let color = if index < 16 {
                ansi[index as usize]
            } else {
                xterm_color(index)
            };
            palette.set(PaletteIndex(index), to_ghostty_rgb(color));
        }
        terminal
            .set_default_color_palette(Some(palette))
            .context("setting palette")?;

        let pending = Rc::new(RefCell::new(Vec::new()));
        let clipboard = Clipboard::default();
        terminal
            .on_pty_write({
                let pending = Rc::clone(&pending);
                move |_terminal, data| pending.borrow_mut().extend_from_slice(data)
            })
            .context("registering PTY replies")?
            .on_bell(move |_terminal| bells.ring())
            .context("registering bell events")?
            .on_device_attributes(|_terminal| {
                Some(DeviceAttributes {
                    primary: PrimaryDeviceAttributes::new(
                        ConformanceLevel::VT220,
                        &[
                            DeviceAttributeFeature::ANSI_COLOR,
                            DeviceAttributeFeature::CLIPBOARD,
                        ],
                    ),
                    secondary: SecondaryDeviceAttributes {
                        device_type: DeviceType::VT220,
                        firmware_version: 1,
                        rom_cartridge: 0,
                    },
                    tertiary: TertiaryDeviceAttributes { unit_id: 0 },
                })
            })
            .context("registering device attributes")?;

        Ok(Self {
            terminal,
            render: RenderState::new().context("creating render state")?,
            row_iter: RowIterator::new().context("creating row iterator")?,
            cell_iter: CellIterator::new().context("creating cell iterator")?,
            pending,
            clipboard,
            profile,
            frame: None,
        })
    }

    pub(super) fn process(&mut self, bytes: &[u8]) {
        self.terminal.vt_write(bytes);
        self.frame = None;
    }

    pub(super) fn take_pending_writes(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }

    /// Kitty keyboard flags the child has pushed onto ghostty's mode stack.
    ///
    /// Ghostty's `Terminal` takes no configuration, so a profile that turns
    /// the protocol off is honored here rather than in the emulator: the
    /// modes are still tracked, they just never reach key encoding.
    pub(super) fn keyboard_mode(&self) -> Result<KeyboardMode> {
        if !self.profile.kitty_keyboard {
            return Ok(KeyboardMode::empty());
        }
        let flags = self
            .terminal
            .kitty_keyboard_flags()
            .context("reading Kitty keyboard flags")?;
        let mut mode = KeyboardMode::empty();
        for (ghostty_flag, keyboard_flag) in [
            (
                KittyKeyFlags::DISAMBIGUATE,
                KeyboardMode::DISAMBIGUATE_ESC_CODES,
            ),
            (
                KittyKeyFlags::REPORT_EVENTS,
                KeyboardMode::REPORT_EVENT_TYPES,
            ),
            (
                KittyKeyFlags::REPORT_ALTERNATES,
                KeyboardMode::REPORT_ALTERNATE_KEYS,
            ),
            (
                KittyKeyFlags::REPORT_ALL,
                KeyboardMode::REPORT_ALL_KEYS_AS_ESC,
            ),
            (
                KittyKeyFlags::REPORT_ASSOCIATED,
                KeyboardMode::REPORT_ASSOCIATED_TEXT,
            ),
        ] {
            mode.set(keyboard_flag, flags.contains(ghostty_flag));
        }
        Ok(mode)
    }

    pub(super) fn set_clipboard(&mut self, clipboard: ClipboardType, text: String) {
        self.clipboard.set(clipboard, text);
    }

    pub(super) fn answer_clipboard_query(
        &mut self,
        clipboard: ClipboardType,
        selector: u8,
        bell_terminated: bool,
    ) {
        let encoded = BASE64.encode(self.clipboard.get(clipboard).as_bytes());
        let terminator = if bell_terminated { "\x07" } else { "\x1b\\" };
        self.pending.borrow_mut().extend_from_slice(
            format!("\x1b]52;{};{encoded}{terminator}", char::from(selector)).as_bytes(),
        );
    }

    pub(super) fn clipboard(&self, clipboard: ClipboardType) -> String {
        self.clipboard.get(clipboard).to_string()
    }

    pub(super) fn clipboard_revision(&self, clipboard: ClipboardType) -> u64 {
        self.clipboard.revision(clipboard)
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.terminal
            .resize(cols.max(1), rows.max(1), 1, 1)
            .context("resizing terminal")?;
        self.frame = None;
        Ok(())
    }

    fn capture(&mut self) -> Result<Frame> {
        let snapshot = self
            .render
            .update(&self.terminal)
            .context("updating render state")?;
        let cols = snapshot.cols().context("reading viewport width")?;
        let rows = snapshot.rows().context("reading viewport height")?;
        let cursor = (
            self.terminal
                .cursor_x()
                .context("reading cursor column")?
                .min(cols.saturating_sub(1)),
            self.terminal
                .cursor_y()
                .context("reading cursor row")?
                .min(rows.saturating_sub(1)),
        );
        let cursor_visible = snapshot
            .cursor_visible()
            .context("reading cursor visibility")?;
        let cursor_shape = match snapshot
            .cursor_visual_style()
            .context("reading cursor shape")?
        {
            CursorVisualStyle::Bar => CursorShape::Bar,
            CursorVisualStyle::Underline => CursorShape::Underline,
            _ => CursorShape::Block,
        };

        let mut output = Vec::with_capacity(rows as usize);
        let mut row_iter = self
            .row_iter
            .update(&snapshot)
            .context("starting row iteration")?;
        while let Some(row) = row_iter.next() {
            let mut output_row = Vec::with_capacity(cols as usize);
            let mut cells = self
                .cell_iter
                .update(row)
                .context("starting cell iteration")?;
            while let Some(cell) = cells.next() {
                let raw = cell.raw_cell().context("reading cell")?;
                let graphemes = if matches!(
                    raw.wide().context("reading cell width")?,
                    CellWide::SpacerHead | CellWide::SpacerTail
                ) {
                    Vec::new()
                } else {
                    cell.graphemes().context("reading cell graphemes")?
                };
                output_row.push(cell_from_ghostty(
                    raw,
                    cell.style().context("reading cell style")?,
                    &graphemes,
                )?);
            }
            if output_row.len() != cols as usize {
                return Err(anyhow!(
                    "Ghostty returned {} cells for a {cols}-column row",
                    output_row.len()
                ));
            }
            output.push(output_row);
        }
        if output.len() != rows as usize {
            return Err(anyhow!(
                "Ghostty returned {} rows for a {rows}-row viewport",
                output.len()
            ));
        }

        Ok(Frame {
            rows: output,
            cursor,
            cursor_visible,
            cursor_shape,
        })
    }

    pub(super) fn frame(&mut self) -> Result<&Frame> {
        if self.frame.is_none() {
            self.frame = Some(self.capture()?);
        }
        Ok(self.frame.as_ref().expect("frame was populated"))
    }

    pub(super) fn full_rows(&mut self) -> Result<Vec<Vec<EmuCell>>> {
        let cols = self.terminal.cols().context("reading terminal width")?;
        let available = self
            .terminal
            .scrollback_rows()
            .context("reading scrollback size")?;
        let history = available.min(self.profile.scrollback);
        let mut output = Vec::with_capacity(history + self.terminal.rows()? as usize);
        for y in available - history..available {
            let y = u32::try_from(y).context("scrollback exceeds Ghostty coordinates")?;
            let mut row = Vec::with_capacity(cols as usize);
            for x in 0..cols {
                let grid = self
                    .terminal
                    .grid_ref(Point::History(PointCoordinate { x, y }))
                    .context("reading scrollback cell")?;
                row.push(cell_from_grid(&grid)?);
            }
            output.push(row);
        }
        output.extend(self.frame()?.rows.clone());
        Ok(output)
    }

    pub(super) fn size(&self) -> Result<(u16, u16)> {
        Ok((self.terminal.cols()?, self.terminal.rows()?))
    }

    pub(super) fn color(&self, slot: ColorSlot) -> Result<Rgb> {
        let configured = match slot {
            ColorSlot::Indexed(index) => self.profile.colors.ansi().get(index as usize).copied(),
            ColorSlot::Foreground => Some(self.profile.colors.foreground),
            ColorSlot::Background => Some(self.profile.colors.background),
            ColorSlot::Cursor => Some(self.profile.colors.cursor),
        };
        let current = match slot {
            ColorSlot::Indexed(index) => Some(
                self.terminal
                    .color_palette()
                    .context("reading palette")?
                    .get(PaletteIndex(index)),
            ),
            ColorSlot::Foreground => self.terminal.fg_color().context("reading foreground")?,
            ColorSlot::Background => self.terminal.bg_color().context("reading background")?,
            ColorSlot::Cursor => self
                .terminal
                .cursor_color()
                .context("reading cursor color")?,
        };
        current
            .map(from_ghostty_rgb)
            .or(configured)
            .ok_or_else(|| anyhow!("Ghostty returned no color for {slot:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_wide_cells_into_the_neutral_grid() {
        let mut core = GhosttyCore::new(5, 3, Profile::default(), BellTracker::default()).unwrap();
        core.process("abcd你".as_bytes());
        let rows = &core.frame().unwrap().rows;
        assert_eq!(rows[0][4].ch, " ");
        assert_eq!(rows[1][0].ch, "你");
        assert_eq!(rows[1][1].ch, CONTINUATION);
    }

    #[test]
    fn queues_terminal_replies_synchronously() {
        let mut core = GhosttyCore::new(10, 4, Profile::default(), BellTracker::default()).unwrap();
        core.process(b"\x1b[3;5H\x1b[6n");
        assert_eq!(core.take_pending_writes(), b"\x1b[3;5R");
    }
}
