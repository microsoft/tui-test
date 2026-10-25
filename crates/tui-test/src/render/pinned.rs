//! Explicit pinned-font policy for strict offline rendering.
//!
//! A [`PinnedFontSet`] names the exact font bytes a render may use: each
//! face carries its bytes, its index inside those bytes, and the style slot
//! it serves. The order of the faces is the fallback order. Under the
//! pinned policy the rasterizer loads only these bytes: it never scans
//! system fonts and never reads ambient font-selection variables such as
//! `TUI_TEST_RECORDING_FONT_FAMILIES`.
//!
//! The renderer hashes the bytes it loaded and reports them, with face
//! indices, styles, and fallback order, in the versioned [`RenderContract`]
//! attached to each strict image. A requested style slot with no face, or a
//! grapheme no specified face contains, is a typed [`PinnedFontError`]:
//! the renderer never substitutes a host font, a replacement glyph, or an
//! unselected synthetic style. Full multi-scalar shaping arrives in a later
//! task; here the renderer stops with an error for sequences it cannot
//! shape.
//!
//! ```
//! use tui_test::{PinnedFace, PinnedFontSet};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let regular = PinnedFace::new(
//!     include_bytes!("../../assets/jetbrains-mono/JetBrainsMono-Regular.ttf").to_vec(),
//!     0,
//!     false,
//!     false,
//! )?;
//! let set = PinnedFontSet::new(vec![regular])?;
//! assert_eq!(set.faces().len(), 1);
//! assert!(!set.allow_synthetic_style());
//! # Ok(())
//! # }
//! ```

use sha2::{Digest, Sha256};

/// Schema version of [`RenderContract`].
pub const RENDER_CONTRACT_VERSION: u32 = 1;

/// Raster policy name recorded when no pinned fonts are selected.
pub const RASTER_POLICY_SYSTEM_V1: &str = "system-v1";

/// Raster policy name recorded for pinned-font renders.
pub const RASTER_POLICY_PINNED_V1: &str = "pinned-v1";

/// Largest accepted pinned face: the total custom-font budget the live
/// rasterizer already enforces, applied per face at the public boundary.
const MAX_PINNED_FACE_BYTES: usize = 64 * 1024 * 1024;

/// One explicitly selected font face: bytes plus the style slot it serves.
///
/// The `index` selects the face inside the bytes (zero for a plain TTF,
/// the collection index for a TTC). `bold` and `italic` name the requested
/// style slot the face serves; the bytes are authoritative for the outlines
/// and are never restyled unless the set explicitly allows synthetic
/// styles.
#[derive(Debug, Clone)]
pub struct PinnedFace {
    bytes: Vec<u8>,
    index: u32,
    bold: bool,
    italic: bool,
}

impl PinnedFace {
    /// Select one face. Rejects empty or oversized bytes and bytes whose
    /// selected index does not parse, before any render reads them.
    pub fn new(
        bytes: Vec<u8>,
        index: u32,
        bold: bool,
        italic: bool,
    ) -> Result<Self, PinnedFontError> {
        if bytes.len() > MAX_PINNED_FACE_BYTES {
            return Err(PinnedFontError::FaceTooLarge {
                bytes: bytes.len(),
                limit: MAX_PINNED_FACE_BYTES as u64,
            });
        }
        if bytes.is_empty() {
            return Err(PinnedFontError::InvalidFace {
                detail: "face bytes are empty".to_string(),
            });
        }
        if let Err(error) = ttf_parser::Face::parse(&bytes, index) {
            return Err(PinnedFontError::InvalidFace {
                detail: format!("face index {index} does not parse: {error}"),
            });
        }
        Ok(Self {
            bytes,
            index,
            bold,
            italic,
        })
    }

    /// The selected font bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The face index inside the bytes.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Whether this face serves the bold style slot.
    pub fn bold(&self) -> bool {
        self.bold
    }

    /// Whether this face serves the italic style slot.
    pub fn italic(&self) -> bool {
        self.italic
    }

    /// SHA-256 over the style slot, face index, and bytes of this face.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"pinned-face-v1");
        hasher.update([u8::from(self.bold), u8::from(self.italic)]);
        hasher.update(self.index.to_le_bytes());
        hasher.update(self.bytes.len().to_le_bytes());
        hasher.update(&self.bytes);
        format!("{:x}", hasher.finalize())
    }
}

