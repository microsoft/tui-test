use std::collections::BTreeSet;

use tiny_skia::Pixmap;

use crate::profile::{ColorSlot, Rgb};
use crate::record::frames::Frame;
use crate::terminal::cell::{EmuCell, CONTINUATION};
use crate::terminal::emu::CursorShape;

use super::svg;
use super::svg::RenderColors;

mod draw;
mod font;

use draw::{
    draw_glyph, fill_circle, fill_rect, fill_rounded_rect, format_glyph_sequence,
    is_default_ignorable, unpremultiply, unsupported_grapheme,
};
use font::{FontSystem, GlyphKey};

#[derive(Debug)]
pub struct RgbaFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn as_raw(&self) -> &[u8] {
        &self.pixels
    }

    pub fn into_raw(self) -> Vec<u8> {
        self.pixels
    }
}

pub trait FrameRenderer {
    fn render(&mut self, frame: &Frame) -> anyhow::Result<RgbaFrame>;
    fn pixel_size(&self) -> (u32, u32);
}

pub struct GridRenderer {
    cols: u16,
    rows: usize,
    scale: u32,
    width: u32,
    height: u32,
    pixmap: Pixmap,
    fonts: FontSystem,
}

impl GridRenderer {
    pub fn new(cols: u16, rows: usize) -> Self {
        Self::with_scale(cols, rows, 1)
    }

    pub fn with_scale(cols: u16, rows: usize, scale: u32) -> Self {
        assert!(scale > 0, "recording raster scale must be non-zero");
        let (base_width, base_height) = svg::pixel_size(cols, rows);
        let width = base_width
            .checked_mul(scale)
            .expect("recording width must fit in u32");
        let height = base_height
            .checked_mul(scale)
            .expect("recording height must fit in u32");
        Self {
            cols,
            rows,
            scale,
            width,
            height,
            pixmap: Pixmap::new(width, height)
                .expect("terminal recording dimensions must fit a pixmap"),
            fonts: FontSystem::new(),
        }
    }
}

impl FrameRenderer for GridRenderer {
    fn render(&mut self, frame: &Frame) -> anyhow::Result<RgbaFrame> {
        let grid = &frame.grid;
        let cols: u16 = grid
            .first()
            .map_or(0, Vec::len)
            .try_into()
            .map_err(|_| anyhow::anyhow!("recording frame width exceeds u16"))?;
        if cols != self.cols || grid.len() != self.rows {
            anyhow::bail!("recording frame dimensions changed during export");
        }
        if grid.iter().any(|row| row.len() != usize::from(cols)) {
            anyhow::bail!("recording frame rows have inconsistent widths");
        }

        let scale = self.scale as f32;
        let colors = &frame.render_state;
        self.pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 0));
        fill_rounded_rect(
            &mut self.pixmap,
            0.0,
            0.0,
            self.width as f32,
            self.height as f32,
            8.0 * scale,
            colors.resolve(None, false),
        );
        for (index, color) in [
            Rgb::new(255, 95, 86),
            Rgb::new(255, 189, 46),
            Rgb::new(39, 201, 63),
        ]
        .into_iter()
        .enumerate()
        {
            let cx = (svg::MARGIN_X + 5.0 + index as f32 * 20.0) * scale;
            let cy = svg::HEADER_H / 2.0 * scale;
            fill_circle(&mut self.pixmap, cx, cy, svg::DOT_R * scale, color);
        }

        let blank = EmuCell::blank();
        for (y, row) in grid.iter().enumerate() {
            for x in 0..usize::from(cols) {
                let cell = row.get(x).unwrap_or(&blank);
                let background = svg::bg_of(cell, colors);
                if background != colors.resolve(None, false) {
                    fill_rect(
                        &mut self.pixmap,
                        (svg::MARGIN_X + x as f32 * svg::CELL_W) * scale,
                        (svg::HEADER_H + y as f32 * svg::CELL_H) * scale,
                        svg::CELL_W * scale,
                        svg::CELL_H * scale,
                        background,
                    );
                }
            }
        }

        let mut missing = BTreeSet::new();
        let (pixmap, fonts) = (&mut self.pixmap, &mut self.fonts);
        for (y, row) in grid.iter().enumerate() {
            for x in 0..usize::from(cols) {
                let cell = row.get(x).unwrap_or(&blank);
                if cell.ch.as_str() == CONTINUATION {
                    continue;
                }
                let style = svg::style_of(cell, colors);
                if style.invisible {
                    continue;
                }
                let span = if row
                    .get(x + 1)
                    .is_some_and(|next| next.ch.as_str() == CONTINUATION)
                {
                    2
                } else {
                    1
                };
                let origin_x = (svg::MARGIN_X + x as f32 * svg::CELL_W) * scale;
                let origin_y = (svg::HEADER_H + y as f32 * svg::CELL_H) * scale;
                let cell_width = svg::CELL_W * span as f32 * scale;
                let cell_height = svg::CELL_H * scale;
                let baseline =
                    (svg::HEADER_H + y as f32 * svg::CELL_H + svg::FONT_BASELINE) * scale;

                if unsupported_grapheme(cell.ch.as_str()) {
                    missing.insert(format_glyph_sequence(cell.ch.as_str()));
                    continue;
                }
                for character in cell.ch.chars() {
                    if character.is_whitespace() || is_default_ignorable(character) {
                        continue;
                    }
                    let key = GlyphKey {
                        character,
                        bold: style.bold,
                        italic: style.italic,
                    };
                    match fonts.resolve(key) {
                        Some(glyph) => draw_glyph(
                            pixmap,
                            glyph,
                            origin_x,
                            origin_y,
                            cell_width,
                            cell_height,
                            baseline,
                            style.fg,
                            scale,
                        ),
                        None => {
                            missing.insert(format!("{character:?} (U+{:04X})", character as u32));
                        }
                    }
                }

                if style.underline {
                    fill_rect(
                        pixmap,
                        origin_x,
                        origin_y + cell_height - 3.0 * scale,
                        cell_width,
                        scale.max(1.0),
                        style.fg,
                    );
                }
                if style.strike {
                    fill_rect(
                        pixmap,
                        origin_x,
                        baseline - svg::FONT_SIZE * 0.32 * scale,
                        cell_width,
                        scale.max(1.0),
                        style.fg,
                    );
                }
            }
        }

        if let Some(cursor) = frame.cursor {
            draw_cursor(
                &mut self.pixmap,
                &mut self.fonts,
                grid,
                cursor,
                colors,
                scale,
                &mut missing,
            );
        }

        if !missing.is_empty() {
            let glyphs = missing.into_iter().collect::<Vec<_>>().join(", ");
            anyhow::bail!(
                "recording rasterizer could not render glyphs: {glyphs}; install an outline font \
                 containing them or set TUI_TEST_RECORDING_FONT_FAMILIES"
            );
        }

        let mut pixels = self.pixmap.data().to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            unpremultiply(pixel);
        }
        Ok(RgbaFrame {
            width: self.width,
            height: self.height,
            pixels,
        })
    }

    fn pixel_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

