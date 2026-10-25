//! A public owned snapshot of terminal state.
//!
//! [`TerminalSnapshot`] is the single owned model for live capture and offline
//! construction. Every field is private; read-only accessors expose the data.
//! Capture reads grid, cursor, dimensions, title, and resolved colors while it
//! holds one terminal-state lock, so a snapshot never mixes data from two
//! terminal moments. Observation metadata (capture time, revision, backend and
//! session identity, source kind) travels with the snapshot but stays out of
//! canonical equality and hashing: two snapshots are canonically equal when
//! their terminal state matches, however they were observed.
//!
//! The default capture records raw visual data. Locator highlighting never
//! leaks into it; a highlighted capture needs an explicit opt-in through
//! [`SnapshotOptions::with_highlight`].

use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::profile::{ColorSlot, Rgb};
use crate::terminal::cell::{Attrs, Color, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

/// Wire schema understood by [`TerminalSnapshot`] deserialization.
///
/// Bumped only when the serialized form changes incompatibly; older payloads
/// keep failing closed with [`SnapshotError::UnsupportedSchema`] instead of
/// being misread as the current layout.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Largest accepted snapshot dimension in columns or rows.
///
/// Matches the downstream image-dimension bound so a snapshot can always be
/// rendered without a second size check.
pub const MAX_SNAPSHOT_DIM: u32 = 32_768;

/// Largest accepted cell count (`cols * rows`) in one snapshot.
///
/// Matches the downstream per-sample cell bound.
pub const MAX_SNAPSHOT_CELLS: u64 = 1_000_000;

/// Largest accepted UTF-8 byte length of one cell grapheme.
///
/// Matches the downstream per-cell string bound.
pub const MAX_CELL_TEXT_BYTES: usize = 4_096;

/// Largest accepted UTF-8 byte length of one hyperlink URI or link identity.
pub const MAX_LINK_BYTES: usize = 4_096;

/// Largest accepted UTF-8 byte length of the window title.
pub const MAX_TITLE_BYTES: usize = 65_536;

/// Which terminal region a snapshot covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotScope {
    /// The rows currently visible in the viewport.
    Viewport,
    /// The active screen, independent of viewport scrolling.
    ActiveScreen,
    /// Scrollback history followed by the active screen.
    FullHistory,
}

impl SnapshotScope {
    /// The name this scope goes by on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            SnapshotScope::Viewport => "viewport",
            SnapshotScope::ActiveScreen => "active_screen",
            SnapshotScope::FullHistory => "full_history",
        }
    }
}

/// How a snapshot cell refers to a color.
///
/// `Default` means the terminal default and stays distinct from an explicit
/// [`SnapshotColor::Rgb`] even when both resolve to the same painted color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotColor {
    /// The terminal default foreground or background.
    Default,
    /// A palette entry, with indices 0-15 using the recorded SGR
    /// normalization (both `SGR 31` and `SGR 38;5;1` land on slot 1).
    Indexed(u8),
    /// An explicit RGB color (`SGR 38;2;r;g;b`).
    Rgb(u8, u8, u8),
}

impl SnapshotColor {
    /// The name or spelling this reference goes by on the wire.
    pub fn wire_name(&self) -> String {
        match self {
            SnapshotColor::Default => "default".to_string(),
            SnapshotColor::Indexed(index) => index.to_string(),
            SnapshotColor::Rgb(r, g, b) => Rgb::new(*r, *g, *b).to_hex(),
        }
    }
}

impl From<Color> for SnapshotColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Named(named) => SnapshotColor::Indexed(named.index()),
            Color::Idx(index) => SnapshotColor::Indexed(index),
            Color::Rgb(r, g, b) => SnapshotColor::Rgb(r, g, b),
        }
    }
}

impl From<Option<Color>> for SnapshotColor {
    fn from(color: Option<Color>) -> Self {
        color
            .map(SnapshotColor::from)
            .unwrap_or(SnapshotColor::Default)
    }
}

impl Serialize for SnapshotColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            SnapshotColor::Default => serializer.serialize_str("default"),
            SnapshotColor::Indexed(index) => serializer.serialize_u8(*index),
            SnapshotColor::Rgb(r, g, b) => serializer.serialize_str(&Rgb::new(*r, *g, *b).to_hex()),
        }
    }
}

impl<'de> Deserialize<'de> for SnapshotColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl serde::de::Visitor<'_> for ColorVisitor {
            type Value = SnapshotColor;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("\"default\", a palette index 0-255, or a #rrggbb color")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value == "default" {
                    return Ok(SnapshotColor::Default);
                }
                Rgb::parse(value)
                    .map(|rgb| SnapshotColor::Rgb(rgb.r, rgb.g, rgb.b))
                    .map_err(|_| E::custom(format!("invalid snapshot color (got {value:?})")))
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                u8::try_from(value)
                    .map(SnapshotColor::Indexed)
                    .map_err(|_| E::custom(format!("palette index out of range (got {value})")))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                u8::try_from(value)
                    .map(SnapshotColor::Indexed)
                    .map_err(|_| E::custom(format!("palette index out of range (got {value})")))
            }
        }

        deserializer.deserialize_any(ColorVisitor)
    }
}