/// Explicit pinned fonts for one offline render.
///
/// The faces are tried in order for a grapheme whose style slot has more
/// than one face; the order is the fallback order. Synthetic bold or
/// italic is off unless [`PinnedFontSet::with_synthetic_style`] selects it.
#[derive(Debug, Clone)]
pub struct PinnedFontSet {
    faces: Vec<PinnedFace>,
    allow_synthetic_style: bool,
}

impl PinnedFontSet {
    /// Select the pinned faces. An empty set is an error: pinned rendering
    /// never falls back to host fonts.
    pub fn new(faces: Vec<PinnedFace>) -> Result<Self, PinnedFontError> {
        if faces.is_empty() {
            return Err(PinnedFontError::EmptySet);
        }
        Ok(Self {
            faces,
            allow_synthetic_style: false,
        })
    }

    /// Allow emboldened or slanted rendering when the bytes lack the
    /// requested style. Off by default: without this selection a requested
    /// style slot with no face is a missing-face error.
    pub fn with_synthetic_style(mut self, allow: bool) -> Self {
        self.allow_synthetic_style = allow;
        self
    }

    /// The pinned faces in fallback order.
    pub fn faces(&self) -> &[PinnedFace] {
        &self.faces
    }

    /// Whether synthetic bold or italic is selected.
    pub fn allow_synthetic_style(&self) -> bool {
        self.allow_synthetic_style
    }

    /// SHA-256 over the policy tag, the synthetic-style selection, and each
    /// face digest in fallback order. System fonts never contribute.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"pinned-set-v1");
        hasher.update([u8::from(self.allow_synthetic_style)]);
        for face in &self.faces {
            hasher.update(face.digest().as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

/// A pinned-font failure. Every variant names what is missing and states
/// that no host font, replacement glyph, or unselected synthetic style is
/// substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedFontError {
    /// The set names no faces, so no pinned render can start.
    EmptySet,
    /// One face exceeds the per-face byte budget.
    FaceTooLarge {
        /// The rejected byte count.
        bytes: usize,
        /// The accepted limit.
        limit: u64,
    },
    /// The selected bytes do not parse as a font face.
    InvalidFace {
        /// What the parser rejected.
        detail: String,
    },
    /// No specified face serves the requested style slot.
    MissingFace {
        /// The requested bold selection.
        bold: bool,
        /// The requested italic selection.
        italic: bool,
    },
    /// No specified face in the requested style slot contains the glyph.
    MissingGlyph {
        /// The glyph no face contains.
        character: char,
        /// The requested bold selection.
        bold: bool,
        /// The requested italic selection.
        italic: bool,
    },
    /// A multi-scalar sequence needs the shaping task; pinned rendering
    /// stops instead of splitting it across faces.
    UnsupportedSequence {
        /// The sequence the renderer cannot shape yet.
        sequence: String,
    },
}

impl std::fmt::Display for PinnedFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PinnedFontError::EmptySet => write!(
                f,
                "pinned font set is empty: specify at least one font face; \
                 pinned rendering never falls back to host fonts"
            ),
            PinnedFontError::FaceTooLarge { bytes, limit } => write!(
                f,
                "pinned font face is {bytes} bytes, above the {limit} byte limit"
            ),
            PinnedFontError::InvalidFace { detail } => {
                write!(f, "pinned font face is not a valid font: {detail}")
            }
            PinnedFontError::MissingFace { bold, italic } => write!(
                f,
                "pinned fonts have no {} face; add one or select synthetic styles",
                style_name(*bold, *italic)
            ),
            PinnedFontError::MissingGlyph {
                character,
                bold,
                italic,
            } => write!(
                f,
                "pinned {} faces contain no glyph for {character:?} (U+{:04X}); \
                 no host font or replacement glyph is substituted",
                style_name(*bold, *italic),
                *character as u32,
            ),
            PinnedFontError::UnsupportedSequence { sequence } => write!(
                f,
                "pinned rendering cannot shape the multi-scalar sequence \
                 {sequence:?} yet; no face is split across it"
            ),
        }
    }
}

impl std::error::Error for PinnedFontError {}

fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (false, false) => "regular",
        (true, false) => "bold",
        (false, true) => "italic",
        (true, true) => "bold-italic",
    }
}

