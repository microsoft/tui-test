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
//! grapheme no specified face contains, is a typed [`PinnedFontError`]
//! unless the set selected synthetic styles, in which case an empty slot
//! synthesizes the style from the set's other faces in fallback order:
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

use crate::api::CaptureBackground;
use crate::profile::Rgb;

/// Schema version of [`RenderContract`].
///
/// Version 2 records every normalized output setting and the
/// synthetic-style selection alongside the fonts and geometry, so renders
/// from different settings never share a contract. The contract names the
/// configuration, not the contents: snapshots with different text or
/// titles can share a contract, and strict gates compare decoded pixels.
pub const RENDER_CONTRACT_VERSION: u32 = 2;

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
    ///
    /// The byte count joins as a fixed-width `u64`: a `usize` encoding
    /// would hash 8 bytes on 64-bit hosts and 4 on 32-bit ones, giving
    /// identical fonts different identities per platform.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"pinned-face-v1");
        hasher.update([u8::from(self.bold), u8::from(self.italic)]);
        hasher.update(self.index.to_le_bytes());
        hasher.update((self.bytes.len() as u64).to_le_bytes());
        hasher.update(&self.bytes);
        format!("{:x}", hasher.finalize())
    }
}

/// Explicit pinned fonts for one offline render.
///
/// The faces are tried in order for a grapheme whose style slot has more
/// than one face; the order is the fallback order. Synthetic bold or
/// italic is off unless [`PinnedFontSet::with_synthetic_style`] selects
/// it, in which case an empty style slot falls back to the set's other
/// faces instead of failing.
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
    /// style slot with no face is a missing-face error, so register an
    /// explicit face per slot served. With this selection an empty slot
    /// falls back to the set's other faces in fallback order, drawing
    /// their outlines emboldened or slanted.
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
/// fallback order, the synthetic-style selection, the output scale, and
/// every raster setting that changes output pixels all join the key: grid
/// geometry (font size, cell metrics, baseline), title chrome (bar, controls,
/// title size, header and divider heights), content and canvas padding,
/// backgrounds (canvas fill and capture override), window and border
/// chrome, the shadow, and the exact-size switch. A system-font entry —
/// keyed by family and path — can never serve a pinned request and one
/// raster configuration can never serve another. Each pinned render builds
/// its fonts from the specified bytes only; the key is recorded in the
/// render contract as the evidence of what was used. The key is a pure
/// identity value: no shared cache is stored behind it.
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
    title_font_size_bits: u32,
    header_height_bits: u32,
    divider_height_bits: u32,
    content_left_bits: u32,
    content_top_bits: u32,
    content_right_bits: u32,
    content_bottom_bits: u32,
    canvas_left: u32,
    canvas_right: u32,
    canvas_top: u32,
    canvas_bottom: u32,
    canvas_background: Rgb,
    background: Option<CaptureBackground>,
    title_bar: bool,
    title_controls: bool,
    window_background: Rgb,
    window_foreground: Rgb,
    window_divider: Rgb,
    border_width_bits: u32,
    border_color: Rgb,
    border_radius_bits: u32,
    shadow_enabled: bool,
    shadow_color: Rgb,
    shadow_offset_bits: u32,
    shadow_spread_bits: u32,
    exact_size: bool,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        set: &PinnedFontSet,
        scale: f64,
        font_size: f32,
        cell_width: f32,
        cell_height: f32,
        baseline: f32,
        title_font_size: f32,
        header_height: f32,
        divider_height: f32,
        content_left: f32,
        content_top: f32,
        content_right: f32,
        content_bottom: f32,
        canvas_left: u32,
        canvas_right: u32,
        canvas_top: u32,
        canvas_bottom: u32,
        canvas_background: Rgb,
        background: Option<CaptureBackground>,
        title_bar: bool,
        title_controls: bool,
        window_background: Rgb,
        window_foreground: Rgb,
        window_divider: Rgb,
        border_width: f32,
        border_color: Rgb,
        border_radius: f32,
        shadow_enabled: bool,
        shadow_color: Rgb,
        shadow_offset: f32,
        shadow_spread: f32,
        exact_size: bool,
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
            title_font_size_bits: title_font_size.to_bits(),
            header_height_bits: header_height.to_bits(),
            divider_height_bits: divider_height.to_bits(),
            content_left_bits: content_left.to_bits(),
            content_top_bits: content_top.to_bits(),
            content_right_bits: content_right.to_bits(),
            content_bottom_bits: content_bottom.to_bits(),
            canvas_left,
            canvas_right,
            canvas_top,
            canvas_bottom,
            canvas_background,
            background,
            title_bar,
            title_controls,
            window_background,
            window_foreground,
            window_divider,
            border_width_bits: border_width.to_bits(),
            border_color,
            border_radius_bits: border_radius.to_bits(),
            shadow_enabled,
            shadow_color,
            shadow_offset_bits: shadow_offset.to_bits(),
            shadow_spread_bits: shadow_spread.to_bits(),
            exact_size,
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

    /// The title font size the key pins.
    pub fn title_font_size(&self) -> f32 {
        f32::from_bits(self.title_font_size_bits)
    }

    /// The title header height the key pins.
    pub fn header_height(&self) -> f32 {
        f32::from_bits(self.header_height_bits)
    }

    /// The title divider height the key pins.
    pub fn divider_height(&self) -> f32 {
        f32::from_bits(self.divider_height_bits)
    }

    /// The left content gap the key pins.
    pub fn content_left(&self) -> f32 {
        f32::from_bits(self.content_left_bits)
    }

    /// The top content gap the key pins.
    pub fn content_top(&self) -> f32 {
        f32::from_bits(self.content_top_bits)
    }

    /// The right content gap the key pins.
    pub fn content_right(&self) -> f32 {
        f32::from_bits(self.content_right_bits)
    }

    /// The bottom content gap the key pins.
    pub fn content_bottom(&self) -> f32 {
        f32::from_bits(self.content_bottom_bits)
    }

    /// The left canvas padding the key pins.
    pub fn canvas_left(&self) -> u32 {
        self.canvas_left
    }

    /// The right canvas padding the key pins.
    pub fn canvas_right(&self) -> u32 {
        self.canvas_right
    }

    /// The top canvas padding the key pins.
    pub fn canvas_top(&self) -> u32 {
        self.canvas_top
    }

    /// The bottom canvas padding the key pins.
    pub fn canvas_bottom(&self) -> u32 {
        self.canvas_bottom
    }

    /// The canvas fill the key pins, used without a capture override.
    pub fn canvas_background(&self) -> Rgb {
        self.canvas_background
    }

    /// The capture background override the key pins, if any.
    pub fn background(&self) -> Option<CaptureBackground> {
        self.background
    }

    /// Whether the title bar the key pins is drawn.
    pub fn title_bar(&self) -> bool {
        self.title_bar
    }

    /// Whether the window controls the key pins are drawn.
    pub fn title_controls(&self) -> bool {
        self.title_controls
    }

    /// The title bar fill the key pins.
    pub fn window_background(&self) -> Rgb {
        self.window_background
    }

    /// The title text color the key pins.
    pub fn window_foreground(&self) -> Rgb {
        self.window_foreground
    }

    /// The title divider color the key pins.
    pub fn window_divider(&self) -> Rgb {
        self.window_divider
    }

    /// The panel border width the key pins.
    pub fn border_width(&self) -> f32 {
        f32::from_bits(self.border_width_bits)
    }

    /// The panel border color the key pins.
    pub fn border_color(&self) -> Rgb {
        self.border_color
    }

    /// The panel corner radius the key pins.
    pub fn border_radius(&self) -> f32 {
        f32::from_bits(self.border_radius_bits)
    }

    /// Whether the drop shadow the key pins is drawn.
    pub fn shadow_enabled(&self) -> bool {
        self.shadow_enabled
    }

    /// The drop shadow color the key pins.
    pub fn shadow_color(&self) -> Rgb {
        self.shadow_color
    }

    /// The drop shadow offset the key pins.
    pub fn shadow_offset(&self) -> f32 {
        f32::from_bits(self.shadow_offset_bits)
    }

    /// The drop shadow spread the key pins.
    pub fn shadow_spread(&self) -> f32 {
        f32::from_bits(self.shadow_spread_bits)
    }

    /// Whether exact (unpadded) output size the key pins is used.
    pub fn exact_size(&self) -> bool {
        self.exact_size
    }

    /// SHA-256 over every key component, for contracts and comparisons.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"pinned-cache-key-v2");
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
        hasher.update(self.title_font_size_bits.to_le_bytes());
        hasher.update(self.header_height_bits.to_le_bytes());
        hasher.update(self.divider_height_bits.to_le_bytes());
        hasher.update(self.content_left_bits.to_le_bytes());
        hasher.update(self.content_top_bits.to_le_bytes());
        hasher.update(self.content_right_bits.to_le_bytes());
        hasher.update(self.content_bottom_bits.to_le_bytes());
        hasher.update(self.canvas_left.to_le_bytes());
        hasher.update(self.canvas_right.to_le_bytes());
        hasher.update(self.canvas_top.to_le_bytes());
        hasher.update(self.canvas_bottom.to_le_bytes());
        for color in [
            self.canvas_background,
            self.window_background,
            self.window_foreground,
            self.window_divider,
            self.border_color,
            self.shadow_color,
        ] {
            hasher.update([color.r, color.g, color.b]);
        }
        match self.background {
            None => hasher.update([0]),
            Some(CaptureBackground::Transparent) => hasher.update([1]),
            Some(CaptureBackground::Color(color)) => {
                hasher.update([2, color.r, color.g, color.b]);
            }
        }
        hasher.update([
            u8::from(self.title_bar),
            u8::from(self.title_controls),
            u8::from(self.shadow_enabled),
            u8::from(self.exact_size),
        ]);
        hasher.update(self.border_width_bits.to_le_bytes());
        hasher.update(self.border_radius_bits.to_le_bytes());
        hasher.update(self.shadow_offset_bits.to_le_bytes());
        hasher.update(self.shadow_spread_bits.to_le_bytes());
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
/// synthetic-style selection, the geometry, the output scale, every
/// normalized output setting (title chrome, content and canvas padding,
/// backgrounds, window and border chrome, the shadow, and the exact-size
/// switch), the palette digest, and the cursor phase with the blink intent
/// kept from the snapshot state. Strict image gates compare decoded pixels;
/// the contract tells reviewers and future runs which font and raster
/// configuration produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderContract {
    contract_version: u32,
    renderer_version: String,
    raster_policy: String,
    fonts: Vec<ContractFont>,
    synthetic: bool,
    cell_width_bits: u32,
    cell_height_bits: u32,
    font_size_bits: u32,
    baseline_bits: u32,
    title_font_size_bits: u32,
    header_height_bits: u32,
    divider_height_bits: u32,
    content_left_bits: u32,
    content_top_bits: u32,
    content_right_bits: u32,
    content_bottom_bits: u32,
    canvas_left: u32,
    canvas_right: u32,
    canvas_top: u32,
    canvas_bottom: u32,
    canvas_background: Rgb,
    background: Option<CaptureBackground>,
    scale_bits: u64,
    title_bar: bool,
    title_controls: bool,
    window_background: Rgb,
    window_foreground: Rgb,
    window_divider: Rgb,
    border_width_bits: u32,
    border_color: Rgb,
    border_radius_bits: u32,
    shadow_enabled: bool,
    shadow_color: Rgb,
    shadow_offset_bits: u32,
    shadow_spread_bits: u32,
    exact_size: bool,
    palette_digest: String,
    cursor_phase: CursorPhase,
    blink: Option<bool>,
}

