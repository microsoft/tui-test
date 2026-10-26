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
    ///
    /// A continuation holds empty text with width zero and sits inside an
    /// open span; see [`TerminalSnapshot::try_new`] for the span rules,
    /// including the clipped-edge representation at a row end.
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

    /// Per-cell size checks. Continuation placement is structural (it needs
    /// the row's span state) and lives in [`check_row_spans`], which
    /// [`assemble`] runs over the same cells right after this.
    fn check_at(&self, col: u32, row: u32) -> Result<(), SnapshotError> {
        if self.text.len() > MAX_CELL_TEXT_BYTES {
            return Err(SnapshotError::CellTextTooLarge {
                col,
                row,
                bytes: self.text.len(),
                limit: MAX_CELL_TEXT_BYTES,
            });
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
    /// A cell that breaks the row's wide-character span: a continuation with
    /// no open span (including any continuation at column zero, since spans
    /// never cross rows), a continuation carrying a nonzero width, or a
    /// leading cell inside an unfinished span (a missing continuation).
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
    ///
    /// Wide-character spans are validated per row with an explicit
    /// remaining-span counter. A leading cell with width `w > 1` opens a
    /// span covering the next `w - 1` cells, each of which must be a
    /// continuation (empty text, width zero). A continuation with no open
    /// span is rejected, as is a leading cell inside an unfinished span.
    /// Spans never cross a row boundary: every row starts outside any span,
    /// so a continuation at column zero is always invalid. A leading cell
    /// whose span would extend past its row's last column is the supported
    /// clipped-edge representation: it keeps its full recorded width and
    /// the continuations past the edge are omitted, so a trailing open span
    /// at row end is accepted. A leading cell with width zero (for example
    /// a lone combining mark or zero-width space, which xterm.js reports as
    /// a genuine cell owning its column) opens no span.
    ///
    /// ```
    /// use tui_test::profile::Rgb;
    /// use tui_test::terminal::cell::Attrs;
    /// use tui_test::{
    ///     SnapshotCell, SnapshotColor, SnapshotCursor, SnapshotCursorShape,
    ///     SnapshotMetadata, SnapshotParts, SnapshotScope, TerminalSnapshot,
    /// };
    ///
    /// let palette = [Rgb::new(0, 0, 0); 256];
    /// let cell = SnapshotCell::new(
    ///     "x".to_string(),
    ///     1,
    ///     SnapshotColor::Default,
    ///     SnapshotColor::Indexed(1),
    ///     Attrs::BOLD,
    ///     tui_test::SnapshotUnderline::None,
    ///     SnapshotColor::Default,
    ///     None,
    ///     None,
    /// )?;
    /// let snapshot = TerminalSnapshot::try_new(SnapshotParts {
    ///     cols: 1,
    ///     rows: 1,
    ///     scope: SnapshotScope::Viewport,
    ///     viewport_origin: 0,
    ///     cells: vec![cell],
    ///     palette,
    ///     default_foreground: Rgb::new(255, 255, 255),
    ///     default_background: Rgb::new(0, 0, 0),
    ///     default_cursor: Rgb::new(255, 255, 255),
    ///     cursor: SnapshotCursor::new(
    ///         0, 0, true, true, SnapshotCursorShape::Block,
    ///         Rgb::new(255, 255, 255), None,
    ///     ),
    ///     title: None,
    ///     metadata: SnapshotMetadata::default(),
    /// })?;
    /// assert_eq!(snapshot.cell_at(0, 0).unwrap().text(), "x");
    /// # Ok::<(), tui_test::SnapshotError>(())
    /// ```
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

/// Validate one row's wide-character spans with an explicit counter of how
/// many continuation cells the current span still covers (`remaining`).
///
/// A leading cell with width `w > 1` opens a span of `w - 1` continuations;
/// each continuation consumes one. A continuation with nothing to consume is
/// uncovered (an orphan), and a leading cell that arrives while `remaining`
/// is nonzero sits inside an unfinished span (a missing continuation). A
/// span that would run past the row's last column is the supported
/// clipped-edge representation: the leading cell keeps its full recorded
/// width and the past-the-edge continuations are omitted, so `remaining` may
/// still be nonzero when the row ends. Spans never cross rows: callers reset
/// the counter by validating each row separately, so a continuation at
/// column zero always fails as uncovered.
fn check_row_spans(row_cells: &[SnapshotCell], row: u32) -> Result<(), SnapshotError> {
    let mut remaining: u32 = 0;
    for (col, cell) in row_cells.iter().enumerate() {
        let col = col as u32;
        if cell.is_continuation() {
            if cell.width() != 0 || remaining == 0 {
                return Err(SnapshotError::BadContinuation { col, row });
            }
            remaining -= 1;
        } else {
            if remaining > 0 {
                return Err(SnapshotError::BadContinuation { col, row });
            }
            remaining = u32::from(cell.width()).saturating_sub(1);
        }
    }
    Ok(())
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
    // `cols` is nonzero and `cells.len() == cols * rows`, so the chunks
    // cover every cell exactly once.
    for (row, chunk) in cells.chunks_exact(cols as usize).enumerate() {
        let row = row as u32;
        for (col, cell) in chunk.iter().enumerate() {
            cell.check_at(col as u32, row)?;
        }
        check_row_spans(chunk, row)?;
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

/// Reconcile a recomputed glyph width with the backend's occupancy record.
///
/// `run` is the number of continuation cells directly behind this one in the
/// same backend row: the columns the backend actually gave the glyph. When
/// recompute and backend agree, the measured width stands. When the backend
/// is wider, the measured width would orphan attested continuations, so the
/// span covers them. When the backend is narrower, the span collapses to the
/// occupied columns — unless the measured span would run past the row edge,
/// which stays the supported clipped-edge representation (the leading cell
/// keeps its full width and the past-the-edge continuations are omitted). A
/// zero-width grapheme keeps width zero: it opens no span, and a spacer
/// behind one would be backend-inconsistent data that validation still
/// rejects.
fn reconcile_live_width(measured: u16, run: u16, col: u32, cols: u32) -> u16 {
    if measured == 0 {
        return 0;
    }
    let occupied = run.saturating_add(1);
    if measured <= occupied {
        return occupied;
    }
    if u64::from(col).saturating_add(u64::from(measured)) > u64::from(cols) {
        return measured;
    }
    occupied
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
                let measured =
                    u16::try_from(display_width(emu_cell.ch.as_str())).map_err(|_| {
                        SnapshotError::CountOverflow {
                            field: "cell width",
                        }
                    })?;
                // The backend attests the occupied columns: every spacer
                // directly behind this cell is one of them. The recomputed
                // width can disagree (heart+VS16 and keycap sequences
                // measure two columns but occupy one here), so reconcile
                // against the attested run instead of emitting a span the
                // next real cell cannot cover.
                let mut run: u16 = 0;
                for follower in row.iter().skip(col_index + 1) {
                    if follower.ch.as_str() != CONTINUATION {
                        break;
                    }
                    run = run.saturating_add(1);
                }
                let width = reconcile_live_width(measured, run, col_index as u32, cols);
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
    let cursor_row32: u32 = cursor_row
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Profile, Rgb};
    use crate::terminal::alacritty::AlacrittyEmu;
    use std::collections::hash_map::DefaultHasher;
    use std::sync::{Arc, Barrier, Mutex};

    const COLS: u16 = 10;
    const ROWS: u16 = 6;

    fn profile() -> Profile {
        Profile::default()
    }

    fn resolved_palette(profile: &Profile) -> [Rgb; 256] {
        std::array::from_fn(|index| profile.colors.rgb(index as u8))
    }

    fn blank_parts(profile: &Profile, cols: u32, rows: u32) -> SnapshotParts {
        let palette = resolved_palette(profile);
        SnapshotParts {
            cols,
            rows,
            scope: SnapshotScope::Viewport,
            viewport_origin: 0,
            cells: (0..cols * rows)
                .map(|_| {
                    SnapshotCell::new(
                        " ".to_string(),
                        1,
                        SnapshotColor::Default,
                        SnapshotColor::Default,
                        Attrs::empty(),
                        SnapshotUnderline::None,
                        SnapshotColor::Default,
                        None,
                        None,
                    )
                    .expect("blank cell is valid")
                })
                .collect(),
            palette,
            default_foreground: palette[0],
            default_background: palette[0],
            default_cursor: palette[0],
            cursor: SnapshotCursor::new(
                0,
                0,
                true,
                true,
                SnapshotCursorShape::Block,
                palette[0],
                None,
            ),
            title: None,
            metadata: SnapshotMetadata::default(),
        }
    }

    fn canonical_hash(snapshot: &TerminalSnapshot) -> u64 {
        let mut hasher = DefaultHasher::new();
        snapshot.hash(&mut hasher);
        hasher.finish()
    }

    /// Feed the Unicode corpus: combining marks, wide char with continuation,
    /// bold/italic/bold-italic, and a wide character at the last column.
    fn feed_corpus(emu: &mut AlacrittyEmu) {
        emu.process(b"\x1b[2J\x1b[H");
        // e + combining acute, a + combining diaeresis, wide char, styles.
        emu.process("é ä 東 \x1b[1mbold\x1b[0m".as_bytes());
        emu.process(b"\r\n\x1b[3mitalic\x1b[0m \x1b[1;3mbi\x1b[0m");
        // Wide character exactly filling the last two columns. The earlier
        // wrap consumed an extra row, so the grid is one row deeper here.
        emu.process("\r\n        東".as_bytes());
        // Park the cursor over the wide character.
        emu.process(b"\x1b[4;9H");
    }

    #[test]
    fn corpus_cells_keep_recorded_geometry() {
        let mut emu = AlacrittyEmu::new(COLS, ROWS, &profile());
        feed_corpus(&mut emu);
        let snapshot = capture_live(
            &emu,
            None,
            &SnapshotOptions::new(SnapshotScope::Viewport),
            SnapshotMetadata::default(),
        )
        .expect("corpus capture succeeds");
        assert_eq!(
            (snapshot.cols(), snapshot.rows()),
            (u32::from(COLS), u32::from(ROWS))
        );

        let first = snapshot.row(0).expect("first row exists");
        assert_eq!(first[0].text(), "é");
        assert_eq!(first[0].width(), 1);
        assert_eq!(first[2].text(), "ä");
        assert_eq!(first[4].text(), "東");
        assert_eq!(first[4].width(), 2);
        assert!(first[5].is_continuation());
        assert_eq!(first[5].width(), 0);

        let bold = first
            .iter()
            .find(|cell| cell.text() == "b")
            .expect("bold text");
        assert!(bold.has(Attrs::BOLD));
        assert!(!bold.has(Attrs::ITALIC));

        // Style runs may wrap onto later rows; find them grid-wide.
        let styled: Vec<&SnapshotCell> = snapshot.cells().iter().collect();
        let bold_only = styled
            .iter()
            .find(|cell| cell.text() == "o" && cell.has(Attrs::BOLD) && !cell.has(Attrs::ITALIC))
            .expect("bold-only text");
        let _ = bold_only;
        let italic_only = styled
            .iter()
            .find(|cell| cell.text() == "t" && cell.has(Attrs::ITALIC) && !cell.has(Attrs::BOLD))
            .expect("italic-only text");
        let _ = italic_only;
        let both = styled
            .iter()
            .find(|cell| cell.text() == "i" && cell.has(Attrs::BOLD | Attrs::ITALIC))
            .expect("bold-italic text");
        let _ = both;

        // Wide character at the last column boundary plus cursor over it.
        let wide_row = snapshot.row(3).expect("wide row exists");
        assert_eq!(wide_row[8].text(), "東");
        assert_eq!(wide_row[8].width(), 2);
        assert!(wide_row[9].is_continuation());
        assert_eq!(snapshot.cursor_local(), Some((8, 3)));
        // Blink is unknown at this backend: recorded as unknown, not false.
        assert_eq!(snapshot.cursor().blink(), None);
    }

    #[test]
    fn sgr_index_normalization_is_recorded() {
        let mut emu = AlacrittyEmu::new(COLS, ROWS, &profile());
        emu.process(b"\x1b[31mA\x1b[38;5;1mB\x1b[38;2;10;20;30mC\x1b[0mD");
        let snapshot = capture_live(
            &emu,
            None,
            &SnapshotOptions::new(SnapshotScope::Viewport),
            SnapshotMetadata::default(),
        )
        .expect("sgr capture succeeds");
        let row = snapshot.row(0).expect("first row exists");
        // Both SGR spellings of slot 1 land on the same recorded reference.
        assert_eq!(row[0].fg(), SnapshotColor::Indexed(1));
        assert_eq!(row[1].fg(), SnapshotColor::Indexed(1));
        assert_eq!(row[2].fg(), SnapshotColor::Rgb(10, 20, 30));
        assert_eq!(row[3].fg(), SnapshotColor::Default);
    }

    #[test]
    fn default_capture_ignores_highlighting() {
        let mut emu = AlacrittyEmu::new(COLS, ROWS, &profile());
        emu.process(b"highlight me");
        let selected = [(0usize, 0usize), (1, 0)];
        let highlight = LiveHighlight { cells: &selected };
        let metadata = SnapshotMetadata::default();
        let raw = capture_live(
            &emu,
            Some(highlight),
            &SnapshotOptions::new(SnapshotScope::Viewport),
            metadata.clone(),
        )
        .expect("raw capture succeeds");
        let marked = capture_live(
            &emu,
            Some(highlight),
            &SnapshotOptions::new(SnapshotScope::Viewport).with_highlight(true),
            metadata,
        )
        .expect("highlighted capture succeeds");
        assert!(!raw.cell_at(0, 0).expect("cell").has(Attrs::INVERSE));
        assert!(marked.cell_at(0, 0).expect("cell").has(Attrs::INVERSE));
        assert!(marked.cell_at(1, 0).expect("cell").has(Attrs::INVERSE));
        assert!(!marked.cell_at(2, 0).expect("cell").has(Attrs::INVERSE));
        assert_ne!(raw, marked);
    }

    /// U02-A: transitions change grid, cursor, dimensions, and palette while
    /// the terminal-state lock is held; barriers pin each capture between two
    /// locked changes, so every capture must equal one permitted state.
    #[test]
    fn atomic_capture_matches_one_permitted_state() {
        struct Transition {
            apply: Box<dyn Fn(&mut AlacrittyEmu) + Send + Sync + 'static>,
            scope: SnapshotScope,
        }

        let profile = profile();
        let emu = Arc::new(Mutex::new(AlacrittyEmu::new(COLS, ROWS, &profile)));
        let barrier = Arc::new(Barrier::new(2));

        let transitions = Arc::new(vec![
            Transition {
                apply: Box::new(|emu| {
                    feed_corpus(emu);
                    emu.process(b"\x1b[1;1H\x1b[7mR\x1b[0m");
                    emu.process(b"\x1b]4;1;#112233\x1b\\");
                    emu.process(b"\x1b]10;#010203\x1b\\\x1b]11;#040506\x1b\\\x1b]12;#070809\x1b\\");
                    emu.process(b"\x1b]0;snap-title\x1b\\");
                    emu.process(b"\x1b]8;id=u02a;https://example.com\x1b\\L\x1b]8;;\x1b\\");
                }),
                scope: SnapshotScope::Viewport,
            },
            Transition {
                apply: Box::new(|emu| {
                    emu.process(b"\x1b]4;1;#445566\x1b\\\x1b]4;200;#aabbcc\x1b\\");
                    emu.process(b"\x1b]10;#0a0b0c\x1b\\");
                    emu.process(b"\x1b[5;5H\x1b[?25l");
                    emu.process(b"\x1b]0;snap-title-2\x1b\\");
                }),
                scope: SnapshotScope::Viewport,
            },
            Transition {
                apply: Box::new(|emu| {
                    emu.resize(20, 8);
                    for line in 0..12 {
                        emu.process(format!("line {line:02}\r\n").as_bytes());
                    }
                    emu.scroll(crate::api::ScrollTarget::Lines(2));
                }),
                scope: SnapshotScope::Viewport,
            },
        ]);

        let writer_emu = Arc::clone(&emu);
        let writer_barrier = Arc::clone(&barrier);
        let writer_transitions = Arc::clone(&transitions);
        let writer = std::thread::spawn(move || {
            for transition in writer_transitions.iter() {
                {
                    let mut guard = writer_emu.lock().expect("writer locks the emulator");
                    (transition.apply)(&mut guard);
                }
                writer_barrier.wait();
                writer_barrier.wait();
            }
        });

        let mut permitted: Vec<TerminalSnapshot> = Vec::new();
        for transition in transitions.iter() {
            barrier.wait();
            let options = SnapshotOptions::new(transition.scope);
            let first = {
                let guard = emu.lock().expect("reader locks the emulator");
                capture_live(
                    &*guard,
                    None,
                    &options,
                    SnapshotMetadata {
                        captured_at_ms: 1,
                        revision: permitted.len() as u64,
                        backend: "alacritty".to_string(),
                        session: "u02-a".to_string(),
                        source: SnapshotSource::Live,
                    },
                )
                .expect("capture under lock succeeds")
            };
            let second = {
                let guard = emu.lock().expect("reader locks the emulator");
                capture_live(
                    &*guard,
                    None,
                    &options,
                    SnapshotMetadata {
                        captured_at_ms: 2,
                        revision: 100 + permitted.len() as u64,
                        backend: "other".to_string(),
                        session: "u02-a".to_string(),
                        source: SnapshotSource::Live,
                    },
                )
                .expect("recapture under lock succeeds")
            };
            // Same locked state, observation-only metadata differs: canonical
            // equality and hashing ignore it (C07).
            assert_eq!(first, second, "metadata-only change passes");
            assert_eq!(canonical_hash(&first), canonical_hash(&second));
            // The capture is stable: no tearing between the two locked reads.
            assert!(permitted
                .iter()
                .all(|known: &TerminalSnapshot| *known != first));
            assert!(permitted
                .iter()
                .all(|known: &TerminalSnapshot| *known != second));
            permitted.push(first);
            barrier.wait();
        }
        writer.join().expect("writer finishes");

        assert_eq!(permitted.len(), 3);

        // C04: corpus text, continuation, styles, and cursor survived T0.
        let base = &permitted[0];
        assert_eq!((base.cols(), base.rows()), (10, 6));
        assert_eq!(base.title(), Some("snap-title"));
        assert_eq!(base.palette()[1], Rgb::new(0x11, 0x22, 0x33));
        assert_eq!(base.default_foreground(), Rgb::new(0x01, 0x02, 0x03));
        assert_eq!(base.default_background(), Rgb::new(0x04, 0x05, 0x06));
        assert_eq!(base.default_cursor(), Rgb::new(0x07, 0x08, 0x09));
        let reversed = base.cell_at(0, 0).expect("reversed cell");
        assert_eq!(reversed.text(), "R");
        assert!(reversed.has(Attrs::INVERSE));
        // Full-history coordinates use u32, never u16.
        let full = {
            let guard = emu.lock().expect("reader locks the emulator");
            capture_live(
                &*guard,
                None,
                &SnapshotOptions::new(SnapshotScope::FullHistory),
                SnapshotMetadata::default(),
            )
            .expect("full-history capture succeeds")
        };
        assert_eq!(full.scope(), SnapshotScope::FullHistory);
        assert_eq!(full.viewport_origin(), 0);
        assert!(full.rows() >= base.rows());

        // C05: T1 changed palette, defaults, cursor visibility, and title.
        let changed = &permitted[1];
        assert_eq!(changed.palette()[1], Rgb::new(0x44, 0x55, 0x66));
        assert_eq!(changed.palette()[200], Rgb::new(0xaa, 0xbb, 0xcc));
        assert_eq!(changed.default_foreground(), Rgb::new(0x0a, 0x0b, 0x0c));
        assert!(!changed.cursor().visible());
        assert_eq!(changed.title(), Some("snap-title-2"));

        // T2 resized, grew history, and scrolled: the cursor is recorded
        // outside the viewport capture, unclamped, with no local coords.
        let scrolled = &permitted[2];
        assert_eq!((scrolled.cols(), scrolled.rows()), (20, 8));
        assert!(!scrolled.cursor().in_capture());
        assert_eq!(scrolled.cursor_local(), None);
        // The position is explicit, not clamped into the 20x8 grid.
        assert!(
            u64::from(scrolled.cursor().row())
                >= scrolled.viewport_origin() + u64::from(scrolled.rows())
        );
        assert_eq!(scrolled.viewport_origin(), {
            let guard = emu.lock().expect("reader locks the emulator");
            guard.viewport().origin() as u64
        });
        for row in 0..scrolled.rows() {
            assert_eq!(
                scrolled.row(row).expect("row exists").len() as u32,
                scrolled.cols(),
                "rectangular geometry after resize and scroll"
            );
        }
    }

    #[test]
    fn constructor_and_json_reject_identical_invalid_geometry() {
        let profile = profile();
        // Row-major length does not match cols * rows.
        let mut mismatch = blank_parts(&profile, 2, 2);
        mismatch.cells.pop();
        assert_eq!(
            TerminalSnapshot::try_new(mismatch.clone()),
            Err(SnapshotError::CellCountMismatch {
                expected: 4,
                actual: 3
            })
        );

        // Continuation cell at column zero.
        let mut bad_continuation = blank_parts(&profile, 2, 1);
        bad_continuation.cells[0] = SnapshotCell::new(
            String::new(),
            0,
            SnapshotColor::Default,
            SnapshotColor::Default,
            Attrs::empty(),
            SnapshotUnderline::None,
            SnapshotColor::Default,
            None,
            None,
        )
        .expect("coordinate-free construction succeeds");
        assert_eq!(
            TerminalSnapshot::try_new(bad_continuation),
            Err(SnapshotError::BadContinuation { col: 0, row: 0 })
        );

        // In-capture cursor outside the grid.
        let mut bad_cursor = blank_parts(&profile, 2, 2);
        bad_cursor.cursor = SnapshotCursor::new(
            7,
            0,
            true,
            true,
            SnapshotCursorShape::Block,
            Rgb::new(0, 0, 0),
            None,
        );
        assert_eq!(
            TerminalSnapshot::try_new(bad_cursor),
            Err(SnapshotError::CursorOutOfBounds {
                col: 7,
                row: 0,
                cols: 2,
                rows: 2
            })
        );

        // Full-history snapshots start at origin zero.
        let mut bad_origin = blank_parts(&profile, 2, 2);
        bad_origin.scope = SnapshotScope::FullHistory;
        bad_origin.viewport_origin = 3;
        assert_eq!(
            TerminalSnapshot::try_new(bad_origin),
            Err(SnapshotError::BadViewportOrigin {
                scope: SnapshotScope::FullHistory,
                origin: 3
            })
        );

        // The same malformed grids fail through JSON deserialization (U02-F).
        let valid = TerminalSnapshot::try_new(blank_parts(&profile, 2, 2)).expect("valid");
        let mut wire = serde_json::to_value(&valid).expect("serializes");
        wire["cells"].as_array_mut().expect("cells array").pop();
        assert!(serde_json::from_value::<TerminalSnapshot>(wire).is_err());

        let mut wire = serde_json::to_value(&valid).expect("serializes");
        wire["schema"] = serde_json::json!(999);
        let error = serde_json::from_value::<TerminalSnapshot>(wire).expect_err("schema 999 fails");
        assert!(
            error.to_string().contains("unsupported snapshot schema"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn wide_spans_need_covering_continuations() {
        let profile = profile();

        fn span_cell(text: &str, width: u16) -> SnapshotCell {
            SnapshotCell::new(
                text.to_string(),
                width,
                SnapshotColor::Default,
                SnapshotColor::Default,
                Attrs::empty(),
                SnapshotUnderline::None,
                SnapshotColor::Default,
                None,
                None,
            )
            .expect("coordinate-free construction succeeds")
        }

        // `try_new` over one row of `cells`; every case below is also
        // mirrored through JSON by splicing the same cells into a valid
        // wire payload, proving constructors and deserialization share the
        // validator.
        fn try_row(
            profile: &Profile,
            cells: Vec<SnapshotCell>,
        ) -> Result<TerminalSnapshot, SnapshotError> {
            let cols = cells.len() as u32;
            let mut parts = blank_parts(profile, cols, 1);
            parts.cells = cells;
            TerminalSnapshot::try_new(parts)
        }

        fn json_row(profile: &Profile, cells: Vec<SnapshotCell>) -> serde_json::Value {
            let cols = cells.len() as u32;
            let valid = TerminalSnapshot::try_new(blank_parts(profile, cols, 1)).expect("valid");
            let mut wire = serde_json::to_value(&valid).expect("serializes");
            wire["cells"] = serde_json::Value::Array(
                cells
                    .iter()
                    .map(|cell| serde_json::to_value(cell).expect("cell serializes"))
                    .collect(),
            );
            wire
        }

        fn assert_rejected(profile: &Profile, cells: Vec<SnapshotCell>, col: u32, row: u32) {
            assert_eq!(
                try_row(profile, cells.clone()),
                Err(SnapshotError::BadContinuation { col, row })
            );
            let error = serde_json::from_value::<TerminalSnapshot>(json_row(profile, cells))
                .expect_err("JSON rejects the same span");
            assert!(
                error.to_string().contains("continuation"),
                "unexpected error: {error}"
            );
        }

        // Orphan continuation: no open span covers column 1.
        assert_rejected(&profile, vec![span_cell("x", 1), span_cell("", 0)], 1, 0);
        // Missing continuation: a leading cell inside an unfinished span.
        assert_rejected(&profile, vec![span_cell("x", 2), span_cell("y", 1)], 1, 0);
        // Continuation carrying a nonzero width is not a continuation.
        assert_rejected(&profile, vec![span_cell("", 1)], 0, 0);
        // A width-3 span short one continuation.
        assert_rejected(
            &profile,
            vec![span_cell("a", 3), span_cell("", 0), span_cell("b", 1)],
            2,
            0,
        );

        // Covered spans pass through both doors.
        for cells in [
            vec![span_cell("東", 2), span_cell("", 0)],
            vec![span_cell("a", 3), span_cell("", 0), span_cell("", 0)],
            // Clipped edge: the span runs past the row end, so the
            // past-the-edge continuations are omitted.
            vec![span_cell("x", 1), span_cell("東", 2)],
            // A zero-width leading cell (lone combining mark) opens no span.
            vec![span_cell("́", 0), span_cell("x", 1)],
        ] {
            let snapshot = try_row(&profile, cells.clone()).expect("span is covered");
            let back: TerminalSnapshot =
                serde_json::from_value(json_row(&profile, cells)).expect("JSON accepts");
            assert_eq!(snapshot, back);
        }

        // Spans never cross rows: row 1 starts outside any span, so its
        // column-zero continuation is uncovered even though row 0 is clean.
        let mut cross = blank_parts(&profile, 2, 2);
        cross.cells = vec![
            span_cell("x", 1),
            span_cell("y", 1),
            span_cell("", 0),
            span_cell(" ", 1),
        ];
        assert_eq!(
            TerminalSnapshot::try_new(cross),
            Err(SnapshotError::BadContinuation { col: 0, row: 1 })
        );
    }

    #[test]
    fn oversize_input_is_rejected() {
        let profile = profile();
        assert_eq!(
            TerminalSnapshot::try_new(SnapshotParts {
                cols: MAX_SNAPSHOT_DIM + 1,
                ..blank_parts(&profile, 2, 2)
            }),
            Err(SnapshotError::DimensionTooLarge {
                cols: u64::from(MAX_SNAPSHOT_DIM + 1),
                rows: 2,
                limit: MAX_SNAPSHOT_DIM
            })
        );
        assert_eq!(
            TerminalSnapshot::try_new(SnapshotParts {
                cols: 0,
                ..blank_parts(&profile, 2, 2)
            }),
            Err(SnapshotError::EmptyDimensions)
        );
        // 32768 x 32768 exceeds the cell budget without exceeding a dimension.
        assert_eq!(
            TerminalSnapshot::try_new(SnapshotParts {
                cols: MAX_SNAPSHOT_DIM,
                rows: MAX_SNAPSHOT_DIM,
                ..blank_parts(&profile, 2, 2)
            }),
            Err(SnapshotError::TooManyCells {
                cells: u64::from(MAX_SNAPSHOT_DIM) * u64::from(MAX_SNAPSHOT_DIM),
                limit: MAX_SNAPSHOT_CELLS
            })
        );
        assert!(SnapshotCell::new(
            "x".repeat(MAX_CELL_TEXT_BYTES + 1),
            1,
            SnapshotColor::Default,
            SnapshotColor::Default,
            Attrs::empty(),
            SnapshotUnderline::None,
            SnapshotColor::Default,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn default_and_explicit_rgb_stay_distinct() {
        let profile = profile();
        let palette = resolved_palette(&profile);
        let mut defaulted = blank_parts(&profile, 1, 1);
        defaulted.default_foreground = palette[3];
        defaulted.cells[0] = SnapshotCell::new(
            "x".to_string(),
            1,
            SnapshotColor::Default,
            SnapshotColor::Default,
            Attrs::empty(),
            SnapshotUnderline::None,
            SnapshotColor::Default,
            None,
            None,
        )
        .expect("cell is valid");
        let mut explicit = defaulted.clone();
        explicit.cells[0] = SnapshotCell::new(
            "x".to_string(),
            1,
            SnapshotColor::Rgb(palette[3].r, palette[3].g, palette[3].b),
            SnapshotColor::Default,
            Attrs::empty(),
            SnapshotUnderline::None,
            SnapshotColor::Default,
            None,
            None,
        )
        .expect("cell is valid");
        let a = TerminalSnapshot::try_new(defaulted).expect("valid");
        let b = TerminalSnapshot::try_new(explicit).expect("valid");
        // Equal resolved colors do not erase different color-reference kinds.
        assert_ne!(a, b);
        assert_eq!(a.cells()[0].fg(), SnapshotColor::Default);
    }

    #[test]
    fn unreferenced_palette_entry_changes_canonical_state() {
        let profile = profile();
        let left = TerminalSnapshot::try_new(blank_parts(&profile, 1, 1)).expect("valid");
        let mut parts = blank_parts(&profile, 1, 1);
        parts.palette[77] = Rgb::new(1, 2, 3);
        let right = TerminalSnapshot::try_new(parts).expect("valid");
        assert_ne!(left, right);
        assert_ne!(canonical_hash(&left), canonical_hash(&right));
    }

    /// Live capture records the columns the backend actually occupied.
    ///
    /// Heart+VS16 and the keycap sequence measure two columns under
    /// `display_width`, but the backend lays each out in a single column
    /// with no spacer cell. Emitting the recomputed width opens a
    /// two-column span the next real cell cannot cover, which failed with
    /// `BadContinuation { col: 1, row: 0 }`; the capture must record the
    /// single occupied column instead. A genuine wide glyph beside them
    /// keeps its width-two span and continuation.
    #[test]
    fn live_capture_matches_backend_occupancy_for_vs16_sequences() {
        let mut emu = AlacrittyEmu::new(COLS, ROWS, &profile());
        emu.process("\u{2764}\u{fe0f}1\u{fe0f}\u{20e3}東".as_bytes());
        let snapshot = capture_live(
            &emu,
            None,
            &SnapshotOptions::new(SnapshotScope::Viewport),
            SnapshotMetadata::default(),
        )
        .expect("VS16 capture succeeds");
        let row = snapshot.row(0).expect("first row exists");
        assert_eq!(row[0].text(), "\u{2764}\u{fe0f}");
        assert_eq!(
            row[0].width(),
            1,
            "heart occupies the one column the backend gave it"
        );
        assert!(!row[0].is_continuation());
        assert_eq!(row[1].text(), "1\u{fe0f}\u{20e3}");
        assert_eq!(
            row[1].width(),
            1,
            "keycap occupies the one column the backend gave it"
        );
        assert!(!row[1].is_continuation());
        assert_eq!(row[2].text(), "東");
        assert_eq!(row[2].width(), 2);
        assert!(row[3].is_continuation());
        assert_eq!(row[3].width(), 0);
    }

    /// A VS16 heart in the last column keeps the clipped-edge form.
    ///
    /// The backend gives the glyph one column but the recomputed span would
    /// run past the row edge, so the leading cell keeps its full measured
    /// width with the past-the-edge continuation omitted — the
    /// representation the validator accepts for a clipped wide glyph.
    #[test]
    fn live_capture_keeps_clipped_edge_for_vs16_at_row_end() {
        let mut emu = AlacrittyEmu::new(3, ROWS, &profile());
        emu.process("ab\u{2764}\u{fe0f}".as_bytes());
        let snapshot = capture_live(
            &emu,
            None,
            &SnapshotOptions::new(SnapshotScope::Viewport),
            SnapshotMetadata::default(),
        )
        .expect("clipped-edge capture succeeds");
        let row = snapshot.row(0).expect("first row exists");
        assert_eq!(row[0].text(), "a");
        assert_eq!(row[0].width(), 1);
        assert_eq!(row[1].text(), "b");
        assert_eq!(row[1].width(), 1);
        assert_eq!(row[2].text(), "\u{2764}\u{fe0f}");
        assert_eq!(row[2].width(), 2, "clipped glyph keeps its measured width");
        assert!(!row[2].is_continuation());
    }

    #[test]
    fn round_trip_preserves_palette_and_links() {
        let profile = profile();
        let mut parts = blank_parts(&profile, 3, 1);
        parts.palette[1] = Rgb::new(0x11, 0x22, 0x33);
        parts.palette[200] = Rgb::new(0xaa, 0xbb, 0xcc);
        parts.title = Some("round-trip".to_string());
        parts.cursor = SnapshotCursor::new(
            2,
            0,
            true,
            true,
            SnapshotCursorShape::Underline,
            Rgb::new(9, 9, 9),
            Some(true),
        );
        parts.cells[0] = SnapshotCell::new(
            "L".to_string(),
            1,
            SnapshotColor::Indexed(200),
            SnapshotColor::Rgb(1, 2, 3),
            Attrs::BOLD | Attrs::INVERSE,
            SnapshotUnderline::Curly,
            SnapshotColor::Indexed(1),
            Some("https://example.com/x".to_string()),
            Some("wrap-id".to_string()),
        )
        .expect("cell is valid");
        let snapshot = TerminalSnapshot::try_new(parts).expect("valid");
        let json = serde_json::to_string(&snapshot).expect("serializes");
        let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(snapshot, back);
        assert_eq!(canonical_hash(&snapshot), canonical_hash(&back));
        assert_eq!(back.palette()[200], Rgb::new(0xaa, 0xbb, 0xcc));
        assert_eq!(
            back.cell_at(0, 0).expect("cell").link_uri(),
            Some("https://example.com/x")
        );
        assert_eq!(back.cell_at(0, 0).expect("cell").link_id(), Some("wrap-id"));
        assert_eq!(back.cursor().blink(), Some(true));
        assert!(back
            .cell_at(0, 0)
            .expect("cell")
            .has(Attrs::BOLD | Attrs::INVERSE));
        assert_eq!(
            back.cell_at(0, 0).expect("cell").underline(),
            SnapshotUnderline::Curly
        );
    }
}