/// The identity a pinned-font cache entry is stored under.
///
/// The policy tag, every face digest with its face index and style slot in
/// fallback order, the synthetic-style selection, the output scale, and the
/// raster geometry (font size, cell metrics, baseline) all join the key, so
/// a system-font entry — keyed by family and path — can never serve a
/// pinned request and one raster configuration can never serve another.
/// Each pinned render builds its fonts from the specified bytes only; the
/// key is recorded in the render contract as the evidence of what was used.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PinnedCacheKey {
    policy: &'static str,
    faces: Vec<PinnedCachedFace>,
    synthetic: bool,
    scale_bits: u64,
    font_size_bits: u32,
    cell_width_bits: u32,
    cell_height_bits: u32,
    baseline_bits: u32,
}

/// One face inside a [`PinnedCacheKey`], in fallback order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PinnedCachedFace {
    digest: String,
    index: u32,
    bold: bool,
    italic: bool,
}

impl PinnedCachedFace {
    /// The face digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The face index inside its bytes.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Whether the face serves the bold style slot.
    pub fn bold(&self) -> bool {
        self.bold
    }

    /// Whether the face serves the italic style slot.
    pub fn italic(&self) -> bool {
        self.italic
    }
}

impl PinnedCacheKey {
    /// Build the cache identity for one render from the pinned set and the
    /// raster settings the render actually uses.
    pub fn new(
        set: &PinnedFontSet,
        scale: f64,
        font_size: f32,
        cell_width: f32,
        cell_height: f32,
        baseline: f32,
    ) -> Self {
        Self {
            policy: RASTER_POLICY_PINNED_V1,
            faces: set
                .faces()
                .iter()
                .map(|face| PinnedCachedFace {
                    digest: face.digest(),
                    index: face.index(),
                    bold: face.bold(),
                    italic: face.italic(),
                })
                .collect(),
            synthetic: set.allow_synthetic_style(),
            scale_bits: scale.to_bits(),
            font_size_bits: font_size.to_bits(),
            cell_width_bits: cell_width.to_bits(),
            cell_height_bits: cell_height.to_bits(),
            baseline_bits: baseline.to_bits(),
        }
    }

    /// The raster policy this key pins.
    pub fn policy(&self) -> &'static str {
        self.policy
    }

    /// The face identities in fallback order.
    pub fn faces(&self) -> &[PinnedCachedFace] {
        &self.faces
    }

    /// Whether synthetic styles join the key.
    pub fn synthetic(&self) -> bool {
        self.synthetic
    }

    /// The output scale the key pins.
    pub fn scale(&self) -> f64 {
        f64::from_bits(self.scale_bits)
    }

    /// The grid font size the key pins.
    pub fn font_size(&self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    /// The cell width the key pins.
    pub fn cell_width(&self) -> f32 {
        f32::from_bits(self.cell_width_bits)
    }

    /// The cell height the key pins.
    pub fn cell_height(&self) -> f32 {
        f32::from_bits(self.cell_height_bits)
    }

    /// The text baseline the key pins.
    pub fn baseline(&self) -> f32 {
        f32::from_bits(self.baseline_bits)
    }

    /// SHA-256 over every key component, for contracts and comparisons.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"pinned-cache-key-v1");
        hasher.update(self.policy.as_bytes());
        hasher.update([u8::from(self.synthetic)]);
        for face in &self.faces {
            hasher.update(face.digest.as_bytes());
            hasher.update(face.index.to_le_bytes());
            hasher.update([u8::from(face.bold), u8::from(face.italic)]);
        }
        hasher.update(self.scale_bits.to_le_bytes());
        hasher.update(self.font_size_bits.to_le_bytes());
        hasher.update(self.cell_width_bits.to_le_bytes());
        hasher.update(self.cell_height_bits.to_le_bytes());
        hasher.update(self.baseline_bits.to_le_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Which cursor phase a strict image shows.
///
/// Strict (pixel-compared) images always use the shown phase: a visible,
/// in-capture cursor is drawn. Blink intent is not a phase; it stays in the
/// snapshot state and is recorded on the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorPhase {
    /// The cursor was drawn.
    Shown,
    /// The cursor was hidden or clipped out of the capture.
    Hidden,
}

/// One font entry in a [`RenderContract`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractFont {
    digest: String,
    face_index: u32,
    bold: bool,
    italic: bool,
    fallback_position: usize,
}

