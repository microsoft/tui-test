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
use crate::terminal::cell::{truncate_to_columns, Attrs, Color, EmuCell, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

pub(crate) const CELL_W: f32 = 10.0;
pub(crate) const CELL_H: f32 = 21.0;
pub(crate) const FONT_SIZE: f32 = 17.0;
pub(crate) const FONT_BASELINE: f32 = (CELL_H - FONT_SIZE) / 2.0 + FONT_SIZE * 0.78;
pub(crate) const MARGIN_X: f32 = 15.0;
pub(crate) const HEADER_H: f32 = 34.0;
pub(crate) const CONTENT_PADDING_TOP: f32 = 4.0;
const MARGIN_BOTTOM: f32 = 14.0;
pub(crate) const DOT_R: f32 = 7.0;
pub(crate) const RED_DOT_R: f32 = 2.5;
pub(crate) const RED_DOT_COLOR: Rgb = Rgb::new(105, 17, 10);
pub(crate) const WINDOW_RADIUS: f32 = 8.0;
pub(crate) const TITLE_DIVIDER_H: f32 = 1.0;
pub(crate) const TITLE_BG: Rgb = Rgb::new(217, 217, 232);
pub(crate) const TITLE_DIVIDER: Rgb = Rgb::new(0, 0, 0);
pub(crate) const CANVAS_PADDING: u32 = 24;
pub(crate) const CANVAS_BACKGROUND: Rgb = Rgb::new(104, 103, 170);
pub(crate) const SHADOW_COLOR: Rgb = Rgb::new(8, 8, 18);
pub(crate) const SHADOW_LAYERS: [(f32, f32, u8); 4] = [
    (7.0, 5.0, 18),
    (5.0, 4.0, 20),
    (3.0, 3.0, 22),
    (1.0, 2.0, 24),
];
pub(crate) const TRAFFIC_LIGHTS: [Rgb; 3] = [
    Rgb::new(236, 106, 94),
    Rgb::new(244, 191, 79),
    Rgb::new(97, 197, 84),
];
/// Title bar text, smaller than the grid font so the chrome does not compete
/// with the terminal content itself.
pub(crate) const TITLE_FONT_SIZE: f32 = 13.0;
pub(crate) const TITLE_FG: Rgb = Rgb::new(65, 65, 69);
/// Where the rightmost traffic light ends. A centred title is kept clear of
/// this on both sides, so it can never be drawn over the controls.
const DOTS_RIGHT: f32 = MARGIN_X + 5.0 + 2.0 * 20.0 + DOT_R;
const FONT_STACK: &str =
    "'Cascadia Code','JetBrains Mono','Fira Code',Menlo,Consolas,'DejaVu Sans Mono',monospace";

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

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Style {
    pub fg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub invisible: bool,
}

pub(crate) fn style_of(cell: &EmuCell, colors: &dyn RenderColors) -> Style {
    let mut fg = colors.resolve(cell.fg, true);
    let bg = colors.resolve(cell.bg, false);
    if cell.has(Attrs::INVERSE) {
        fg = bg;
    }
    if cell.has(Attrs::DIM) {
        fg = dim(fg);
    }
    Style {
        fg,
        bold: cell.has(Attrs::BOLD),
        italic: cell.has(Attrs::ITALIC),
        underline: cell.underline.is_underlined(),
        strike: cell.has(Attrs::STRIKE),
        invisible: cell.has(Attrs::INVISIBLE),
    }
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
    start: usize,
    end: usize,
    y: usize,
    style: Style,
    nerd_font: &NerdFont,
) {
    if style.invisible {
        return;
    }
    let fg = hex(style.fg);
    let tx = MARGIN_X + start as f32 * CELL_W;
    let baseline = HEADER_H + CONTENT_PADDING_TOP + y as f32 * CELL_H + FONT_BASELINE;
    let width = (end - start) as f32 * CELL_W;
    let original_text = run_text(row, start, end);
    let (text, run_x_adjust) = nerd_font.prepare_run(&original_text, width, CELL_W);

    // Preserve decoration for runs containing only vector glyphs.
    if !original_text.trim().is_empty() {
        let weight = if style.bold {
            r#" font-weight="bold""#
        } else {
            ""
        };
        let italic = if style.italic {
            r#" font-style="italic""#
        } else {
            ""
        };
        let deco = match (style.underline, style.strike) {
            (true, true) => r#" text-decoration="underline line-through""#,
            (true, false) => r#" text-decoration="underline""#,
            (false, true) => r#" text-decoration="line-through""#,
            (false, false) => "",
        };
        let _ = write!(
            out,
            r#"<text x="{tx:.2}" y="{baseline:.2}" fill="{fg}"{weight}{italic}{deco} textLength="{width:.2}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc}</text>"#,
            esc = escape(&text)
        );
    }
    for i in start..end {
        for c in cell_at(row, i).ch.chars() {
            nerd_font.write_use(
                out,
                c,
                (
                    MARGIN_X + i as f32 * CELL_W,
                    HEADER_H + CONTENT_PADDING_TOP + y as f32 * CELL_H,
                ),
                (CELL_W, CELL_H),
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
pub(crate) fn title_advance() -> f32 {
    TITLE_FONT_SIZE * (CELL_W / FONT_SIZE)
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
) -> Option<String> {
    const GAP: f32 = 8.0;
    let available = width - 2.0 * (DOTS_RIGHT + GAP);
    let fits = (available / title_advance()).floor().max(0.0) as usize;
    if fits == 0 {
        return None;
    }
    Some(media_title(title, cols, rows, fits))
}

fn write_title(out: &mut String, title: Option<&str>, cols: u16, rows: usize, width: f32) {
    let Some(shown) = visible_title(title, cols, rows, width) else {
        return;
    };
    let _ = write!(
        out,
        r#"<text x="{cx:.2}" y="{baseline:.2}" fill="{fill}" font-size="{TITLE_FONT_SIZE}px" font-weight="bold" text-anchor="middle" xml:space="preserve">{esc}</text>"#,
        cx = width / 2.0,
        baseline = HEADER_H / 2.0 + TITLE_FONT_SIZE * 0.35,
        fill = hex(TITLE_FG),
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
) {
    let Some(row) = rows.get(cy) else {
        return;
    };
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
    let w = span * CELL_W;
    let x = MARGIN_X + cx as f32 * CELL_W;
    let y = HEADER_H + CONTENT_PADDING_TOP + cy as f32 * CELL_H;
    let fill = hex(colors.color(ColorSlot::Cursor));

    let (rx, ry, rw, rh) = match colors.cursor_shape() {
        CursorShape::Block => (x, y, w, CELL_H),
        CursorShape::Underline => (x, y + CELL_H - CURSOR_THICKNESS, w, CURSOR_THICKNESS),
        CursorShape::Bar => (x, y, CURSOR_THICKNESS, CELL_H),
    };
    let _ = write!(
        out,
        r#"<rect x="{rx:.2}" y="{ry:.2}" width="{rw:.2}" height="{rh:.2}" fill="{fill}"/>"#
    );

    if colors.cursor_shape() != CursorShape::Block {
        return;
    }
    let mut style = style_of(cell, colors);
    style.fg = bg_of(cell, colors);
    write_text_run(
        out,
        row,
        cx as usize,
        cx as usize + span as usize,
        cy,
        style,
        nerd_font,
    );
}

/// Render the grid. `cursor` is where to draw the cursor *within `rows`*, so a
/// caller passing scrollback has already offset it, and `None` means the
/// terminal is not showing one. `title` is the window title a program set,
/// drawn in the title bar, and `None` leaves the bar bare.
///
/// Its row is a `usize` because it indexes `rows`, which for a full-history
/// render is as long as the scrollback and so is not bounded by the screen.
#[cfg(test)]
pub(crate) fn render_svg(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn RenderColors,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
) -> String {
    render_svg_with_zoom(rows, cols, colors, cursor, title, 1.0, None)
}

pub(crate) fn render_svg_with_zoom(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn RenderColors,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
    zoom: f64,
    background: Option<CaptureBackground>,
) -> String {
    let nerd_font = NerdFont::new(rows, FONT_SIZE);
    let cols = cols as usize;
    let x0 = MARGIN_X;
    let y0 = HEADER_H + CONTENT_PADDING_TOP;
    let panel_width = MARGIN_X * 2.0 + cols as f32 * CELL_W;
    let panel_height =
        HEADER_H + CONTENT_PADDING_TOP + MARGIN_BOTTOM + rows.len().max(1) as f32 * CELL_H;
    let padding = CANVAS_PADDING as f32;
    let width = panel_width + padding * 2.0;
    let height = panel_height + padding * 2.0;
    let output_width = svg_dimension(f64::from(width) * zoom);
    let output_height = svg_dimension(f64::from(height) * zoom);

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{output_width}" height="{output_height}" viewBox="0 0 {width:.0} {height:.0}" font-family="{FONT_STACK}" font-size="{FONT_SIZE}px">"#
    );
    nerd_font.write_defs(&mut out);
    match background {
        Some(CaptureBackground::Transparent) => {}
        Some(CaptureBackground::Color(color)) => {
            let _ = write!(
                out,
                r#"<rect width="{width:.0}" height="{height:.0}" fill="{}"/>"#,
                hex(color)
            );
        }
        None => {
            let _ = write!(
                out,
                r#"<rect width="{width:.0}" height="{height:.0}" fill="{}"/>"#,
                hex(CANVAS_BACKGROUND)
            );
        }
    }
    for (spread, offset_y, alpha) in SHADOW_LAYERS {
        let shadow_x = padding - spread;
        let shadow_y = padding - spread + offset_y;
        let shadow_width = panel_width + spread * 2.0;
        let shadow_height = panel_height + spread * 2.0;
        let shadow_radius = WINDOW_RADIUS + spread;
        let opacity = f32::from(alpha) / 255.0;
        let _ = write!(
            out,
            r#"<rect x="{shadow_x:.1}" y="{shadow_y:.1}" width="{shadow_width:.1}" height="{shadow_height:.1}" rx="{shadow_radius:.1}" fill="{}" fill-opacity="{opacity:.6}"/>"#,
            hex(SHADOW_COLOR)
        );
    }
    let _ = write!(
        out,
        r#"<g transform="translate({padding:.0} {padding:.0})"><rect width="{panel_width:.0}" height="{panel_height:.0}" rx="{WINDOW_RADIUS:.0}" fill="{}"/>"#,
        hex(colors.resolve(None, false))
    );
    let title_bottom = HEADER_H - TITLE_DIVIDER_H;
    let right_curve = panel_width - WINDOW_RADIUS;
    let _ = write!(
        out,
        r#"<path d="M0 {WINDOW_RADIUS:.1} Q0 0 {WINDOW_RADIUS:.1} 0 H{right_curve:.1} Q{panel_width:.1} 0 {panel_width:.1} {WINDOW_RADIUS:.1} V{title_bottom:.1} H0 Z" fill="{}"/>"#,
        hex(TITLE_BG)
    );
    let _ = write!(
        out,
        r#"<rect y="{title_bottom:.1}" width="{panel_width:.0}" height="{TITLE_DIVIDER_H:.1}" fill="{}"/>"#,
        hex(TITLE_DIVIDER)
    );
    for (i, dot) in TRAFFIC_LIGHTS.iter().copied().enumerate() {
        let cx = MARGIN_X + 5.0 + i as f32 * 20.0;
        let _ = write!(
            out,
            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{DOT_R:.1}" fill="{}"/>"#,
            hex(dot),
            cy = HEADER_H / 2.0,
        );
    }
    let _ = write!(
        out,
        r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{RED_DOT_R:.1}" fill="{}"/>"#,
        hex(RED_DOT_COLOR),
        cx = MARGIN_X + 5.0,
        cy = HEADER_H / 2.0,
    );
    write_title(&mut out, title, cols as u16, rows.len().max(1), panel_width);

    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        while x < cols {
            let bg = bg_of(cell_at(row, x), colors);
            let mut run = 1;
            while x + run < cols && bg_of(cell_at(row, x + run), colors) == bg {
                run += 1;
            }
            if bg != colors.resolve(None, false) {
                let rx = x0 + x as f32 * CELL_W;
                let ry = y0 + y as f32 * CELL_H;
                let rw = run as f32 * CELL_W;
                let _ = write!(
                    out,
                    r#"<rect x="{rx:.2}" y="{ry:.2}" width="{rw:.2}" height="{CELL_H:.2}" fill="{}"/>"#,
                    hex(bg)
                );
            }
            x += run;
        }
    }

    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        while x < cols {
            let style = style_of(cell_at(row, x), colors);
            let mut run = 1;
            while x + run < cols && style_of(cell_at(row, x + run), colors) == style {
                run += 1;
            }
            write_text_run(&mut out, row, x, x + run, y, style, &nerd_font);
            x += run;
        }
    }

    if let Some(at) = cursor {
        write_cursor(&mut out, rows, at, colors, &nerd_font);
    }

    out.push_str("</g></svg>");
    out
}

#[cfg(feature = "recording-raster")]
pub(crate) fn pixel_size(cols: u16, rows: usize) -> (u32, u32) {
    let width = (MARGIN_X * 2.0 + f32::from(cols) * CELL_W).ceil() as u32;
    let height = (HEADER_H + CONTENT_PADDING_TOP + MARGIN_BOTTOM + rows.max(1) as f32 * CELL_H)
        .ceil() as u32;
    (width + width % 2, height + height % 2)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let block = render_svg(&rows, 1, &emu, Some((0, 0)), None);
        assert!(
            block.contains(&format!(
                r#"width="10.00" height="21.00" fill="{cursor_fill}""#
            )),
            "a block covers the whole cell: {block}"
        );

        emu.process(b"\x1b[4 q");
        let underline = render_svg(&rows, 1, &emu, Some((0, 0)), None);
        assert!(
            underline.contains(&format!(
                r#"width="10.00" height="2.00" fill="{cursor_fill}""#
            )),
            "an underline is a thin full-width bar: {underline}"
        );

        emu.process(b"\x1b[6 q");
        let bar = render_svg(&rows, 1, &emu, Some((0, 0)), None);
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
        let svg = render_svg(&rows, 1, &colors(), Some((0, 0)), None);
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
        let svg = render_svg(&[vec![hidden]], 1, &colors(), Some((0, 0)), None);
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
        let svg = render_svg(&[vec![styled]], 1, &colors(), Some((0, 0)), None);

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
        let svg = render_svg(&rows, 3, &colors(), Some((0, 0)), None);
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
        let svg = render_svg(&rows, 1, &colors(), Some((0, 0)), None);
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
        let svg = render_svg(&rows, 1, &colors(), Some((0, row)), None);
        let expected = HEADER_H + CONTENT_PADDING_TOP + row as f32 * CELL_H;
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
            render_svg(&rows, 1, &emu, Some((0, 0)), None).contains("#ff00ff"),
            "the cursor is drawn when there is one to draw"
        );
        assert!(
            !render_svg(&rows, 1, &emu, None, None).contains("#ff00ff"),
            "a terminal not showing a cursor gets none"
        );
        // A row past the end of the grid: reachable if a caller miscounts the
        // scrollback offset, and not worth a panic.
        assert!(
            !render_svg(&rows, 1, &emu, Some((0, 9)), None).contains("#ff00ff"),
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
        assert!(render_svg(&rows, 1, &emu, Some((0, 0)), None).contains("#ff00ff"));
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

        let before = render_svg(&rows, 1, &emu, None, None);
        assert!(before.contains(&hex(Profile::default().colors.red)));
        assert!(before.contains(&hex(Profile::default().colors.background)));

        // The program picks its own background and recolors palette slot 1.
        emu.process(b"\x1b]11;#3b0764\x07\x1b]4;1;#22c55e\x07");

        let after = render_svg(&rows, 1, &emu, None, None);
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

        let after = render_svg(&rows, 1, &emu, None, None);
        assert!(after.contains(&hex(Profile::default().colors.background)));
        assert!(after.contains(&hex(Profile::default().colors.red)));
    }

    #[test]
    fn emits_valid_svg_with_text_and_color() {
        let rows = vec![vec![
            cell("h", Some(Color::from_index(1)), None),
            cell("i", Some(Color::from_index(1)), None),
        ]];
        let svg = render_svg(&rows, 2, &colors(), None, None);
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
        let svg = render_svg_with_zoom(&rows, 1, &colors(), None, None, 0.5, None);
        assert!(svg.contains(r#"width="44" height="60.5" viewBox="0 0 88 121""#));
    }

    #[test]
    fn canvas_background_can_be_custom_or_transparent() {
        let rows = vec![vec![cell("x", None, None)]];
        let custom = render_svg_with_zoom(
            &rows,
            1,
            &colors(),
            None,
            None,
            1.0,
            Some(CaptureBackground::Color(Rgb::new(1, 2, 3))),
        );
        assert!(custom.contains(r##"<rect width="88" height="121" fill="#010203"/>"##));

        let transparent = render_svg_with_zoom(
            &rows,
            1,
            &colors(),
            None,
            None,
            1.0,
            Some(CaptureBackground::Transparent),
        );
        assert!(!transparent.contains(r##"<rect width="88" height="121" fill="#6867aa"/>"##));
    }

    #[test]
    fn emits_window_chrome() {
        let svg = render_svg(&[vec![cell(" ", None, None)]], 1, &colors(), None, None);
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
        let svg = render_svg(&[vec![cell("x", None, None)]], 1, &colors(), None, None);
        let expected_baseline = HEADER_H + CONTENT_PADDING_TOP + FONT_BASELINE;
        assert!(svg.contains(&format!(r#"y="{expected_baseline:.2}""#)));
    }

    #[test]
    fn escapes_markup_characters() {
        let rows = vec![vec![cell("<", None, None)]];
        let svg = render_svg(&rows, 1, &colors(), None, None);
        assert!(svg.contains("&lt;"));
        assert!(!svg.contains("><</text>"));
    }

    #[test]
    fn background_run_emitted_for_non_default_bg() {
        let rows = vec![vec![cell(" ", None, Some(Color::from_index(4)))]];
        let svg = render_svg(&rows, 1, &colors(), None, None);
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
        let svg = render_svg(&rows, 3, &colors(), None, None);

        assert!(svg.contains(r#"<path id="nf-f115" d=""#));
        assert!(svg.contains(r##"<use href="#nf-f115""##));
        assert!(svg.contains(">a b</text>"));
        assert!(!svg.contains(glyph));
        assert!(svg.contains(&format!(r#"font-family="{FONT_STACK}" font-size="#)));
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
        );

        assert_eq!(svg.matches(r#"<path id="nf-f115""#).count(), 1);
        assert_eq!(svg.matches(r##"<use href="#nf-f115""##).count(), 2);
    }

    #[test]
    fn leaves_unknown_private_use_glyphs_as_text() {
        let glyph = "\u{10fffd}";
        let svg = render_svg(&[vec![cell(glyph, None, None)]], 1, &colors(), None, None);

        assert!(svg.contains(glyph));
        assert!(!svg.contains("<defs>"));
        assert!(!svg.contains(r#"<use href="#));
    }

    /// A window title is drawn centred with the rendered cell dimensions, and
    /// media without a program title gets a useful default.
    #[test]
    fn draws_the_window_title_centred_in_the_bar() {
        let rows = vec![vec![cell("x", None, None); 40]];
        let bare = render_svg(&rows, 40, &colors(), None, None);
        let titled = render_svg(&rows, 40, &colors(), None, Some("vim: notes.md"));

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
        let svg = render_svg(&rows, 20, &colors(), None, Some(long));

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
        let svg = render_svg(&rows, 24, &colors(), None, Some(&"你".repeat(40)));
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
        let panel = MARGIN_X * 2.0 + cols * CELL_W;
        let drawn_width = crate::terminal::cell::display_width(drawn) as f32 * title_advance();
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
        );

        assert!(!svg.contains("<script>"), "no injected element: {svg}");
        assert!(
            svg.contains("&lt;script&gt;"),
            "it is escaped instead: {svg}"
        );
    }
}
