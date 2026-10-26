//! Crisp, full-color SVG screenshot of a terminal grid, styled after
//! `svg-term-cli`: a rounded window panel with macOS-style controls.
//!
//! Vector output renders sharply at any zoom. The viewer supplies a monospace
//! face for text, while Nerd Font icons are emitted from a bundled symbols font
//! as SVG paths. Colors, bold/italic/underline/strike, inverse, and dim are all
//! preserved. Each run of same-styled cells is forced to its exact column width
//! via `textLength`, so alignment is independent of the rendering font's
//! metrics.

use std::fmt::Write;

use super::nerd_font::NerdFont;
use crate::api::CaptureBackground;
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::render::style::{FontFamilies, Style};
use crate::terminal::cell::{truncate_to_columns, Attrs, Color, EmuCell, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

pub(crate) const DOT_R: f32 = 7.0;
pub(crate) const RED_DOT_R: f32 = 2.5;
pub(crate) const RED_DOT_COLOR: Rgb = Rgb::new(105, 17, 10);
fn dots_right(layout: &ResolvedLayout) -> f32 {
    layout.content_left + 5.0 + 2.0 * 20.0 + DOT_R
}

fn hex(c: Rgb) -> String {
    c.to_hex()
}

fn svg_dimension(value: f64) -> String {
    let mut output = format!("{value:.4}");
    while output.contains('.') && output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.pop();
    }
    output
}

/// Ceil one scaled axis to whole pixels, or `None` past zero, nonfinite,
/// or `u32` range.
fn ceil_pixel(value: f64) -> Option<u32> {
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

fn dim(c: Rgb) -> Rgb {
    let s = |v: u8| (v as f32 * 0.6) as u8;
    Rgb::new(s(c.r), s(c.g), s(c.b))
}

static BLANK: EmuCell = EmuCell::blank();

fn cell_at(row: &[EmuCell], x: usize) -> &EmuCell {
    row.get(x).unwrap_or(&BLANK)
}

pub(crate) trait RenderColors {
    fn color(&self, slot: ColorSlot) -> Rgb;
    fn cursor_shape(&self) -> CursorShape;

    fn resolve(&self, color: Option<Color>, is_fg: bool) -> Rgb {
        match color {
            None => self.color(if is_fg {
                ColorSlot::Foreground
            } else {
                ColorSlot::Background
            }),
            Some(Color::Named(n)) => self.color(ColorSlot::Indexed(n.index())),
            Some(Color::Idx(i)) => self.color(ColorSlot::Indexed(i)),
            Some(Color::Rgb(r, g, b)) => Rgb::new(r, g, b),
        }
    }
}

impl<T: Emulator + ?Sized> RenderColors for T {
    fn color(&self, slot: ColorSlot) -> Rgb {
        Emulator::color(self, slot)
    }

    fn cursor_shape(&self) -> CursorShape {
        Emulator::cursor_shape(self)
    }
}

impl RenderColors for Profile {
    fn color(&self, slot: ColorSlot) -> Rgb {
        match slot {
            ColorSlot::Indexed(index) => self.colors.rgb(index),
            ColorSlot::Foreground => self.colors.foreground,
            ColorSlot::Background => self.colors.background,
            ColorSlot::Cursor => self.colors.cursor,
        }
    }

    fn cursor_shape(&self) -> CursorShape {
        CursorShape::Block
    }
}

/// The renderer-owned terminal state captured together with the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderState {
    indexed: [Rgb; 256],
    foreground: Rgb,
    background: Rgb,
    cursor: Rgb,
    cursor_shape: CursorShape,
}

impl RenderState {
    pub(crate) fn capture(emu: &dyn Emulator) -> Self {
        Self {
            indexed: std::array::from_fn(|index| emu.color(ColorSlot::Indexed(index as u8))),
            foreground: emu.color(ColorSlot::Foreground),
            background: emu.color(ColorSlot::Background),
            cursor: emu.color(ColorSlot::Cursor),
            cursor_shape: emu.cursor_shape(),
        }
    }
}

impl RenderColors for RenderState {
    fn color(&self, slot: ColorSlot) -> Rgb {
        match slot {
            ColorSlot::Indexed(index) => self.indexed[index as usize],
            ColorSlot::Foreground => self.foreground,
            ColorSlot::Background => self.background,
            ColorSlot::Cursor => self.cursor,
        }
    }

    fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }
}

impl RenderState {
    /// Build from resolved snapshot data, without a live emulator.
    ///
    /// The offline renderer converts a [`crate::snapshot::TerminalSnapshot`]
    /// into these fields so snapshots render through the same code as live
    /// screenshots; the recording [`crate::record::frames::Frame`] stays
    /// internal to the crate.
    pub(crate) fn from_parts(
        indexed: [Rgb; 256],
        foreground: Rgb,
        background: Rgb,
        cursor: Rgb,
        cursor_shape: CursorShape,
    ) -> Self {
        Self {
            indexed,
            foreground,
            background,
            cursor,
            cursor_shape,
        }
    }
}

/// One resolved geometry and chrome bundle for both render paths.
///
/// Live screenshots and recordings build it from a [`Style`]; offline
/// snapshot rendering builds it from
/// [`crate::render::offline::SnapshotRenderOptions`]. The SVG and raster
/// renderers both draw from it, so there is one renderer, not one per input
/// kind. Layout defaults are preserved by construction: [`from_style`] copies
/// the style's resolved values unchanged.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedLayout {
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
    pub title_font_size: f32,
    pub baseline: f32,
    pub header_h: f32,
    pub divider_h: f32,
    pub content_left: f32,
    pub content_top: f32,
    pub content_right: f32,
    pub content_bottom: f32,
    pub canvas_left: u32,
    pub canvas_right: u32,
    pub canvas_top: u32,
    pub canvas_bottom: u32,
    pub canvas_background: Rgb,
    pub font: FontFamilies,
    pub title_bar: bool,
    pub title_controls: bool,
    pub window_background: Rgb,
    pub window_foreground: Rgb,
    pub window_divider: Rgb,
    pub border_width: f32,
    pub border_color: Rgb,
    pub border_radius: f32,
    pub shadow_enabled: bool,
    pub shadow_color: Rgb,
    pub shadow_offset: f32,
    pub shadow_spread: f32,
    pub zoom: f64,
}

impl ResolvedLayout {
    /// Resolve a live style at one zoom, copying every value unchanged.
    pub(crate) fn from_style(style: &Style, zoom: f64) -> Self {
        Self {
            cell_w: style.cell_width(),
            cell_h: style.cell_height(),
            font_size: style.font_size,
            title_font_size: style.title_font_size,
            baseline: style.baseline(),
            header_h: style.header_height(),
            divider_h: style.divider_height(),
            content_left: style.content_left(),
            content_top: style.content_top(),
            content_right: style.content_right(),
            content_bottom: style.content_bottom(),
            canvas_left: style.canvas_left(),
            canvas_right: style.canvas_right(),
            canvas_top: style.canvas_top(),
            canvas_bottom: style.canvas_bottom(),
            canvas_background: style.canvas_background,
            font: style.font.clone(),
            title_bar: style.window.title_bar,
            title_controls: style.window.traffic_lights,
            window_background: style.window.background,
            window_foreground: style.window.foreground,
            window_divider: style.window.divider,
            border_width: style.border.width,
            border_color: style.border.color,
            border_radius: style.border.radius,
            shadow_enabled: style.shadow.enabled,
            shadow_color: style.shadow.color,
            shadow_offset: style.shadow.offset,
            shadow_spread: style.shadow.spread,
            zoom,
        }
    }

    /// The shadow's stacked rectangles, empty when it is turned off.
    ///
    /// Kept in sync with the shadow math the style used before the layout
    /// type existed: `(spread, offset, alpha)` layers, largest first.
    pub(crate) fn shadow_layers(&self) -> Vec<(f32, f32, u8)> {
        if !self.shadow_enabled {
            return Vec::new();
        }
        let spread_step = self.shadow_spread * 2.0 / 7.0;
        let offset_step = self.shadow_offset / 5.0;
        let layers: [(f32, f32, u8); 4] = std::array::from_fn(|index| {
            let step = index as f32;
            (
                self.shadow_spread - step * spread_step,
                self.shadow_offset - step * offset_step,
                18 + index as u8 * 2,
            )
        });
        layers.to_vec()
    }

    /// The three controls, drawn only with the title bar and its switch on.
    pub(crate) fn traffic_lights(&self) -> &[Rgb] {
        const LIGHTS: [Rgb; 3] = [
            Rgb::new(236, 106, 94),
            Rgb::new(244, 191, 79),
            Rgb::new(97, 197, 84),
        ];
        if self.title_bar && self.title_controls {
            &LIGHTS
        } else {
            &[]
        }
    }

