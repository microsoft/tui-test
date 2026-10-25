//! One shaping path for pinned cell text, cursor repaint, and titles.
//!
//! A cell holds one grapheme: one displayed unit that can contain more than
//! one Unicode scalar value (a base character plus combining marks). The
//! shaper (rustybuzz) positions each grapheme with a single face selected
//! from the pinned set in fallback order: the first face in the requested
//! style slot whose bytes map every scalar to a real glyph. A combining
//! sequence is never divided across unrelated faces. When no single face
//! covers the full grapheme, the result is a typed missing-glyph error that
//! names the scalar, never a silent scalar drop and never a `.notdef` box.
//!
//! Each cell is shaped with its own shaper call, so OpenType features can
//! only merge glyphs inside one cell: ligatures never reach across cell
//! boundaries. Glyph positions stay in font units here; the caller scales
//! them into the cell geometry the snapshot carries, so terminal column
//! widths do not change after shaping. Titles are split into grapheme
//! clusters with the shaper's own cluster indices (no local Unicode tables)
//! and each cluster draws through the same per-grapheme selection; title
//! shaping disables the ligature features so one cluster keeps its own
//! columns, exactly like the per-character layout it replaces.

use super::draw::{format_glyph_sequence, is_default_ignorable};
use super::font::{FontSystem, GlyphKey, GlyphOutline, PinnedFontSystem};
use crate::render::pinned::PinnedFontError;

/// rustybuzz 0.20 reports advances and offsets in font units: its face
/// carries no scale factor and the fallback shaper stores
/// `glyph_h_advance` verbatim (verified: the `e` advance below equals the
/// `hmtx` advance). There is no 26.6 divisor.
const POSITION_SCALE: f32 = 1.0;

/// One glyph the shaper placed: its outline, the pen offset in font units
/// (right and up positive) from the grapheme origin, and the pen advance
/// in font units the next glyph starts after.
pub(super) struct ShapedGlyph {
    pub outline: GlyphOutline,
    pub dx: f32,
    pub dy: f32,
    pub advance: f32,
}

/// One cell's worth of shaped text.
pub(super) enum ShapedRun {
    /// Nothing to draw: the text is empty or only whitespace and default
    /// ignorables.
    Empty,
    /// One cached outline drawn exactly like a legacy single scalar, so
    /// single-scalar cells keep their recorded pixels.
    Verbatim(GlyphOutline),
    /// Positioned glyphs from one face. `advance` is the total pen advance
    /// in font units; the caller maps it onto the cell width.
    Shaped {
        glyphs: Vec<ShapedGlyph>,
        advance: f32,
    },
}

impl ShapedRun {
    /// Whether the run went through the shaper (as opposed to the verbatim
    /// single-scalar path). The renderer records these graphemes as glyph
    /// evidence.
    pub fn is_shaped(&self) -> bool {
        matches!(self, Self::Shaped { .. })
    }
}

/// Shape one pinned cell grapheme with one face.
///
/// Empty, whitespace-only, and ignorable-only text draws nothing.
/// A single scalar resolves through the pinned cache and draws verbatim.
/// A multi-scalar grapheme is shaped with the first face in the requested
/// style slot covering every scalar; a grapheme no single face covers is a
/// missing-glyph error naming the scalar. Cells shape with the default
/// OpenType features.
pub(super) fn shape_pinned_text(
    fonts: &mut PinnedFontSystem,
    text: &str,
    bold: bool,
    italic: bool,
) -> Result<ShapedRun, PinnedFontError> {
    shape_pinned_text_with_features(fonts, text, bold, italic, &[])
}

/// Shape one grapheme with explicit OpenType features. Titles pass
/// [`NO_LIGATURES`] so one cluster keeps its own columns; cells use the
/// shaper defaults so the font positions combining marks as designed.
fn shape_pinned_text_with_features(
    fonts: &mut PinnedFontSystem,
    text: &str,
    bold: bool,
    italic: bool,
    features: &[rustybuzz::Feature],
) -> Result<ShapedRun, PinnedFontError> {
    let mut scalars = text.chars();
    let Some(first) = scalars.next() else {
        return Ok(ShapedRun::Empty);
    };
    if scalars.next().is_none() {
        if first.is_whitespace() || is_default_ignorable(first) {
            return Ok(ShapedRun::Empty);
        }
        let key = GlyphKey {
            character: first,
            bold,
            italic,
        };
        return Ok(ShapedRun::Verbatim(fonts.resolve(key)?.clone()));
    }
    if text
        .chars()
        .all(|scalar| scalar.is_whitespace() || is_default_ignorable(scalar))
    {
        return Ok(ShapedRun::Empty);
    }
    let positions = fonts.style_positions(bold, italic);
    if positions.is_empty() {
        return Err(PinnedFontError::MissingFace { bold, italic });
    }
    let mut selected = None;
    for &position in &positions {
        if text.chars().all(|scalar| fonts.has_glyph(position, scalar)) {
            selected = Some(position);
            break;
        }
    }
    let Some(position) = selected else {
        return Err(missing_glyph_for(text, &positions, fonts, bold, italic));
    };
    shape_with_face(fonts, position, text, bold, italic, features)
}