/// The shape of a cell underline, mirroring [`UnderlineStyle`] wire names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotUnderline {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl SnapshotUnderline {
    /// The name this shape goes by on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            SnapshotUnderline::None => "none",
            SnapshotUnderline::Single => "single",
            SnapshotUnderline::Double => "double",
            SnapshotUnderline::Curly => "curly",
            SnapshotUnderline::Dotted => "dotted",
            SnapshotUnderline::Dashed => "dashed",
        }
    }
}

impl From<UnderlineStyle> for SnapshotUnderline {
    fn from(style: UnderlineStyle) -> Self {
        match style {
            UnderlineStyle::None => SnapshotUnderline::None,
            UnderlineStyle::Single => SnapshotUnderline::Single,
            UnderlineStyle::Double => SnapshotUnderline::Double,
            UnderlineStyle::Curly => SnapshotUnderline::Curly,
            UnderlineStyle::Dotted => SnapshotUnderline::Dotted,
            UnderlineStyle::Dashed => SnapshotUnderline::Dashed,
        }
    }
}

/// One captured cell: grapheme, width, colors, attributes, and hyperlink.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SnapshotCell {
    text: String,
    width: u16,
    fg: SnapshotColor,
    bg: SnapshotColor,
    bold: bool,
    dim: bool,
    italic: bool,
    inverse: bool,
    invisible: bool,
    strike: bool,
    blink: bool,
    underline: SnapshotUnderline,
    underline_color: SnapshotColor,
    link_uri: Option<String>,
    link_id: Option<String>,
}

impl SnapshotCell {
    /// Build a cell, rejecting oversize text and links.
    ///
    /// Continuation placement (empty text away from column zero) and grid
    /// bounds are checked by [`TerminalSnapshot::try_new`], which knows each
    /// cell's coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        text: String,
        width: u16,
        fg: SnapshotColor,
        bg: SnapshotColor,
        attrs: Attrs,
        underline: SnapshotUnderline,
        underline_color: SnapshotColor,
        link_uri: Option<String>,
        link_id: Option<String>,
    ) -> Result<Self, SnapshotError> {
        if text.len() > MAX_CELL_TEXT_BYTES {
            return Err(SnapshotError::OversizeText {
                bytes: text.len(),
                limit: MAX_CELL_TEXT_BYTES,
            });
        }
        if let Some(uri) = link_uri.as_deref() {
            if uri.len() > MAX_LINK_BYTES {
                return Err(SnapshotError::OversizeLink {
                    bytes: uri.len(),
                    limit: MAX_LINK_BYTES,
                });
            }
        }
        if let Some(id) = link_id.as_deref() {
            if id.len() > MAX_LINK_BYTES {
                return Err(SnapshotError::OversizeLink {
                    bytes: id.len(),
                    limit: MAX_LINK_BYTES,
                });
            }
        }
        Ok(SnapshotCell {
            text,
            width,
            fg,
            bg,
            bold: attrs.contains(Attrs::BOLD),
            dim: attrs.contains(Attrs::DIM),
            italic: attrs.contains(Attrs::ITALIC),
            inverse: attrs.contains(Attrs::INVERSE),
            invisible: attrs.contains(Attrs::INVISIBLE),
            strike: attrs.contains(Attrs::STRIKE),
            blink: attrs.contains(Attrs::BLINK),
            underline,
            underline_color,
            link_uri,
            link_id,
        })
    }

    /// The cell grapheme; empty for a continuation cell trailing a wide char.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Terminal columns this cell occupies; zero for continuation cells.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Whether this cell continues the wide character in the previous column.
    pub fn is_continuation(&self) -> bool {
        self.text.is_empty()
    }

    /// Foreground color reference (`Default` is the terminal default).
    pub fn fg(&self) -> SnapshotColor {
        self.fg
    }

    /// Background color reference (`Default` is the terminal default).
    pub fn bg(&self) -> SnapshotColor {
        self.bg
    }

    /// Whether the cell carries the named boolean attribute.
    pub fn has(&self, attr: Attrs) -> bool {
        let mut probe = Attrs::empty();
        if self.bold {
            probe |= Attrs::BOLD;
        }
        if self.dim {
            probe |= Attrs::DIM;
        }
        if self.italic {
            probe |= Attrs::ITALIC;
        }
        if self.inverse {
            probe |= Attrs::INVERSE;
        }
        if self.invisible {
            probe |= Attrs::INVISIBLE;
        }
        if self.strike {
            probe |= Attrs::STRIKE;
        }
        if self.blink {
            probe |= Attrs::BLINK;
        }
        probe.contains(attr)
    }

    /// The underline shape; [`SnapshotUnderline::None`] when not underlined.
    pub fn underline(&self) -> SnapshotUnderline {
        self.underline
    }

    /// Underline color reference (`Default` follows the cell foreground).
    pub fn underline_color(&self) -> SnapshotColor {
        self.underline_color
    }

    /// The OSC 8 hyperlink URI, if the cell sits inside a link.
    pub fn link_uri(&self) -> Option<&str> {
        self.link_uri.as_deref()
    }

    /// The link `id=` parameter, if the sequence carried one.
    pub fn link_id(&self) -> Option<&str> {
        self.link_id.as_deref()
    }

    fn check_at(&self, col: u32, row: u32) -> Result<(), SnapshotError> {
        if self.text.len() > MAX_CELL_TEXT_BYTES {
            return Err(SnapshotError::CellTextTooLarge {
                col,
                row,
                bytes: self.text.len(),
                limit: MAX_CELL_TEXT_BYTES,
            });
        }
        if self.text.is_empty() && (col == 0 || self.width != 0) {
            return Err(SnapshotError::BadContinuation { col, row });
        }
        for uri in self.link_uri.iter().chain(self.link_id.iter()) {
            if uri.len() > MAX_LINK_BYTES {
                return Err(SnapshotError::LinkTooLarge {
                    col,
                    row,
                    bytes: uri.len(),
                    limit: MAX_LINK_BYTES,
                });
            }
        }
        Ok(())
    }
}

