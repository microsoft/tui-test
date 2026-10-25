//! Public offline rendering for owned snapshots.
//!
//! [`render_snapshot_svg`] draws a [`TerminalSnapshot`] as SVG text without a
//! live session, and [`render_snapshot_png`] (with the `recording-raster`
//! feature) draws it as decoded PNG pixels. Neither starts a child process,
//! PTY, session, or daemon: they convert the snapshot into the same grid,
//! colors, cursor, and title the live screenshot path uses, then draw through
//! the same renderer.
//!
//! SVG output keeps text as text, so it stays sharp at any viewer zoom but
//! never promises pixel identity across viewer fonts. Strict visual gates
//! must decode the PNG bytes and compare pixels.
//!
//! ```
//! use tui_test::profile::Rgb;
//! use tui_test::terminal::cell::Attrs;
//! use tui_test::{
//!     render_snapshot_svg, SnapshotCell, SnapshotColor, SnapshotCursor,
//!     SnapshotCursorShape, SnapshotMetadata, SnapshotParts, SnapshotRenderOptions,
//!     SnapshotScope, SnapshotUnderline, TerminalSnapshot,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let palette = [Rgb::new(0, 0, 0); 256];
//! let cell = SnapshotCell::new(
//!     "x".to_string(),
//!     1,
//!     SnapshotColor::Default,
//!     SnapshotColor::Default,
//!     Attrs::empty(),
//!     SnapshotUnderline::None,
//!     SnapshotColor::Default,
//!     None,
//!     None,
//! )?;
//! let snapshot = TerminalSnapshot::try_new(SnapshotParts {
//!     cols: 1,
//!     rows: 1,
//!     scope: SnapshotScope::Viewport,
//!     viewport_origin: 0,
//!     cells: vec![cell],
//!     palette,
//!     default_foreground: Rgb::new(192, 192, 192),
//!     default_background: Rgb::new(0, 0, 0),
//!     default_cursor: Rgb::new(192, 192, 192),
//!     cursor: SnapshotCursor::new(
//!         0,
//!         0,
//!         true,
//!         false,
//!         SnapshotCursorShape::Block,
//!         Rgb::new(192, 192, 192),
//!         None,
//!     ),
//!     title: None,
//!     metadata: SnapshotMetadata::default(),
//! })?;
//! let rendered = render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())?;
//! assert!(rendered.svg().contains(">x<"));
//! assert!(!rendered.evidence().cursor_drawn());
//! # Ok(())
//! # }
//! ```

use std::path::Path;
use std::sync::Arc;

use compact_str::CompactString;

#[cfg(feature = "recording-raster")]
use super::raster::FrameRenderer;
use super::style::{Padding, PaddingSides, Style};
use super::svg::{RenderState, ResolvedLayout};
use crate::api::CaptureBackground;
use crate::snapshot::{SnapshotColor, SnapshotUnderline, TerminalSnapshot};
use crate::terminal::cell::{Attrs, Color, EmuCell, Hyperlink, UnderlineStyle};
use crate::terminal::emu::CursorShape;

/// Largest accepted cell edge, border width, or corner radius in pixels.
///
/// Mirrors the length bound the live [`Style`] validation uses, so offline
/// options cannot size what live styles must reject.
const MAX_METRIC_PX: f64 = 10_000.0;

/// Largest accepted grid font size in pixels, mirroring live validation.
const MAX_FONT_SIZE_PX: f64 = 1_000.0;

/// Largest accepted canvas or content padding per side, mirroring live validation.
const MAX_PADDING_PX: u32 = 10_000;

/// Largest accepted render scale, mirroring the live zoom bound.
const MAX_SCALE: f64 = f32::MAX as f64 / 2.0;

/// Largest accepted decoded image: 100 megapixels, the raster limit.
const MAX_RENDER_PIXELS: u64 = 100_000_000;

