use std::collections::BTreeSet;

use tiny_skia::Pixmap;

use crate::profile::ColorSlot;
use crate::record::frames::Frame;
use crate::terminal::cell::{EmuCell, CONTINUATION};
use crate::terminal::emu::CursorShape;

use super::svg;
use super::svg::RenderColors;

mod draw;
mod font;

use draw::{
    draw_glyph, fill_antialiased_rect, fill_circle, fill_pixel_rect, fill_rounded_rect,
    fill_rounded_rect_alpha, fill_top_rounded_rect, format_glyph_sequence, is_default_ignorable,
    unpremultiply, unsupported_grapheme,
};
use font::{FontSystem, GlyphKey};

use crate::render::style::Style;

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
    scale: f32,
    width: u32,
    height: u32,
    pixmap: Pixmap,
    fonts: FontSystem,
    style: Style,
}

impl GridRenderer {
    pub fn new(cols: u16, rows: usize) -> Self {
        Self::with_scale(cols, rows, 1)
    }

    pub fn with_scale(cols: u16, rows: usize, scale: u32) -> Self {
        Self::with_zoom(cols, rows, f64::from(scale), Style::default())
            .expect("recording raster scale must fit output dimensions")
    }

    pub fn with_zoom(cols: u16, rows: usize, zoom: f64, style: Style) -> anyhow::Result<Self> {
        if !zoom.is_finite() || zoom <= 0.0 || zoom > f64::from(f32::MAX) {
            anyhow::bail!("recording zoom must be finite and greater than zero");
        }
        let (base_width, base_height) = svg::pixel_size(cols, rows, &style);
        let padding = style
            .padding
            .checked_mul(2)
            .expect("recording canvas padding must fit in u32");
        let width = base_width
            .checked_add(padding)
            .ok_or_else(|| anyhow::anyhow!("recording width must fit in u32"))?;
        let height = base_height
            .checked_add(padding)
            .ok_or_else(|| anyhow::anyhow!("recording height must fit in u32"))?;
        let width = scaled_dimension(width, zoom, "width")?;
        let height = scaled_dimension(height, zoom, "height")?;
        Ok(Self {
            max_cols: cols,
            max_rows: rows,
            scale: zoom as f32,
            width,
            height,
            pixmap: Pixmap::new(width, height).ok_or_else(|| {
                anyhow::anyhow!("terminal recording dimensions must fit a pixmap")
            })?,
            fonts: FontSystem::new(),
            style,
        })
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

        let scale = self.scale;
        let colors = &frame.render_state;
        let style = &self.style;
        let (base_width, base_height) = svg::pixel_size(cols, rows, style);
        let panel_width = scaled_dimension(base_width, f64::from(self.scale), "frame width")?;
        let panel_height = scaled_dimension(base_height, f64::from(self.scale), "frame height")?;
        let origin_x = (self.width - panel_width) as f32 / 2.0;
        let origin_y = (self.height - panel_height) as f32 / 2.0;
        self.pixmap.fill(tiny_skia::Color::from_rgba8(
            style.background.r,
            style.background.g,
            style.background.b,
            255,
        ));
        draw_shadow(
            &mut self.pixmap,
            origin_x,
            origin_y,
            panel_width as f32,
            panel_height as f32,
            scale,
            style,
        );
        fill_rounded_rect(
            &mut self.pixmap,
            origin_x,
            origin_y,
            panel_width as f32,
            panel_height as f32,
            style.border.radius * scale,
            colors.resolve(None, false),
        );
        fill_top_rounded_rect(
            &mut self.pixmap,
            origin_x,
            origin_y,
            panel_width as f32,
            (style.header_height() - style.divider_height()) * scale,
            style.border.radius * scale,
            style.window.background,
        );
        fill_antialiased_rect(
            &mut self.pixmap,
            origin_x,
            origin_y + (style.header_height() - style.divider_height()) * scale,
            panel_width as f32,
            style.divider_height() * scale,
            style.window.divider,
        );
        for (index, color) in style.window.traffic_lights().iter().copied().enumerate() {
            let cx = origin_x + (svg::MARGIN_X + 5.0 + index as f32 * 20.0) * scale;
            let cy = origin_y + style.header_height() / 2.0 * scale;
            fill_circle(&mut self.pixmap, cx, cy, svg::DOT_R * scale, color);
        }
        fill_circle(
            &mut self.pixmap,
            origin_x + (svg::MARGIN_X + 5.0) * scale,
            origin_y + style.header_height() / 2.0 * scale,
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
            style,
        );

        let blank = EmuCell::blank();
        for (y, row) in grid.iter().enumerate() {
            let mut x = 0;
            while x < usize::from(cols) {
                let cell = row.get(x).unwrap_or(&blank);
                let background = svg::bg_of(cell, colors);
                let mut run = 1;
                while x + run < usize::from(cols)
                    && svg::bg_of(row.get(x + run).unwrap_or(&blank), colors) == background
                {
                    run += 1;
                }
                if background != colors.resolve(None, false) {
                    let left = grid_x(origin_x, x, scale, style);
                    let right = grid_x(origin_x, x + run, scale, style);
                    let top = grid_y(origin_y, y, scale, style);
                    let bottom = grid_y(origin_y, y + 1, scale, style);
                    fill_pixel_rect(&mut self.pixmap, left, top, right, bottom, background);
                }
                x += run;
            }
        }

        let (pixmap, fonts) = (&mut self.pixmap, &mut self.fonts);
        for (y, row) in grid.iter().enumerate() {
            for x in 0..usize::from(cols) {
                let cell = row.get(x).unwrap_or(&blank);
                if cell.ch.as_str() == CONTINUATION {
                    continue;
                }
                let paint = svg::cell_paint(cell, colors);
                if paint.invisible {
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
                let cell_origin_x =
                    origin_x + (svg::MARGIN_X + x as f32 * style.cell_width()) * scale;
                let cell_origin_y = origin_y
                    + (style.header_height()
                        + svg::CONTENT_PADDING_TOP
                        + y as f32 * style.cell_height())
                        * scale;
                let cell_width = style.cell_width() * span as f32 * scale;
                let cell_height = style.cell_height() * scale;
                let baseline = origin_y
                    + (style.header_height()
                        + svg::CONTENT_PADDING_TOP
                        + y as f32 * style.cell_height()
                        + style.baseline())
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
                        bold: paint.bold,
                        italic: paint.italic,
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
                            paint.fg,
                            style.font_size,
                            scale,
                        ),
                        None => {
                            missing.insert(format!("{character:?} (U+{:04X})", character as u32));
                        }
                    }
                }

                if paint.underline {
                    fill_antialiased_rect(
                        pixmap,
                        cell_origin_x,
                        cell_origin_y + cell_height - 3.0 * scale,
                        cell_width,
                        scale.max(1.0),
                        paint.fg,
                    );
                }
                if paint.strike {
                    fill_antialiased_rect(
                        pixmap,
                        cell_origin_x,
                        baseline - style.font_size * 0.32 * scale,
                        cell_width,
                        scale.max(1.0),
                        paint.fg,
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
                style,
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
        for pixel in pixels.as_chunks_mut::<4>().0 {
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

fn grid_x(origin_x: f32, column: usize, scale: f32, style: &Style) -> u32 {
    (origin_x + (svg::MARGIN_X + column as f32 * style.cell_width()) * scale).round() as u32
}

fn grid_y(origin_y: f32, row: usize, scale: f32, style: &Style) -> u32 {
    (origin_y
        + (style.header_height() + svg::CONTENT_PADDING_TOP + row as f32 * style.cell_height())
            * scale)
        .round() as u32
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
    style: &Style,
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
    let column = usize::from(cx);
    let origin_x = panel_origin_x + (svg::MARGIN_X + f32::from(cx) * style.cell_width()) * scale;
    let origin_y = panel_origin_y
        + (style.header_height() + svg::CONTENT_PADDING_TOP + cy as f32 * style.cell_height())
            * scale;
    let cell_width = style.cell_width() * span as f32 * scale;
    let cell_height = style.cell_height() * scale;
    let left = grid_x(panel_origin_x, column, scale, style);
    let right = grid_x(panel_origin_x, column + span, scale, style);
    let top = grid_y(panel_origin_y, cy, scale, style);
    let bottom = grid_y(panel_origin_y, cy + 1, scale, style);
    let thickness = (2.0 * scale).round().max(1.0) as u32;
    let color = colors.color(ColorSlot::Cursor);
    match colors.cursor_shape() {
        CursorShape::Block => {
            fill_pixel_rect(pixmap, left, top, right, bottom, color);
        }
        CursorShape::Underline => {
            fill_pixel_rect(
                pixmap,
                left,
                bottom.saturating_sub(thickness).max(top),
                right,
                bottom,
                color,
            );
            return;
        }
        CursorShape::Bar => {
            fill_pixel_rect(
                pixmap,
                left,
                top,
                left.saturating_add(thickness).min(right),
                bottom,
                color,
            );
            return;
        }
    }

    if cell.ch.as_str() == CONTINUATION || cell.ch.chars().all(char::is_whitespace) {
        return;
    }
    let paint = svg::cell_paint(cell, colors);
    if paint.invisible {
        return;
    }
    if unsupported_grapheme(cell.ch.as_str()) {
        missing.insert(format_glyph_sequence(cell.ch.as_str()));
        return;
    }
    let baseline = panel_origin_y
        + (style.header_height()
            + svg::CONTENT_PADDING_TOP
            + cy as f32 * style.cell_height()
            + style.baseline())
            * scale;
    for character in cell.ch.chars() {
        if is_default_ignorable(character) {
            continue;
        }
        let key = GlyphKey {
            character,
            bold: paint.bold,
            italic: paint.italic,
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
                style.font_size,
                scale,
            ),
            None => {
                missing.insert(format!("{character:?} (U+{:04X})", character as u32));
            }
        }
    }
}

fn draw_shadow(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
    style: &Style,
) {
    for &(spread, offset_y, alpha) in style.shadow_layers() {
        let spread = spread * scale;
        fill_rounded_rect_alpha(
            pixmap,
            x - spread,
            y - spread + offset_y * scale,
            width + spread * 2.0,
            height + spread * 2.0,
            style.border.radius * scale + spread,
            style.shadow.color,
            alpha,
        );
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
    style: &Style,
) {
    let Some(title) = svg::visible_title(title, cols, rows, panel_width, style) else {
        return;
    };
    let advance = svg::title_advance(style);
    let title_width = crate::terminal::cell::display_width(&title) as f32 * advance * scale;
    let mut x = origin_x + (panel_width * scale - title_width) / 2.0;
    let baseline = origin_y + (style.header_height() / 2.0 + style.title_font_size * 0.35) * scale;

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
                    style.header_height() * scale,
                    baseline,
                    style.window.foreground,
                    style.title_font_size,
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

fn scaled_dimension(base: u32, zoom: f64, name: &str) -> anyhow::Result<u32> {
    let scaled = f64::from(base) * zoom;
    if !scaled.is_finite() || scaled > f64::from(u32::MAX) {
        anyhow::bail!("recording {name} is too large");
    }
    Ok(scaled.ceil().max(1.0) as u32)
}

#[cfg(test)]
mod tests;
