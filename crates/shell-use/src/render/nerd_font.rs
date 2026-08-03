use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use ttf_parser::{Face, OutlineBuilder};

use crate::terminal::cell::EmuCell;

// Nerd Fonts Symbols v3.4.0 is MIT-licensed; see the adjacent LICENSE.
const FONT_DATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/nerd-fonts/SymbolsNerdFontMono-Regular.ttf"
));

struct Glyph {
    path: String,
    advance: u16,
    bounds: ttf_parser::Rect,
}

#[derive(Default)]
struct SvgPathBuilder {
    path: String,
}

impl OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.path, "M{x:.1},{y:.1}");
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.path, "L{x:.1},{y:.1}");
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(self.path, "Q{x1:.1},{y1:.1},{x:.1},{y:.1}");
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(self.path, "C{x1:.1},{y1:.1},{x2:.1},{y2:.1},{x:.1},{y:.1}");
    }

    fn close(&mut self) {
        self.path.push('Z');
    }
}

struct GlyphTransform {
    x: f32,
    y: f32,
    scale_x: f32,
    scale_y: f32,
}

pub(super) struct NerdFont {
    glyphs: BTreeMap<char, Glyph>,
    scale: f32,
}

impl NerdFont {
    pub fn new(rows: &[Vec<EmuCell>], font_size: f32) -> Self {
        let chars: BTreeSet<char> = rows
            .iter()
            .flatten()
            .flat_map(|cell| cell.ch.chars())
            .filter(|c| is_private_use(*c))
            .collect();
        if chars.is_empty() {
            return Self {
                glyphs: BTreeMap::new(),
                scale: 1.0,
            };
        }

        let face = Face::parse(FONT_DATA, 0).expect("bundled Nerd Font must be valid");
        let mut glyphs = BTreeMap::new();
        for c in chars {
            let Some(glyph_id) = face.glyph_index(c) else {
                continue;
            };
            let mut builder = SvgPathBuilder::default();
            let Some(bounds) = face.outline_glyph(glyph_id, &mut builder) else {
                continue;
            };
            if builder.path.is_empty() {
                continue;
            }
            glyphs.insert(
                c,
                Glyph {
                    path: builder.path,
                    advance: face
                        .glyph_hor_advance(glyph_id)
                        .unwrap_or(face.units_per_em()),
                    bounds,
                },
            );
        }

        Self {
            glyphs,
            scale: font_size / face.units_per_em() as f32,
        }
    }

