use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::super::svg;
use super::font::GlyphOutline;
use crate::profile::Rgb;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_glyph(
    pixmap: &mut Pixmap,
    glyph: &GlyphOutline,
    origin_x: f32,
    origin_y: f32,
    cell_width: f32,
    cell_height: f32,
    baseline: f32,
    color: Rgb,
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
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, transform, None);
    if glyph.synthetic_bold {
        let mut bold = transform;
        bold.tx += 0.65 * output_scale;
        pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, bold, None);
    }
}

pub(super) fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: Rgb) {
    let Some(rect) = Rect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

pub(super) fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: Rgb) {
    let Some(path) = PathBuilder::from_circle(x, y, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fill_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: Rgb,
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
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fill_top_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: Rgb,
) {
    let right = x + width;
    let bottom = y + height;
    let radius = radius.min(width / 2.0).min(height);
    let mut path = PathBuilder::new();
    path.move_to(x, y + radius);
    path.quad_to(x, y, x + radius, y);
    path.line_to(right - radius, y);
    path.quad_to(right, y, right, y + radius);
    path.line_to(right, bottom);
    path.line_to(x, bottom);
    path.close();
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

pub(super) fn is_default_ignorable(character: char) -> bool {
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

pub(super) fn unsupported_grapheme(grapheme: &str) -> bool {
    grapheme.chars().count() > 1
}

pub(super) fn format_glyph_sequence(grapheme: &str) -> String {
    let codepoints = grapheme
        .chars()
        .map(|character| format!("U+{:04X}", character as u32))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{grapheme:?} ({codepoints})")
}

pub(super) fn unpremultiply(pixel: &mut [u8]) {
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