/// The shape the snapshot cursor is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotCursorShape {
    Block,
    Underline,
    Bar,
}

impl SnapshotCursorShape {
    /// The name this shape goes by on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            SnapshotCursorShape::Block => "block",
            SnapshotCursorShape::Underline => "underline",
            SnapshotCursorShape::Bar => "bar",
        }
    }
}

impl From<CursorShape> for SnapshotCursorShape {
    fn from(shape: CursorShape) -> Self {
        match shape {
            CursorShape::Block => SnapshotCursorShape::Block,
            CursorShape::Underline => SnapshotCursorShape::Underline,
            CursorShape::Bar => SnapshotCursorShape::Bar,
        }
    }
}

/// The captured cursor: position, visibility, shape, color, and blink intent.
///
/// Position is in full-grid coordinates (history rows first, active screen
/// last), never clamped: a cursor outside the captured region keeps its real
/// position and reports [`SnapshotCursor::in_capture`] as `false`. Terminal
/// cursor visibility stays separate from capture clipping: a hidden cursor
/// still has a position, and a visible cursor can sit outside the capture.
///
/// Blink is `None` when the backend does not report blink intent. An unknown
/// blink stays unknown; it is never normalized to `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SnapshotCursor {
    col: u32,
    row: u32,
    in_capture: bool,
    visible: bool,
    shape: SnapshotCursorShape,
    color: Rgb,
    blink: Option<bool>,
}

impl SnapshotCursor {
    /// Build a cursor value. Bounds against the capture grid are checked by
    /// [`TerminalSnapshot::try_new`], which knows the grid dimensions.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        col: u32,
        row: u32,
        in_capture: bool,
        visible: bool,
        shape: SnapshotCursorShape,
        color: Rgb,
        blink: Option<bool>,
    ) -> Self {
        SnapshotCursor {
            col,
            row,
            in_capture,
            visible,
            shape,
            color,
            blink,
        }
    }

    /// Full-grid column (history rows share the same column frame).
    pub fn col(&self) -> u32 {
        self.col
    }

    /// Full-grid row, counting from the oldest history row as zero.
    pub fn row(&self) -> u32 {
        self.row
    }

    /// Whether the cursor position falls inside the captured grid.
    pub fn in_capture(&self) -> bool {
        self.in_capture
    }

    /// Whether the terminal draws the cursor (`DECTCEM`).
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The shape the terminal draws the cursor as.
    pub fn shape(&self) -> SnapshotCursorShape {
        self.shape
    }

    /// The cursor color as currently resolved (`OSC 12` applied).
    pub fn color(&self) -> Rgb {
        self.color
    }

    /// Blink intent: `Some(true)` blinking, `Some(false)` steady, `None`
    /// when the backend does not report it.
    pub fn blink(&self) -> Option<bool> {
        self.blink
    }

    fn hash_fields<H: Hasher>(&self, state: &mut H) {
        self.col.hash(state);
        self.row.hash(state);
        self.in_capture.hash(state);
        self.visible.hash(state);
        self.shape.hash(state);
        hash_rgb(&self.color, state);
        self.blink.hash(state);
    }

    fn check_in(&self, cols: u32, rows: u32, origin: u64) -> Result<(), SnapshotError> {
        if !self.in_capture {
            return Ok(());
        }
        let out_of_bounds = || SnapshotError::CursorOutOfBounds {
            col: self.col,
            row: self.row,
            cols,
            rows,
        };
        let local_row = u64::from(self.row)
            .checked_sub(origin)
            .ok_or_else(out_of_bounds)?;
        if self.col >= cols || local_row >= u64::from(rows) {
            return Err(SnapshotError::CursorOutOfBounds {
                col: self.col,
                row: self.row,
                cols,
                rows,
            });
        }
        Ok(())
    }
}