/// A typed offline-render failure.
#[derive(Debug)]
pub enum RenderError {
    /// A metric that must be finite was not (`cell_width`, `scale`, ...).
    NonFinite {
        /// The option field that failed validation.
        field: &'static str,
    },
    /// A metric that must be positive was zero or negative.
    NonPositive {
        /// The option field that failed validation.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A metric that must not be negative was negative.
    Negative {
        /// The option field that failed validation.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A metric above its accepted limit.
    TooLarge {
        /// The option field that failed validation.
        field: &'static str,
        /// The rejected value.
        value: f64,
        /// The accepted limit.
        limit: f64,
    },
    /// A cell edge below one logical pixel.
    ///
    /// Subpixel cells round to nothing in the renderers, so offline options
    /// reject them exactly like the live style does ("too small to draw a
    /// cell"), at any scale: scaling cannot restore a degenerate SVG viewBox.
    CellTooSmall {
        /// The option field that failed validation.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The text baseline falls outside the cell it must sit in.
    BaselineOutsideCell {
        /// The rejected baseline, measured from the top of the cell.
        baseline: f32,
        /// The cell height it must fit in.
        cell_height: f32,
    },
    /// Checked image-size arithmetic overflowed, left `u32`, or passed the
    /// pixel cap, so no image was allocated.
    ImageTooLarge {
        /// Snapshot columns the size was computed for.
        cols: u32,
        /// Snapshot rows the size was computed for.
        rows: u32,
    },
    /// The raster backend failed: missing glyphs, allocation, or encoding.
    RasterFailed {
        /// What the raster backend reported.
        message: String,
    },
    /// A snapshot file could not be written.
    Io(std::io::Error),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NonFinite { field } => {
                write!(f, "{field} must be finite")
            }
            RenderError::NonPositive { field, value } => {
                write!(f, "{field} must be greater than zero (got {value})")
            }
            RenderError::Negative { field, value } => {
                write!(f, "{field} must not be negative (got {value})")
            }
            RenderError::TooLarge {
                field,
                value,
                limit,
            } => {
                write!(f, "{field} must not exceed {limit} (got {value})")
            }
            RenderError::CellTooSmall { field, value } => {
                write!(f, "{field} must be at least one pixel (got {value})")
            }
            RenderError::BaselineOutsideCell {
                baseline,
                cell_height,
            } => {
                write!(
                    f,
                    "text baseline {baseline} falls outside a {cell_height}-pixel cell"
                )
            }
            RenderError::ImageTooLarge { cols, rows } => {
                write!(
                    f,
                    "rendered image for a {cols}x{rows} grid does not fit the pixel budget"
                )
            }
            RenderError::RasterFailed { message } => {
                write!(f, "raster render failed: {message}")
            }
            RenderError::Io(error) => {
                write!(f, "snapshot render write failed: {error}")
            }
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenderError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RenderError> for crate::api::TuiTestError {
    fn from(error: RenderError) -> Self {
        crate::api::TuiTestError::internal(error.to_string())
    }
}

/// Render settings for one offline snapshot.
///
/// Every field is private; the constructor, accessors, and builder methods
/// below are the whole surface. Defaults mirror the live [`Style`] geometry
/// (10-by-21 cells, title bar with controls, shadow, default padding), so a
/// default offline render matches a default live screenshot of the same
/// terminal state. U04 extends this type with pinned fonts; it stays
/// raster-independent either way.
#[derive(Debug, Clone)]
pub struct SnapshotRenderOptions {
    cell_width: f32,
    cell_height: f32,
    font_size: f32,
    baseline: f32,
    scale: f64,
    canvas_padding: Padding,
    content_padding: Padding,
    title_bar: bool,
    title_controls: bool,
    shadow: bool,
    border_width: f32,
    border_radius: f32,
    background: Option<CaptureBackground>,
}

impl Default for SnapshotRenderOptions {
    fn default() -> Self {
        let cell_width = 10.0f32;
        let cell_height = 21.0f32;
        let font_size = 17.0f32;
        Self {
            cell_width,
            cell_height,
            font_size,
            // Same formula the live style uses: half the leading plus the
            // distance from the font top to its baseline.
            baseline: (cell_height - font_size) / 2.0 + font_size * 0.78,
            scale: 1.0,
            canvas_padding: Padding::Uniform(24),
            content_padding: Padding::Sides(PaddingSides {
                top: Some(8),
                right: Some(15),
                bottom: Some(14),
                left: Some(15),
            }),
            title_bar: true,
            title_controls: true,
            shadow: true,
            border_width: 0.0,
            border_radius: 8.0,
            background: None,
        }
    }
}

impl SnapshotRenderOptions {
    /// Default options: the live style geometry with no overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Terminal columns are this many pixels wide; must be finite and positive.
    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    /// Set the cell width in pixels.
    pub fn with_cell_width(mut self, cell_width: f32) -> Self {
        self.cell_width = cell_width;
        self
    }

    /// Terminal rows are this many pixels tall; must be finite and positive.
    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }

    /// Set the cell height in pixels.
    pub fn with_cell_height(mut self, cell_height: f32) -> Self {
        self.cell_height = cell_height;
        self
    }

    /// Grid font size in pixels; must be finite and positive.
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Set the grid font size in pixels.
    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Distance from the top of a cell to its text baseline; must sit in the
    /// cell (`0.0` through `cell_height`).
    pub fn baseline(&self) -> f32 {
        self.baseline
    }

    /// Set the text baseline offset in pixels from the top of the cell.
    pub fn with_baseline(mut self, baseline: f32) -> Self {
        self.baseline = baseline;
        self
    }

    /// Output scale factor; must be finite and positive.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Set the output scale factor.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Gap around the window.
    pub fn canvas_padding(&self) -> Padding {
        self.canvas_padding
    }

    /// Set the gap around the window.
    pub fn with_canvas_padding(mut self, canvas_padding: Padding) -> Self {
        self.canvas_padding = canvas_padding;
        self
    }

    /// Gap between the window and the grid drawn inside it.
    pub fn content_padding(&self) -> Padding {
        self.content_padding
    }

    /// Set the gap between the window and the grid.
    pub fn with_content_padding(mut self, content_padding: Padding) -> Self {
        self.content_padding = content_padding;
        self
    }

    /// Whether the title bar, divider, and title are drawn.
    pub fn title_bar(&self) -> bool {
        self.title_bar
    }

    /// Show or hide the title bar.
    pub fn with_title_bar(mut self, title_bar: bool) -> Self {
        self.title_bar = title_bar;
        self
    }

    /// Whether the three window controls are drawn when the bar is visible.
    pub fn title_controls(&self) -> bool {
        self.title_controls
    }

    /// Show or hide the window controls.
    pub fn with_title_controls(mut self, title_controls: bool) -> Self {
        self.title_controls = title_controls;
        self
    }

    /// Whether the drop shadow under the panel is drawn.
    pub fn shadow(&self) -> bool {
        self.shadow
    }

    /// Turn the drop shadow on or off.
    pub fn with_shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    /// Panel border width in pixels; zero disables it.
    pub fn border_width(&self) -> f32 {
        self.border_width
    }

    /// Set the panel border width in pixels.
    pub fn with_border_width(mut self, border_width: f32) -> Self {
        self.border_width = border_width;
        self
    }

    /// Panel corner radius in pixels.
    pub fn border_radius(&self) -> f32 {
        self.border_radius
    }

    /// Set the panel corner radius in pixels.
    pub fn with_border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Canvas background override: a color, transparent, or the default fill.
    pub fn background(&self) -> Option<CaptureBackground> {
        self.background
    }

    /// Set the canvas background override.
    pub fn with_background(mut self, background: Option<CaptureBackground>) -> Self {
        self.background = background;
        self
    }

    /// Reject nonfinite values, bad dimensions, subpixel cells, and a baseline
    /// outside the cell, before any image-size arithmetic or allocation.
    pub fn validate(&self) -> Result<(), RenderError> {
        for (field, value) in [
            ("cell_width", f64::from(self.cell_width)),
            ("cell_height", f64::from(self.cell_height)),
            ("font_size", f64::from(self.font_size)),
            ("scale", self.scale),
        ] {
            if !value.is_finite() {
                return Err(RenderError::NonFinite { field });
            }
            if value <= 0.0 {
                return Err(RenderError::NonPositive { field, value });
            }
        }
        if f64::from(self.font_size) > MAX_FONT_SIZE_PX {
            return Err(RenderError::TooLarge {
                field: "font_size",
                value: f64::from(self.font_size),
                limit: MAX_FONT_SIZE_PX,
            });
        }
        for (field, value) in [
            ("cell_width", f64::from(self.cell_width)),
            ("cell_height", f64::from(self.cell_height)),
        ] {
            if value > MAX_METRIC_PX {
                return Err(RenderError::TooLarge {
                    field,
                    value,
                    limit: MAX_METRIC_PX,
                });
            }
            // Same floor the live style enforces: a subpixel logical cell
            // rounds to nothing, so SVG and PNG size math diverge (a zero
            // viewBox against nonzero pixels). Scale cannot fix it, because
            // the viewBox rounds the unscaled size.
            if value < 1.0 {
                return Err(RenderError::CellTooSmall { field, value });
            }
        }
        if !self.baseline.is_finite() {
            return Err(RenderError::NonFinite { field: "baseline" });
        }
        if self.baseline < 0.0 || self.baseline > self.cell_height {
            return Err(RenderError::BaselineOutsideCell {
                baseline: self.baseline,
                cell_height: self.cell_height,
            });
        }
        if self.scale > MAX_SCALE {
            return Err(RenderError::TooLarge {
                field: "scale",
                value: self.scale,
                limit: MAX_SCALE,
            });
        }
        for (field, value) in [
            ("border_width", f64::from(self.border_width)),
            ("border_radius", f64::from(self.border_radius)),
        ] {
            if !value.is_finite() {
                return Err(RenderError::NonFinite { field });
            }
            if value < 0.0 {
                return Err(RenderError::Negative { field, value });
            }
            if value > MAX_METRIC_PX {
                return Err(RenderError::TooLarge {
                    field,
                    value,
                    limit: MAX_METRIC_PX,
                });
            }
        }
        check_padding("canvas_padding", &self.canvas_padding)?;
        check_padding("content_padding", &self.content_padding)?;
        Ok(())
    }
}

/// One padding side must fit its pixel budget.
fn check_padding(field: &'static str, padding: &Padding) -> Result<(), RenderError> {
    let sides = match padding {
        Padding::Uniform(value) => [*value; 4],
        Padding::Sides(sides) => {
            [sides.top, sides.right, sides.bottom, sides.left].map(|side| side.unwrap_or(0))
        }
    };
    for value in sides {
        if u64::from(value) > u64::from(MAX_PADDING_PX) {
            return Err(RenderError::TooLarge {
                field,
                value: f64::from(value),
                limit: f64::from(MAX_PADDING_PX),
            });
        }
    }
    Ok(())
}

/// Structured evidence from one offline render.
///
/// Dimensions plus the behaviors the spec calls out: whether the cursor was
/// drawn (hidden and clipped cursors stay hidden), whether the canvas is
/// transparent, and which title the bar showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderEvidence {
    width: u32,
    height: u32,
    cursor_drawn: bool,
    transparent_background: bool,
    title: Option<String>,
}

impl RenderEvidence {
    /// Output image width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Output image height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether the cursor was drawn: false when it is hidden or clipped out
    /// of the capture, exactly like a live screenshot.
    pub fn cursor_drawn(&self) -> bool {
        self.cursor_drawn
    }

    /// Whether the canvas was left transparent instead of filled.
    pub fn transparent_background(&self) -> bool {
        self.transparent_background
    }

    /// The title the bar showed, or `None` with the bar off or nothing fitting.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// One rendered SVG image with its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSvg {
    svg: String,
    evidence: RenderEvidence,
}

impl RenderedSvg {
    /// The SVG document. Text stays text: sharp at any viewer zoom, with no
    /// pixel-identity promise across viewer fonts.
    pub fn svg(&self) -> &str {
        &self.svg
    }

    /// Output width in pixels.
    pub fn width(&self) -> u32 {
        self.evidence.width
    }

    /// Output height in pixels.
    pub fn height(&self) -> u32 {
        self.evidence.height
    }

    /// Dimensions and recorded cursor, background, and title behavior.
    pub fn evidence(&self) -> &RenderEvidence {
        &self.evidence
    }

    /// The SVG document as bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.svg.into_bytes()
    }
}

/// Draw a snapshot as SVG text, without a live session.
///
/// The input snapshot is never changed. A hidden cursor, or one clipped out
/// of the capture, is not drawn, and the evidence records that.
pub fn render_snapshot_svg(
    snapshot: &TerminalSnapshot,
    options: &SnapshotRenderOptions,
) -> Result<RenderedSvg, RenderError> {
    let layout = layout_from_options(options)?;
    let (cols, rows) = snapshot_grid_size(snapshot)?;
    let (width, height) = checked_output_size(&layout, snapshot.cols(), snapshot.rows())?;
    let grid = snapshot_rows(snapshot);
    let colors = snapshot_colors(snapshot);
    let cursor = snapshot_cursor_position(snapshot);
    let svg = super::svg::render_svg_with_layout(
        &grid,
        cols,
        &colors,
        cursor,
        snapshot.title(),
        &layout,
        options.background,
    );
    Ok(RenderedSvg {
        svg,
        evidence: RenderEvidence {
            width,
            height,
            cursor_drawn: cursor.is_some(),
            transparent_background: matches!(
                options.background,
                Some(CaptureBackground::Transparent)
            ),
            title: shown_title(snapshot.title(), cols, rows, &layout),
        },
    })
}

/// Draw a snapshot as SVG and save it to `path`, without a live session.
///
/// Returns the same evidence [`render_snapshot_svg`] returns for the same
/// inputs.
pub fn write_snapshot_svg(
    path: &Path,
    snapshot: &TerminalSnapshot,
    options: &SnapshotRenderOptions,
) -> Result<RenderEvidence, RenderError> {
    let rendered = render_snapshot_svg(snapshot, options)?;
    std::fs::write(path, rendered.svg()).map_err(RenderError::Io)?;
    Ok(rendered.evidence)
}

/// One rendered PNG image with its evidence.
#[cfg(feature = "recording-raster")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPng {
    png: Vec<u8>,
    evidence: RenderEvidence,
}