impl RenderContract {
    /// Build the contract for a pinned render from the cache key that
    /// identifies the render and the snapshot it drew.
    ///
    /// The key carries the synthetic-style selection, the capture
    /// background override, and the exact-size switch the layout alone
    /// does not hold; everything else comes from the shared layout.
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
            key.synthetic(),
            layout,
            scale,
            key.background(),
            key.exact_size(),
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
        background: Option<CaptureBackground>,
        exact_size: bool,
    ) -> Self {
        Self::for_render(
            RASTER_POLICY_SYSTEM_V1,
            Vec::new(),
            false,
            layout,
            scale,
            background,
            exact_size,
            snapshot,
            cursor_drawn,
        )
    }

    #[cfg(feature = "recording-raster")]
    #[allow(clippy::too_many_arguments)]
    fn for_render(
        raster_policy: &str,
        fonts: Vec<ContractFont>,
        synthetic: bool,
        layout: &crate::render::svg::ResolvedLayout,
        scale: f64,
        background: Option<CaptureBackground>,
        exact_size: bool,
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
            synthetic,
            cell_width_bits: layout.cell_w.to_bits(),
            cell_height_bits: layout.cell_h.to_bits(),
            font_size_bits: layout.font_size.to_bits(),
            baseline_bits: layout.baseline.to_bits(),
            title_font_size_bits: layout.title_font_size.to_bits(),
            header_height_bits: layout.header_h.to_bits(),
            divider_height_bits: layout.divider_h.to_bits(),
            content_left_bits: layout.content_left.to_bits(),
            content_top_bits: layout.content_top.to_bits(),
            content_right_bits: layout.content_right.to_bits(),
            content_bottom_bits: layout.content_bottom.to_bits(),
            canvas_left: layout.canvas_left,
            canvas_right: layout.canvas_right,
            canvas_top: layout.canvas_top,
            canvas_bottom: layout.canvas_bottom,
            canvas_background: layout.canvas_background,
            background,
            scale_bits: scale.to_bits(),
            title_bar: layout.title_bar,
            title_controls: layout.title_controls,
            window_background: layout.window_background,
            window_foreground: layout.window_foreground,
            window_divider: layout.window_divider,
            border_width_bits: layout.border_width.to_bits(),
            border_color: layout.border_color,
            border_radius_bits: layout.border_radius.to_bits(),
            shadow_enabled: layout.shadow_enabled,
            shadow_color: layout.shadow_color,
            shadow_offset_bits: layout.shadow_offset.to_bits(),
            shadow_spread_bits: layout.shadow_spread.to_bits(),
            exact_size,
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

    /// Whether synthetic bold or italic rendering was selected.
    pub fn synthetic(&self) -> bool {
        self.synthetic
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

    /// The title font size in pixels.
    pub fn title_font_size(&self) -> f32 {
        f32::from_bits(self.title_font_size_bits)
    }

    /// The title header height in pixels.
    pub fn header_height(&self) -> f32 {
        f32::from_bits(self.header_height_bits)
    }

    /// The title divider height in pixels.
    pub fn divider_height(&self) -> f32 {
        f32::from_bits(self.divider_height_bits)
    }

    /// The left content gap in pixels.
    pub fn content_left(&self) -> f32 {
        f32::from_bits(self.content_left_bits)
    }

    /// The top content gap in pixels.
    pub fn content_top(&self) -> f32 {
        f32::from_bits(self.content_top_bits)
    }

    /// The right content gap in pixels.
    pub fn content_right(&self) -> f32 {
        f32::from_bits(self.content_right_bits)
    }

    /// The bottom content gap in pixels.
    pub fn content_bottom(&self) -> f32 {
        f32::from_bits(self.content_bottom_bits)
    }

    /// The left canvas padding in pixels.
    pub fn canvas_left(&self) -> u32 {
        self.canvas_left
    }

    /// The right canvas padding in pixels.
    pub fn canvas_right(&self) -> u32 {
        self.canvas_right
    }

    /// The top canvas padding in pixels.
    pub fn canvas_top(&self) -> u32 {
        self.canvas_top
    }

    /// The bottom canvas padding in pixels.
    pub fn canvas_bottom(&self) -> u32 {
        self.canvas_bottom
    }

    /// The canvas fill used without a capture override.
    pub fn canvas_background(&self) -> Rgb {
        self.canvas_background
    }

    /// The capture background override, if any.
    pub fn background(&self) -> Option<CaptureBackground> {
        self.background
    }

    /// Whether the window controls were drawn.
    pub fn title_controls(&self) -> bool {
        self.title_controls
    }

    /// The title bar fill.
    pub fn window_background(&self) -> Rgb {
        self.window_background
    }

    /// The title text color.
    pub fn window_foreground(&self) -> Rgb {
        self.window_foreground
    }

    /// The title divider color.
    pub fn window_divider(&self) -> Rgb {
        self.window_divider
    }

    /// The panel border width in pixels.
    pub fn border_width(&self) -> f32 {
        f32::from_bits(self.border_width_bits)
    }

    /// The panel border color.
    pub fn border_color(&self) -> Rgb {
        self.border_color
    }

    /// The panel corner radius in pixels.
    pub fn border_radius(&self) -> f32 {
        f32::from_bits(self.border_radius_bits)
    }

    /// Whether the drop shadow was drawn.
    pub fn shadow_enabled(&self) -> bool {
        self.shadow_enabled
    }

    /// The drop shadow color.
    pub fn shadow_color(&self) -> Rgb {
        self.shadow_color
    }

    /// The drop shadow offset in pixels.
    pub fn shadow_offset(&self) -> f32 {
        f32::from_bits(self.shadow_offset_bits)
    }

    /// The drop shadow spread in pixels.
    pub fn shadow_spread(&self) -> f32 {
        f32::from_bits(self.shadow_spread_bits)
    }

    /// Whether exact (unpadded) output size was used.
    pub fn exact_size(&self) -> bool {
        self.exact_size
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
    fn face_digest_is_a_fixed_known_vector() {
        // The length field is a fixed-width u64, so this vector holds on
        // every platform: a usize encoding would hash a different stream
        // on 32-bit hosts (see the P2 hash-byte-stream repro).
        let face = PinnedFace::new(regular_bytes(), 0, false, false).unwrap();
        assert_eq!(
            face.digest(),
            "08b324eb7d651773b79dfa0147484b9eeaf882055935a7b20304c23430bfd4b9"
        );
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

    fn test_key(
        set: &PinnedFontSet,
        scale: f64,
        title_bar: bool,
        background: Option<CaptureBackground>,
        canvas_left: u32,
    ) -> PinnedCacheKey {
        PinnedCacheKey::new(
            set,
            scale,
            17.0,
            10.0,
            21.0,
            14.0,
            13.0,
            28.0,
            1.0,
            15.0,
            8.0,
            15.0,
            14.0,
            canvas_left,
            24,
            24,
            24,
            Rgb::new(30, 30, 30),
            background,
            title_bar,
            false,
            Rgb::new(40, 40, 40),
            Rgb::new(200, 200, 200),
            Rgb::new(60, 60, 60),
            0.0,
            Rgb::new(70, 70, 70),
            8.0,
            false,
            Rgb::new(0, 0, 0),
            4.0,
            6.0,
            true,
        )
    }

    #[test]
    fn cache_keys_pin_policy_faces_and_raster_settings() {
        let face = PinnedFace::new(regular_bytes(), 0, false, false).unwrap();
        let set = PinnedFontSet::new(vec![face]).unwrap();
        let key = test_key(&set, 1.0, false, None, 24);
        assert_eq!(key.policy(), RASTER_POLICY_PINNED_V1);
        assert_eq!(key.faces().len(), 1);
        assert_eq!(key.scale(), 1.0);
        assert_eq!(key.cell_width(), 10.0);
        assert!(!key.title_bar());
        assert_eq!(key.background(), None);
        assert_eq!(key.canvas_left(), 24);
        assert!(key.exact_size());
        let scaled = test_key(&set, 2.0, false, None, 24);
        assert_ne!(key, scaled, "scale joins the cache key");
        assert_ne!(key.digest(), scaled.digest());
        let titled = test_key(&set, 1.0, true, None, 24);
        assert_ne!(key, titled, "title bar joins the cache key");
        assert_ne!(key.digest(), titled.digest());
        let tinted = test_key(&set, 1.0, false, Some(CaptureBackground::Transparent), 24);
        assert_ne!(key, tinted, "background joins the cache key");
        assert_ne!(key.digest(), tinted.digest());
        let padded = test_key(&set, 1.0, false, None, 32);
        assert_ne!(key, padded, "canvas padding joins the cache key");
        assert_ne!(key.digest(), padded.digest());
    }
}