/// Where a snapshot came from. Kept as observation metadata, outside
/// canonical equality and hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSource {
    /// Captured from a live terminal under one state lock.
    Live,
    /// Built offline through [`TerminalSnapshot::try_new`] or deserialization.
    #[default]
    Offline,
}

/// Observation metadata: when and where a snapshot was taken.
///
/// Every field is observation-only: changing metadata never changes canonical
/// equality or hashing. There are no invariants, so the fields are public.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", deny_unknown_fields, default)]
pub struct SnapshotMetadata {
    /// Milliseconds since the Unix epoch when the snapshot was taken.
    pub captured_at_ms: u64,
    /// Terminal visual revision at capture time (live captures only).
    pub revision: u64,
    /// Backend that produced the grid (`alacritty`, `ghostty`, ...).
    pub backend: String,
    /// Session the grid was captured from (live captures only).
    pub session: String,
    /// Whether the snapshot came from a live terminal or offline input.
    pub source: SnapshotSource,
}

/// The validated input bundle for [`TerminalSnapshot::try_new`].
///
/// Plain data with public fields; every invariant is checked by `try_new`,
/// which reports the offending field and coordinates.
#[derive(Debug, Clone)]
pub struct SnapshotParts {
    /// Logical terminal columns.
    pub cols: u32,
    /// Logical terminal rows for the selected scope.
    pub rows: u32,
    /// Which terminal region the cells cover.
    pub scope: SnapshotScope,
    /// Full-grid index of the first captured row.
    pub viewport_origin: u64,
    /// Row-major cells, exactly `cols * rows` entries.
    pub cells: Vec<SnapshotCell>,
    /// Every resolved palette entry, indices 0-255.
    pub palette: [Rgb; 256],
    /// Resolved default foreground (`OSC 10` applied).
    pub default_foreground: Rgb,
    /// Resolved default background (`OSC 11` applied).
    pub default_background: Rgb,
    /// Resolved cursor color (`OSC 12` applied).
    pub default_cursor: Rgb,
    /// The captured cursor, in full-grid coordinates.
    pub cursor: SnapshotCursor,
    /// Window title (`None` when the program set none).
    pub title: Option<String>,
    /// Observation metadata, outside canonical equality.
    pub metadata: SnapshotMetadata,
}