/// Shape one system-backend cell grapheme with one catalog face.
///
/// The run geometry is shared with the pinned path: a [`ShapedRun`] the
/// caller draws through the same positioned run, so cells and cursor
/// repaint shape equally on both backends. Only the selection policy
/// differs: the first catalog candidate for the style slot covering every
/// scalar wins, keeping the recorded host fallback order, and a grapheme
/// no single face can draw reports the full sequence for the caller's
/// missing-glyph collection instead of a typed error. Empty,
/// whitespace-only, and ignorable-only text draws nothing. Single-scalar
/// text never reaches here: the caller keeps the legacy verbatim path so
/// recorded pixels do not change.
pub(super) fn shape_system_text(
    fonts: &FontSystem,
    text: &str,
    bold: bool,
    italic: bool,
) -> Result<ShapedRun, String> {
    debug_assert!(
        text.chars().count() > 1,
        "single scalars keep the legacy verbatim path"
    );
    if text
        .chars()
        .all(|scalar| scalar.is_whitespace() || is_default_ignorable(scalar))
    {
        return Ok(ShapedRun::Empty);
    }
    let base = text.chars().next().unwrap_or('\0');
    for id in fonts.style_candidates(bold, italic, base) {
        if !text.chars().all(|scalar| fonts.face_has_glyph(id, scalar)) {
            continue;
        }
        let shaped = fonts.with_face_data(id, |data, index| {
            shape_system_data(fonts, id, data, index, text, bold, italic)
        });
        match shaped.flatten() {
            Some(result) => return result,
            // Readable coverage but unreadable bytes: try the next face
            // covering the full grapheme rather than failing here.
            None => continue,
        }
    }
    Err(format_glyph_sequence(text))
}

/// Shape `text` with the catalog face `id`, already known to cover every
/// scalar. `None` means the face bytes do not parse, so the caller tries
/// the next covering face. A `.notdef` id the shaper still emits is a
/// missing sequence naming every scalar, never tofu. Glyphs with no
/// outline (spaces, zero-width marks) keep their advance but draw nothing;
/// when no glyph draws anything visible, the sequence is reported instead
/// of a blank success, so a visible scalar is never silently dropped.
fn shape_system_data(
    fonts: &FontSystem,
    id: fontdb::ID,
    data: &[u8],
    index: u32,
    text: &str,
    bold: bool,
    italic: bool,
) -> Option<Result<ShapedRun, String>> {
    let face = rustybuzz::Face::from_slice(data, index)?;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let infos = shaped.glyph_infos();
    let advances = shaped.glyph_positions();
    let mut glyphs = Vec::new();
    let mut advance = 0.0f32;
    for (info, placement) in infos.iter().zip(advances.iter()) {
        advance += placement.x_advance as f32 / POSITION_SCALE;
        let glyph_id = u16::try_from(info.glyph_id).ok()?;
        if glyph_id == 0 {
            return Some(Err(format_glyph_sequence(text)));
        }
        if let Some(outline) = fonts.outline_for_shaped_glyph(
            id,
            ttf_parser::GlyphId(glyph_id),
            bold,
            italic,
        ) {
            glyphs.push(ShapedGlyph {
                outline,
                dx: placement.x_offset as f32 / POSITION_SCALE,
                dy: placement.y_offset as f32 / POSITION_SCALE,
                advance: placement.x_advance as f32 / POSITION_SCALE,
            });
        }
    }
    if glyphs.is_empty()
        && text
            .chars()
            .any(|scalar| !scalar.is_whitespace() && !is_default_ignorable(scalar))
    {
        return Some(Err(format_glyph_sequence(text)));
    }
    Some(Ok(ShapedRun::Shaped { glyphs, advance }))
}

/// Name the scalar a missing-glyph error reports. A scalar no candidate
/// face holds is a genuine hole and is named first; otherwise the grapheme
/// is divided across faces, and the first scalar the base face lacks is
/// named. Both keep the full grapheme in the caller's evidence.
fn missing_glyph_for(
    text: &str,
    positions: &[usize],
    fonts: &PinnedFontSystem,
    bold: bool,
    italic: bool,
) -> PinnedFontError {
    for scalar in text.chars() {
        if positions
            .iter()
            .all(|&position| !fonts.has_glyph(position, scalar))
        {
            return PinnedFontError::MissingGlyph {
                character: scalar,
                bold,
                italic,
            };
        }
    }
    let base = text.chars().next().unwrap_or('\0');
    let character = positions
        .iter()
        .find(|&&position| fonts.has_glyph(position, base))
        .and_then(|&position| {
            text.chars()
                .find(|&scalar| !fonts.has_glyph(position, scalar))
        })
        .unwrap_or(base);
    PinnedFontError::MissingGlyph {
        character,
        bold,
        italic,
    }
}

