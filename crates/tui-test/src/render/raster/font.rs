use std::collections::HashMap;

use tiny_skia::{Path, PathBuilder};
use ttf_parser::{Face, OutlineBuilder};

use super::super::{font as catalog, nerd_font, pinned};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GlyphKey {
    pub character: char,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone)]
pub(super) struct GlyphOutline {
    pub path: Path,
    pub bounds: ttf_parser::Rect,
    pub advance: u16,
    pub units_per_em: u16,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
    pub powerline: bool,
}

pub(super) struct FontSystem {
    catalog: std::sync::Arc<catalog::Catalog>,
    glyphs: HashMap<GlyphKey, Option<GlyphOutline>>,
}

impl FontSystem {
    /// Built from the style's fonts, so a recording draws with the family the
    /// config named rather than whatever the catalog ranked first.
    pub fn new(font: &crate::render::style::FontFamilies) -> anyhow::Result<Self> {
        Ok(Self {
            catalog: catalog::catalog_for(font)?,
            glyphs: HashMap::new(),
        })
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

/// Which font store backs one render: the recorded system-font catalog or
/// the explicitly pinned bytes. The pinned side never touches the catalog
/// cache, the system font database, or ambient font-selection variables.
pub(super) enum FontBackend {
    System(FontSystem),
    Pinned(PinnedFontSystem),
}

impl FontBackend {
    pub fn system(font: &crate::render::style::FontFamilies) -> anyhow::Result<Self> {
        Ok(Self::System(FontSystem::new(font)?))
    }

    pub fn pinned(set: &pinned::PinnedFontSet) -> Result<Self, pinned::PinnedFontError> {
        Ok(Self::Pinned(PinnedFontSystem::new(set)?))
    }

    pub fn is_pinned(&self) -> bool {
        matches!(self, Self::Pinned(_))
    }

    /// Resolve one glyph. The system side keeps its recorded fallback: a
    /// missing glyph is `None` for the caller to collect. The pinned side
    /// returns a typed error instead: a missing style slot or a missing
    /// glyph never falls back to a host font.
    pub fn resolve_strict(
        &mut self,
        key: GlyphKey,
    ) -> Result<Option<&GlyphOutline>, pinned::PinnedFontError> {
        match self {
            Self::System(system) => Ok(system.resolve(key)),
            Self::Pinned(pinned) => pinned.resolve(key).map(Some),
        }
    }
}

/// Glyph outlines drawn only from explicitly pinned bytes.
///
/// The faces hold caller-supplied bytes with their collection indices, in
/// fallback order. No `fontdb` database is built here on purpose: the
/// catalog's job — scanning system fonts, ranking families, reading
/// `TUI_TEST_RECORDING_FONT_FAMILIES` — is exactly what the pinned policy
/// must not do. Each render owns its instance, so no system-font cache
/// entry can serve a pinned request by construction. The recorded
/// [`pinned::PinnedCacheKey`] is the identity a shared cache would use.
pub(super) struct PinnedFontSystem {
    faces: Vec<PinnedCatalogFace>,
    synthetic: bool,
    glyphs: HashMap<GlyphKey, GlyphOutline>,
}

struct PinnedCatalogFace {
    bytes: Vec<u8>,
    index: u32,
    bold: bool,
    italic: bool,
}

impl PinnedFontSystem {
    pub fn new(set: &pinned::PinnedFontSet) -> Result<Self, pinned::PinnedFontError> {
        if set.faces().is_empty() {
            return Err(pinned::PinnedFontError::EmptySet);
        }
        let mut faces = Vec::with_capacity(set.faces().len());
        for face in set.faces() {
            // The public constructor already parsed this index; re-check
            // here so a render never trusts stale validation.
            if ttf_parser::Face::parse(face.bytes(), face.index()).is_err() {
                return Err(pinned::PinnedFontError::InvalidFace {
                    detail: format!("pinned face index {} does not parse", face.index()),
                });
            }
            faces.push(PinnedCatalogFace {
                bytes: face.bytes().to_vec(),
                index: face.index(),
                bold: face.bold(),
                italic: face.italic(),
            });
        }
        Ok(Self {
            faces,
            synthetic: set.allow_synthetic_style(),
            glyphs: HashMap::new(),
        })
    }

    /// Resolve one glyph from the specified faces in fallback order.
    ///
    /// Only faces serving the requested style slot are candidates: an
    /// empty slot is a missing-face error, and a glyph no candidate holds
    /// is a missing-glyph error. Synthetic bold or italic applies only
    /// when the set selected it.
    pub fn resolve(&mut self, key: GlyphKey) -> Result<&GlyphOutline, pinned::PinnedFontError> {
        if !self.glyphs.contains_key(&key) {
            let positions: Vec<usize> = self
                .faces
                .iter()
                .enumerate()
                .filter(|(_, face)| face.bold == key.bold && face.italic == key.italic)
                .map(|(position, _)| position)
                .collect();
            if positions.is_empty() {
                return Err(pinned::PinnedFontError::MissingFace {
                    bold: key.bold,
                    italic: key.italic,
                });
            }
            let mut found = None;
            for position in positions {
                if let Some(outline) = self.load(position, key) {
                    found = Some(outline);
                    break;
                }
            }
            match found {
                Some(outline) => {
                    self.glyphs.insert(key, outline);
                }
                None => {
                    return Err(pinned::PinnedFontError::MissingGlyph {
                        character: key.character,
                        bold: key.bold,
                        italic: key.italic,
                    });
                }
            }
        }
        // The outline was just cached above when it was absent; the error
        // arm only guards the map lookup without panicking.
        match self.glyphs.get(&key) {
            Some(outline) => Ok(outline),
            None => Err(pinned::PinnedFontError::MissingGlyph {
                character: key.character,
                bold: key.bold,
                italic: key.italic,
            }),
        }
    }

    fn load(&self, position: usize, key: GlyphKey) -> Option<GlyphOutline> {
        let face = self.faces.get(position)?;
        let parsed = ttf_parser::Face::parse(&face.bytes, face.index).ok()?;
        let glyph_id = parsed.glyph_index(key.character)?;
        let mut builder = TinyPathBuilder::default();
        let bounds = parsed.outline_glyph(glyph_id, &mut builder)?;
        let path = builder.finish()?;
        // Style slots are caller-selected, so only an explicit synthetic
        // selection emboldens or slants; otherwise the bytes draw verbatim.
        let synthetic_bold =
            self.synthetic && key.bold && !is_bold_weight(parsed.weight().to_number());
        let synthetic_italic = self.synthetic && key.italic && !is_italic_style(&parsed);
        Some(GlyphOutline {
            path,
            bounds,
            advance: parsed
                .glyph_hor_advance(glyph_id)
                .unwrap_or(parsed.units_per_em()),
            units_per_em: parsed.units_per_em(),
            synthetic_bold,
            synthetic_italic,
            powerline: nerd_font::is_powerline_separator(key.character),
        })
    }
}

fn is_bold_weight(weight: u16) -> bool {
    weight >= fontdb::Weight::SEMIBOLD.0
}

fn is_italic_style(face: &ttf_parser::Face<'_>) -> bool {
    face.is_italic() || face.is_oblique()
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