impl ContractFont {
    /// The SHA-256 of the face bytes, slot, and index.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The face index inside its bytes.
    pub fn face_index(&self) -> u32 {
        self.face_index
    }

    /// Whether the face serves the bold style slot.
    pub fn bold(&self) -> bool {
        self.bold
    }

    /// Whether the face serves the italic style slot.
    pub fn italic(&self) -> bool {
        self.italic
    }

    /// The position of the face in the fallback order.
    pub fn fallback_position(&self) -> usize {
        self.fallback_position
    }
}

/// Versioned evidence of what one strict render used.
///
/// The contract names the renderer version, the raster policy version, the
/// pinned fonts (digests, face indices, styles, fallback order), the
/// geometry, the output scale, the palette digest, and the cursor phase
/// with the blink intent kept from the snapshot state. Strict image gates
/// compare decoded pixels; the contract tells reviewers and future runs
/// which font and raster configuration produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderContract {
    contract_version: u32,
    renderer_version: String,
    raster_policy: String,
    fonts: Vec<ContractFont>,
    cell_width_bits: u32,
    cell_height_bits: u32,
    font_size_bits: u32,
    baseline_bits: u32,
    scale_bits: u64,
    title_bar: bool,
    palette_digest: String,
    cursor_phase: CursorPhase,
    blink: Option<bool>,
}

impl RenderContract {
    /// Build the contract for a pinned render from the cache key that
    /// identifies the render and the snapshot it drew.
    #[cfg(feature = "recording-raster")]
    pub(crate) fn pinned(
        key: &PinnedCacheKey,
        layout: &crate::render::svg::ResolvedLayout,
        scale: f64,
        snapshot: &crate::snapshot::TerminalSnapshot,
        cursor_drawn: bool,
    ) -> Self {
        Self::for_render(
            RASTER_POLICY_PINNED_V1,
            key.faces()
                .iter()
                .enumerate()
                .map(|(position, face)| ContractFont {
                    digest: face.digest().to_string(),
                    face_index: face.index(),
                    bold: face.bold(),
                    italic: face.italic(),
                    fallback_position: position,
                })
                .collect(),
            layout,
            scale,
            snapshot,
            cursor_drawn,
        )
    }

    /// Build the contract for a system-font render. No font bytes are
    /// hashed: system faces come from host configuration, so the policy
    /// name — not a digest — marks them as non-strict inputs.
    #[cfg(feature = "recording-raster")]
    pub(crate) fn system(
        layout: &crate::render::svg::ResolvedLayout,
        scale: f64,
        snapshot: &crate::snapshot::TerminalSnapshot,
        cursor_drawn: bool,
    ) -> Self {
        Self::for_render(
            RASTER_POLICY_SYSTEM_V1,
            Vec::new(),
            layout,
            scale,
            snapshot,
            cursor_drawn,
        )
    }