/// Shape `text` with the pinned face at `position`. Every scalar is covered
/// by construction; a `.notdef` id the shaper still emits (for example a
/// variation selector the font maps but cannot shape) is a missing-glyph
/// error naming the scalar at that glyph's cluster, never tofu. Glyphs with
/// no outline (spaces, zero-width marks) keep their advance but draw
/// nothing, so no positioned scalar is silently dropped.
fn shape_with_face(
    fonts: &PinnedFontSystem,
    position: usize,
    text: &str,
    bold: bool,
    italic: bool,
    features: &[rustybuzz::Feature],
) -> Result<ShapedRun, PinnedFontError> {
    let Some((bytes, index)) = fonts.face_bytes(position) else {
        return Err(PinnedFontError::InvalidFace {
            detail: format!("pinned face {position} has no bytes for shaping"),
        });
    };
    let Some(face) = rustybuzz::Face::from_slice(bytes, index) else {
        return Err(PinnedFontError::InvalidFace {
            detail: format!("pinned face {index} does not parse for shaping"),
        });
    };
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let shaped = rustybuzz::shape(&face, features, buffer);
    let infos = shaped.glyph_infos();
    let advances = shaped.glyph_positions();
    let mut glyphs = Vec::new();
    let mut advance = 0.0f32;
    for (info, placement) in infos.iter().zip(advances.iter()) {
        advance += placement.x_advance as f32 / POSITION_SCALE;
        let glyph_id = u16::try_from(info.glyph_id).map_err(|_| PinnedFontError::InvalidFace {
            detail: format!("shaper glyph id {} does not fit u16", info.glyph_id),
        })?;
        if glyph_id == 0 {
            return Err(PinnedFontError::MissingGlyph {
                character: scalar_at_cluster(text, info.cluster),
                bold,
                italic,
            });
        }
        if let Some(outline) =
            fonts.outline_for_glyph(position, ttf_parser::GlyphId(glyph_id), bold, italic)
        {
            glyphs.push(ShapedGlyph {
                outline,
                dx: placement.x_offset as f32 / POSITION_SCALE,
                dy: placement.y_offset as f32 / POSITION_SCALE,
                advance: placement.x_advance as f32 / POSITION_SCALE,
            });
        }
    }
    Ok(ShapedRun::Shaped { glyphs, advance })
}

/// The scalar at a shaper cluster byte offset, for missing-glyph errors.
/// Falls back to the first scalar when the offset is not a boundary.
fn scalar_at_cluster(text: &str, cluster: u32) -> char {
    let offset = (cluster as usize).min(text.len());
    text.get(offset..)
        .and_then(|tail| tail.chars().next())
        .or_else(|| text.chars().next())
        .unwrap_or('\0')
}

/// One title grapheme cluster: a byte range plus the terminal columns it
/// keeps. Columns come from the terminal width function, so shaping never
/// changes the title's total width or centering.
pub(super) struct TitleCluster<'a> {
    pub text: &'a str,
    pub columns: u32,
}

/// Split a title into grapheme clusters with the shaper's own cluster
/// indices, shaped once with the first bold-slot face. No local Unicode
/// tables: boundaries come from real shaping. The ligature features stay
/// off here and in [`shape_title_cluster`], so one cluster keeps its own
/// columns exactly like the per-character layout this replaces.
pub(super) fn title_clusters<'a>(
    fonts: &PinnedFontSystem,
    title: &'a str,
) -> Result<Vec<TitleCluster<'a>>, PinnedFontError> {
    if title.is_empty() {
        return Ok(Vec::new());
    }
    let positions = fonts.style_positions(true, false);
    let Some(&first) = positions.first() else {
        return Err(PinnedFontError::MissingFace {
            bold: true,
            italic: false,
        });
    };
    let Some((bytes, index)) = fonts.face_bytes(first) else {
        return Err(PinnedFontError::InvalidFace {
            detail: format!("pinned face {first} has no bytes for shaping"),
        });
    };
    let Some(face) = rustybuzz::Face::from_slice(bytes, index) else {
        return Err(PinnedFontError::InvalidFace {
            detail: format!("pinned face {index} does not parse for shaping"),
        });
    };
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(title);
    let shaped = rustybuzz::shape(&face, &NO_LIGATURES, buffer);
    let mut bounds: Vec<usize> = shaped
        .glyph_infos()
        .iter()
        .map(|info| (info.cluster as usize).min(title.len()))
        .collect();
    bounds.push(0);
    bounds.push(title.len());
    bounds.sort_unstable();
    bounds.dedup();
    let mut clusters = Vec::new();
    for pair in bounds.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let Some(text) = title.get(start..end) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let columns = crate::terminal::cell::display_width(text).max(1) as u32;
        clusters.push(TitleCluster { text, columns });
    }
    Ok(clusters)
}