/// A snapshot validation failure, naming the field or coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// No columns or rows (a snapshot always covers a non-empty grid).
    EmptyDimensions,
    /// A dimension exceeds [`MAX_SNAPSHOT_DIM`].
    DimensionTooLarge { cols: u64, rows: u64, limit: u32 },
    /// `cols * rows` exceeds [`MAX_SNAPSHOT_CELLS`].
    TooManyCells { cells: u64, limit: u64 },
    /// The cell vector length is not `cols * rows`.
    CellCountMismatch { expected: u64, actual: usize },
    /// A wire palette that does not hold exactly 256 resolved entries.
    PaletteLengthMismatch { actual: usize },
    /// A grid row does not hold exactly `cols` cells (live capture only).
    MalformedRow {
        row: u32,
        expected: u32,
        actual: usize,
    },
    /// A cell grapheme exceeds [`MAX_CELL_TEXT_BYTES`].
    CellTextTooLarge {
        col: u32,
        row: u32,
        bytes: usize,
        limit: usize,
    },
    /// A cell grapheme exceeds [`MAX_CELL_TEXT_BYTES`] (coordinate-free).
    OversizeText { bytes: usize, limit: usize },
    /// A continuation cell (empty text) at column zero, or a non-continuation
    /// width on empty text.
    BadContinuation { col: u32, row: u32 },
    /// A hyperlink URI or identity exceeds [`MAX_LINK_BYTES`].
    LinkTooLarge {
        col: u32,
        row: u32,
        bytes: usize,
        limit: usize,
    },
    /// A hyperlink URI or identity exceeds [`MAX_LINK_BYTES`]
    /// (coordinate-free).
    OversizeLink { bytes: usize, limit: usize },
    /// An in-capture cursor outside the captured grid.
    CursorOutOfBounds {
        col: u32,
        row: u32,
        cols: u32,
        rows: u32,
    },
    /// A nonzero viewport origin on a full-history snapshot.
    BadViewportOrigin { scope: SnapshotScope, origin: u64 },
    /// The window title exceeds [`MAX_TITLE_BYTES`].
    TitleTooLarge { bytes: usize, limit: usize },
    /// A count that does not fit its narrow terminal interface.
    CountOverflow { field: &'static str },
    /// A wire payload with an unknown schema version.
    UnsupportedSchema { version: u32 },
    /// A wire color that is neither `default`, an index, nor `#rrggbb`.
    InvalidColor { value: String },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::EmptyDimensions => f.write_str("snapshot dimensions must be nonzero"),
            SnapshotError::DimensionTooLarge { cols, rows, limit } => {
                write!(
                    f,
                    "snapshot dimensions {cols}x{rows} exceed the limit of {limit}"
                )
            }
            SnapshotError::TooManyCells { cells, limit } => {
                write!(
                    f,
                    "snapshot cell count {cells} exceeds the limit of {limit}"
                )
            }
            SnapshotError::CellCountMismatch { expected, actual } => {
                write!(
                    f,
                    "snapshot holds {actual} cells for a {expected}-cell grid"
                )
            }
            SnapshotError::PaletteLengthMismatch { actual } => {
                write!(f, "snapshot palette holds {actual} entries, expected 256")
            }
            SnapshotError::MalformedRow {
                row,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "snapshot row {row} holds {actual} cells, expected {expected}"
                )
            }
            SnapshotError::CellTextTooLarge {
                col,
                row,
                bytes,
                limit,
            } => {
                write!(
                    f,
                    "snapshot cell ({col}, {row}) holds {bytes} bytes, limit {limit}"
                )
            }
            SnapshotError::OversizeText { bytes, limit } => {
                write!(f, "snapshot cell text holds {bytes} bytes, limit {limit}")
            }
            SnapshotError::BadContinuation { col, row } => {
                write!(
                    f,
                    "snapshot cell ({col}, {row}) is not a valid continuation cell"
                )
            }
            SnapshotError::LinkTooLarge {
                col,
                row,
                bytes,
                limit,
            } => {
                write!(
                    f,
                    "snapshot cell ({col}, {row}) link holds {bytes} bytes, limit {limit}"
                )
            }
            SnapshotError::OversizeLink { bytes, limit } => {
                write!(f, "snapshot link holds {bytes} bytes, limit {limit}")
            }
            SnapshotError::CursorOutOfBounds {
                col,
                row,
                cols,
                rows,
            } => {
                write!(
                    f,
                    "in-capture cursor ({col}, {row}) falls outside a {cols}x{rows} grid"
                )
            }
            SnapshotError::BadViewportOrigin { scope, origin } => {
                write!(
                    f,
                    "snapshot scope {} must start at origin 0 (got {origin})",
                    scope.name()
                )
            }
            SnapshotError::TitleTooLarge { bytes, limit } => {
                write!(f, "snapshot title holds {bytes} bytes, limit {limit}")
            }
            SnapshotError::CountOverflow { field } => {
                write!(
                    f,
                    "snapshot count {field} does not fit its terminal interface"
                )
            }
            SnapshotError::UnsupportedSchema { version } => {
                write!(
                    f,
                    "unsupported snapshot schema {version} (this build reads {SNAPSHOT_SCHEMA_VERSION})"
                )
            }
            SnapshotError::InvalidColor { value } => {
                write!(f, "invalid snapshot color (got {value:?})")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<SnapshotError> for crate::api::TuiTestError {
    fn from(error: SnapshotError) -> Self {
        crate::api::TuiTestError::internal(error.to_string())
    }
}

/// The owned terminal snapshot: validated grid, colors, cursor, and title.
///
/// Fields are private; read-only accessors expose the data. Canonical
/// equality and hashing cover the terminal state (dimensions, cells, palette,
/// defaults, cursor, title, scope, origin) and skip [`SnapshotMetadata`]:
/// re-observing the same terminal state compares equal however it was
/// observed.
#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    cols: u32,
    rows: u32,
    scope: SnapshotScope,
    viewport_origin: u64,
    cells: Vec<SnapshotCell>,
    palette: [Rgb; 256],
    default_foreground: Rgb,
    default_background: Rgb,
    default_cursor: Rgb,
    cursor: SnapshotCursor,
    title: Option<String>,
    metadata: SnapshotMetadata,
}

impl TerminalSnapshot {
    /// Build a validated snapshot from parts.
    ///
    /// Rejects empty dimensions, oversize input, ragged cells, invalid
    /// continuations, out-of-bounds in-capture cursors, misplaced origins,
    /// and oversize titles and links, using checked arithmetic throughout.
    /// Cell geometry is recorded, never recomputed from UTF-8 byte length.
    pub fn try_new(parts: SnapshotParts) -> Result<Self, SnapshotError> {
        assemble(parts)
    }

    /// Logical terminal columns.
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// Logical terminal rows for the captured scope.
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Which terminal region the cells cover.
    pub fn scope(&self) -> SnapshotScope {
        self.scope
    }

    /// Full-grid index of the first captured row.
    pub fn viewport_origin(&self) -> u64 {
        self.viewport_origin
    }

    /// Row-major cells, exactly `cols * rows` entries.
    pub fn cells(&self) -> &[SnapshotCell] {
        &self.cells
    }

    /// One grid row, or `None` past the last row.
    pub fn row(&self, row: u32) -> Option<&[SnapshotCell]> {
        let start = (u64::from(row)).checked_mul(u64::from(self.cols))?;
        let start = usize::try_from(start).ok()?;
        let end = start.checked_add(self.cols as usize)?;
        self.cells.get(start..end)
    }

    /// The cell at `(col, row)`, or `None` outside the grid.
    pub fn cell_at(&self, col: u32, row: u32) -> Option<&SnapshotCell> {
        self.row(row)?.get(col as usize)
    }

