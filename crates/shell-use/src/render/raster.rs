use std::collections::{BTreeSet, HashMap};

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Transform};
use ttf_parser::{Face, OutlineBuilder};

use crate::terminal::cell::{EmuCell, CONTINUATION};

use super::{font, nerd_font, svg};

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
    fn render(&mut self, grid: &[Vec<EmuCell>], cols: u16) -> anyhow::Result<RgbaFrame>;
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
    fn render(&mut self, grid: &[Vec<EmuCell>], cols: u16) -> anyhow::Result<RgbaFrame> {
        if cols != self.cols || grid.len() != self.rows {
            anyhow::bail!("recording frame dimensions changed during export");
        }

        let scale = self.scale as f32;
        let theme = svg::Theme::default();
        self.pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 0));
        fill_rounded_rect(
            &mut self.pixmap,
            0.0,
            0.0,
            self.width as f32,
            self.height as f32,
            8.0 * scale,
            theme.default_bg,
        );
        for (index, color) in [(255, 95, 86), (255, 189, 46), (39, 201, 63)]
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
                let background = svg::bg_of(cell, &theme);
                if background != theme.default_bg {
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
                let style = svg::style_of(cell, &theme);
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

        if !missing.is_empty() {
            let glyphs = missing.into_iter().collect::<Vec<_>>().join(", ");
            anyhow::bail!(
                "recording rasterizer could not render glyphs: {glyphs}; install an outline font \
                 containing them or set SHELL_USE_RECORDING_FONT_FAMILIES"
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    character: char,
    bold: bool,
    italic: bool,
}

struct GlyphOutline {
    path: Path,
    bounds: ttf_parser::Rect,
    advance: u16,
    units_per_em: u16,
    synthetic_bold: bool,
    synthetic_italic: bool,
    powerline: bool,
}

struct FontSystem {
    catalog: &'static font::Catalog,
    glyphs: HashMap<GlyphKey, Option<GlyphOutline>>,
}

impl FontSystem {
    fn new() -> Self {
        Self {
            catalog: font::catalog(),
            glyphs: HashMap::new(),
        }
    }

    fn resolve(&mut self, key: GlyphKey) -> Option<&GlyphOutline> {
        if !self.glyphs.contains_key(&key) {
            let glyph = self.load(key);
            self.glyphs.insert(key, glyph);
        }
        self.glyphs.get(&key).and_then(Option::as_ref)
    }

    fn load(&self, key: GlyphKey) -> Option<GlyphOutline> {
        for id in self.catalog.candidates(key.bold, key.italic, key.character) {
            let info = self.catalog.database.face(id)?;
            let weight = info.weight;
            let style = info.style;
            let glyph = self
                .catalog
                .database
                .with_face_data(id, |data, index| {
                    let face = Face::parse(data, index).ok()?;
                    let glyph_id = face.glyph_index(key.character)?;
                    let mut builder = TinyPathBuilder::default();
                    let bounds = face.outline_glyph(glyph_id, &mut builder)?;
                    let path = builder.finish()?;
                    Some(GlyphOutline {
                        path,
                        bounds,
                        advance: face
                            .glyph_hor_advance(glyph_id)
                            .unwrap_or(face.units_per_em()),
                        units_per_em: face.units_per_em(),
                        synthetic_bold: key.bold && weight.0 < fontdb::Weight::SEMIBOLD.0,
                        synthetic_italic: key.italic && style == fontdb::Style::Normal,
                        powerline: nerd_font::is_powerline_separator(key.character),
                    })
                })
                .flatten();
            if glyph.is_some() {
                return glyph;
            }
        }
        None
    }
}

#[derive(Default)]
struct TinyPathBuilder {
    inner: PathBuilder,
}

impl TinyPathBuilder {
    fn finish(self) -> Option<Path> {
        self.inner.finish()
    }
}

impl OutlineBuilder for TinyPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.inner.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.inner.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.inner.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.inner.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.inner.close();
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    pixmap: &mut Pixmap,
    glyph: &GlyphOutline,
    origin_x: f32,
    origin_y: f32,
    cell_width: f32,
    cell_height: f32,
    baseline: f32,
    color: (u8, u8, u8),
    output_scale: f32,
) {
    let bounds_width = f32::from(glyph.bounds.x_max - glyph.bounds.x_min).max(1.0);
    let bounds_height = f32::from(glyph.bounds.y_max - glyph.bounds.y_min).max(1.0);
    let transform = if glyph.powerline {
        let scale_x = cell_width / bounds_width;
        let scale_y = cell_height / bounds_height;
        Transform::from_row(
            scale_x,
            0.0,
            0.0,
            -scale_y,
            origin_x - f32::from(glyph.bounds.x_min) * scale_x,
            origin_y + f32::from(glyph.bounds.y_max) * scale_y,
        )
    } else {
        let scale_y = svg::FONT_SIZE * output_scale / f32::from(glyph.units_per_em);
        let (scale_x, x) = if glyph.advance == 0 {
            let rendered_width = bounds_width * scale_y;
            (
                scale_y,
                origin_x + (cell_width - rendered_width) / 2.0
                    - f32::from(glyph.bounds.x_min) * scale_y,
            )
        } else {
            (cell_width / f32::from(glyph.advance), origin_x)
        };
        let shear = if glyph.synthetic_italic {
            0.2 * scale_y
        } else {
            0.0
        };
        Transform::from_row(scale_x, 0.0, shear, -scale_y, x, baseline)
    };

    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, 255);
    pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, transform, None);
    if glyph.synthetic_bold {
        let mut bold = transform;
        bold.tx += 0.65 * output_scale;
        pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, bold, None);
    }
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: (u8, u8, u8)) {
    let Some(rect) = Rect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, 255);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: (u8, u8, u8)) {
    let Some(path) = PathBuilder::from_circle(x, y, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn fill_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: (u8, u8, u8),
) {
    let right = x + width;
    let bottom = y + height;
    let radius = radius.min(width / 2.0).min(height / 2.0);
    let mut path = PathBuilder::new();
    path.move_to(x + radius, y);
    path.line_to(right - radius, y);
    path.quad_to(right, y, right, y + radius);
    path.line_to(right, bottom - radius);
    path.quad_to(right, bottom, right - radius, bottom);
    path.line_to(x + radius, bottom);
    path.quad_to(x, bottom, x, bottom - radius);
    path.line_to(x, y + radius);
    path.quad_to(x, y, x + radius, y);
    path.close();
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn is_default_ignorable(character: char) -> bool {
    matches!(
        character as u32,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xE0100..=0xE01EF
    )
}

fn unsupported_grapheme(grapheme: &str) -> bool {
    grapheme.chars().count() > 1
}

fn format_glyph_sequence(grapheme: &str) -> String {
    let codepoints = grapheme
        .chars()
        .map(|character| format!("U+{:04X}", character as u32))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{grapheme:?} ({codepoints})")
}

fn unpremultiply(pixel: &mut [u8]) {
    let alpha = u16::from(pixel[3]);
    if alpha == 0 {
        pixel[..3].fill(0);
    } else if alpha < 255 {
        for channel in &mut pixel[..3] {
            let value = (u16::from(*channel) * 255 + alpha / 2) / alpha;
            *channel = value.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::cell::{Attrs, Color};

    fn cell(character: &str, attrs: Attrs) -> EmuCell {
        EmuCell {
            ch: character.into(),
            fg: Some(Color::Rgb(220, 220, 220)),
            attrs,
            ..EmuCell::blank()
        }
    }

    #[test]
    fn repeated_renders_are_byte_identical() {
        let grid = vec![vec![cell("x", Attrs::empty())]];
        let mut renderer = GridRenderer::new(1, 1);
        let first = renderer.render(&grid, 1).unwrap();
        let second = renderer.render(&grid, 1).unwrap();
        assert_eq!(first.as_raw(), second.as_raw());
        assert_eq!(renderer.pixel_size().1 % 2, 0);
    }

    #[test]
    fn scaled_renderers_multiply_output_dimensions() {
        let standard = GridRenderer::new(80, 30);
        let hidpi = GridRenderer::with_scale(80, 30, 2);
        assert_eq!(
            hidpi.pixel_size(),
            (standard.pixel_size().0 * 2, standard.pixel_size().1 * 2)
        );
    }

    #[test]
    fn bold_and_italic_change_the_rasterized_glyph() {
        let mut renderer = GridRenderer::new(1, 1);
        let regular = renderer
            .render(&[vec![cell("M", Attrs::empty())]], 1)
            .unwrap();
        let bold = renderer.render(&[vec![cell("M", Attrs::BOLD)]], 1).unwrap();
        let italic = renderer
            .render(&[vec![cell("M", Attrs::ITALIC)]], 1)
            .unwrap();
        assert_ne!(regular.as_raw(), bold.as_raw());
        assert_ne!(regular.as_raw(), italic.as_raw());
    }

    #[test]
    fn supported_unicode_renders_and_missing_unicode_is_reported() {
        let mut renderer = GridRenderer::new(1, 1);
        renderer
            .render(&[vec![cell("é", Attrs::empty())]], 1)
            .unwrap();
        let error = renderer
            .render(&[vec![cell("\u{10fffd}", Attrs::empty())]], 1)
            .unwrap_err();
        assert!(error.to_string().contains("U+10FFFD"));
    }

    #[test]
    fn unsupported_emoji_sequences_are_reported_instead_of_misrendered() {
        let mut renderer = GridRenderer::new(2, 1);
        let error = renderer
            .render(&[vec![cell("👩‍💻", Attrs::empty())]], 2)
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("U+1F469"));
        assert!(message.contains("U+200D"));
        assert!(message.contains("U+1F4BB"));
    }

    #[test]
    fn cjk_uses_a_system_fallback_or_reports_the_missing_glyph() {
        let mut renderer = GridRenderer::new(2, 1);
        let grid = vec![vec![
            cell("界", Attrs::empty()),
            cell(CONTINUATION, Attrs::empty()),
        ]];
        if let Err(error) = renderer.render(&grid, 2) {
            assert!(error.to_string().contains("U+754C"));
        }
    }
}