#[cfg(feature = "recording-raster")]
impl RenderedPng {
    /// The encoded PNG bytes; decode them for strict pixel gates.
    pub fn png(&self) -> &[u8] {
        &self.png
    }

    /// Output width in pixels.
    pub fn width(&self) -> u32 {
        self.evidence.width
    }

    /// Output height in pixels.
    pub fn height(&self) -> u32 {
        self.evidence.height
    }

    /// Dimensions and recorded cursor, background, and title behavior.
    pub fn evidence(&self) -> &RenderEvidence {
        &self.evidence
    }

    /// The encoded PNG bytes, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.png
    }
}

/// Draw a snapshot as PNG bytes, without a live session.
///
/// Uses the same [`GridRenderer`](super::raster::GridRenderer) the live
/// screenshots use, at screenshot (exact, unpadded) size. The input snapshot
/// is never changed. A hidden cursor, or one clipped out of the capture, is
/// not drawn, and the evidence records that.
#[cfg(feature = "recording-raster")]
pub fn render_snapshot_png(
    snapshot: &TerminalSnapshot,
    options: &SnapshotRenderOptions,
) -> Result<RenderedPng, RenderError> {
    let layout = layout_from_options(options)?;
    // Checked before the renderer allocates its pixmap.
    let _ = checked_output_size(&layout, snapshot.cols(), snapshot.rows())?;
    let (cols, rows) = snapshot_grid_size(snapshot)?;
    let mut renderer = super::raster::GridRenderer::with_layout(
        cols,
        rows,
        layout.clone(),
        options.background,
        true,
    )
    .map_err(|error| RenderError::RasterFailed {
        message: error.to_string(),
    })?;
    let frame = snapshot_frame(snapshot);
    let png = super::encode::encode_png_bytes(&frame, &mut renderer).map_err(|error| {
        RenderError::RasterFailed {
            message: error.to_string(),
        }
    })?;
    let (width, height) = renderer.pixel_size();
    Ok(RenderedPng {
        png,
        evidence: RenderEvidence {
            width,
            height,
            cursor_drawn: frame.cursor.is_some(),
            transparent_background: matches!(
                options.background,
                Some(CaptureBackground::Transparent)
            ),
            title: shown_title(snapshot.title(), cols, rows, &layout),
        },
    })
}