/// Shape one title cluster with the same per-grapheme selection as cells.
/// Titles always draw in the bold slot, matching the title renderer.
pub(super) fn shape_title_cluster(
    fonts: &mut PinnedFontSystem,
    cluster: &str,
) -> Result<ShapedRun, PinnedFontError> {
    shape_pinned_text_with_features(fonts, cluster, true, false, &NO_LIGATURES)
}

/// Probe the raw shaper output for one grapheme: glyph ids, advances,
/// and offsets in font units. The acceptance tests below pin the
/// corpus-relevant values.
#[cfg(test)]
pub(super) fn probe_shape(bytes: &[u8], index: u32, text: &str) -> Vec<(u32, f32, f32, f32)> {
    let face = rustybuzz::Face::from_slice(bytes, index).expect("probe face parses");
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions().iter())
        .map(|(info, placement)| {
            (
                info.glyph_id,
                placement.x_advance as f32 / POSITION_SCALE,
                placement.x_offset as f32 / POSITION_SCALE,
                placement.y_offset as f32 / POSITION_SCALE,
            )
        })
        .collect()
}

/// Ligature features off for titles: shaping must not merge clusters that
/// the column layout counts separately.
const NO_LIGATURES: [rustybuzz::Feature; 4] = [
    rustybuzz::Feature {
        tag: ttf_parser::Tag::from_bytes(b"liga"),
        value: 0,
        start: 0,
        end: u32::MAX,
    },
    rustybuzz::Feature {
        tag: ttf_parser::Tag::from_bytes(b"clig"),
        value: 0,
        start: 0,
        end: u32::MAX,
    },
    rustybuzz::Feature {
        tag: ttf_parser::Tag::from_bytes(b"dlig"),
        value: 0,
        start: 0,
        end: u32::MAX,
    },
    rustybuzz::Feature {
        tag: ttf_parser::Tag::from_bytes(b"calt"),
        value: 0,
        start: 0,
        end: u32::MAX,
    },
];

#[cfg(test)]
mod tests {
    use super::super::font::PinnedFontSystem;
    use super::*;
    use crate::render::pinned::{PinnedFace, PinnedFontSet};

    fn regular_bytes() -> &'static [u8] {
        include_bytes!("../../../assets/jetbrains-mono/JetBrainsMono-Regular.ttf")
    }

    fn regular_system() -> PinnedFontSystem {
        let face = PinnedFace::new(regular_bytes().to_vec(), 0, false, false)
            .expect("bundled face parses");
        let set = PinnedFontSet::new(vec![face]).expect("set builds");
        PinnedFontSystem::new(&set).expect("system builds")
    }

    /// The shaper composes `e` plus U+0301 through the font's `ccmp` table
    /// into one glyph: one face, no `.notdef`, and the advance the font
    /// table reports for that glyph.
    #[test]
    fn e_acute_composes_to_one_glyph() {
        let probe = probe_shape(regular_bytes(), 0, "e\u{301}");
        assert_eq!(probe.len(), 1);
        let (glyph_id, advance, dx, dy) = probe[0];
        assert_ne!(glyph_id, 0, "no .notdef from the shaper");
        assert_eq!((dx, dy), (0.0, 0.0));
        let parsed = ttf_parser::Face::parse(regular_bytes(), 0).expect("bundled face parses");
        let glyph_id = ttf_parser::GlyphId(u16::try_from(glyph_id).expect("glyph id fits u16"));
        assert_eq!(
            advance,
            f32::from(parsed.glyph_hor_advance(glyph_id).unwrap_or(0)),
            "shaped advance is the font advance in font units"
        );
        assert!(advance > 0.0);
    }

    /// The cell run carries that composed glyph with a positive advance so
    /// the caller maps it onto the cell width.
    #[test]
    fn e_acute_run_positions_composed_glyph() {
        let mut system = regular_system();
        let run = shape_pinned_text(&mut system, "e\u{301}", false, false).expect("e-acute shapes");
        match &run {
            ShapedRun::Shaped { glyphs, advance } => {
                assert_eq!(glyphs.len(), 1);
                assert!(*advance > 0.0);
                assert_eq!(glyphs[0].advance, *advance);
                assert_eq!((glyphs[0].dx, glyphs[0].dy), (0.0, 0.0));
            }
            other => panic!("expected a shaped run, got shaped={}", other.is_shaped()),
        }
    }
}