    /// Every resolved palette entry, indices 0-255.
    pub fn palette(&self) -> &[Rgb; 256] {
        &self.palette
    }

    /// Resolved default foreground (`OSC 10` applied).
    pub fn default_foreground(&self) -> Rgb {
        self.default_foreground
    }

    /// Resolved default background (`OSC 11` applied).
    pub fn default_background(&self) -> Rgb {
        self.default_background
    }

    /// Resolved cursor color (`OSC 12` applied).
    pub fn default_cursor(&self) -> Rgb {
        self.default_cursor
    }

    /// The captured cursor, in full-grid coordinates.
    pub fn cursor(&self) -> &SnapshotCursor {
        &self.cursor
    }

    /// Capture-local cursor coordinates, or `None` outside the capture.
    pub fn cursor_local(&self) -> Option<(u32, u32)> {
        if !self.cursor.in_capture {
            return None;
        }
        let row64 = u64::from(self.cursor.row).checked_sub(self.viewport_origin)?;
        let row = u32::try_from(row64).ok()?;
        Some((self.cursor.col, row))
    }

    /// Window title (`None` when the program set none).
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Observation metadata (capture time, revision, identities, source).
    pub fn metadata(&self) -> &SnapshotMetadata {
        &self.metadata
    }
}

impl TryFrom<SnapshotParts> for TerminalSnapshot {
    type Error = SnapshotError;

    fn try_from(parts: SnapshotParts) -> Result<Self, Self::Error> {
        TerminalSnapshot::try_new(parts)
    }
}

impl PartialEq for TerminalSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.cols == other.cols
            && self.rows == other.rows
            && self.scope == other.scope
            && self.viewport_origin == other.viewport_origin
            && self.cells == other.cells
            && self.palette == other.palette
            && self.default_foreground == other.default_foreground
            && self.default_background == other.default_background
            && self.default_cursor == other.default_cursor
            && self.cursor == other.cursor
            && self.title == other.title
    }
}

impl Eq for TerminalSnapshot {}

/// Hash one resolved color field by field (`Rgb` itself has no `Hash` impl).
fn hash_rgb<H: Hasher>(color: &Rgb, state: &mut H) {
    color.r.hash(state);
    color.g.hash(state);
    color.b.hash(state);
}

impl Hash for TerminalSnapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cols.hash(state);
        self.rows.hash(state);
        self.scope.hash(state);
        self.viewport_origin.hash(state);
        self.cells.hash(state);
        for entry in &self.palette {
            hash_rgb(entry, state);
        }
        hash_rgb(&self.default_foreground, state);
        hash_rgb(&self.default_background, state);
        hash_rgb(&self.default_cursor, state);
        self.cursor.hash_fields(state);
        self.title.hash(state);
    }
}

impl Hash for SnapshotCursor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_fields(state);
    }
}

/// Versioned wire form. Private: Serde on [`TerminalSnapshot`] goes through
/// it, so constructors and deserialization share [`assemble`] validation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct WireSnapshot {
    #[serde(default)]
    schema: u32,
    cols: u32,
    rows: u32,
    scope: SnapshotScope,
    viewport_origin: u64,
    cells: Vec<SnapshotCell>,
    /// A sequence on the wire: serde has no array impl for 256 entries.
    palette: Vec<Rgb>,
    default_foreground: Rgb,
    default_background: Rgb,
    default_cursor: Rgb,
    cursor: SnapshotCursor,
    title: Option<String>,
    #[serde(default)]
    metadata: SnapshotMetadata,
}

impl Serialize for TerminalSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireSnapshot {
            schema: SNAPSHOT_SCHEMA_VERSION,
            cols: self.cols,
            rows: self.rows,
            scope: self.scope,
            viewport_origin: self.viewport_origin,
            cells: self.cells.clone(),
            palette: self.palette.to_vec(),
            default_foreground: self.default_foreground,
            default_background: self.default_background,
            default_cursor: self.default_cursor,
            cursor: self.cursor.clone(),
            title: self.title.clone(),
            metadata: self.metadata.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TerminalSnapshot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WireSnapshot::deserialize(deserializer)?;
        if wire.schema != SNAPSHOT_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(
                SnapshotError::UnsupportedSchema {
                    version: wire.schema,
                }
                .to_string(),
            ));
        }
        let palette: [Rgb; 256] = wire.palette.try_into().map_err(|palette: Vec<Rgb>| {
            serde::de::Error::custom(
                SnapshotError::PaletteLengthMismatch {
                    actual: palette.len(),
                }
                .to_string(),
            )
        })?;
        assemble(SnapshotParts {
            cols: wire.cols,
            rows: wire.rows,
            scope: wire.scope,
            viewport_origin: wire.viewport_origin,
            cells: wire.cells,
            palette,
            default_foreground: wire.default_foreground,
            default_background: wire.default_background,
            default_cursor: wire.default_cursor,
            cursor: wire.cursor,
            title: wire.title,
            metadata: wire.metadata,
        })
        .map_err(serde::de::Error::custom)
    }
}

