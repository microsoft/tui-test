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
    draw_glyph, fill_circle, fill_rect, fill_rounded_rect, fill_top_rounded_rect,
    format_glyph_sequence, is_default_ignorable, unpremultiply, unsupported_grapheme,
};
use font::{FontSystem, GlyphKey};

pub(crate) const CANVAS_PADDING: u32 = 24;
pub(crate) const CANVAS_BACKGROUND: Rgb = Rgb::new(104, 103, 170);

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
    max_cols: u16,
    max_rows: usize,
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
        let padding = CANVAS_PADDING
            .checked_mul(scale)
            .and_then(|padding| padding.checked_mul(2))
            .expect("recording canvas padding must fit in u32");
        let width = base_width
            .checked_mul(scale)
            .and_then(|width| width.checked_add(padding))
            .expect("recording width must fit in u32");
        let height = base_height
            .checked_mul(scale)
            .and_then(|height| height.checked_add(padding))
            .expect("recording height must fit in u32");
        Self {
            max_cols: cols,
            max_rows: rows,
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
        let (cols, rows) = frame.dimensions()?;
        if cols > self.max_cols || rows > self.max_rows {
            anyhow::bail!(
                "recording frame dimensions {cols}x{rows} exceed canvas dimensions {}x{}",
                self.max_cols,
                self.max_rows
            );
        }
        if grid.iter().any(|row| row.len() > usize::from(cols)) {
            anyhow::bail!("recording frame row exceeds its declared width");
        }

        let scale = self.scale as f32;
        let colors = &frame.render_state;
        let (base_width, base_height) = svg::pixel_size(cols, rows);
        let panel_width = base_width
            .checked_mul(self.scale)
            .expect("recording frame width must fit in u32");
        let panel_height = base_height
            .checked_mul(self.scale)
            .expect("recording frame height must fit in u32");
        let origin_x = (self.width - panel_width) as f32 / 2.0;
        let origin_y = (self.height - panel_height) as f32 / 2.0;
        self.pixmap.fill(tiny_skia::Color::from_rgba8(
            CANVAS_BACKGROUND.r,
            CANVAS_BACKGROUND.g,
            CANVAS_BACKGROUND.b,
            255,
        ));
        fill_rounded_rect(
            &mut self.pixmap,
            origin_x,
            origin_y,
            panel_width as f32,
            panel_height as f32,
            svg::WINDOW_RADIUS * scale,
            colors.resolve(None, false),
        );
        fill_top_rounded_rect(
            &mut self.pixmap,
            origin_x,
            origin_y,
            panel_width as f32,
            (svg::HEADER_H - svg::TITLE_DIVIDER_H) * scale,
            svg::WINDOW_RADIUS * scale,
            svg::TITLE_BG,
        );
        fill_rect(
            &mut self.pixmap,
            origin_x,
            origin_y + (svg::HEADER_H - svg::TITLE_DIVIDER_H) * scale,
            panel_width as f32,
            svg::TITLE_DIVIDER_H * scale,
            svg::TITLE_DIVIDER,
        );
        for (index, color) in svg::TRAFFIC_LIGHTS.iter().copied().enumerate() {
            let cx = origin_x + (svg::MARGIN_X + 5.0 + index as f32 * 20.0) * scale;
            let cy = origin_y + svg::HEADER_H / 2.0 * scale;
            fill_circle(&mut self.pixmap, cx, cy, svg::DOT_R * scale, color);
        }
        fill_circle(
            &mut self.pixmap,
            origin_x + (svg::MARGIN_X + 5.0) * scale,
            origin_y + svg::HEADER_H / 2.0 * scale,
            svg::RED_DOT_R * scale,
            svg::RED_DOT_COLOR,
        );

        let mut missing = BTreeSet::new();
        draw_title(
            &mut self.pixmap,
            &mut self.fonts,
            frame.title.as_deref(),
            cols,
            rows,
            base_width as f32,
            origin_x,
            origin_y,
            scale,
            &mut missing,
        );

        let blank = EmuCell::blank();
        for (y, row) in grid.iter().enumerate() {
            for x in 0..usize::from(cols) {
                let cell = row.get(x).unwrap_or(&blank);
                let background = svg::bg_of(cell, colors);
                if background != colors.resolve(None, false) {
                    fill_rect(
                        &mut self.pixmap,
                        origin_x + (svg::MARGIN_X + x as f32 * svg::CELL_W) * scale,
                        origin_y
                            + (svg::HEADER_H + svg::CONTENT_PADDING_TOP + y as f32 * svg::CELL_H)
                                * scale,
                        svg::CELL_W * scale,
                        svg::CELL_H * scale,
                        background,
                    );
                }
            }
        }

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
                let cell_origin_x = origin_x + (svg::MARGIN_X + x as f32 * svg::CELL_W) * scale;
                let cell_origin_y = origin_y
                    + (svg::HEADER_H + svg::CONTENT_PADDING_TOP + y as f32 * svg::CELL_H) * scale;
                let cell_width = svg::CELL_W * span as f32 * scale;
                let cell_height = svg::CELL_H * scale;
                let baseline = origin_y
                    + (svg::HEADER_H
                        + svg::CONTENT_PADDING_TOP
                        + y as f32 * svg::CELL_H
                        + svg::FONT_BASELINE)
                        * scale;

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
                            cell_origin_x,
                            cell_origin_y,
                            cell_width,
                            cell_height,
                            baseline,
                            style.fg,
                            svg::FONT_SIZE,
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
                        cell_origin_x,
                        cell_origin_y + cell_height - 3.0 * scale,
                        cell_width,
                        scale.max(1.0),
                        style.fg,
                    );
                }
                if style.strike {
                    fill_rect(
                        pixmap,
                        cell_origin_x,
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
                origin_x,
                origin_y,
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

#[allow(clippy::too_many_arguments)]
fn draw_cursor(
    pixmap: &mut Pixmap,
    fonts: &mut FontSystem,
    grid: &[Vec<EmuCell>],
    (cx, cy): (u16, usize),
    colors: &dyn RenderColors,
    panel_origin_x: f32,
    panel_origin_y: f32,
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
    let origin_x = panel_origin_x + (svg::MARGIN_X + f32::from(cx) * svg::CELL_W) * scale;
    let origin_y = panel_origin_y
        + (svg::HEADER_H + svg::CONTENT_PADDING_TOP + cy as f32 * svg::CELL_H) * scale;
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
    let style = svg::style_of(cell, colors);
    if style.invisible {
        return;
    }
    if unsupported_grapheme(cell.ch.as_str()) {
        missing.insert(format_glyph_sequence(cell.ch.as_str()));
        return;
    }
    let baseline = panel_origin_y
        + (svg::HEADER_H + svg::CONTENT_PADDING_TOP + cy as f32 * svg::CELL_H + svg::FONT_BASELINE)
            * scale;
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
                svg::FONT_SIZE,
                scale,
            ),
            None => {
                missing.insert(format!("{character:?} (U+{:04X})", character as u32));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_title(
    pixmap: &mut Pixmap,
    fonts: &mut FontSystem,
    title: Option<&str>,
    cols: u16,
    rows: usize,
    panel_width: f32,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
    missing: &mut BTreeSet<String>,
) {
    let Some(title) = svg::visible_title(title, cols, rows, panel_width) else {
        return;
    };
    let advance = svg::title_advance();
    let title_width = crate::terminal::cell::display_width(&title) as f32 * advance * scale;
    let mut x = origin_x + (panel_width * scale - title_width) / 2.0;
    let baseline = origin_y + (svg::HEADER_H / 2.0 + svg::TITLE_FONT_SIZE * 0.35) * scale;

    for character in title.chars() {
        let columns = crate::terminal::cell::display_width(&character.to_string()).max(1);
        let width = columns as f32 * advance * scale;
        if !character.is_whitespace() && !is_default_ignorable(character) {
            let key = GlyphKey {
                character,
                bold: true,
                italic: false,
            };
            match fonts.resolve(key) {
                Some(glyph) => draw_glyph(
                    pixmap,
                    glyph,
                    x,
                    origin_y,
                    width,
                    svg::HEADER_H * scale,
                    baseline,
                    svg::TITLE_FG,
                    svg::TITLE_FONT_SIZE,
                    scale,
                ),
                None => {
                    missing.insert(format!("{character:?} (U+{:04X})", character as u32));
                }
            }
        }
        x += width;
    }
}

#[cfg(test)]
mod tests;
