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
use crate::profile::{ColorSlot, Rgb};
use crate::terminal::cell::{truncate_to_columns, Attrs, EmuCell, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

const CELL_W: f32 = 10.0;
const CELL_H: f32 = 21.0;
const FONT_SIZE: f32 = 17.0;
const FONT_BASELINE: f32 = (CELL_H - FONT_SIZE) / 2.0 + FONT_SIZE * 0.78;
const MARGIN_X: f32 = 15.0;
const HEADER_H: f32 = 38.0;
const MARGIN_BOTTOM: f32 = 14.0;
const DOT_R: f32 = 7.0;
/// Title bar text, smaller than the grid font so the chrome does not compete
/// with the terminal content itself.
const TITLE_FONT_SIZE: f32 = 13.0;
/// Where the rightmost traffic light ends. A centred title is kept clear of
/// this on both sides, so it can never be drawn over the controls.
const DOTS_RIGHT: f32 = MARGIN_X + 5.0 + 2.0 * 20.0 + DOT_R;
const FONT_STACK: &str =
    "'Cascadia Code','JetBrains Mono','Fira Code',Menlo,Consolas,'DejaVu Sans Mono',monospace";

fn hex(c: Rgb) -> String {
    c.to_hex()
}

fn dim(c: Rgb) -> Rgb {
    let s = |v: u8| (v as f32 * 0.6) as u8;
    Rgb::new(s(c.r), s(c.g), s(c.b))
}

static BLANK: EmuCell = EmuCell::blank();

fn cell_at(row: &[EmuCell], x: usize) -> &EmuCell {
    row.get(x).unwrap_or(&BLANK)
}

/// Resolved background color for a cell (honoring inverse).
fn bg_of(cell: &EmuCell, colors: &dyn Emulator) -> Rgb {
    let bg = colors.resolve(cell.bg, false);
    let fg = colors.resolve(cell.fg, true);
    if cell.has(Attrs::INVERSE) {
        fg
    } else {
        bg
    }
}

#[derive(PartialEq)]
struct Style {
    fg: Rgb,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    invisible: bool,
}

fn style_of(cell: &EmuCell, colors: &dyn Emulator) -> Style {
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

/// Draw the window title centred in the title bar.
///
/// The title is chrome rather than grid content, so unlike a cell run it is
/// not forced to a `textLength`: stretching a proportional string to a
/// computed width would distort it. It is instead truncated to what fits, and
/// kept clear of the traffic lights by reserving the same margin on both
/// sides, which also keeps it centred on the space that remains.
fn write_title(out: &mut String, title: &str, width: f32, colors: &dyn Emulator) {
    const GAP: f32 = 8.0;
    let available = width - 2.0 * (DOTS_RIGHT + GAP);
    // A monospace advance, scaled from the grid font's known cell width.
    let advance = TITLE_FONT_SIZE * (CELL_W / FONT_SIZE);
    let fits = (available / advance).floor().max(0.0) as usize;
    if fits == 0 {
        return;
    }

    // Budgeted in columns, not characters: the title bar inherits the
    // monospace stack, so a CJK glyph takes two advances and a title sized by
    // character count would be twice as wide as measured and, being centred,
    // would spill over the window controls at both ends.
    let shown = truncate_to_columns(title, fits);
    let _ = write!(
        out,
        r#"<text x="{cx:.2}" y="{baseline:.2}" fill="{fill}" font-size="{TITLE_FONT_SIZE}px" text-anchor="middle" xml:space="preserve">{esc}</text>"#,
        cx = width / 2.0,
        baseline = HEADER_H / 2.0 + TITLE_FONT_SIZE * 0.35,
        // The dim grey of the palette, so the title reads as chrome next to
        // the terminal's own foreground.
        fill = hex(colors.color(ColorSlot::Indexed(8))),
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
    colors: &dyn Emulator,
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
    let y = HEADER_H + cy as f32 * CELL_H;
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

    if colors.cursor_shape() != CursorShape::Block || cell.ch.trim().is_empty() {
        return;
    }
    // Redraw exactly as the text pass would, so a vector glyph comes back as a
    // glyph rather than as a character the text font may not even have.
    let under = hex(bg_of(cell, colors));
    let (text, run_x_adjust) = nerd_font.prepare_run(&cell.ch, w, CELL_W);
    if !text.trim().is_empty() {
        let _ = write!(
            out,
            r#"<text x="{x:.2}" y="{baseline:.2}" fill="{under}" textLength="{w:.2}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc}</text>"#,
            baseline = y + FONT_BASELINE,
            esc = escape(&text),
        );
    }
    for c in cell.ch.chars() {
        nerd_font.write_use(out, c, (x, y), (CELL_W, CELL_H), run_x_adjust, &under);
    }
}

/// Render the grid. `cursor` is where to draw the cursor *within `rows`*, so a
/// caller passing scrollback has already offset it, and `None` means the
/// terminal is not showing one. `title` is the window title a program set,
/// drawn in the title bar, and `None` leaves the bar bare.
///
/// Its row is a `usize` because it indexes `rows`, which for a full-history
/// render is as long as the scrollback and so is not bounded by the screen.
pub fn render_svg(
    rows: &[Vec<EmuCell>],
    cols: u16,
    colors: &dyn Emulator,
    cursor: Option<(u16, usize)>,
    title: Option<&str>,
) -> String {
    let nerd_font = NerdFont::new(rows, FONT_SIZE);
    let cols = cols as usize;
    let x0 = MARGIN_X;
    let y0 = HEADER_H;
    let width = MARGIN_X * 2.0 + cols as f32 * CELL_W;
    let height = HEADER_H + MARGIN_BOTTOM + rows.len().max(1) as f32 * CELL_H;

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}" font-family="{FONT_STACK}" font-size="{FONT_SIZE}px">"#
    );
    nerd_font.write_defs(&mut out);
    let _ = write!(
        out,
        r#"<rect width="{width:.0}" height="{height:.0}" rx="8" fill="{}"/>"#,
        hex(colors.resolve(None, false))
    );
    for (i, dot) in ["#ff5f56", "#ffbd2e", "#27c93f"].iter().enumerate() {
        let cx = MARGIN_X + 5.0 + i as f32 * 20.0;
        let _ = write!(
            out,
            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{DOT_R:.1}" fill="{dot}"/>"#,
            cy = HEADER_H / 2.0,
        );
    }
    if let Some(title) = title {
        write_title(&mut out, title, width, colors);
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
        let baseline = y0 + y as f32 * CELL_H + FONT_BASELINE;
        let mut x = 0;
        while x < cols {
            let style = style_of(cell_at(row, x), colors);
            let mut run = 1;
            while x + run < cols && style_of(cell_at(row, x + run), colors) == style {
                run += 1;
            }
            if !style.invisible {
                let fg = hex(style.fg);
                let tx = x0 + x as f32 * CELL_W;
                let tl = run as f32 * CELL_W;
                let original_text = run_text(row, x, x + run);
                let (text, run_x_adjust) = nerd_font.prepare_run(&original_text, tl, CELL_W);
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
                        r#"<text x="{tx:.2}" y="{baseline:.2}" fill="{fg}"{weight}{italic}{deco} textLength="{tl:.2}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{esc}</text>"#,
                        esc = escape(&text)
                    );
                }
                for i in x..x + run {
                    for c in cell_at(row, i).ch.chars() {
                        nerd_font.write_use(
                            &mut out,
                            c,
                            (x0 + i as f32 * CELL_W, y0 + y as f32 * CELL_H),
                            (CELL_W, CELL_H),
                            run_x_adjust,
                            &fg,
                        );
                    }
                }
            }
            x += run;
        }
    }

    if let Some(at) = cursor {
        write_cursor(&mut out, rows, at, colors, &nerd_font);
    }

    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ColorSlot, Profile};
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::Color;

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
            .split("<use href=")
            .skip(1)
            .filter(|glyph| glyph.contains(&format!(r#"fill="{background}""#)))
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
        let expected = HEADER_H + row as f32 * CELL_H;
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
            svg.contains(&hex(colors().color(ColorSlot::Indexed(1)))),
            "slot 1 is painted with the profile color"
        );
        assert!(svg.contains(">hi</text>"));
        assert!(!svg.contains("<defs>"));
        assert!(!svg.contains("<use"));
    }

    #[test]
    fn emits_window_chrome() {
        let svg = render_svg(&[vec![cell(" ", None, None)]], 1, &colors(), None, None);
        assert!(svg.contains("<circle"));
        assert!(svg.contains("#ff5f56"));
        assert!(svg.contains("#ffbd2e"));
        assert!(svg.contains("#27c93f"));
    }

    #[test]
    fn centers_the_text_font_box_in_each_cell() {
        let svg = render_svg(&[vec![cell("x", None, None)]], 1, &colors(), None, None);
        let expected_baseline = HEADER_H + FONT_BASELINE;
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
            svg.contains(&hex(colors().color(ColorSlot::Indexed(4)))),
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

    /// A window title is drawn centred in the title bar, and no title leaves
    /// the bar exactly as it was, so every screenshot taken without one is
    /// unchanged by this feature.
    #[test]
    fn draws_the_window_title_centred_in_the_bar() {
        let rows = vec![vec![cell("x", None, None); 40]];
        let bare = render_svg(&rows, 40, &colors(), None, None);
        let titled = render_svg(&rows, 40, &colors(), None, Some("vim: notes.md"));

        assert!(
            !bare.contains("text-anchor=\"middle\""),
            "no title means nothing extra is drawn: {bare}"
        );
        assert!(
            titled.contains(">vim: notes.md</text>") && titled.contains("text-anchor=\"middle\""),
            "the title is drawn, centred: {titled}"
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
        assert!(drawn.ends_with('…'), "truncation is marked: {drawn}");
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
        let advance = TITLE_FONT_SIZE * (CELL_W / FONT_SIZE);
        let drawn_width = crate::terminal::cell::display_width(drawn) as f32 * advance;
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
