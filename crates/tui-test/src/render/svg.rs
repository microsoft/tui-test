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
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::render::style::Style;
use crate::terminal::cell::{truncate_to_columns, Attrs, Color, EmuCell, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

pub(crate) const MARGIN_X: f32 = 15.0;
pub(crate) const CONTENT_PADDING_TOP: f32 = 4.0;
const MARGIN_BOTTOM: f32 = 14.0;
pub(crate) const DOT_R: f32 = 7.0;
pub(crate) const RED_DOT_R: f32 = 2.5;
pub(crate) const RED_DOT_COLOR: Rgb = Rgb::new(105, 17, 10);
/// Title bar text, smaller than the grid font so the chrome does not compete
/// with the terminal content itself.
/// Where the rightmost traffic light ends. A centred title is kept clear of
/// this on both sides, so it can never be drawn over the controls.
const DOTS_RIGHT: f32 = MARGIN_X + 5.0 + 2.0 * 20.0 + DOT_R;

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

/// What to paint one cell's text with, after inverse and dim have been
/// resolved. Distinct from [`crate::render::style::Style`], which describes
/// the whole image rather than a cell in it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct CellPaint {
    pub fg: Rgb,
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
    CellPaint {
        fg,
        bold: cell.has(Attrs::BOLD),
        italic: cell.has(Attrs::ITALIC),
        underline: cell.underline.is_underlined(),
        strike: cell.has(Attrs::STRIKE),
        invisible: cell.has(Attrs::INVISIBLE),
    }
}

/// Escape a value going into a double-quoted XML attribute.
///
/// [`escape`] is for text content, where a quote is harmless. Inside an
/// attribute a quote closes it, so a font family named `Foo" onload="x` would
/// otherwise write arbitrary markup into the document.
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
    style: &Style,
) {
    if paint.invisible {
        return;
    }
    let (start, end) = (run.start, run.end);
    let cell_w = style.cell_width();
    let cell_h = style.cell_height();
    let header_h = style.header_height();
    let font_baseline = style.baseline();
    let fg = hex(paint.fg);
    let tx = MARGIN_X + start as f32 * cell_w;
    let baseline = header_h + CONTENT_PADDING_TOP + y as f32 * cell_h + font_baseline;
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
        let deco = match (paint.underline, paint.strike) {
            (true, true) => r#" text-decoration="underline line-through""#,
            (true, false) => r#" text-decoration="underline""#,
            (false, true) => r#" text-decoration="line-through""#,
            (false, false) => "",
        };
        // Only when the variant names a family of its own; otherwise the root
        // font-family already says it, and repeating it on every run would
        // bloat the document for no change in what is drawn.
        let resolved = style.font.resolve(paint.bold, paint.italic);
        let family = if resolved == style.font.family {
            String::new()
        } else {
            format!(r#" font-family="{}""#, escape_attribute(resolved))
        };
        let _ = write!(
            out,
            r#"<text x="{tx:.2}" y="{baseline:.2}" fill="{fg}"{family}{weight}{italic}{deco} textLength="{width:.2}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc}</text>"#,
            esc = escape(&text)
        );
    }
    for i in start..end {
        for c in cell_at(row, i).ch.chars() {
            nerd_font.write_use(
                out,
                c,
                (
                    MARGIN_X + i as f32 * cell_w,
                    header_h + CONTENT_PADDING_TOP + y as f32 * cell_h,
                ),
                (cell_w, cell_h),
                run_x_adjust,
                &fg,
            );
        }
    }
}

