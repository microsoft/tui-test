use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::font::GlyphOutline;
use crate::profile::Rgb;

#[derive(Clone, Copy)]
pub(super) struct PixelBounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl PixelBounds {
    pub fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    fn dimensions(self) -> Option<(u32, u32)> {
        let width = self.right.checked_sub(self.left)?;
        let height = self.bottom.checked_sub(self.top)?;
        (width > 0 && height > 0).then_some((width, height))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_glyph(
    pixmap: &mut Pixmap,
    clip_mask: Option<&mut Mask>,
    glyph: &GlyphOutline,
    origin_x: f32,
    origin_y: f32,
    cell_width: f32,
    cell_height: f32,
    pixel_bounds: Option<PixelBounds>,
    baseline: f32,
    color: Rgb,
    font_size: f32,
    output_scale: f32,
) {
    let bounds_width = f32::from(glyph.bounds.x_max - glyph.bounds.x_min).max(1.0);
    let bounds_height = f32::from(glyph.bounds.y_max - glyph.bounds.y_min).max(1.0);
    let transform = if glyph.powerline || (glyph.fills_cell && pixel_bounds.is_some()) {
        let (origin_x, origin_y, cell_width, cell_height) = if let Some(bounds) = pixel_bounds {
            let Some((width, height)) = bounds.dimensions() else {
                return;
            };
            (
                bounds.left as f32,
                bounds.top as f32,
                width as f32,
                height as f32,
            )
        } else {
            (origin_x, origin_y, cell_width, cell_height)
        };
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
        let scale_y = font_size * output_scale / f32::from(glyph.units_per_em);
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
    if glyph.clip_to_cell {
        if let (Some(bounds), Some(clip_mask)) = (pixel_bounds, clip_mask) {
            set_clip_rect(clip_mask, bounds, 255);
            draw_transformed_glyph(
                pixmap,
                glyph,
                &paint,
                transform,
                output_scale,
                Some(clip_mask),
            );
            set_clip_rect(clip_mask, bounds, 0);
            return;
        }
    }
    draw_transformed_glyph(pixmap, glyph, &paint, transform, output_scale, None);
}

fn set_clip_rect(mask: &mut Mask, bounds: PixelBounds, value: u8) {
    let width = mask.width() as usize;
    let left = bounds.left.min(mask.width()) as usize;
    let right = bounds.right.min(mask.width()) as usize;
    let top = bounds.top.min(mask.height()) as usize;
    let bottom = bounds.bottom.min(mask.height()) as usize;
    if left >= right || top >= bottom {
        return;
    }
    let data = mask.data_mut();
    for y in top..bottom {
        data[y * width + left..y * width + right].fill(value);
    }
}

fn draw_transformed_glyph(
    pixmap: &mut Pixmap,
    glyph: &GlyphOutline,
    paint: &Paint,
    transform: Transform,
    output_scale: f32,
    mask: Option<&Mask>,
) {
    pixmap.fill_path(&glyph.path, paint, FillRule::Winding, transform, mask);
    if glyph.synthetic_bold {
        let mut bold = transform;
        bold.tx += 0.65 * output_scale;
        pixmap.fill_path(&glyph.path, paint, FillRule::Winding, bold, mask);
    }
}

pub(super) fn fill_antialiased_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Rgb,
) {
    let Some(rect) = Rect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

pub(super) fn fill_pixel_rect(
    pixmap: &mut Pixmap,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    color: Rgb,
) {
    if left >= right || top >= bottom {
        return;
    }
    let Some(rect) = Rect::from_xywh(
        left as f32,
        top as f32,
        (right - left) as f32,
        (bottom - top) as f32,
    ) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    paint.anti_alias = false;
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
pub(super) fn fill_rounded_rect_alpha(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: Rgb,
    alpha: u8,
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
    paint.set_color_rgba8(color.r, color.g, color.b, alpha);
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