    pub fn write_defs(&self, out: &mut String) {
        if self.glyphs.is_empty() {
            return;
        }
        out.push_str("<defs>");
        for (c, glyph) in &self.glyphs {
            let codepoint = *c as u32;
            let _ = write!(out, r#"<path id="nf-{codepoint:x}" d="{}"/>"#, glyph.path);
        }
        out.push_str("</defs>");
    }

    pub fn prepare_run(&self, text: &str, width: f32, cell_width: f32) -> (String, f32) {
        let mut masked = String::with_capacity(text.len());
        let mut extra_width = 0.0;
        for c in text.chars() {
            if let Some(glyph) = self.glyphs.get(&c) {
                masked.push(' ');
                if !is_powerline_separator(c) {
                    extra_width += glyph.advance as f32 * self.scale - cell_width;
                }
            } else {
                masked.push(c);
            }
        }
        let natural_width = width + extra_width;
        let x_adjust = if natural_width > 0.0 {
            width / natural_width
        } else {
            1.0
        };
        (masked, x_adjust)
    }

    pub fn write_use(
        &self,
        out: &mut String,
        c: char,
        origin: (f32, f32),
        size: (f32, f32),
        run_x_adjust: f32,
        fill: &str,
    ) {
        let Some(transform) = self.transform(c, origin, size, run_x_adjust) else {
            return;
        };
        let codepoint = c as u32;
        let _ = write!(
            out,
            r##"<use href="#nf-{codepoint:x}" transform="translate({x:.2} {y:.2}) scale({scale_x:.6} -{scale_y:.6})" fill="{fill}"/>"##,
            x = transform.x,
            y = transform.y,
            scale_x = transform.scale_x,
            scale_y = transform.scale_y,
        );
    }

    fn transform(
        &self,
        c: char,
        (cell_x, cell_y): (f32, f32),
        (cell_width, cell_height): (f32, f32),
        run_x_adjust: f32,
    ) -> Option<GlyphTransform> {
        let glyph = self.glyphs.get(&c)?;
        let bounds_width = (glyph.bounds.x_max - glyph.bounds.x_min) as f32;
        let bounds_height = (glyph.bounds.y_max - glyph.bounds.y_min) as f32;
        if is_powerline_separator(c) {
            let scale_x = cell_width / bounds_width;
            let scale_y = cell_height / bounds_height;
            Some(GlyphTransform {
                x: cell_x - glyph.bounds.x_min as f32 * scale_x,
                y: cell_y + glyph.bounds.y_max as f32 * scale_y,
                scale_x,
                scale_y,
            })
        } else {
            let scale_x = self.scale * run_x_adjust;
            let rendered_advance = glyph.advance as f32 * scale_x;
            let rendered_height = bounds_height * self.scale;
            Some(GlyphTransform {
                x: cell_x + (cell_width - rendered_advance) / 2.0,
                y: cell_y
                    + (cell_height - rendered_height) / 2.0
                    + glyph.bounds.y_max as f32 * self.scale,
                scale_x,
                scale_y: self.scale,
            })
        }
    }
}

fn is_private_use(c: char) -> bool {
    matches!(
        c as u32,
        0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd
    )
}

fn is_powerline_separator(c: char) -> bool {
    matches!(c as u32, 0xe0b0..=0xe0d4)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cell(ch: &str) -> EmuCell {
        EmuCell {
            ch: ch.into(),
            ..EmuCell::blank()
        }
    }

    #[test]
    fn powerline_glyphs_fill_the_terminal_cell() {
        let rows = vec![vec![cell("\u{e0b0}")]];
        let font = NerdFont::new(&rows, 17.0);
        let transform = font
            .transform('\u{e0b0}', (15.0, 38.0), (10.0, 21.0), 1.0)
            .unwrap();
        let glyph = &font.glyphs[&'\u{e0b0}'];

        let left = transform.x + glyph.bounds.x_min as f32 * transform.scale_x;
        let right = transform.x + glyph.bounds.x_max as f32 * transform.scale_x;
        let top = transform.y - glyph.bounds.y_max as f32 * transform.scale_y;
        let bottom = transform.y - glyph.bounds.y_min as f32 * transform.scale_y;
        assert!((left - 15.0).abs() < 0.001);
        assert!((right - 25.0).abs() < 0.001);
        assert!((top - 38.0).abs() < 0.001);
        assert!((bottom - 59.0).abs() < 0.001);
    }

    #[test]
    fn regular_glyphs_are_vertically_centered() {
        let rows = vec![vec![cell("\u{f115}")]];
        let font = NerdFont::new(&rows, 17.0);
        let transform = font
            .transform('\u{f115}', (15.0, 38.0), (10.0, 21.0), 1.0)
            .unwrap();
        let glyph = &font.glyphs[&'\u{f115}'];

        let top = transform.y - glyph.bounds.y_max as f32 * transform.scale_y;
        let bottom = transform.y - glyph.bounds.y_min as f32 * transform.scale_y;
        assert!(((top - 38.0) - (59.0 - bottom)).abs() < 0.001);
    }

    #[test]
    fn powerline_glyphs_do_not_adjust_run_width() {
        let rows = vec![vec![cell("\u{e0b0}")]];
        let font = NerdFont::new(&rows, 17.0);
        let (_, x_adjust) = font.prepare_run("\u{e0b0}", 10.0, 10.0);
        assert_eq!(x_adjust, 1.0);
    }

    #[test]
    fn bundled_font_includes_its_mit_license() {
        let license = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/nerd-fonts/LICENSE"
        ));
        assert!(license.starts_with("The MIT License (MIT)"));
        assert!(license.contains("Copyright (c) 2014 Ryan L McIntyre"));
    }
}