/// Draw a snapshot as PNG and save it to `path`, without a live session.
///
/// Shares the staged file writer the live screenshots use, so bytes and path
/// wrappers decode to equal pixels. Returns the same evidence
/// [`render_snapshot_png`] returns for the same inputs.
#[cfg(feature = "recording-raster")]
pub fn write_snapshot_png(
    path: &Path,
    snapshot: &TerminalSnapshot,
    options: &SnapshotRenderOptions,
) -> Result<RenderEvidence, RenderError> {
    let layout = layout_from_options(options)?;
    // Checked before the renderer allocates its pixmap.
    let _ = checked_output_size(&layout, snapshot.cols(), snapshot.rows())?;
    let (cols, rows) = snapshot_grid_size(snapshot)?;
    let mut renderer = super::raster::GridRenderer::with_layout(
        cols,
        rows,
        layout.clone(),
        options.background,
        true,
    )
    .map_err(|error| RenderError::RasterFailed {
        message: error.to_string(),
    })?;
    let frame = snapshot_frame(snapshot);
    super::encode::encode_png(path, &frame, &mut renderer).map_err(|error| {
        RenderError::RasterFailed {
            message: error.to_string(),
        }
    })?;
    let (width, height) = renderer.pixel_size();
    Ok(RenderEvidence {
        width,
        height,
        cursor_drawn: frame.cursor.is_some(),
        transparent_background: matches!(options.background, Some(CaptureBackground::Transparent)),
        title: shown_title(snapshot.title(), cols, rows, &layout),
    })
}

