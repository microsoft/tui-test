use std::collections::HashMap;

use tiny_skia::{Path, PathBuilder};
use ttf_parser::{Face, OutlineBuilder};

use super::super::{font as catalog, nerd_font};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GlyphKey {
    pub character: char,
    pub bold: bool,
    pub italic: bool,
}

pub(super) struct GlyphOutline {
    pub path: Path,
    pub bounds: ttf_parser::Rect,
    pub advance: u16,
    pub units_per_em: u16,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
    pub powerline: bool,
    pub fills_cell: bool,
    pub clip_to_cell: bool,
}

pub(super) struct FontSystem {
    catalog: &'static catalog::Catalog,
    glyphs: HashMap<GlyphKey, Option<GlyphOutline>>,
}

impl FontSystem {
    pub fn new() -> Self {
        Self {
            catalog: catalog::catalog(),
            glyphs: HashMap::new(),
        }
    }

    pub fn resolve(&mut self, key: GlyphKey) -> Option<&GlyphOutline> {
        if !self.glyphs.contains_key(&key) {
            let glyph = self.load(key);
            self.glyphs.insert(key, glyph);
        }
        self.glyphs.get(&key).and_then(Option::as_ref)
    }

    fn load(&self, key: GlyphKey) -> Option<GlyphOutline> {
        for id in self.catalog.candidates(key.bold, key.italic, key.character) {
            let info = self.catalog.database.face(id)?;
            let synthetic_bold = key.bold && !is_bold_face(info);
            let synthetic_italic = key.italic && info.style == fontdb::Style::Normal;
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
                        synthetic_bold,
                        synthetic_italic,
                        powerline: nerd_font::is_powerline_separator(key.character),
                        fills_cell: is_full_cell_block(key.character),
                        clip_to_cell: nerd_font::is_private_use(key.character)
                            || is_block_element(key.character),
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

fn is_block_element(character: char) -> bool {
    matches!(character as u32, 0x2580..=0x259f)
}

fn is_full_cell_block(character: char) -> bool {
    matches!(character as u32, 0x2588 | 0x2591..=0x2593)
}

fn is_bold_face(face: &fontdb::FaceInfo) -> bool {
    face.weight.0 >= fontdb::Weight::SEMIBOLD.0
        || face.post_script_name.to_ascii_lowercase().contains("bold")
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