/// Shared validation for [`TerminalSnapshot::try_new`] and deserialization.
fn assemble(parts: SnapshotParts) -> Result<TerminalSnapshot, SnapshotError> {
    let SnapshotParts {
        cols,
        rows,
        scope,
        viewport_origin,
        cells,
        palette,
        default_foreground,
        default_background,
        default_cursor,
        cursor,
        title,
        metadata,
    } = parts;
    if cols == 0 || rows == 0 {
        return Err(SnapshotError::EmptyDimensions);
    }
    if cols > MAX_SNAPSHOT_DIM || rows > MAX_SNAPSHOT_DIM {
        return Err(SnapshotError::DimensionTooLarge {
            cols: u64::from(cols),
            rows: u64::from(rows),
            limit: MAX_SNAPSHOT_DIM,
        });
    }
    let count =
        u64::from(cols)
            .checked_mul(u64::from(rows))
            .ok_or(SnapshotError::CountOverflow {
                field: "cols * rows",
            })?;
    if count > MAX_SNAPSHOT_CELLS {
        return Err(SnapshotError::TooManyCells {
            cells: count,
            limit: MAX_SNAPSHOT_CELLS,
        });
    }
    if cells.len() as u64 != count {
        return Err(SnapshotError::CellCountMismatch {
            expected: count,
            actual: cells.len(),
        });
    }
    for (index, cell) in cells.iter().enumerate() {
        let col = (index as u32) % cols;
        let row = (index as u32) / cols;
        cell.check_at(col, row)?;
    }
    if scope == SnapshotScope::FullHistory && viewport_origin != 0 {
        return Err(SnapshotError::BadViewportOrigin {
            scope,
            origin: viewport_origin,
        });
    }
    if let Some(title) = title.as_deref() {
        if title.len() > MAX_TITLE_BYTES {
            return Err(SnapshotError::TitleTooLarge {
                bytes: title.len(),
                limit: MAX_TITLE_BYTES,
            });
        }
    }
    cursor.check_in(cols, rows, viewport_origin)?;
    Ok(TerminalSnapshot {
        cols,
        rows,
        scope,
        viewport_origin,
        cells,
        palette,
        default_foreground,
        default_background,
        default_cursor,
        cursor,
        title,
        metadata,
    })
}

/// Capture options: scope plus an explicit highlighting opt-in.
///
/// The default capture records raw visual data. A highlighted capture needs
/// [`SnapshotOptions::with_highlight`] set to `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotOptions {
    scope: SnapshotScope,
    highlight: bool,
}

impl SnapshotOptions {
    /// Capture the selected scope with raw visual data.
    pub fn new(scope: SnapshotScope) -> Self {
        SnapshotOptions {
            scope,
            highlight: false,
        }
    }

    /// Which terminal region to capture.
    pub fn scope(&self) -> SnapshotScope {
        self.scope
    }

    /// Whether locator highlighting is folded into the captured cells.
    pub fn highlighted(&self) -> bool {
        self.highlight
    }

    /// Opt into (or out of) folding locator highlighting into the capture.
    pub fn with_highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }
}

impl From<SnapshotScope> for SnapshotOptions {
    fn from(scope: SnapshotScope) -> Self {
        SnapshotOptions::new(scope)
    }
}

/// Highlighted cells in full-grid coordinates, borrowed from terminal state.
///
/// The caller holds the terminal-state lock for the whole capture; this only
/// carries the reference across.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LiveHighlight<'a> {
    pub cells: &'a [(usize, usize)],
}

