use std::collections::BTreeSet;

use tiny_skia::Pixmap;

use crate::api::CaptureBackground;
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
    stroke_rounded_rect, unpremultiply, unsupported_grapheme,
};
use font::{FontSystem, GlyphKey};

use crate::render::style::Style;

/// The largest canvas that will be allocated, in pixels.
///
/// Generous: a 4K frame is 8 megapixels, so this allows more than ten of them
/// and still caps the buffer at about 400 MB.
const MAX_PIXELS: u64 = 100_000_000;

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
    /// Overrides the style's canvas background for one capture.
    background: Option<CaptureBackground>,
}

impl GridRenderer {
    /// The canvas this renderer draws onto.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn new(cols: u16, rows: usize) -> Self {
        Self::with_scale(cols, rows, 1)
    }

    pub fn with_scale(cols: u16, rows: usize, scale: u32) -> Self {
        Self::with_zoom(cols, rows, f64::from(scale), Style::default())
            .expect("recording raster scale must fit output dimensions")
    }

    pub fn with_zoom(cols: u16, rows: usize, zoom: f64, style: Style) -> anyhow::Result<Self> {
        Self::with_zoom_and_background(cols, rows, zoom, style, None)
    }

    pub fn with_zoom_and_background(
        cols: u16,
        rows: usize,
        zoom: f64,
        style: Style,
        background: Option<CaptureBackground>,
    ) -> anyhow::Result<Self> {
        if !zoom.is_finite() || zoom <= 0.0 || zoom > f64::from(f32::MAX) {
            anyhow::bail!("recording zoom must be finite and greater than zero");
        }
        // Before pixel_size, which is where an unbounded font size overflows.
        style.validate().map_err(|error| anyhow::anyhow!(error))?;
        let (base_width, base_height) = svg::pixel_size(cols, rows, &style);
        let horizontal = style
            .canvas_horizontal()
            .ok_or_else(|| anyhow::anyhow!("recording canvas padding must fit in u32"))?;
        let vertical = style
            .canvas_vertical()
            .ok_or_else(|| anyhow::anyhow!("recording canvas padding must fit in u32"))?;
        let width = base_width
            .checked_add(horizontal)
            .ok_or_else(|| anyhow::anyhow!("recording width must fit in u32"))?;
        let height = base_height
            .checked_add(vertical)
            .ok_or_else(|| anyhow::anyhow!("recording height must fit in u32"))?;
        let width = scaled_dimension(width, zoom, "width")?;
        let height = scaled_dimension(height, zoom, "height")?;
        // Each axis can pass its own bound while the area is enormous: a
        // 500x200 grid at the largest allowed font and padding is under
        // u32::MAX on both axes and still a 312 GB pixmap. The product is what
        // gets allocated, so the product is what has to be bounded.
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_PIXELS {
            anyhow::bail!(
                "recording is {width}x{height}, which is {pixels} pixels; the limit is {MAX_PIXELS}"
            );
        }
        Ok(Self {
            max_cols: cols,
            max_rows: rows,
            scale: zoom as f32,
            width,
            height,
            pixmap: Pixmap::new(width, height).ok_or_else(|| {
                anyhow::anyhow!("terminal recording dimensions must fit a pixmap")
            })?,
            fonts: FontSystem::new(&style.font),
            style,
            background,
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
        // The window sits at its own left and top gap. A frame smaller than the
        // canvas -- the terminal shrank mid-recording -- is still centred, but
        // within the area the gaps leave rather than the whole image. With
        // equal gaps this is the midpoint it always was.
        let pad_left = style.canvas_left() as f32 * scale;
        let pad_top = style.canvas_top() as f32 * scale;
        let pad_right = style.canvas_right() as f32 * scale;
        let pad_bottom = style.canvas_bottom() as f32 * scale;
        let content_width = (self.width as f32 - pad_left - pad_right).max(0.0);
        let content_height = (self.height as f32 - pad_top - pad_bottom).max(0.0);
        let origin_x = pad_left + (content_width - panel_width as f32).max(0.0) / 2.0;
        let origin_y = pad_top + (content_height - panel_height as f32).max(0.0) / 2.0;
        // A capture may override the configured canvas, including with nothing
        // at all; naming none leaves the style in charge.
        match self.background {
            Some(CaptureBackground::Transparent) => {
                self.pixmap.fill(tiny_skia::Color::TRANSPARENT);
            }
            Some(CaptureBackground::Color(color)) => {
                self.pixmap
                    .fill(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, 255));
            }
            None => {
                self.pixmap.fill(tiny_skia::Color::from_rgba8(
                    style.canvas_background.r,
                    style.canvas_background.g,
                    style.canvas_background.b,
                    255,
                ));
            }
        }
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
        // One decision, as in the SVG path: with no title bar there is no
        // rounded strip, no divider, no controls and no title text, rather
        // than each of them drawn at a collapsed height over the grid.
        let mut missing = BTreeSet::new();
        if style.window.title_bar {
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
            let lights = style.window.traffic_lights();
            for (index, color) in lights.iter().copied().enumerate() {
                let cx = origin_x + (style.content_left() + 5.0 + index as f32 * 20.0) * scale;
                let cy = origin_y + style.header_height() / 2.0 * scale;
                fill_circle(&mut self.pixmap, cx, cy, svg::DOT_R * scale, color);
            }
            if !lights.is_empty() {
                fill_circle(
                    &mut self.pixmap,
                    origin_x + (style.content_left() + 5.0) * scale,
                    origin_y + style.header_height() / 2.0 * scale,
                    svg::RED_DOT_R * scale,
                    svg::RED_DOT_COLOR,
                );
            }
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
        }

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
                    origin_x + (style.content_left() + x as f32 * style.cell_width()) * scale;
                let cell_origin_y = origin_y
                    + (style.header_height()
                        + style.content_top()
                        + y as f32 * style.cell_height())
                        * scale;
                let cell_width = style.cell_width() * span as f32 * scale;
                let cell_height = style.cell_height() * scale;
                let baseline = origin_y
                    + (style.header_height()
                        + style.content_top()
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

        // Last, so the content it frames cannot paint over it.
        if style.border.width > 0.0 {
            stroke_rounded_rect(
                &mut self.pixmap,
                origin_x,
                origin_y,
                panel_width as f32,
                panel_height as f32,
                style.border.radius * scale,
                style.border.color,
                style.border.width * scale,
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
    (origin_x + (style.content_left() + column as f32 * style.cell_width()) * scale).round() as u32
}

fn grid_y(origin_y: f32, row: usize, scale: f32, style: &Style) -> u32 {
    (origin_y
        + (style.header_height() + style.content_top() + row as f32 * style.cell_height()) * scale)
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
    let origin_x =
        panel_origin_x + (style.content_left() + f32::from(cx) * style.cell_width()) * scale;
    let origin_y = panel_origin_y
        + (style.header_height() + style.content_top() + cy as f32 * style.cell_height()) * scale;
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
            + style.content_top()
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
    for (spread, offset_y, alpha) in style.shadow_layers() {
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