/// Resolve validated options to the shared layout.
///
/// Starts from the live style defaults so preserved geometry is inherited,
/// not re-typed, then applies the explicit cell metrics and chrome switches.
fn layout_from_options(options: &SnapshotRenderOptions) -> Result<ResolvedLayout, RenderError> {
    options.validate()?;
    let mut layout = ResolvedLayout::from_style(&Style::default(), options.scale);
    layout.cell_w = options.cell_width;
    layout.cell_h = options.cell_height;
    layout.font_size = options.font_size;
    layout.baseline = options.baseline;
    let (canvas_top, canvas_right, canvas_bottom, canvas_left) =
        padding_values(&options.canvas_padding, (24, 24, 24, 24));
    layout.canvas_top = canvas_top;
    layout.canvas_right = canvas_right;
    layout.canvas_bottom = canvas_bottom;
    layout.canvas_left = canvas_left;
    let (content_top, content_right, content_bottom, content_left) =
        padding_values(&options.content_padding, (8, 15, 14, 15));
    // Content gaps are f32 in the layout, but validated as u32 above.
    layout.content_top = content_top as f32;
    layout.content_right = content_right as f32;
    layout.content_bottom = content_bottom as f32;
    layout.content_left = content_left as f32;
    layout.title_bar = options.title_bar;
    layout.title_controls = options.title_controls;
    if !options.title_bar {
        // Same rule the live style uses: no bar, no header or divider.
        layout.header_h = 0.0;
        layout.divider_h = 0.0;
    }
    layout.shadow_enabled = options.shadow;
    layout.border_width = options.border_width;
    layout.border_radius = options.border_radius;
    Ok(layout)
}

