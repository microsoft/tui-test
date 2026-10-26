use tiny_skia::{FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::font::GlyphOutline;
use super::shaping::ShapedRun;
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
    font_size: f32,
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
    pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, transform, None);
    if glyph.synthetic_bold {
        let mut bold = transform;
        bold.tx += 0.65 * output_scale;
        pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, bold, None);
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

/// The one rounded rectangle every panel, shadow layer and border is cut from.
fn rounded_rect_path(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
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
    path.finish()
}

/// Draw the panel's border. Inset by half the stroke because tiny-skia, like
/// SVG, centers a stroke on its path, and a border straddling the panel edge
/// would bleed into the padding.
#[allow(clippy::too_many_arguments)]
pub(super) fn stroke_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: Rgb,
    stroke_width: f32,
) {
    let inset = stroke_width / 2.0;
    let Some(path) = rounded_rect_path(
        x + inset,
        y + inset,
        (width - stroke_width).max(0.0),
        (height - stroke_width).max(0.0),
        (radius - inset).max(0.0),
    ) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    let stroke = Stroke {
        width: stroke_width,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
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
    let Some(path) = rounded_rect_path(x, y, width, height, radius) else {
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
    let Some(path) = rounded_rect_path(x, y, width, height, radius) else {
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

/// Whether a cell grapheme holds more than one Unicode scalar value (a base
/// character plus combining marks). Both font backends shape these with one
/// face covering the full sequence; single scalars keep the legacy verbatim
/// path so recorded pixels do not change.
pub(super) fn multi_scalar_grapheme(grapheme: &str) -> bool {
    grapheme.chars().count() > 1
}

/// Whether a scalar needs a visible glyph: anything but whitespace and
/// the default-ignorable scalars the shaper handles without ink (controls
/// like CGJ, variation selectors, ZWJ). Coverage preflights and
/// missing-glyph inspection only require these scalars; the rest ride
/// along in the shaping buffer and draw nothing.
pub(super) fn needs_visible_glyph(scalar: char) -> bool {
    !scalar.is_whitespace() && !is_default_ignorable(scalar)
}

/// Whether a multi-scalar grapheme is a base character plus combining
/// marks: the sequences the shaper positions on both backends. Anything
/// else multi-scalar (ZWJ emoji, regional indicators, ...) keeps the
/// legacy report instead of a misrender. Mark detection uses the
/// maintained `unicode-properties` General Category tables the shaper
/// itself is built on — no local Unicode tables.
pub(super) fn combining_sequence(grapheme: &str) -> bool {
    let mut scalars = grapheme.chars();
    if scalars.next().is_none() {
        return false;
    }
    scalars.all(is_combining_mark)
}

/// Whether a scalar is a combining mark (General Category M: spacing,
/// nonspacing, or enclosing).
fn is_combining_mark(character: char) -> bool {
    use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
    character.general_category_group() == GeneralCategoryGroup::Mark
}

/// Draw one shaped cell run. A verbatim run draws exactly like a legacy
/// single scalar. A shaped run maps its total advance onto the cell width
/// (mirroring the legacy advance stretch) and places each glyph at its
/// shaper offset, clipped to `clip` so ink stays in the permitted region.
/// A missing clip mask draws unclipped.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_shaped_run(
    pixmap: &mut Pixmap,
    run: &ShapedRun,
    origin_x: f32,
    origin_y: f32,
    cell_width: f32,
    cell_height: f32,
    baseline: f32,
    color: Rgb,
    font_size: f32,
    output_scale: f32,
    clip: Option<&Mask>,
) {
    match run {
        ShapedRun::Empty => {}
        ShapedRun::Verbatim(outline) => {
            // A clipped single (a wide scalar at the grid edge) draws
            // through the shaped math with zero offsets: its transform
            // matches the legacy advance branch exactly, and the mask
            // keeps the ink in the permitted region. Zero-advance and
            // powerline outlines keep the legacy branches verbatim.
            if clip.is_some() && outline.advance != 0 && !outline.powerline {
                let single = ShapedRun::Shaped {
                    glyphs: vec![super::shaping::ShapedGlyph {
                        outline: Some(outline.clone()),
                        dx: 0.0,
                        dy: 0.0,
                        advance: f32::from(outline.advance),
                    }],
                    advance: f32::from(outline.advance),
                };
                draw_shaped_run(
                    pixmap,
                    &single,
                    origin_x,
                    origin_y,
                    cell_width,
                    cell_height,
                    baseline,
                    color,
                    font_size,
                    output_scale,
                    clip,
                );
                return;
            }
            draw_glyph(
                pixmap,
                outline,
                origin_x,
                origin_y,
                cell_width,
                cell_height,
                baseline,
                color,
                font_size,
                output_scale,
            );
        }
        ShapedRun::Shaped { glyphs, advance } => {
            // Advance-only entries (spaces, zero-width marks) carry no
            // outline but still move the pen; the scale comes from the
            // first drawable glyph (one face per run, so the units agree)
            // and a run with no drawable glyph paints nothing.
            let Some(first) = glyphs.iter().find_map(|glyph| glyph.outline.as_ref()) else {
                return;
            };
            let units = f32::from(first.units_per_em).max(1.0);
            let scale_y = font_size * output_scale / units;
            // Mirror the legacy advance stretch: the whole run fills the
            // cell width. A zero total advance (only zero-width glyphs)
            // draws at natural size from the cell origin instead.
            let scale_x = if *advance > 0.0 {
                cell_width / *advance
            } else {
                scale_y
            };
            let mut pen = 0.0f32;
            for glyph in glyphs {
                if let Some(outline) = &glyph.outline {
                    let shear = if outline.synthetic_italic {
                        0.2 * scale_y
                    } else {
                        0.0
                    };
                    let transform = Transform::from_row(
                        scale_x,
                        0.0,
                        shear,
                        -scale_y,
                        origin_x + pen + glyph.dx * scale_x,
                        baseline - glyph.dy * scale_y,
                    );
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, 255);
                    pixmap.fill_path(&outline.path, &paint, FillRule::Winding, transform, clip);
                    if outline.synthetic_bold {
                        let mut bold = transform;
                        bold.tx += 0.65 * output_scale;
                        pixmap.fill_path(&outline.path, &paint, FillRule::Winding, bold, clip);
                    }
                }
                pen += glyph.advance * scale_x;
            }
        }
    }
}

/// The clip mask for one pixel rectangle, reused across shaped cells.
/// Shaped runs paint through it so ink stays in the permitted region
/// (their cell, or the surviving column of a wide cell at the grid edge).
/// The cached mask is allocated once per pixmap size and cleared per cell
/// instead of allocating a full-canvas mask per shaped cell. Returns `None`
/// for an empty rectangle or when no mask can be built; the caller then
/// draws unclipped.
pub(super) fn cell_clip_reused(
    cached: &mut Option<Mask>,
    pixmap_width: u32,
    pixmap_height: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> Option<&Mask> {
    if left >= right || top >= bottom {
        return None;
    }
    let reuse = cached
        .as_ref()
        .is_some_and(|mask| mask.width() == pixmap_width && mask.height() == pixmap_height);
    let mask = if reuse {
        let mask = cached.as_mut().expect("cached mask is present");
        mask.clear();
        mask
    } else {
        cached.insert(Mask::new(pixmap_width, pixmap_height)?)
    };
    let mut builder = PathBuilder::new();
    builder.move_to(left as f32, top as f32);
    builder.line_to(right as f32, top as f32);
    builder.line_to(right as f32, bottom as f32);
    builder.line_to(left as f32, bottom as f32);
    builder.close();
    let path = builder.finish()?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

/// Name a multi-scalar grapheme the renderer cannot draw: the sequence
/// plus every codepoint, so no scalar is silently dropped from the report.
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