/// Capture from a live emulator. The caller holds the terminal-state lock;
/// every field below (grid, cursor, dimensions, title, resolved colors) is
/// read under that one lock, so the snapshot is one atomic terminal moment.
///
/// Highlighting applies only when `options.highlighted()` is set and a
/// highlight is supplied; the default capture keeps raw visual data.
pub(crate) fn capture_live(
    emu: &dyn Emulator,
    highlight: Option<LiveHighlight<'_>>,
    options: &SnapshotOptions,
    metadata: SnapshotMetadata,
) -> Result<TerminalSnapshot, SnapshotError> {
    use crate::terminal::cell::display_width;

    let (cols16, _) = emu.size();
    let cols = u32::from(cols16);
    if cols == 0 {
        return Err(SnapshotError::EmptyDimensions);
    }
    if cols > MAX_SNAPSHOT_DIM {
        return Err(SnapshotError::DimensionTooLarge {
            cols: u64::from(cols),
            rows: 0,
            limit: MAX_SNAPSHOT_DIM,
        });
    }
    let viewport = emu.viewport();
    let history = viewport.history as u64;
    let (rows_data, origin) = match options.scope() {
        SnapshotScope::Viewport => (emu.viewable_rows(), viewport.origin() as u64),
        SnapshotScope::ActiveScreen => (emu.active_rows(), history),
        SnapshotScope::FullHistory => (emu.full_rows(), 0),
    };
    let row_count = rows_data.len() as u64;
    if row_count == 0 {
        return Err(SnapshotError::EmptyDimensions);
    }
    if row_count > u64::from(MAX_SNAPSHOT_DIM) {
        return Err(SnapshotError::DimensionTooLarge {
            cols: u64::from(cols),
            rows: row_count,
            limit: MAX_SNAPSHOT_DIM,
        });
    }
    let rows = row_count as u32;
    let count = row_count
        .checked_mul(u64::from(cols))
        .ok_or(SnapshotError::CountOverflow {
            field: "cols * rows",
        })?;
    if count > MAX_SNAPSHOT_CELLS {
        return Err(SnapshotError::TooManyCells {
            cells: count,
            limit: MAX_SNAPSHOT_CELLS,
        });
    }

    let highlighted: Option<HashSet<(usize, u64)>> = if options.highlighted() {
        highlight.map(|selected| selected.cells.iter().map(|&(x, y)| (x, y as u64)).collect())
    } else {
        None
    };

    let mut cells = Vec::new();
    cells
        .try_reserve(rows_data.len().saturating_mul(cols as usize))
        .map_err(|_| SnapshotError::CountOverflow {
            field: "snapshot cell buffer",
        })?;
    for (row_index, row) in rows_data.iter().enumerate() {
        if row.len() as u64 != u64::from(cols) {
            return Err(SnapshotError::MalformedRow {
                row: row_index as u32,
                expected: cols,
                actual: row.len(),
            });
        }
        let full_row = origin.saturating_add(row_index as u64);
        for (col_index, emu_cell) in row.iter().enumerate() {
            let mut attrs = emu_cell.attrs;
            if let Some(selected) = highlighted.as_ref() {
                if selected.contains(&(col_index, full_row)) {
                    attrs.toggle(Attrs::INVERSE);
                }
            }
            let (text, width) = if emu_cell.ch.as_str() == CONTINUATION {
                (String::new(), 0)
            } else {
                let width = u16::try_from(display_width(emu_cell.ch.as_str())).map_err(|_| {
                    SnapshotError::CountOverflow {
                        field: "cell width",
                    }
                })?;
                (emu_cell.ch.to_string(), width)
            };
            cells.push(
                SnapshotCell::new(
                    text,
                    width,
                    SnapshotColor::from(emu_cell.fg),
                    SnapshotColor::from(emu_cell.bg),
                    attrs,
                    SnapshotUnderline::from(emu_cell.underline),
                    SnapshotColor::from(emu_cell.underline_color),
                    emu_cell.hyperlink.as_ref().map(|link| link.uri.to_string()),
                    emu_cell
                        .hyperlink
                        .as_ref()
                        .and_then(|link| link.id.as_deref())
                        .map(str::to_string),
                )
                .map_err(|error| match error {
                    SnapshotError::OversizeText { bytes, limit } => {
                        SnapshotError::CellTextTooLarge {
                            col: col_index as u32,
                            row: row_index as u32,
                            bytes,
                            limit,
                        }
                    }
                    SnapshotError::OversizeLink { bytes, limit } => SnapshotError::LinkTooLarge {
                        col: col_index as u32,
                        row: row_index as u32,
                        bytes,
                        limit,
                    },
                    other => other,
                })?,
            );
        }
    }

    let (cursor_x, cursor_y) = emu.cursor();
    let cursor_row =
        history
            .checked_add(u64::from(cursor_y))
            .ok_or(SnapshotError::CountOverflow {
                field: "cursor row",
            })?;
    let cursor_row32: u32 =
        cursor_row
            .try_into()
            .map_err(|_| SnapshotError::CountOverflow {
                field: "cursor row",
            })?;
    let cursor_col = u32::from(cursor_x);
    let in_capture = match options.scope() {
        SnapshotScope::Viewport => {
            cursor_row
                .checked_sub(origin)
                .is_some_and(|local| local < row_count)
                && cursor_col < cols
        }
        SnapshotScope::ActiveScreen => {
            if u64::from(cursor_y) >= row_count || cursor_col >= cols {
                return Err(SnapshotError::CursorOutOfBounds {
                    col: cursor_col,
                    row: cursor_row32,
                    cols,
                    rows,
                });
            }
            true
        }
        SnapshotScope::FullHistory => {
            if cursor_row >= row_count || cursor_col >= cols {
                return Err(SnapshotError::CursorOutOfBounds {
                    col: cursor_col,
                    row: cursor_row32,
                    cols,
                    rows,
                });
            }
            true
        }
    };

    let table = emu.colors();
    assemble(SnapshotParts {
        cols,
        rows,
        scope: options.scope(),
        viewport_origin: origin,
        cells,
        palette: table.palette,
        default_foreground: table.foreground,
        default_background: table.background,
        default_cursor: table.cursor,
        cursor: SnapshotCursor::new(
            cursor_col,
            cursor_row32,
            in_capture,
            emu.cursor_visible(),
            SnapshotCursorShape::from(emu.cursor_shape()),
            emu.color(ColorSlot::Cursor),
            None,
        ),
        title: emu.title(),
        metadata,
    })
}

/// Current Unix time in milliseconds; zero when the clock is unavailable.
pub(crate) fn capture_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}