    #[cfg(feature = "recording-raster")]
    /// What the canvas grows by to hold the window, or `None` past `u32`.
    pub(crate) fn canvas_horizontal(&self) -> Option<u32> {
        self.canvas_left.checked_add(self.canvas_right)
    }

    #[cfg(feature = "recording-raster")]
    /// What the canvas grows by to hold the window, or `None` past `u32`.
    pub(crate) fn canvas_vertical(&self) -> Option<u32> {
        self.canvas_top.checked_add(self.canvas_bottom)
    }

    /// F32 panel size for the grid: content gaps, header, and cells, no canvas.
    pub(crate) fn panel_size(&self, cols: usize, rows: usize) -> (f32, f32) {
        let width = self.content_left + self.content_right + cols as f32 * self.cell_w;
        let height = self.header_h
            + self.content_top
            + self.content_bottom
            + rows.max(1) as f32 * self.cell_h;
        (width, height)
    }

    #[cfg(feature = "recording-raster")]
    /// Ceiled grid pixel size for the raster renderer, before canvas and zoom.
    ///
    /// Screenshots keep the exact size; recordings round up to even
    /// dimensions for the video encoders through
    /// [`even_grid_pixel_size`].
    pub(crate) fn grid_pixel_size(&self, cols: u16, rows: usize) -> (u32, u32) {
        let width =
            (self.content_left + self.content_right + f32::from(cols) * self.cell_w).ceil() as u32;
        let height = (self.header_h
            + self.content_top
            + self.content_bottom
            + rows.max(1) as f32 * self.cell_h)
            .ceil() as u32;
        (width, height)
    }

    #[cfg(feature = "recording-raster")]
    /// Ceiled grid pixel size rounded up to even dimensions, for recordings.
    pub(crate) fn even_grid_pixel_size(&self, cols: u16, rows: usize) -> (u32, u32) {
        let (width, height) = self.grid_pixel_size(cols, rows);
        (width + width % 2, height + height % 2)
    }

    /// ONE output-size calculation for every renderer.
    ///
    /// PNG pixels, SVG `width`/`height`, evidence dimensions, and the SVG
    /// viewBox all derive from this: the unscaled panel plus canvas, times
    /// the zoom, ceiled once at the pixel-allocation boundary. Ceiling the
    /// panel before scaling (as the raster path did) or not at all (as the
    /// SVG document did) gave one grid up to three sizes under fractional
    /// metrics; rounding once, here, keeps them identical.
    ///
    /// `None` past zero, nonfinite, or `u32` range; callers map that to
    /// their own error.
    pub(crate) fn output_pixel_size(&self, cols: u32, rows: usize) -> Option<(u32, u32)> {
        let (panel_width, panel_height) = self.panel_size(cols as usize, rows);
        let width =
            (f64::from(panel_width) + f64::from(self.canvas_left) + f64::from(self.canvas_right))
                * self.zoom;
        let height =
            (f64::from(panel_height) + f64::from(self.canvas_top) + f64::from(self.canvas_bottom))
                * self.zoom;
        Some((ceil_pixel(width)?, ceil_pixel(height)?))
    }

    /// Panel-only variant of [`output_pixel_size`](Self::output_pixel_size),
    /// without the canvas: per-frame placement adds the canvas separately
    /// when it centers a frame, so it rounds the same scaled panel rather
    /// than re-deriving one.
    pub(crate) fn panel_pixel_size(&self, cols: u16, rows: usize) -> Option<(u32, u32)> {
        let (panel_width, panel_height) = self.panel_size(cols as usize, rows);
        Some((
            ceil_pixel(f64::from(panel_width) * self.zoom)?,
            ceil_pixel(f64::from(panel_height) * self.zoom)?,
        ))
    }
}

/// Resolved background color for a cell (honoring inverse).
pub(crate) fn bg_of(cell: &EmuCell, colors: &dyn RenderColors) -> Rgb {
    let bg = colors.resolve(cell.bg, false);
    let fg = colors.resolve(cell.fg, true);
    if cell.has(Attrs::INVERSE) {
        fg
    } else {
        bg
    }
}

/// Cell colors and attributes after inverse and dim are resolved.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct CellPaint {
    pub fg: Rgb,
    /// The underline color: the cell's explicit underline color resolved
    /// through the palette (RGB values used directly), or the foreground
    /// color when the cell supplies none. Kept equal to `fg` for cells
    /// without an underline so run grouping stays unchanged.
    pub underline_color: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub invisible: bool,
}

pub(crate) fn cell_paint(cell: &EmuCell, colors: &dyn RenderColors) -> CellPaint {
    let mut fg = colors.resolve(cell.fg, true);
    let bg = colors.resolve(cell.bg, false);
    if cell.has(Attrs::INVERSE) {
        fg = bg;
    }
    if cell.has(Attrs::DIM) {
        fg = dim(fg);
    }
    let underlined = cell.underline.is_underlined();
    let underline_color = if underlined {
        cell.underline_color
            .map_or(fg, |color| colors.resolve(Some(color), true))
    } else {
        fg
    };
    CellPaint {
        fg,
        underline_color,
        bold: cell.has(Attrs::BOLD),
        italic: cell.has(Attrs::ITALIC),
        underline: underlined,
        strike: cell.has(Attrs::STRIKE),
        invisible: cell.has(Attrs::INVISIBLE),
    }
}