fn draw_cursor(
    pixmap: &mut Pixmap,
    fonts: &mut FontSystem,
    grid: &[Vec<EmuCell>],
    (cx, cy): (u16, usize),
    colors: &dyn RenderColors,
    scale: f32,
    missing: &mut BTreeSet<String>,
) {
    let Some(row) = grid.get(cy) else {
        return;
    };
    let Some(cell) = row.get(usize::from(cx)) else {
        return;
    };
    let span = if row
        .get(usize::from(cx) + 1)
        .is_some_and(|next| next.ch.as_str() == CONTINUATION)
    {
        2
    } else {
        1
    };
    let origin_x = (svg::MARGIN_X + f32::from(cx) * svg::CELL_W) * scale;
    let origin_y = (svg::HEADER_H + cy as f32 * svg::CELL_H) * scale;
    let cell_width = svg::CELL_W * span as f32 * scale;
    let cell_height = svg::CELL_H * scale;
    let thickness = 2.0 * scale;
    let color = colors.color(ColorSlot::Cursor);
    match colors.cursor_shape() {
        CursorShape::Block => {
            fill_rect(pixmap, origin_x, origin_y, cell_width, cell_height, color);
        }
        CursorShape::Underline => {
            fill_rect(
                pixmap,
                origin_x,
                origin_y + cell_height - thickness,
                cell_width,
                thickness,
                color,
            );
            return;
        }
        CursorShape::Bar => {
            fill_rect(pixmap, origin_x, origin_y, thickness, cell_height, color);
            return;
        }
    }

    if cell.ch.as_str() == CONTINUATION || cell.ch.chars().all(char::is_whitespace) {
        return;
    }
    if unsupported_grapheme(cell.ch.as_str()) {
        missing.insert(format_glyph_sequence(cell.ch.as_str()));
        return;
    }
    let style = svg::style_of(cell, colors);
    let baseline = (svg::HEADER_H + cy as f32 * svg::CELL_H + svg::FONT_BASELINE) * scale;
    for character in cell.ch.chars() {
        if is_default_ignorable(character) {
            continue;
        }
        let key = GlyphKey {
            character,
            bold: style.bold,
            italic: style.italic,
        };
        match fonts.resolve(key) {
            Some(glyph) => draw_glyph(
                pixmap,
                glyph,
                origin_x,
                origin_y,
                cell_width,
                cell_height,
                baseline,
                svg::bg_of(cell, colors),
                scale,
            ),
            None => {
                missing.insert(format!("{character:?} (U+{:04X})", character as u32));
            }
        }
    }
}

#[cfg(test)]
mod tests;