/// Resolve one padding to `(top, right, bottom, left)`, filling omitted sides
/// from the given defaults.
fn padding_values(padding: &Padding, defaults: (u32, u32, u32, u32)) -> (u32, u32, u32, u32) {
    match padding {
        Padding::Uniform(value) => (*value, *value, *value, *value),
        Padding::Sides(sides) => (
            sides.top.unwrap_or(defaults.0),
            sides.right.unwrap_or(defaults.1),
            sides.bottom.unwrap_or(defaults.2),
            sides.left.unwrap_or(defaults.3),
        ),
    }
}

/// Checked grid size for the renderers: snapshot dimensions always fit the
/// `u16` columns and `usize` rows the renderer indexes with, but the
/// conversion stays checked rather than assumed.
fn snapshot_grid_size(snapshot: &TerminalSnapshot) -> Result<(u16, usize), RenderError> {
    let cols = u16::try_from(snapshot.cols()).map_err(|_| RenderError::ImageTooLarge {
        cols: snapshot.cols(),
        rows: snapshot.rows(),
    })?;
    let rows = usize::try_from(snapshot.rows()).map_err(|_| RenderError::ImageTooLarge {
        cols: snapshot.cols(),
        rows: snapshot.rows(),
    })?;
    Ok((cols, rows))
}