/// Escape text and quotes for a double-quoted XML attribute.
fn escape_attribute(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn run_text(row: &[EmuCell], start: usize, end: usize) -> String {
    let mut text = String::with_capacity(end - start);
    for i in start..end {
        text.push_str(&cell_at(row, i).ch);
    }
    text
}

fn write_text_run(
    out: &mut String,
    row: &[EmuCell],
    run: std::ops::Range<usize>,
    y: usize,
    paint: CellPaint,
    nerd_font: &NerdFont,
    layout: &ResolvedLayout,
) {
    if paint.invisible {
        return;
    }
    let (start, end) = (run.start, run.end);
    let cell_w = layout.cell_w;
    let cell_h = layout.cell_h;
    let header_h = layout.header_h;
    let font_baseline = layout.baseline;
    let fg = hex(paint.fg);
    let tx = layout.content_left + start as f32 * cell_w;
    let baseline = header_h + layout.content_top + y as f32 * cell_h + font_baseline;
    let width = (end - start) as f32 * cell_w;
    let original_text = run_text(row, start, end);
    let (text, run_x_adjust) = nerd_font.prepare_run(&original_text, width, cell_w);

    // Preserve decoration for runs containing only vector glyphs.
    if !original_text.trim().is_empty() {
        let weight = if paint.bold {
            r#" font-weight="bold""#
        } else {
            ""
        };
        let italic = if paint.italic {
            r#" font-style="italic""#
        } else {
            ""
        };
        let explicit = paint.underline && paint.underline_color != paint.fg;
        let deco = match (paint.underline, paint.strike) {
            (true, true) if explicit => r#" text-decoration="underline""#,
            (true, true) => r#" text-decoration="underline line-through""#,
            (true, false) => r#" text-decoration="underline""#,
            (false, true) => r#" text-decoration="line-through""#,
            (false, false) => "",
        };
        // `text-decoration-color` is not an SVG presentation attribute (SVG 2
        // keeps it in CSS), so an explicit underline color rides in `style`.
        // Cells without one keep the previous bare `text-decoration` bytes.
        let underline_color = if explicit {
            format!(
                r#" style="text-decoration-color: {}""#,
                hex(paint.underline_color)
            )
        } else {
            String::new()
        };
        // One `text-decoration-color` would tint both lines, but the
        // strike-through keeps the foreground color: when the colors differ,
        // each decoration gets its own element and its own CSS color.
        let content = if paint.underline && paint.strike && explicit {
            format!(
                r#"<tspan text-decoration="line-through" style="text-decoration-color: {fg}">{esc}</tspan>"#,
                esc = escape(&text)
            )
        } else {
            escape(&text)
        };
        let resolved = layout.font.resolve(paint.bold, paint.italic);
        let family = if resolved == layout.font.family {
            String::new()
        } else {
            format!(r#" font-family="{}""#, escape_attribute(resolved))
        };
        let _ = write!(
            out,
            r#"<text x="{tx:.2}" y="{baseline:.2}" fill="{fg}"{family}{weight}{italic}{deco}{underline_color} textLength="{width:.2}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{content}</text>"#,
            content = content
        );
    }
    for i in start..end {
        for c in cell_at(row, i).ch.chars() {
            nerd_font.write_use(
                out,
                c,
                (
                    layout.content_left + i as f32 * cell_w,
                    header_h + layout.content_top + y as f32 * cell_h,
                ),
                (cell_w, cell_h),
                run_x_adjust,
                &fg,
            );
        }
    }
}

pub(crate) fn title_advance(layout: &ResolvedLayout) -> f32 {
    layout.title_font_size * (layout.cell_w / layout.font_size)
}

pub(crate) fn media_title(title: Option<&str>, cols: u16, rows: usize, fits: usize) -> String {
    let base = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("tui-test capture");
    let suffix = format!(" - {cols}x{rows}");
    let full = format!("{base}{suffix}");
    if crate::terminal::cell::display_width(&full) <= fits {
        return full;
    }

    let suffix_width = crate::terminal::cell::display_width(&suffix);
    if suffix_width >= fits {
        return truncate_to_columns(&full, fits);
    }
    format!("{}{suffix}", truncate_to_columns(base, fits - suffix_width))
}

pub(crate) fn visible_title(
    title: Option<&str>,
    cols: u16,
    rows: usize,
    width: f32,
    layout: &ResolvedLayout,
) -> Option<String> {
    const GAP: f32 = 8.0;
    let margin = if layout.title_controls {
        dots_right(layout) + GAP
    } else {
        layout.content_left.max(layout.content_right).max(GAP)
    };
    let available = width - 2.0 * margin;
    let fits = (available / title_advance(layout)).floor().max(0.0) as usize;
    if fits == 0 {
        return None;
    }
    Some(media_title(title, cols, rows, fits))
}

fn write_title(
    out: &mut String,
    title: Option<&str>,
    cols: u16,
    rows: usize,
    width: f32,
    layout: &ResolvedLayout,
) {
    let Some(shown) = visible_title(title, cols, rows, width, layout) else {
        return;
    };
    let header_h = layout.header_h;
    let title_font_size = layout.title_font_size;
    let title_fg = layout.window_foreground;
    let resolved = layout.font.resolve(true, false);
    let family = if resolved == layout.font.family {
        String::new()
    } else {
        format!(r#" font-family="{}""#, escape_attribute(resolved))
    };
    let _ = write!(
        out,
        r#"<text x="{cx:.2}" y="{baseline:.2}" fill="{fill}" font-size="{title_font_size}px"{family} font-weight="bold" text-anchor="middle" xml:space="preserve">{esc}</text>"#,
        cx = width / 2.0,
        baseline = header_h / 2.0 + title_font_size * 0.35,
        fill = hex(title_fg),
        esc = escape(&shown),
    );
}

/// How much of a cell the thin cursor shapes cover.
const CURSOR_THICKNESS: f32 = 2.0;

/// Draw the cursor over the cell it sits on.
///
/// A block is filled and the character redrawn in the cell's background color,
/// which is how a terminal keeps the character under a block cursor readable.
/// It is drawn after the text pass so the block covers the first, normally
/// colored draw of that character.
fn write_cursor(
    out: &mut String,
    rows: &[Vec<EmuCell>],
    (cx, cy): (u16, usize),
    colors: &dyn RenderColors,
    nerd_font: &NerdFont,
    layout: &ResolvedLayout,
) {
    let Some(row) = rows.get(cy) else {
        return;
    };
    let cell_w = layout.cell_w;
    let cell_h = layout.cell_h;
    let header_h = layout.header_h;
    let cell = cell_at(row, cx as usize);
    // A double-width character stores its second half as a continuation cell,
    // so the cursor has to cover both or it clips the glyph down the middle.
    let span = if row
        .get(cx as usize + 1)
        .is_some_and(|next| next.ch == CONTINUATION)
    {
        2.0
    } else {
        1.0
    };
    let w = span * cell_w;
    let x = layout.content_left + cx as f32 * cell_w;
    let y = header_h + layout.content_top + cy as f32 * cell_h;
    let fill = hex(colors.color(ColorSlot::Cursor));

    let (rx, ry, rw, rh) = match colors.cursor_shape() {
        CursorShape::Block => (x, y, w, cell_h),
        CursorShape::Underline => (x, y + cell_h - CURSOR_THICKNESS, w, CURSOR_THICKNESS),
        CursorShape::Bar => (x, y, CURSOR_THICKNESS, cell_h),
    };
    let _ = write!(
        out,
        r#"<rect x="{rx:.2}" y="{ry:.2}" width="{rw:.2}" height="{rh:.2}" fill="{fill}"/>"#
    );

    if colors.cursor_shape() != CursorShape::Block {
        return;
    }
    let mut paint = cell_paint(cell, colors);
    paint.fg = bg_of(cell, colors);
    if cell.underline_color.is_none() {
        // A missing underline color follows the foreground: the redrawn
        // glyph uses the cell background as its fill, so the decorations
        // follow it and the redraw keeps the grid pass markup.
        paint.underline_color = paint.fg;
    }
    write_text_run(
        out,
        row,
        cx as usize..cx as usize + span as usize,
        cy,
        paint,
        nerd_font,
        layout,
    );
}

/// Render the grid. `cursor` is where to draw the cursor *within `rows`*, so a
/// caller passing scrollback has already offset it, and `None` means the
/// terminal is not showing one. `title` is the window title a program set,
/// drawn in the title bar, and `None` leaves the bar bare.
///
/// Its row is a `usize` because it indexes `rows`, which for a full-history
/// render is as long as the scrollback and so is not bounded by the screen.
///
/// The style entry point keeps the recorded layout: it resolves the style to
/// a [`ResolvedLayout`] and draws from that, exactly like offline rendering.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_svg(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn RenderColors,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
    style: &Style,
    zoom: f64,
    background: Option<CaptureBackground>,
) -> String {
    let layout = ResolvedLayout::from_style(style, zoom);
    render_svg_with_layout(rows, cols, colors, cursor, title, &layout, background)
}

/// Draw the grid from an already-resolved layout.
///
/// Live screenshots resolve their [`Style`] first; offline snapshots resolve
/// [`crate::render::offline::SnapshotRenderOptions`]. Both draw here, so
/// there is one renderer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_svg_with_layout(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn RenderColors,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
    layout: &ResolvedLayout,
    background: Option<CaptureBackground>,
) -> String {
    let cell_w = layout.cell_w;
    let cell_h = layout.cell_h;
    let font_size = layout.font_size;
    let header_h = layout.header_h;
    let divider_h = layout.divider_h;
    let radius = layout.border_radius;
    let shadow_color = layout.shadow_color;
    let title_bg = layout.window_background;
    let title_divider = layout.window_divider;
    let font_family = escape_attribute(&layout.font.family);
    let nerd_font = NerdFont::new(rows, font_size);
    let (panel_width, panel_height) = layout.panel_size(cols as usize, rows.len());
    let pad_left = layout.canvas_left as f32;
    let pad_top = layout.canvas_top as f32;
    let width = panel_width + pad_left + layout.canvas_right as f32;
    let height = panel_height + pad_top + layout.canvas_bottom as f32;
    // THE size the PNG pixmap and both evidences share: one rounding at
    // the pixel boundary. Callers validate metrics and zoom first (live
    // resolves the zoom, offline checks the size before drawing), so this
    // is only `None` on already-validated input past `u32` range.
    let (output_width, output_height) = layout
        .output_pixel_size(u32::from(cols), rows.len())
        .expect("output size was checked by the caller");
    // The viewBox is the exact unscaled canvas, so the drawn content fills
    // it with no rounding sliver; the integer `width`/`height` above is the
    // same rect in pixels.
    let view_width = svg_dimension(f64::from(width));
    let view_height = svg_dimension(f64::from(height));
    let cols = cols as usize;
    let x0 = layout.content_left;
    let y0 = header_h + layout.content_top;

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{output_width}" height="{output_height}" viewBox="0 0 {view_width} {view_height}" font-family="{font_family}" font-size="{font_size}px">"#
    );
    nerd_font.write_defs(&mut out);
    match background {
        Some(CaptureBackground::Transparent) => {}
        Some(CaptureBackground::Color(color)) => {
            let _ = write!(
                out,
                r#"<rect width="{view_width}" height="{view_height}" fill="{}"/>"#,
                hex(color)
            );
        }
        None => {
            let _ = write!(
                out,
                r#"<rect width="{view_width}" height="{view_height}" fill="{}"/>"#,
                hex(layout.canvas_background)
            );
        }
    }
    for (spread, offset_y, alpha) in layout.shadow_layers() {
        let shadow_x = pad_left - spread;
        let shadow_y = pad_top - spread + offset_y;
        let shadow_width = panel_width + spread * 2.0;
        let shadow_height = panel_height + spread * 2.0;
        let shadow_radius = radius + spread;
        let opacity = f32::from(alpha) / 255.0;
        let _ = write!(
            out,
            r#"<rect x="{shadow_x:.1}" y="{shadow_y:.1}" width="{shadow_width:.1}" height="{shadow_height:.1}" rx="{shadow_radius:.1}" fill="{}" fill-opacity="{opacity:.6}"/>"#,
            hex(shadow_color)
        );
    }
    // Exact panel decimals, like the viewBox: rounding here would leave a
    // canvas sliver along the right and bottom edges under fractional
    // metrics. Integer layouts format identically to before.
    let panel_w = svg_dimension(f64::from(panel_width));
    let panel_h = svg_dimension(f64::from(panel_height));
    let _ = write!(
        out,
        r#"<g transform="translate({pad_left:.0} {pad_top:.0})"><rect width="{panel_w}" height="{panel_h}" rx="{radius:.0}" fill="{}"/>"#,
        hex(colors.resolve(None, false))
    );
    if layout.title_bar {
        let title_bottom = header_h - divider_h;
        let radius = radius.min(panel_width / 2.0).min(title_bottom);
        let right_curve = panel_width - radius;
        let _ = write!(
            out,
            r#"<path d="M0 {radius:.1} Q0 0 {radius:.1} 0 H{right_curve:.1} Q{panel_width:.1} 0 {panel_width:.1} {radius:.1} V{title_bottom:.1} H0 Z" fill="{}"/>"#,
            hex(title_bg)
        );
        let _ = write!(
            out,
            r#"<rect y="{title_bottom:.1}" width="{panel_w}" height="{divider_h:.1}" fill="{}"/>"#,
            hex(title_divider)
        );
        let lights = layout.traffic_lights();
        for (i, dot) in lights.iter().copied().enumerate() {
            let cx = layout.content_left + 5.0 + i as f32 * 20.0;
            let _ = write!(
                out,
                r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{DOT_R:.1}" fill="{}"/>"#,
                hex(dot),
                cy = header_h / 2.0,
            );
        }
        if !lights.is_empty() {
            let _ = write!(
                out,
                r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{RED_DOT_R:.1}" fill="{}"/>"#,
                hex(RED_DOT_COLOR),
                cx = layout.content_left + 5.0,
                cy = header_h / 2.0,
            );
        }
        write_title(
            &mut out,
            title,
            cols as u16,
            rows.len().max(1),
            panel_width,
            layout,
        );
    }

    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        while x < cols {
            let bg = bg_of(cell_at(row, x), colors);
            let mut run = 1;
            while x + run < cols && bg_of(cell_at(row, x + run), colors) == bg {
                run += 1;
            }
            if bg != colors.resolve(None, false) {
                let rx = x0 + x as f32 * cell_w;
                let ry = y0 + y as f32 * cell_h;
                let rw = run as f32 * cell_w;
                let _ = write!(
                    out,
                    r#"<rect x="{rx:.2}" y="{ry:.2}" width="{rw:.2}" height="{cell_h:.2}" fill="{}"/>"#,
                    hex(bg)
                );
            }
            x += run;
        }
    }

    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        while x < cols {
            let paint = cell_paint(cell_at(row, x), colors);
            let mut run = 1;
            while x + run < cols && cell_paint(cell_at(row, x + run), colors) == paint {
                run += 1;
            }
            write_text_run(&mut out, row, x..x + run, y, paint, &nerd_font, layout);
            x += run;
        }
    }

    if let Some(at) = cursor {
        write_cursor(&mut out, rows, at, colors, &nerd_font, layout);
    }

    // Last, so the content it frames cannot paint over it. The rect is inset
    // by half the stroke because SVG centers a stroke on its path, and a
    // border that straddled the panel edge would bleed into the padding.
    if layout.border_width > 0.0 {
        let inset = layout.border_width / 2.0;
        let _ = write!(
            out,
            r#"<rect x="{inset:.2}" y="{inset:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            (panel_width - layout.border_width).max(0.0),
            (panel_height - layout.border_width).max(0.0),
            (radius - inset).max(0.0),
            hex(layout.border_color),
            layout.border_width,
        );
    }

    out.push_str("</g></svg>");
    out
}