/// Draw the window title centred in the title bar.
///
/// The title is chrome rather than grid content, so unlike a cell run it is
/// not forced to a `textLength`: stretching a proportional string to a
/// computed width would distort it. It is instead truncated to what fits, and
/// kept clear of the traffic lights by reserving the same margin on both
/// sides, which also keeps it centred on the space that remains.
pub(crate) fn title_advance(style: &Style) -> f32 {
    style.title_font_size * (style.cell_width() / style.font_size)
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
    style: &Style,
) -> Option<String> {
    const GAP: f32 = 8.0;
    let available = width - 2.0 * (DOTS_RIGHT + GAP);
    let fits = (available / title_advance(style)).floor().max(0.0) as usize;
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
    style: &Style,
) {
    let Some(shown) = visible_title(title, cols, rows, width, style) else {
        return;
    };
    let header_h = style.header_height();
    let title_font_size = style.title_font_size;
    let title_fg = style.window.foreground;
    let _ = write!(
        out,
        r#"<text x="{cx:.2}" y="{baseline:.2}" fill="{fill}" font-size="{title_font_size}px" font-weight="bold" text-anchor="middle" xml:space="preserve">{esc}</text>"#,
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
    style: &Style,
) {
    let Some(row) = rows.get(cy) else {
        return;
    };
    let cell_w = style.cell_width();
    let cell_h = style.cell_height();
    let header_h = style.header_height();
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
    let x = MARGIN_X + cx as f32 * cell_w;
    let y = header_h + CONTENT_PADDING_TOP + cy as f32 * cell_h;
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
    write_text_run(
        out,
        row,
        cx as usize..cx as usize + span as usize,
        cy,
        paint,
        nerd_font,
        style,
    );
}

/// Render the grid. `cursor` is where to draw the cursor *within `rows`*, so a
/// caller passing scrollback has already offset it, and `None` means the
/// terminal is not showing one. `title` is the window title a program set,
/// drawn in the title bar, and `None` leaves the bar bare.
///
/// Its row is a `usize` because it indexes `rows`, which for a full-history
/// render is as long as the scrollback and so is not bounded by the screen.
pub(crate) fn render_svg(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn RenderColors,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
    style: &Style,
    zoom: f64,
) -> String {
    let cell_w = style.cell_width();
    let cell_h = style.cell_height();
    let font_size = style.font_size;
    let header_h = style.header_height();
    let divider_h = style.divider_height();
    let radius = style.border.radius;
    let canvas_padding = style.canvas_padding;
    let canvas_background = style.canvas_background;
    let shadow_color = style.shadow.color;
    let title_bg = style.window.background;
    let title_divider = style.window.divider;
    let font_family = escape_attribute(&style.font.family);
    let nerd_font = NerdFont::new(rows, font_size);
    let cols = cols as usize;
    let x0 = MARGIN_X;
    let y0 = header_h + CONTENT_PADDING_TOP;
    let panel_width = MARGIN_X * 2.0 + cols as f32 * cell_w;
    let panel_height =
        header_h + CONTENT_PADDING_TOP + MARGIN_BOTTOM + rows.len().max(1) as f32 * cell_h;
    let padding = canvas_padding as f32;
    let width = panel_width + padding * 2.0;
    let height = panel_height + padding * 2.0;
    let output_width = svg_dimension(f64::from(width) * zoom);
    let output_height = svg_dimension(f64::from(height) * zoom);

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{output_width}" height="{output_height}" viewBox="0 0 {width:.0} {height:.0}" font-family="{font_family}" font-size="{font_size}px">"#
    );
    nerd_font.write_defs(&mut out);
    let _ = write!(
        out,
        r#"<rect width="{width:.0}" height="{height:.0}" fill="{}"/>"#,
        hex(canvas_background)
    );
    for (spread, offset_y, alpha) in style.shadow_layers() {
        let shadow_x = padding - spread;
        let shadow_y = padding - spread + offset_y;
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
    let _ = write!(
        out,
        r#"<g transform="translate({padding:.0} {padding:.0})"><rect width="{panel_width:.0}" height="{panel_height:.0}" rx="{radius:.0}" fill="{}"/>"#,
        hex(colors.resolve(None, false))
    );
    // The whole title bar is one decision. Each piece used to be drawn
    // unconditionally, so turning the bar off left a strip of title color
    // across the grid's top corners, the close button's dot at 0,0, and the
    // title written over the first row of content.
    if style.window.title_bar {
        let title_bottom = header_h - divider_h;
        let right_curve = panel_width - radius;
        let _ = write!(
            out,
            r#"<path d="M0 {radius:.1} Q0 0 {radius:.1} 0 H{right_curve:.1} Q{panel_width:.1} 0 {panel_width:.1} {radius:.1} V{title_bottom:.1} H0 Z" fill="{}"/>"#,
            hex(title_bg)
        );
        let _ = write!(
            out,
            r#"<rect y="{title_bottom:.1}" width="{panel_width:.0}" height="{divider_h:.1}" fill="{}"/>"#,
            hex(title_divider)
        );
        let lights = style.window.traffic_lights();
        for (i, dot) in lights.iter().copied().enumerate() {
            let cx = MARGIN_X + 5.0 + i as f32 * 20.0;
            let _ = write!(
                out,
                r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{DOT_R:.1}" fill="{}"/>"#,
                hex(dot),
                cy = header_h / 2.0,
            );
        }
        // The darker centre of the close button belongs with the lights, not
        // beside them, or it survives them being turned off.
        if !lights.is_empty() {
            let _ = write!(
                out,
                r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{RED_DOT_R:.1}" fill="{}"/>"#,
                hex(RED_DOT_COLOR),
                cx = MARGIN_X + 5.0,
                cy = header_h / 2.0,
            );
        }
        write_title(
            &mut out,
            title,
            cols as u16,
            rows.len().max(1),
            panel_width,
            style,
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
            write_text_run(&mut out, row, x..x + run, y, paint, &nerd_font, style);
            x += run;
        }
    }

    if let Some(at) = cursor {
        write_cursor(&mut out, rows, at, colors, &nerd_font, style);
    }

    // Last, so the content it frames cannot paint over it. The rect is inset
    // by half the stroke because SVG centers a stroke on its path, and a
    // border that straddled the panel edge would bleed into the padding.
    if style.border.width > 0.0 {
        let inset = style.border.width / 2.0;
        let _ = write!(
            out,
            r#"<rect x="{inset:.2}" y="{inset:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            (panel_width - style.border.width).max(0.0),
            (panel_height - style.border.width).max(0.0),
            (radius - inset).max(0.0),
            hex(style.border.color),
            style.border.width,
        );
    }

    out.push_str("</g></svg>");
    out
}

#[cfg(feature = "recording-raster")]
pub(crate) fn pixel_size(cols: u16, rows: usize, style: &Style) -> (u32, u32) {
    let cell_w = style.cell_width();
    let cell_h = style.cell_height();
    let header_h = style.header_height();
    let width = (MARGIN_X * 2.0 + f32::from(cols) * cell_w).ceil() as u32;
    let height = (header_h + CONTENT_PADDING_TOP + MARGIN_BOTTOM + rows.max(1) as f32 * cell_h)
        .ceil() as u32;
    (width + width % 2, height + height % 2)
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
        let block = render_svg(&rows, 1, &emu, Some((0, 0)), None, &Style::default(), 1.0);
        assert!(
            block.contains(&format!(
                r#"width="10.00" height="21.00" fill="{cursor_fill}""#
            )),
            "a block covers the whole cell: {block}"
        );

        emu.process(b"\x1b[4 q");
        let underline = render_svg(&rows, 1, &emu, Some((0, 0)), None, &Style::default(), 1.0);
        assert!(
            underline.contains(&format!(
                r#"width="10.00" height="2.00" fill="{cursor_fill}""#
            )),
            "an underline is a thin full-width bar: {underline}"
        );

        emu.process(b"\x1b[6 q");
        let bar = render_svg(&rows, 1, &emu, Some((0, 0)), None, &Style::default(), 1.0);
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
        );
        assert!(
            !svg.contains('X'),
            "the cursor must not redraw text hidden with SGR 8: {svg}"
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
        );
        let expected = header_h() + CONTENT_PADDING_TOP + row as f32 * cell_h();
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
            render_svg(&rows, 1, &emu, Some((0, 0)), None, &Style::default(), 1.0,)
                .contains("#ff00ff"),
            "the cursor is drawn when there is one to draw"
        );
        assert!(
            !render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0,).contains("#ff00ff"),
            "a terminal not showing a cursor gets none"
        );
        // A row past the end of the grid: reachable if a caller miscounts the
        // scrollback offset, and not worth a panic.
        assert!(
            !render_svg(&rows, 1, &emu, Some((0, 9)), None, &Style::default(), 1.0,)
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
        assert!(
            render_svg(&rows, 1, &emu, Some((0, 0)), None, &Style::default(), 1.0,)
                .contains("#ff00ff")
        );
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

        let before = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0);
        assert!(before.contains(&hex(Profile::default().colors.red)));
        assert!(before.contains(&hex(Profile::default().colors.background)));

        // The program picks its own background and recolors palette slot 1.
        emu.process(b"\x1b]11;#3b0764\x07\x1b]4;1;#22c55e\x07");

        let after = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0);
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

        let after = render_svg(&rows, 1, &emu, None, None, &Style::default(), 1.0);
        assert!(after.contains(&hex(Profile::default().colors.background)));
        assert!(after.contains(&hex(Profile::default().colors.red)));
    }

    #[test]
    fn emits_valid_svg_with_text_and_color() {
        let rows = vec![vec![
            cell("h", Some(Color::from_index(1)), None),
            cell("i", Some(Color::from_index(1)), None),
        ]];
        let svg = render_svg(&rows, 2, &colors(), None, None, &Style::default(), 1.0);
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
        let svg = render_svg(&rows, 1, &colors(), None, None, &Style::default(), 0.5);
        assert!(svg.contains(r#"width="44" height="60.5" viewBox="0 0 88 121""#));
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
        );
        assert!(svg.contains("<circle"));
        assert!(svg.contains(r##"<rect width="88" height="121" fill="#6867aa"/>"##));
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
        );
        let expected_baseline = header_h() + CONTENT_PADDING_TOP + font_baseline();
        assert!(svg.contains(&format!(r#"y="{expected_baseline:.2}""#)));
    }

    #[test]
    fn escapes_markup_characters() {
        let rows = vec![vec![cell("<", None, None)]];
        let svg = render_svg(&rows, 1, &colors(), None, None, &Style::default(), 1.0);
        assert!(svg.contains("&lt;"));
        assert!(!svg.contains("><</text>"));
    }

    #[test]
    fn background_run_emitted_for_non_default_bg() {
        let rows = vec![vec![cell(" ", None, Some(Color::from_index(4)))]];
        let svg = render_svg(&rows, 1, &colors(), None, None, &Style::default(), 1.0);
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
        let svg = render_svg(&rows, 3, &colors(), None, None, &Style::default(), 1.0);

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
        let bare = render_svg(&rows, 40, &colors(), None, None, &Style::default(), 1.0);
        let titled = render_svg(
            &rows,
            40,
            &colors(),
            None,
            Some("vim: notes.md"),
            &Style::default(),
            1.0,
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

    /// A title too long for the bar is truncated rather than drawn over the
    /// window controls or past the panel edge.
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
        let panel = MARGIN_X * 2.0 + cols * cell_w();
        let drawn_width =
            crate::terminal::cell::display_width(drawn) as f32 * title_advance(&geometry());
        assert!(
            drawn_width <= panel - 2.0 * DOTS_RIGHT,
            "title {drawn:?} is {drawn_width} wide, past the {} available",
            panel - 2.0 * DOTS_RIGHT
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
            |style: &Style| render_svg(&rows, 40, &colors(), None, Some("title"), style, 1.0);
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
            canvas_padding: 40,
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
        let svg = render_svg(&rows, 4, &colors(), None, Some("t"), &style, 1.0);

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
            |style: &Style| render_svg(&rows, 40, &colors(), None, Some("title"), style, 1.0);

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
        let svg = render_svg(&rows, 2, &colors(), None, Some("t"), &style, 1.0);

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
    /// nothing else in the suite compares whole output. Regenerate the file
    /// deliberately if the default look is meant to change.
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
        );
        assert_eq!(rendered, include_str!("testdata/default-style.svg"));
    }
}