    #[cfg(feature = "recording-raster")]
    #[allow(clippy::too_many_arguments)]
    fn for_render(
        raster_policy: &str,
        fonts: Vec<ContractFont>,
        layout: &crate::render::svg::ResolvedLayout,
        scale: f64,
        snapshot: &crate::snapshot::TerminalSnapshot,
        cursor_drawn: bool,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"render-palette-v1");
        for entry in snapshot.palette().iter() {
            hasher.update([entry.r, entry.g, entry.b]);
        }
        for entry in [
            snapshot.default_foreground(),
            snapshot.default_background(),
            snapshot.default_cursor(),
        ] {
            hasher.update([entry.r, entry.g, entry.b]);
        }
        Self {
            contract_version: RENDER_CONTRACT_VERSION,
            renderer_version: env!("CARGO_PKG_VERSION").to_string(),
            raster_policy: raster_policy.to_string(),
            fonts,
            cell_width_bits: layout.cell_w.to_bits(),
            cell_height_bits: layout.cell_h.to_bits(),
            font_size_bits: layout.font_size.to_bits(),
            baseline_bits: layout.baseline.to_bits(),
            scale_bits: scale.to_bits(),
            title_bar: layout.title_bar,
            palette_digest: format!("{:x}", hasher.finalize()),
            cursor_phase: if cursor_drawn {
                CursorPhase::Shown
            } else {
                CursorPhase::Hidden
            },
            blink: snapshot.cursor().blink(),
        }
    }

    /// The contract schema version.
    pub fn contract_version(&self) -> u32 {
        self.contract_version
    }

    /// The renderer crate version that produced the image.
    pub fn renderer_version(&self) -> &str {
        &self.renderer_version
    }

    /// The raster policy version: `pinned-v1` or `system-v1`.
    pub fn raster_policy(&self) -> &str {
        &self.raster_policy
    }

    /// The pinned fonts in fallback order; empty for system renders.
    pub fn fonts(&self) -> &[ContractFont] {
        &self.fonts
    }

    /// The cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        f32::from_bits(self.cell_width_bits)
    }

    /// The cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        f32::from_bits(self.cell_height_bits)
    }

    /// The grid font size in pixels.
    pub fn font_size(&self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    /// The text baseline in pixels from the top of the cell.
    pub fn baseline(&self) -> f32 {
        f32::from_bits(self.baseline_bits)
    }

    /// The output scale factor.
    pub fn scale(&self) -> f64 {
        f64::from_bits(self.scale_bits)
    }

    /// Whether the title bar was drawn.
    pub fn title_bar(&self) -> bool {
        self.title_bar
    }

    /// SHA-256 over the snapshot palette and default colors.
    pub fn palette_digest(&self) -> &str {
        &self.palette_digest
    }

    /// The cursor phase the strict image shows.
    pub fn cursor_phase(&self) -> CursorPhase {
        self.cursor_phase
    }

    /// Blink intent kept from the snapshot state, never normalized.
    pub fn blink(&self) -> Option<bool> {
        self.blink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regular_bytes() -> Vec<u8> {
        include_bytes!("../../assets/jetbrains-mono/JetBrainsMono-Regular.ttf").to_vec()
    }

    #[test]
    fn an_empty_set_is_rejected_before_any_render() {
        let error = PinnedFontSet::new(Vec::new()).expect_err("empty sets must fail");
        assert_eq!(error, PinnedFontError::EmptySet);
        assert!(error.to_string().contains("never falls back to host fonts"));
    }

    #[test]
    fn oversized_and_empty_faces_are_rejected() {
        let oversized = PinnedFace::new(vec![0u8; MAX_PINNED_FACE_BYTES + 1], 0, false, false)
            .expect_err("oversized faces must fail");
        assert!(matches!(oversized, PinnedFontError::FaceTooLarge { .. }));
        let empty =
            PinnedFace::new(Vec::new(), 0, false, false).expect_err("empty faces must fail");
        assert!(matches!(empty, PinnedFontError::InvalidFace { .. }));
        let garbage = PinnedFace::new(b"not a font".to_vec(), 0, false, false)
            .expect_err("garbage must fail");
        assert!(matches!(garbage, PinnedFontError::InvalidFace { .. }));
    }

    #[test]
    fn digests_cover_bytes_order_and_style() {
        let first = PinnedFace::new(regular_bytes(), 0, false, false).unwrap();
        let second = PinnedFace::new(regular_bytes(), 0, true, false).unwrap();
        assert_ne!(
            first.digest(),
            second.digest(),
            "the style slot joins the digest"
        );
        let regular = PinnedFontSet::new(vec![first.clone()]).unwrap();
        let reordered = PinnedFontSet::new(vec![first]).unwrap();
        assert_eq!(regular.digest(), reordered.digest());
        let bolded = PinnedFontSet::new(vec![second]).unwrap();
        assert_ne!(regular.digest(), bolded.digest());
    }

    #[test]
    fn cache_keys_pin_policy_faces_and_raster_settings() {
        let face = PinnedFace::new(regular_bytes(), 0, false, false).unwrap();
        let set = PinnedFontSet::new(vec![face]).unwrap();
        let key = PinnedCacheKey::new(&set, 1.0, 17.0, 10.0, 21.0, 14.0);
        assert_eq!(key.policy(), RASTER_POLICY_PINNED_V1);
        assert_eq!(key.faces().len(), 1);
        assert_eq!(key.scale(), 1.0);
        assert_eq!(key.cell_width(), 10.0);
        let scaled = PinnedCacheKey::new(&set, 2.0, 17.0, 10.0, 21.0, 14.0);
        assert_ne!(key, scaled, "scale joins the cache key");
        assert_ne!(key.digest(), scaled.digest());
    }
}