/// Checked canvas-inclusive output size at the layout zoom, before allocation.
///
/// Mirrors the renderers' size math (ceiled panel plus canvas, scaled) with
/// finite and range checks on every step and the shared pixel cap at the end.
fn checked_output_size(
    layout: &ResolvedLayout,
    cols: u32,
    rows: u32,
) -> Result<(u32, u32), RenderError> {
    let too_large = || RenderError::ImageTooLarge { cols, rows };
    let panel_w = f64::from(layout.content_left)
        + f64::from(layout.content_right)
        + f64::from(cols) * f64::from(layout.cell_w);
    let panel_h = f64::from(layout.header_h)
        + f64::from(layout.content_top)
        + f64::from(layout.content_bottom)
        + f64::from(rows.max(1)) * f64::from(layout.cell_h);
    let width =
        (panel_w + f64::from(layout.canvas_left) + f64::from(layout.canvas_right)) * layout.zoom;
    let height =
        (panel_h + f64::from(layout.canvas_top) + f64::from(layout.canvas_bottom)) * layout.zoom;
    let width = ceil_u32(width).ok_or_else(too_large)?;
    let height = ceil_u32(height).ok_or_else(too_large)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_RENDER_PIXELS {
        return Err(too_large());
    }
    Ok((width, height))
}

/// Ceil a size to `u32`, or `None` past zero, nonfinite, or `u32::MAX`.
fn ceil_u32(value: f64) -> Option<u32> {
    if !value.is_finite() || value <= 0.0 || value > f64::from(u32::MAX) {
        return None;
    }
    let ceiled = value.ceil();
    if ceiled > f64::from(u32::MAX) {
        return None;
    }
    // In range by the checks above; `as` cannot saturate here.
    Some(ceiled as u32)
}

/// Convert snapshot cells to the renderer grid.
///
/// Widths are recorded on the snapshot and never recomputed from the text;
/// continuation cells (empty text off column zero) convert to the empty
/// marker the renderer already treats as a wide-character trailer.
fn snapshot_rows(snapshot: &TerminalSnapshot) -> Vec<Vec<EmuCell>> {
    let cols = snapshot.cols() as usize;
    snapshot
        .cells()
        .chunks(cols.max(1))
        .map(|row| row.iter().map(snapshot_cell_to_emu).collect())
        .collect()
}