/// Grid pixel size for tests that derive expectations from the style.
///
/// The renderers read the same method through the layout, so these stay in
/// sync by construction rather than by duplicated math.
#[cfg(all(test, feature = "recording-raster"))]
pub(crate) fn test_pixel_size(cols: u16, rows: usize, style: &Style) -> (u32, u32) {
    ResolvedLayout::from_style(style, 1.0).even_grid_pixel_size(cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The geometry the assertions below are written against.
    fn geometry() -> Style {
        Style::default()
    }
    fn cell_w() -> f32 {
        geometry().cell_width()
    }
    fn cell_h() -> f32 {
        geometry().cell_height()
    }
    fn header_h() -> f32 {
        geometry().header_height()
    }
    fn font_baseline() -> f32 {
        geometry().baseline()
    }
    use crate::profile::{ColorSlot, Profile};
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{Color, UnderlineStyle};

    /// A real emulator: the renderer resolves through the same path a session
    /// uses, so a stand-in could not drift from it.
    fn colors() -> AlacrittyEmu {
        AlacrittyEmu::new(10, 2, &Profile::default())
    }

    fn cell(ch: &str, fg: Option<Color>, bg: Option<Color>) -> EmuCell {
        EmuCell {
            ch: ch.into(),
            fg,
            bg,
            ..EmuCell::blank()
        }
    }

    /// Each shape draws something recognisably different.
    ///
    /// A block covers the cell, an underline sits on the bottom edge, and a
    /// bar on the left, so all three are checked by the rectangle they emit
    /// rather than by merely appearing.
    #[test]
    fn each_cursor_shape_draws_its_own_rectangle() {
        use crate::terminal::emu::Emulator;
        let rows = vec![vec![cell("x", None, None)]];
        let cursor_fill = hex(Profile::default().colors.cursor);

        let mut emu = colors();
        let block = render_svg(
            &rows,
            1,
            &emu,
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(
            block.contains(&format!(
                r#"width="10.00" height="21.00" fill="{cursor_fill}""#
            )),
            "a block covers the whole cell: {block}"
        );

        emu.process(b"\x1b[4 q");
        let underline = render_svg(
            &rows,
            1,
            &emu,
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(
            underline.contains(&format!(
                r#"width="10.00" height="2.00" fill="{cursor_fill}""#
            )),
            "an underline is a thin full-width bar: {underline}"
        );

        emu.process(b"\x1b[6 q");
        let bar = render_svg(
            &rows,
            1,
            &emu,
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(
            bar.contains(&format!(
                r#"width="2.00" height="21.00" fill="{cursor_fill}""#
            )),
            "a bar is a thin full-height stripe: {bar}"
        );
    }

    /// The character under a block cursor is redrawn in the cell background,
    /// which is how a terminal keeps it readable rather than hiding it behind
    /// the block.
    #[test]
    fn a_block_cursor_keeps_its_character_readable() {
        let rows = vec![vec![cell("Z", None, None)]];
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        let background = hex(Profile::default().colors.background);
        assert!(
            svg.contains(&format!(r#"fill="{background}""#)) && svg.matches(">Z<").count() == 2,
            "the character is drawn again, in the background color: {svg}"
        );
    }

    #[test]
    fn a_block_cursor_does_not_reveal_invisible_text() {
        let mut hidden = cell("X", None, None);
        hidden.attrs = Attrs::INVISIBLE;
        let svg = render_svg(
            &[vec![hidden]],
            1,
            &colors(),
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(
            !svg.contains('X'),
            "the cursor must not redraw text hidden with SGR 8: {svg}"
        );
    }

    /// An explicit underline color wins; otherwise the paint keeps the
    /// resolved foreground color, for live and offline colors alike.
    #[test]
    fn underline_paint_prefers_the_explicit_color() {
        let emu = colors();
        let foreground = RenderColors::color(&emu, ColorSlot::Foreground);

        let mut rgb = cell("u", None, None);
        rgb.underline = UnderlineStyle::Single;
        rgb.underline_color = Some(Color::Rgb(10, 20, 30));
        let paint = cell_paint(&rgb, &emu);
        assert!(paint.underline);
        assert_eq!(paint.underline_color, Rgb::new(10, 20, 30));

        let mut indexed = cell("u", None, None);
        indexed.underline = UnderlineStyle::Single;
        indexed.underline_color = Some(Color::Idx(1));
        let paint = cell_paint(&indexed, &emu);
        assert_eq!(
            paint.underline_color,
            RenderColors::color(&emu, ColorSlot::Indexed(1)),
            "an indexed underline color resolves through the palette"
        );

        let mut missing = cell("u", None, None);
        missing.underline = UnderlineStyle::Single;
        let paint = cell_paint(&missing, &emu);
        assert_eq!(paint.underline_color, paint.fg);
        assert_eq!(paint.fg, foreground);

        let plain = cell("u", None, None);
        let paint = cell_paint(&plain, &emu);
        assert!(!paint.underline);
        assert_eq!(
            paint.underline_color, paint.fg,
            "cells without an underline keep the previous paint"
        );
    }

    #[test]
    fn a_block_cursor_preserves_the_character_style() {
        let mut styled = cell("S", None, None);
        styled.attrs = Attrs::BOLD | Attrs::ITALIC | Attrs::STRIKE;
        styled.underline = UnderlineStyle::Single;
        let svg = render_svg(
            &[vec![styled]],
            1,
            &colors(),
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );

        for attribute in [
            r#"font-weight="bold""#,
            r#"font-style="italic""#,
            r#"text-decoration="underline line-through""#,
        ] {
            assert_eq!(
                svg.matches(attribute).count(),
                2,
                "the normal draw and cursor redraw both preserve {attribute}: {svg}"
            );
        }
    }

    /// A double-width character keeps both of its halves.
    ///
    /// The second half lives in a continuation cell, so a cursor sized to one
    /// cell would cover half the glyph and redraw it squashed into that half.
    #[test]
    fn a_block_cursor_covers_a_double_width_character() {
        let rows = vec![vec![
            cell("日", None, None),
            cell(CONTINUATION, None, None),
            cell("a", None, None),
        ]];
        let svg = render_svg(
            &rows,
            3,
            &colors(),
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        let cursor_fill = hex(Profile::default().colors.cursor);
        assert!(
            svg.contains(&format!(
                r#"width="20.00" height="21.00" fill="{cursor_fill}""#
            )),
            "the block spans both halves: {svg}"
        );
        assert!(
            svg.contains(
                r#"textLength="20.00" lengthAdjust="spacingAndGlyphs" xml:space="preserve">日<"#
            ),
            "the redraw is given both halves too, so it is not squashed: {svg}"
        );
    }

    /// A vector glyph under a block cursor comes back as a glyph.
    ///
    /// Nerd font characters are drawn as `<use>` references and masked out of
    /// the text run, so redrawing one as text would emit a character the text
    /// font has no glyph for and the block would simply swallow it.
    #[test]
    fn a_block_cursor_redraws_a_vector_glyph() {
        let rows = vec![vec![cell("\u{f115}", None, None)]];
        let background = hex(Profile::default().colors.background);
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert_eq!(
            svg.matches("<use href=\"#nf-f115\"").count(),
            2,
            "the glyph is drawn once normally and once over the block: {svg}"
        );
        let in_background = svg
            .match_indices(r##"<use href="#nf-f115""##)
            .filter(|(start, _)| {
                svg[*start..]
                    .split("/>")
                    .next()
                    .is_some_and(|glyph| glyph.contains(&format!(r#"fill="{background}""#)))
            })
            .count();
        assert_eq!(
            in_background, 1,
            "exactly the redrawn glyph is in the cell background color: {svg}"
        );
    }

    /// A row past what a `u16` holds is still drawn on its own line.
    ///
    /// A profile can set a scrollback deeper than 65535 rows, and a full
    /// render is as long as the scrollback. Counting the offset in a `u16`
    /// wrapped it, which put the cursor on a line that looks plausible and is
    /// tens of thousands of rows from where the terminal left it.
    #[test]
    fn a_cursor_below_the_u16_mark_keeps_its_row() {
        let row = 70_000usize;
        let rows = vec![vec![cell("x", None, None)]; row + 1];
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            Some((0, row)),
            None,
            &Style::default(),
            1.0,
            None,
        );
        let expected = header_h() + Style::default().content_top() + row as f32 * cell_h();
        assert!(
            svg.contains(&format!(r#"<rect x="15.00" y="{expected:.2}""#)),
            "the cursor sits on row {row}, not on a wrapped one"
        );
    }

    /// No cursor is drawn when the caller says the terminal is not showing
    /// one, and an out-of-range position is ignored rather than panicking.
    #[test]
    fn a_hidden_or_out_of_range_cursor_draws_nothing() {
        use crate::terminal::emu::Emulator;
        let rows = vec![vec![cell("x", None, None)]];
        // A color nothing else in the image uses, so finding it can only mean
        // the cursor was drawn. The default cursor color is the foreground,
        // which the text itself is painted with.
        let mut emu = colors();
        emu.process(b"\x1b]12;#ff00ff\x07");

        assert!(
            render_svg(
                &rows,
                1,
                &emu,
                Some((0, 0)),
                None,
                &Style::default(),
                1.0,
                None
            )
            .contains("#ff00ff"),
            "the cursor is drawn when there is one to draw"
        );
        assert!(
            !render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0, None)
                .contains("#ff00ff"),
            "a terminal not showing a cursor gets none"
        );
        // A row past the end of the grid: reachable if a caller miscounts the
        // scrollback offset, and not worth a panic.
        assert!(
            !render_svg(
                &rows,
                1,
                &emu,
                Some((0, 9)),
                None,
                &Style::default(),
                1.0,
                None
            )
            .contains("#ff00ff"),
            "an out-of-range position is ignored"
        );
    }

    /// The cursor is painted in the color `OSC 12` sets, like every other
    /// color the terminal shows.
    #[test]
    fn the_cursor_follows_a_color_a_program_set() {
        use crate::terminal::emu::Emulator;
        let rows = vec![vec![cell("x", None, None)]];
        let mut emu = colors();
        emu.process(b"\x1b]12;#ff00ff\x07");
        assert!(render_svg(
            &rows,
            1,
            &emu,
            Some((0, 0)),
            None,
            &Style::default(),
            1.0,
            None
        )
        .contains("#ff00ff"));
    }

    /// A program that repaints the terminal repaints the screenshot.
    ///
    /// The renderer draws what the terminal is currently showing, not what it
    /// was configured with, so a background set with `OSC 11` is the one that
    /// gets painted. Nothing else covers the path from an escape sequence to
    /// a rendered pixel.
    #[test]
    fn a_screenshot_follows_colors_a_program_set() {
        use crate::terminal::emu::Emulator;
        let mut emu = colors();
        let rows = vec![vec![cell("x", Some(Color::from_index(1)), None)]];

        let before = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0, None);
        assert!(before.contains(&hex(Profile::default().colors.red)));
        assert!(before.contains(&hex(Profile::default().colors.background)));

        // The program picks its own background and recolors palette slot 1.
        emu.process(b"\x1b]11;#3b0764\x07\x1b]4;1;#22c55e\x07");

        let after = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0, None);
        assert!(
            after.contains("#3b0764"),
            "the window is painted with the background the program set"
        );
        assert!(
            after.contains("#22c55e"),
            "a cell follows the slot the program recolored"
        );
        assert!(
            !after.contains(&hex(Profile::default().colors.red)),
            "the configured red is no longer what slot 1 shows"
        );
    }

    /// And a reset puts the configured colors back on screen.
    #[test]
    fn a_screenshot_returns_to_the_profile_after_a_reset() {
        use crate::terminal::emu::Emulator;
        let mut emu = colors();
        let rows = vec![vec![cell("x", Some(Color::from_index(1)), None)]];

        emu.process(b"\x1b]11;#3b0764\x07\x1b]4;1;#22c55e\x07");
        emu.process(b"\x1b]111\x07\x1b]104;1\x07");

        let after = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0, None);
        assert!(after.contains(&hex(Profile::default().colors.background)));
        assert!(after.contains(&hex(Profile::default().colors.red)));
    }

    #[test]
    fn emits_valid_svg_with_text_and_color() {
        let rows = vec![vec![
            cell("h", Some(Color::from_index(1)), None),
            cell("i", Some(Color::from_index(1)), None),
        ]];
        let svg = render_svg(
            &rows,
            2,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("textLength"));
        assert!(
            svg.contains(&hex(Emulator::color(&colors(), ColorSlot::Indexed(1)))),
            "slot 1 is painted with the profile color"
        );
        assert!(svg.contains(">hi</text>"));
        assert!(!svg.contains("<defs>"));
        assert!(!svg.contains("<use"));
    }

    #[test]
    fn zoom_changes_output_size_without_changing_the_view_box() {
        let rows = vec![vec![cell("x", None, None)]];
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            0.5,
            None,
        );
        let full = render_svg(
            &rows,
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        let view_box = {
            let start = full.find("viewBox=\"").expect("a viewBox");
            let rest = &full[start..];
            rest[..rest[9..].find('"').expect("a close") + 10].to_string()
        };
        let (w, h) = {
            let inner = view_box
                .trim_start_matches("viewBox=\"0 0 ")
                .trim_end_matches('"');
            let mut parts = inner.split(' ');
            (
                parts.next().unwrap().parse::<f32>().unwrap(),
                parts.next().unwrap().parse::<f32>().unwrap(),
            )
        };
        // One rounding at the pixel boundary: halving an odd extent (125)
        // ceils to whole pixels (63), exactly like the PNG pixmap.
        assert!(
            svg.contains(&format!(
                r#"width="{}" height="{}" {view_box}"#,
                (w / 2.0).ceil() as u32,
                (h / 2.0).ceil() as u32
            )),
            "halving the zoom halves the drawn size and leaves the view box: {svg}"
        );
    }

    #[test]
    fn canvas_background_can_be_custom_or_transparent() {
        let rows = vec![vec![cell("x", None, None)]];
        let draw = |background| {
            render_svg(
                &rows,
                1,
                &colors(),
                None,
                None,
                &Style::default(),
                1.0,
                background,
            )
        };

        let custom = draw(Some(CaptureBackground::Color(Rgb::new(1, 2, 3))));
        assert!(
            custom.contains(r##"fill="#010203"/>"##),
            "a capture may name its own canvas: {custom}"
        );

        let transparent = draw(Some(CaptureBackground::Transparent));
        assert!(
            !transparent.contains(&hex(Style::default().canvas_background)),
            "or none at all: {transparent}"
        );

        assert!(
            draw(None).contains(&hex(Style::default().canvas_background)),
            "and naming none leaves the style in charge"
        );
    }

    #[test]
    fn emits_window_chrome() {
        let svg = render_svg(
            &[vec![cell(" ", None, None)]],
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(svg.contains("<circle"));
        // Derived, so a deliberate change to a default gap does not read as a
        // broken chrome test.
        let style = Style::default();
        let canvas_width = style.content_left() + style.content_right() + style.cell_width();
        let canvas_height = style.header_height()
            + style.content_top()
            + style.content_bottom()
            + style.cell_height();
        let width = canvas_width + (style.canvas_left() + style.canvas_right()) as f32;
        let height = canvas_height + (style.canvas_top() + style.canvas_bottom()) as f32;
        assert!(
            svg.contains(&format!(
                r##"<rect width="{width:.0}" height="{height:.0}" fill="#6867aa"/>"##
            )),
            "the canvas is the panel plus its gaps: {svg}"
        );
        assert!(svg.contains(r#"<g transform="translate(24 24)">"#));
        assert!(svg.contains(r##"fill="#080812" fill-opacity="0.070588""##));
        assert!(svg.contains(&hex(Profile::default().colors.background)));
        assert!(svg.contains("#d9d9e8"));
        assert!(svg.contains("#000000"));
        assert!(svg.contains("#ec6a5e"));
        assert!(svg.contains("#f4bf4f"));
        assert!(svg.contains("#61c554"));
        assert!(svg.contains("#69110a"));
        assert!(svg.contains(r#"r="2.5""#));
    }

    #[test]
    fn centers_the_text_font_box_in_each_cell() {
        let svg = render_svg(
            &[vec![cell("x", None, None)]],
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        let expected_baseline = header_h() + Style::default().content_top() + font_baseline();
        assert!(svg.contains(&format!(r#"y="{expected_baseline:.2}""#)));
    }

    #[test]
    fn escapes_markup_characters() {
        let rows = vec![vec![cell("<", None, None)]];
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(svg.contains("&lt;"));
        assert!(!svg.contains("><</text>"));
    }

    #[test]
    fn background_run_emitted_for_non_default_bg() {
        let rows = vec![vec![cell(" ", None, Some(Color::from_index(4)))]];
        let svg = render_svg(
            &rows,
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        assert!(
            svg.contains(&hex(Emulator::color(&colors(), ColorSlot::Indexed(4)))),
            "slot 4 is painted with the profile color"
        );
    }

    #[test]
    fn embeds_nerd_font_glyphs_as_vector_paths() {
        let glyph = "\u{f115}";
        let rows = vec![vec![
            cell("a", None, None),
            cell(glyph, None, None),
            cell("b", None, None),
        ]];
        let svg = render_svg(
            &rows,
            3,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );

        assert!(svg.contains(r#"<path id="nf-f115" d=""#));
        assert!(svg.contains(r##"<use href="#nf-f115""##));
        assert!(svg.contains(">a b</text>"));
        assert!(!svg.contains(glyph));
        let family = &Style::default().font.family;
        assert!(svg.contains(&format!(r#"font-family="{family}" font-size="#)));
    }

    #[test]
    fn defines_repeated_nerd_font_glyph_once() {
        let glyph = "\u{f115}";
        let svg = render_svg(
            &[vec![cell(glyph, None, None), cell(glyph, None, None)]],
            2,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );

        assert_eq!(svg.matches(r#"<path id="nf-f115""#).count(), 1);
        assert_eq!(svg.matches(r##"<use href="#nf-f115""##).count(), 2);
    }

    #[test]
    fn leaves_unknown_private_use_glyphs_as_text() {
        let glyph = "\u{10fffd}";
        let svg = render_svg(
            &[vec![cell(glyph, None, None)]],
            1,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );

        assert!(svg.contains(glyph));
        assert!(!svg.contains("<defs>"));
        assert!(!svg.contains(r#"<use href="#));
    }

    /// A window title is drawn centred with the rendered cell dimensions, and
    /// media without a program title gets a useful default.
    #[test]
    fn draws_the_window_title_centred_in_the_bar() {
        let rows = vec![vec![cell("x", None, None); 40]];
        let bare = render_svg(
            &rows,
            40,
            &colors(),
            None,
            None,
            &Style::default(),
            1.0,
            None,
        );
        let titled = render_svg(
            &rows,
            40,
            &colors(),
            None,
            Some("vim: notes.md"),
            &Style::default(),
            1.0,
            None,
        );

        assert!(
            bare.contains(">tui-test capture - 40x1</text>"),
            "untitled media gets the capture default: {bare}"
        );
        assert!(
            titled.contains(">vim: notes.md - 40x1</text>")
                && titled.contains("text-anchor=\"middle\""),
            "the title is drawn, centred: {titled}"
        );
        assert!(
            titled.contains(r##"fill="#414145" font-size="13px" font-weight="bold""##),
            "the title uses the dark title-bar foreground: {titled}"
        );
        // The panel is 2*15 margin + 40 cells of 10, so its middle is 215.
        assert!(
            titled.contains(r#"<text x="215.00""#),
            "centred on the panel, not on the grid origin: {titled}"
        );
    }

    #[test]
    fn title_uses_the_configured_bold_font_family() {
        for bold in [None, Some("Base Mono"), Some(r#"Heavy "Mono" & Co"#)] {
            let style = Style {
                font: crate::render::style::FontFamilies {
                    family: "Base Mono".into(),
                    bold: bold.map(str::to_string),
                    ..Default::default()
                },
                ..Style::default()
            };
            let svg = render_svg(&[], 40, &colors(), None, Some("title"), &style, 1.0, None);
            let title = svg
                .split("<text ")
                .find(|element| element.contains(">title - 40x1</text>"))
                .expect("title element");
            assert!(title.contains(r#"font-weight="bold""#));
            if bold.is_some_and(|family| family != "Base Mono") {
                assert!(title.contains(r#"font-family="Heavy &quot;Mono&quot; &amp; Co""#));
            } else {
                assert!(!title.contains("font-family="));
            }
        }
    }

    #[test]
    fn title_uses_the_space_released_by_disabled_traffic_lights() {
        let mut style = Style::default();
        let layout = ResolvedLayout::from_style(&style, 1.0);
        assert_eq!(visible_title(Some("title"), 12, 1, 150.0, &layout), None);
        style.window.traffic_lights = false;
        let layout = ResolvedLayout::from_style(&style, 1.0);
        assert_eq!(
            visible_title(Some("title"), 12, 1, 150.0, &layout).as_deref(),
            Some("title - 12x1")
        );
        let svg = render_svg(&[], 12, &colors(), None, Some("title"), &style, 1.0, None);
        assert!(svg.contains(">title - 12x1</text>"));
    }

    #[test]
    fn title_bar_radius_stays_within_its_bounds() {
        let mut style = Style::default();
        style.border.radius = 10_000.0;
        for (cols, expected) in [
            (1, "M0 20.0 Q0 0 20.0 0 H20.0 Q40.0 0 40.0 20.0 V33.0 H0 Z"),
            (
                40,
                "M0 33.0 Q0 0 33.0 0 H397.0 Q430.0 0 430.0 33.0 V33.0 H0 Z",
            ),
        ] {
            let svg = render_svg(&[], cols, &colors(), None, None, &style, 1.0, None);
            assert!(svg.contains(expected), "{svg}");
        }
    }

    #[test]
    fn truncates_a_title_that_does_not_fit() {
        let rows = vec![vec![cell("x", None, None); 20]];
        let long = "a-very-long-window-title-that-cannot-possibly-fit";
        let svg = render_svg(
            &rows,
            20,
            &colors(),
            None,
            Some(long),
            &Style::default(),
            1.0,
            None,
        );

        assert!(!svg.contains(long), "the full title cannot have been drawn");
        let drawn = svg
            .split("text-anchor=\"middle\" xml:space=\"preserve\">")
            .nth(1)
            .and_then(|rest| rest.split("</text>").next())
            .expect("a title element");
        assert!(drawn.contains('…'), "truncation is marked: {drawn}");
        assert!(
            drawn.ends_with(" - 20x1"),
            "the dimensions survive truncation: {drawn}"
        );
        assert_fits_clear_of_the_controls(drawn, 20.0);
    }

    /// A wide-glyph title is budgeted by the columns it really occupies.
    ///
    /// The bar inherits the monospace stack, so a CJK glyph takes two
    /// advances. Sized by character count it would be drawn twice as wide as
    /// measured and, being centred, would spill over the controls at one end
    /// and past the panel at the other.
    #[test]
    fn budgets_a_wide_glyph_title_by_column() {
        let rows = vec![vec![cell("x", None, None); 24]];
        let svg = render_svg(
            &rows,
            24,
            &colors(),
            None,
            Some(&"你".repeat(40)),
            &Style::default(),
            1.0,
            None,
        );
        let drawn = svg
            .split("text-anchor=\"middle\" xml:space=\"preserve\">")
            .nth(1)
            .and_then(|rest| rest.split("</text>").next())
            .expect("a title element");
        assert_fits_clear_of_the_controls(drawn, 24.0);
    }

    /// The drawn title must sit inside the space between the traffic lights
    /// and the mirrored margin on the right.
    fn assert_fits_clear_of_the_controls(drawn: &str, cols: f32) {
        let panel =
            Style::default().content_left() + Style::default().content_right() + cols * cell_w();
        let layout = ResolvedLayout::from_style(&geometry(), 1.0);
        let drawn_width =
            crate::terminal::cell::display_width(drawn) as f32 * title_advance(&layout);
        assert!(
            drawn_width <= panel - 2.0 * dots_right(&layout),
            "title {drawn:?} is {drawn_width} wide, past the {} available",
            panel - 2.0 * dots_right(&layout)
        );
    }

    /// A title is markup-escaped like any other text. It comes from whatever
    /// the program chose to send, so an unescaped one would let that program
    /// inject elements into the image.
    #[test]
    fn escapes_markup_in_the_title() {
        let rows = vec![vec![cell("x", None, None); 40]];
        let svg = render_svg(
            &rows,
            40,
            &colors(),
            None,
            Some("</text><script>x</script>"),
            &Style::default(),
            1.0,
            None,
        );

        assert!(!svg.contains("<script>"), "no injected element: {svg}");
        assert!(
            svg.contains("&lt;script&gt;"),
            "it is escaped instead: {svg}"
        );
    }

    /// Each knob has to reach the output. The golden pins the default, which
    /// would keep passing if the renderer ignored every configured value, so
    /// this renders non-default styles and looks for the difference.
    #[test]
    fn a_configured_style_changes_what_is_drawn() {
        use crate::render::style::{BorderStyle, FontFamilies, ShadowStyle, WindowStyle};
        // Wide enough that the title fits, or its color is never painted.
        let rows = vec![vec![cell("x", None, None); 40]];
        let draw =
            |style: &Style| render_svg(&rows, 40, &colors(), None, Some("title"), style, 1.0, None);
        let plain = draw(&Style::default());

        let bigger = draw(&Style {
            font_size: 34.0,
            ..Style::default()
        });
        assert!(
            bigger.contains(r#"font-size="34px""#),
            "the font size reaches the root: {bigger}"
        );

        let recolored = draw(&Style {
            canvas_background: Rgb::new(1, 2, 3),
            ..Style::default()
        });
        assert!(
            recolored.contains("#010203"),
            "the canvas background is painted"
        );
        assert!(!plain.contains("#010203"));

        let padded = draw(&Style {
            canvas_padding: crate::render::style::Padding::Uniform(40),
            ..Style::default()
        });
        assert!(
            padded.contains("translate(40 40)"),
            "padding offsets the panel: {padded}"
        );

        let bare = draw(&Style {
            window: WindowStyle {
                title_bar: false,
                ..WindowStyle::default()
            },
            ..Style::default()
        });
        assert!(!bare.contains("#69110a"), "no close button without a bar");
        assert!(!bare.contains(">title"), "and no title drawn over the grid");

        let no_lights = draw(&Style {
            window: WindowStyle {
                traffic_lights: false,
                ..WindowStyle::default()
            },
            ..Style::default()
        });
        assert!(
            !no_lights.contains("#ec6a5e"),
            "the lights can be turned off"
        );
        assert!(
            no_lights.contains("#d9d9e8"),
            "while the bar they sit on stays"
        );

        let themed = draw(&Style {
            window: WindowStyle {
                background: Rgb::new(9, 9, 9),
                foreground: Rgb::new(8, 8, 8),
                divider: Rgb::new(7, 7, 7),
                ..WindowStyle::default()
            },
            ..Style::default()
        });
        for expected in ["#090909", "#080808", "#070707"] {
            assert!(themed.contains(expected), "{expected} is painted: {themed}");
        }

        let unshadowed = draw(&Style {
            shadow: ShadowStyle {
                enabled: false,
                ..ShadowStyle::default()
            },
            ..Style::default()
        });
        assert!(
            !unshadowed.contains("#080812"),
            "the shadow can be turned off"
        );
        assert!(plain.contains("#080812"));

        let recast = draw(&Style {
            shadow: ShadowStyle {
                color: Rgb::new(4, 5, 6),
                ..ShadowStyle::default()
            },
            ..Style::default()
        });
        assert!(
            recast.contains("#040506"),
            "the shadow color reaches the output"
        );

        let rounded = draw(&Style {
            border: BorderStyle {
                radius: 2.0,
                ..BorderStyle::default()
            },
            ..Style::default()
        });
        assert!(rounded.contains(r#"rx="2""#), "the corner radius applies");

        let lettered = draw(&Style {
            font: FontFamilies {
                family: "Berkeley Mono".into(),
                ..FontFamilies::default()
            },
            ..Style::default()
        });
        assert!(lettered.contains(r#"font-family="Berkeley Mono""#));
    }

    /// A font family is a configured string that lands inside a quoted XML
    /// attribute, so a quote in it would close the attribute and let the rest
    /// become markup. Text escaping is not enough there: a quote is harmless
    /// in content and fatal in an attribute.
    #[test]
    fn a_font_family_cannot_break_out_of_its_attribute() {
        use crate::render::style::FontFamilies;
        let rows = vec![vec![cell("x", None, None); 2]];
        let svg = render_svg(
            &rows,
            2,
            &colors(),
            None,
            None,
            &Style {
                font: FontFamilies {
                    family: r#"Evil" onload="alert(1)"#.into(),
                    ..FontFamilies::default()
                },
                ..Style::default()
            },
            1.0,
            None,
        );
        assert!(
            !svg.contains(r#"onload="alert(1)""#),
            "the quote must not close the attribute: {svg}"
        );
        assert!(svg.contains("&quot;"), "it is escaped instead: {svg}");
    }

    /// A config file has to reach the picture. Every other test here starts
    /// from a `Style` built in Rust, so the whole chain from TOML to output
    /// could be broken — a hop substituting `Style::default()` — and they
    /// would all still pass.
    #[test]
    fn a_style_from_a_config_file_reaches_the_output() {
        let config = crate::profile::ConfigFile::parse(
            "[profiles.docs.recording.style]\nfont_size = 24\ncanvas_padding = 40\n\
             canvas_background = \"#101112\"\n\
             \n[profiles.docs.recording.style.window]\ntitle_bar = false\n",
        )
        .expect("the config parses");
        let style = config
            .settings(Some("docs"))
            .expect("the profile resolves")
            .style;

        let rows = vec![vec![cell("x", None, None); 4]];
        let svg = render_svg(&rows, 4, &colors(), None, Some("t"), &style, 1.0, None);

        assert!(svg.contains(r#"font-size="24px""#), "font_size: {svg}");
        assert!(svg.contains("translate(40 40)"), "padding: {svg}");
        assert!(svg.contains("#101112"), "background: {svg}");
        assert!(!svg.contains("#d9d9e8"), "title_bar = false: {svg}");
    }

    /// Knobs the broader test does not reach, each of which would otherwise
    /// only be pinned for its default by the golden.
    #[test]
    fn the_remaining_knobs_reach_the_output() {
        use crate::render::style::ShadowStyle;
        let rows = vec![vec![cell("x", None, None); 40]];
        let draw =
            |style: &Style| render_svg(&rows, 40, &colors(), None, Some("title"), style, 1.0, None);

        let titled = draw(&Style {
            title_font_size: 21.0,
            ..Style::default()
        });
        assert!(
            titled.contains(r#"font-size="21px""#),
            "the title font size is its own knob: {titled}"
        );

        // The shadow's geometry is derived, so this proves the derivation is
        // wired into the renderer rather than only unit-tested.
        let default_shadow = draw(&Style::default());
        let cast = draw(&Style {
            shadow: ShadowStyle {
                offset: 20.0,
                spread: 28.0,
                ..ShadowStyle::default()
            },
            ..Style::default()
        });
        assert_ne!(
            default_shadow, cast,
            "changing the shadow's offset and spread moves the rectangles"
        );
        assert!(
            cast.contains(r#"rx="36.0""#),
            "the outermost layer is spread past the corner radius: {cast}"
        );
    }

    /// `border.width` and `border.color` were accepted, validated and then
    /// ignored by both renderers.
    #[test]
    fn a_border_is_stroked_when_one_is_asked_for() {
        use crate::render::style::BorderStyle;
        let rows = vec![vec![cell("x", None, None); 4]];
        let draw = |border: BorderStyle| {
            render_svg(
                &rows,
                4,
                &colors(),
                None,
                Some("t"),
                &Style {
                    border,
                    ..Style::default()
                },
                1.0,
                None,
            )
        };

        assert!(
            !draw(BorderStyle::default()).contains("stroke"),
            "the default asks for no border and gets none"
        );

        let bordered = draw(BorderStyle {
            width: 4.0,
            color: crate::profile::Rgb::new(255, 0, 0),
            radius: 8.0,
        });
        assert!(
            bordered.contains(r##"stroke="#ff0000""##),
            "the configured color is used: {bordered}"
        );
        assert!(
            bordered.contains(r#"stroke-width="4.00""#),
            "the configured width is used: {bordered}"
        );
        assert!(
            bordered.contains(r#"x="2.00" y="2.00""#),
            "and the stroke is inset by half its width so it stays on the panel: {bordered}"
        );
    }

    /// `font.bold` and friends were accepted and never drawn.
    #[test]
    fn a_variant_font_is_asked_for_only_where_it_differs() {
        use crate::render::style::FontFamilies;
        let rows = vec![vec![
            cell("a", None, None),
            EmuCell {
                attrs: Attrs::BOLD,
                ..cell("b", None, None)
            },
        ]];
        let style = Style {
            font: FontFamilies {
                family: "Base Mono".into(),
                bold: Some("Heavy Mono".into()),
                ..FontFamilies::default()
            },
            ..Style::default()
        };
        let svg = render_svg(&rows, 2, &colors(), None, Some("t"), &style, 1.0, None);

        assert!(
            svg.contains(r#"font-family="Base Mono""#),
            "the root carries the base family: {svg}"
        );
        assert!(
            svg.contains(r#"font-family="Heavy Mono" font-weight="bold""#),
            "and a bold run asks for the bold family: {svg}"
        );
        assert_eq!(
            svg.matches("font-family=").count(),
            2,
            "the plain run inherits the root rather than repeating it: {svg}"
        );
    }

    /// One number used to set every gap, so a wider bottom meant a wider
    /// everything.
    #[test]
    fn each_gap_around_the_window_can_differ() {
        use crate::render::style::{Padding, PaddingSides};
        let rows = vec![vec![cell("x", None, None); 4]];
        let style = Style {
            canvas_padding: Padding::Sides(PaddingSides {
                top: Some(10),
                right: Some(20),
                bottom: Some(60),
                left: Some(30),
            }),
            ..Style::default()
        };
        let svg = render_svg(&rows, 4, &colors(), None, Some("t"), &style, 1.0, None);

        assert!(
            svg.contains("translate(30 10)"),
            "the window sits at the left and top gaps: {svg}"
        );

        // Measured against the default rather than recomputed: the default is
        // 24 on every side, so each axis differs by the gaps it actually got.
        let box_of = |svg: &str| -> (f32, f32) {
            let start = svg.find("viewBox=\"0 0 ").expect("a viewBox") + 13;
            let rest = &svg[start..];
            let end = rest.find('"').expect("a closing quote");
            let mut parts = rest[..end].split(' ');
            (
                parts.next().unwrap().parse().unwrap(),
                parts.next().unwrap().parse().unwrap(),
            )
        };
        let plain = render_svg(
            &rows,
            4,
            &colors(),
            None,
            Some("t"),
            &Style::default(),
            1.0,
            None,
        );
        let (plain_w, plain_h) = box_of(&plain);
        let (wide_w, wide_h) = box_of(&svg);
        assert_eq!(
            (wide_w - plain_w, wide_h - plain_h),
            ((30.0 + 20.0) - 48.0, (10.0 + 60.0) - 48.0),
            "each axis grows by its own two gaps, not by one of them doubled"
        );

        // A table naming one side leaves the rest at the default rather than
        // collapsing them to zero.
        let one = Style {
            canvas_padding: Padding::Sides(PaddingSides {
                bottom: Some(60),
                ..crate::render::style::PaddingSides::default()
            }),
            ..Style::default()
        };
        let svg = render_svg(&rows, 4, &colors(), None, Some("t"), &one, 1.0, None);
        assert!(
            svg.contains("translate(24 24)"),
            "the sides it did not name keep the default: {svg}"
        );
    }

    /// The gap inside the window was three constants, and the one above the
    /// grid was the smallest of them, so text sat against the title bar and
    /// against the top edge when there was no title bar at all.
    #[test]
    fn the_gap_inside_the_window_is_configurable_on_every_side() {
        use crate::render::style::{Padding, PaddingSides};
        let rows = vec![vec![cell("x", None, None); 4]];
        let default = Style::default();
        assert_eq!(
            (
                default.content_top(),
                default.content_right(),
                default.content_bottom(),
                default.content_left()
            ),
            (8.0, 15.0, 14.0, 15.0),
            "the default top gap is no longer a quarter of the side gaps"
        );

        let style = Style {
            content_padding: Padding::Sides(PaddingSides {
                top: Some(40),
                left: Some(30),
                ..PaddingSides::default()
            }),
            ..Style::default()
        };
        let svg = render_svg(&rows, 4, &colors(), None, Some("t"), &style, 1.0, None);

        // The first run starts at the left gap, and its baseline sits below the
        // title bar by the top gap.
        assert!(
            svg.contains(r#"<text x="30.00""#),
            "the grid starts at the left gap: {svg}"
        );
        let baseline = style.header_height() + 40.0 + style.baseline();
        assert!(
            svg.contains(&format!(r#"y="{baseline:.2}""#)),
            "and below the title bar by the top gap: {svg}"
        );

        // A side the table does not name keeps its own default rather than the
        // one the named sides happen to use.
        assert_eq!(style.content_bottom(), 14.0);
        assert_eq!(style.content_right(), 15.0);
    }
}

#[cfg(test)]
mod golden {
    use super::*;
    use crate::profile::Profile;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::emu::Emulator;

    /// The default style must render exactly what the renderer did when every
    /// value here was a constant.
    ///
    /// The appearance was moved out of the source and into [`Style`], which is
    /// only safe if the defaults reproduce it: a refactor that shifted a
    /// margin by a pixel would silently restyle every existing recording, and
    /// nothing else in the suite compares whole output.
    ///
    /// Set `TUI_TEST_UPDATE_GOLDEN=1` to rewrite the file when the default look
    /// is meant to change, so the new bytes land in a diff a reviewer can read
    /// rather than being pasted out of an assertion message.
    #[test]
    fn the_default_style_renders_the_original_bytes() {
        let mut emu = AlacrittyEmu::new(20, 3, &Profile::default());
        emu.process(b"\x1b]0;golden\x07");
        emu.process("\x1b[1mbold\x1b[0m \x1b[3mit\x1b[0m \x1b[4mul\x1b[0m \u{4f60}".as_bytes());
        emu.process(b"\r\n\x1b[31;44mcolor\x1b[0m");
        let rendered = render_svg(
            &emu.viewable_rows(),
            20,
            &emu as &dyn RenderColors,
            Some((0, 1)),
            Some("golden"),
            &Style::default(),
            1.0,
            None,
        );
        let golden = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/render/testdata/default-style.svg"
        );
        if std::env::var_os("TUI_TEST_UPDATE_GOLDEN").is_some() {
            std::fs::write(golden, &rendered).expect("rewrite the golden");
            return;
        }
        assert_eq!(rendered, include_str!("testdata/default-style.svg"));
    }
}