/// Convert one snapshot cell to a renderer cell.
fn snapshot_cell_to_emu(cell: &crate::snapshot::SnapshotCell) -> EmuCell {
    let mut attrs = Attrs::empty();
    for flag in [
        Attrs::BOLD,
        Attrs::DIM,
        Attrs::ITALIC,
        Attrs::INVERSE,
        Attrs::INVISIBLE,
        Attrs::STRIKE,
        Attrs::BLINK,
    ] {
        if cell.has(flag) {
            attrs |= flag;
        }
    }
    EmuCell {
        ch: CompactString::from(cell.text()),
        fg: snapshot_color_to_emu(cell.fg()),
        bg: snapshot_color_to_emu(cell.bg()),
        underline: snapshot_underline_to_emu(cell.underline()),
        underline_color: snapshot_color_to_emu(cell.underline_color()),
        attrs,
        hyperlink: cell.link_uri().map(|uri| {
            Arc::new(Hyperlink {
                id: cell.link_id().map(CompactString::from),
                uri: CompactString::from(uri),
            })
        }),
    }
}

/// Snapshot color references become the cell slots the renderer resolves:
/// defaults stay defaults, indices keep their normalization, RGB stays RGB.
fn snapshot_color_to_emu(color: SnapshotColor) -> Option<Color> {
    match color {
        SnapshotColor::Default => None,
        SnapshotColor::Indexed(index) => Some(Color::from_index(index)),
        SnapshotColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Snapshot underlines use the same shapes the renderer draws.
fn snapshot_underline_to_emu(underline: SnapshotUnderline) -> UnderlineStyle {
    match underline {
        SnapshotUnderline::None => UnderlineStyle::None,
        SnapshotUnderline::Single => UnderlineStyle::Single,
        SnapshotUnderline::Double => UnderlineStyle::Double,
        SnapshotUnderline::Curly => UnderlineStyle::Curly,
        SnapshotUnderline::Dotted => UnderlineStyle::Dotted,
        SnapshotUnderline::Dashed => UnderlineStyle::Dashed,
    }
}

/// Resolve snapshot colors to the render state the renderer reads.
///
/// The cursor slot carries the captured cursor color, not the default: the
/// two match on a fresh live capture, but a snapshot may resolve them
/// differently, and the renderer must draw what was captured.
fn snapshot_colors(snapshot: &TerminalSnapshot) -> RenderState {
    RenderState::from_parts(
        *snapshot.palette(),
        snapshot.default_foreground(),
        snapshot.default_background(),
        snapshot.cursor().color(),
        match snapshot.cursor().shape() {
            crate::snapshot::SnapshotCursorShape::Block => CursorShape::Block,
            crate::snapshot::SnapshotCursorShape::Underline => CursorShape::Underline,
            crate::snapshot::SnapshotCursorShape::Bar => CursorShape::Bar,
        },
    )
}

/// Where to draw the cursor in the captured rows, or `None`.
///
/// A hidden cursor is never drawn, and one clipped out of the capture keeps
/// its real position on the snapshot but stays off the image, exactly like a
/// live screenshot.
fn snapshot_cursor_position(snapshot: &TerminalSnapshot) -> Option<(u16, usize)> {
    if !snapshot.cursor().visible() {
        return None;
    }
    let (col, row) = snapshot.cursor_local()?;
    let col = u16::try_from(col).ok()?;
    let row = usize::try_from(row).ok()?;
    Some((col, row))
}

/// One internal recording frame from a snapshot, for the raster renderer.
///
/// The frame type blocks external access by design; this conversion runs
/// inside the crate so offline rendering needs no public worker or sampler
/// types.
#[cfg(feature = "recording-raster")]
fn snapshot_frame(snapshot: &TerminalSnapshot) -> crate::record::frames::Frame {
    crate::record::frames::Frame {
        grid: snapshot_rows(snapshot),
        title: snapshot.title().map(str::to_string),
        duration: std::time::Duration::ZERO,
        render_state: snapshot_colors(snapshot),
        cursor: snapshot_cursor_position(snapshot),
    }
}

/// The title the bar shows, or `None` with the bar off or nothing fitting.
fn shown_title(
    title: Option<&str>,
    cols: u16,
    rows: usize,
    layout: &ResolvedLayout,
) -> Option<String> {
    if !layout.title_bar {
        return None;
    }
    let (panel_width, _) = layout.panel_size(cols as usize, rows);
    super::svg::visible_title(title, cols, rows, panel_width, layout)
}
