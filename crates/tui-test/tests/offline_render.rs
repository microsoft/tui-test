//! U03 acceptance: public offline rendering from owned snapshots.
//!
//! Live content is produced by the child (`printf` at startup, then `sleep`),
//! so no PTY echo path can mangle escape sequences. Each live assertion is
//! polled to a known marker before rendering, so no assertion depends on
//! timing. Synthetic snapshots cover the offline-only paths with no child,
//! PTY, session, or daemon anywhere in the test.

use std::time::{Duration, Instant};

use tui_test::profile::{Profile, Rgb};
use tui_test::terminal::cell::Attrs;
use tui_test::{
    SnapshotCell, SnapshotColor, SnapshotCursor, SnapshotCursorShape, SnapshotMetadata,
    SnapshotParts, SnapshotRenderOptions, SnapshotScope, SnapshotUnderline, TerminalSnapshot,
};

fn resolved_palette() -> [Rgb; 256] {
    let profile = Profile::default();
    std::array::from_fn(|index| profile.colors.rgb(index as u8))
}

fn cell(text: &str) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        1,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("test cell is valid")
}

fn hidden_cursor() -> SnapshotCursor {
    SnapshotCursor::new(
        0,
        0,
        true,
        false,
        SnapshotCursorShape::Block,
        Rgb::new(9, 9, 9),
        None,
    )
}

fn snapshot_parts(cols: u32, rows: u32, texts: &[&str]) -> SnapshotParts {
    assert_eq!(
        texts.len() as u64,
        u64::from(cols) * u64::from(rows),
        "test grid must fill the snapshot"
    );
    SnapshotParts {
        cols,
        rows,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells: texts.iter().map(|text| cell(text)).collect(),
        palette: resolved_palette(),
        default_foreground: Rgb::new(192, 192, 192),
        default_background: Rgb::new(0, 0, 0),
        default_cursor: Rgb::new(9, 9, 9),
        cursor: hidden_cursor(),
        title: None,
        metadata: SnapshotMetadata::default(),
    }
}

fn snapshot_of(cols: u32, rows: u32, texts: &[&str]) -> TerminalSnapshot {
    TerminalSnapshot::try_new(snapshot_parts(cols, rows, texts)).expect("test snapshot is valid")
}

fn undecorated() -> SnapshotRenderOptions {
    use tui_test::render::style::Padding;
    SnapshotRenderOptions::default()
        .with_canvas_padding(Padding::Uniform(0))
        .with_content_padding(Padding::Uniform(0))
        .with_title_bar(false)
        .with_title_controls(false)
        .with_shadow(false)
        .with_border_width(0.0)
}

/// One cell with an explicit recorded span: the offline conversion preserves
/// this width into the render grid instead of remeasuring the text.
fn span_cell(text: &str, width: u16, underline: SnapshotUnderline) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        width,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        underline,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("test span cell is valid")
}

fn span_parts(cols: u32, cells: Vec<SnapshotCell>, cursor: SnapshotCursor) -> SnapshotParts {
    assert_eq!(
        cells.len() as u64,
        u64::from(cols),
        "test grid must fill its single row"
    );
    SnapshotParts {
        cols,
        rows: 1,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells,
        palette: resolved_palette(),
        default_foreground: Rgb::new(192, 192, 192),
        default_background: Rgb::new(0, 0, 0),
        default_cursor: Rgb::new(9, 9, 9),
        cursor,
        title: None,
        metadata: SnapshotMetadata::default(),
    }
}

fn shown_cursor(col: u32, shape: SnapshotCursorShape) -> SnapshotCursor {
    SnapshotCursor::new(col, 0, true, true, shape, Rgb::new(255, 0, 0), None)
}

/// U03-B: offline rendering starts no process, PTY, session, or daemon.
///
/// This test builds its snapshot from validated parts and renders it with no
/// session object anywhere in scope; success means no live machinery ran.
#[test]
fn offline_svg_renders_without_a_session() {
    let snapshot = snapshot_of(2, 1, &["a", " "]);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    assert!(rendered.svg().contains(">a</text>"), "{}", rendered.svg());
    assert_eq!((rendered.width(), rendered.height()), (98, 125));
    assert!(!rendered.evidence().cursor_drawn());
}

/// U03-D: hidden and clipped cursors stay hidden in SVG output.
///
/// The cursor color is distinctive, so any drawn cursor would leave its fill
/// in the document; the evidence records the same condition.
#[test]
fn hidden_and_clipped_cursors_stay_hidden() {
    for (name, cursor) in [
        (
            "hidden",
            SnapshotCursor::new(
                0,
                0,
                true,
                false,
                SnapshotCursorShape::Block,
                Rgb::new(1, 2, 3),
                None,
            ),
        ),
        (
            "clipped",
            SnapshotCursor::new(
                9,
                9,
                false,
                true,
                SnapshotCursorShape::Block,
                Rgb::new(1, 2, 3),
                None,
            ),
        ),
    ] {
        let mut parts = snapshot_parts(2, 1, &["a", " "]);
        parts.cursor = cursor;
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
            .expect("offline SVG renders");
        assert!(
            !rendered.svg().contains("#010203"),
            "{name} cursor must not be drawn: {}",
            rendered.svg()
        );
        assert!(
            !rendered.evidence().cursor_drawn(),
            "{name} cursor must be recorded as not drawn"
        );
    }
}

/// R14 (SVG half): the drawn cursor uses the captured cursor color.
///
/// A snapshot whose captured cursor (red) differs from its default cursor
/// (green) must draw the captured color; the default must not leak into the
/// document.
#[test]
fn drawn_cursor_uses_captured_cursor_color() {
    let mut parts = snapshot_parts(2, 1, &["a", " "]);
    parts.default_cursor = Rgb::new(0, 255, 0);
    parts.cursor = SnapshotCursor::new(
        0,
        0,
        true,
        true,
        SnapshotCursorShape::Block,
        Rgb::new(255, 0, 0),
        None,
    );
    let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
    let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    assert!(rendered.evidence().cursor_drawn());
    assert!(
        rendered.svg().contains("#ff0000"),
        "captured cursor color must be drawn: {}",
        rendered.svg()
    );
    assert!(
        !rendered.svg().contains("#00ff00"),
        "default cursor color must not leak: {}",
        rendered.svg()
    );
}

/// R15 (SVG half): subpixel logical cells fail, at any scale.
///
/// Quarter-pixel cells at 40x scale used to render a zero viewBox against
/// nonzero pixels. Like the live style, offline options reject cells under
/// one logical pixel before any allocation or rendering.
#[test]
fn subpixel_cells_fail_without_rendering() {
    let snapshot = snapshot_of(1, 1, &[" "]);
    for scale in [1.0, 40.0] {
        let options = SnapshotRenderOptions::default()
            .with_cell_width(0.25)
            .with_cell_height(0.25)
            .with_font_size(0.2)
            .with_baseline(0.2)
            .with_scale(scale);
        let error = tui_test::render_snapshot_svg(&snapshot, &options)
            .expect_err("subpixel cells must fail");
        assert!(
            matches!(error, tui_test::RenderError::CellTooSmall { .. }),
            "scale {scale}: unexpected error {error}"
        );
    }
}

/// U03-G (SVG half): a 3-column, 2-row grid with 10-by-20 cells and no
/// decoration reports a 30-by-40 image.
#[test]
fn undecorated_grid_reports_exact_svg_size() {
    let snapshot = snapshot_of(3, 2, &[" "; 6]);
    let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (30, 40));
    assert_eq!(
        (rendered.evidence().width(), rendered.evidence().height()),
        (30, 40)
    );
}

/// Parse the SVG `viewBox="0 0 w h"` into its two unscaled extents.
fn svg_view_box(svg: &str) -> (f32, f32) {
    let start = svg.find("viewBox=\"0 0 ").expect("an SVG viewBox") + 13;
    let rest = &svg[start..];
    let end = rest.find('"').expect("a closing quote");
    let mut parts = rest[..end].split(' ');
    (
        parts
            .next()
            .expect("a viewBox width")
            .parse()
            .expect("a number"),
        parts
            .next()
            .expect("a viewBox height")
            .parse()
            .expect("a number"),
    )
}

/// Round-2: fractional metrics share one output size.
///
/// A 3-by-2 grid with 10.1-by-20.1 cells used to produce three sizes (SVG
/// document 60.6x80.4 over a rounded 30x40 viewBox, evidence 61x81, PNG
/// 62x82): PNG sizing rounded before scaling while SVG and evidence used
/// different arithmetic. One rounding at the pixel-allocation boundary
/// gives one size everywhere, and the viewBox stays the exact unscaled
/// canvas so the content still fills it.
#[test]
fn fractional_metrics_share_one_svg_size() {
    for scale in [1.0, 1.5, 2.0] {
        let snapshot = snapshot_of(3, 2, &[" "; 6]);
        let options = undecorated()
            .with_cell_width(10.1)
            .with_cell_height(20.1)
            .with_scale(scale);
        let rendered =
            tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
        let (width, height) = (rendered.width(), rendered.height());
        assert_eq!(
            (rendered.evidence().width(), rendered.evidence().height()),
            (width, height),
            "scale {scale}: evidence must match the reported size"
        );
        assert!(
            rendered
                .svg()
                .contains(&format!(r#"width="{width}" height="{height}""#)),
            "scale {scale}: the document size must match the evidence: {}",
            rendered.svg()
        );
        let (view_w, view_h) = svg_view_box(rendered.svg());
        assert!(
            view_w > 0.0 && view_h > 0.0,
            "scale {scale}: the viewBox must stay non-degenerate: {}",
            rendered.svg()
        );
        assert_eq!(
            (
                (f64::from(view_w) * scale).ceil() as u32,
                (f64::from(view_h) * scale).ceil() as u32
            ),
            (width, height),
            "scale {scale}: the viewBox scaled and ceiled must recover the pixels"
        );
    }
}

/// Round-3: a recorded width-3 span paints all three columns in SVG.
///
/// The conversion used to drop `SnapshotCell.width`, so the renderer capped
/// every span at two columns: an underlined width-3 blank drew a 20px
/// underline while its span covered 30px, stretched the neighbor into the
/// third column, and sized the cursor to two. Spans now come from the
/// recorded width: the underline covers the full 30px, `x` keeps its own
/// 10px, and the block cursor covers 30px.
#[test]
fn recorded_width3_span_paints_three_columns_in_svg() {
    let parts = span_parts(
        4,
        vec![
            span_cell(" ", 3, SnapshotUnderline::Single),
            span_cell("", 0, SnapshotUnderline::None),
            span_cell("", 0, SnapshotUnderline::None),
            span_cell("x", 1, SnapshotUnderline::None),
        ],
        shown_cursor(0, SnapshotCursorShape::Block),
    );
    let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
    let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (40, 20));
    let svg = rendered.svg();
    assert!(
        svg.contains(
            "text-decoration=\"underline\" textLength=\"30.00\" \
             lengthAdjust=\"spacingAndGlyphs\" xml:space=\"preserve\"> <"
        ),
        "the blank span underlines all three columns: {svg}"
    );
    assert!(
        svg.contains(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">x<"
        ),
        "the neighbor keeps its own column instead of stretching: {svg}"
    );
    assert!(
        !svg.contains(
            "textLength=\"30.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">x<"
        ),
        "the neighbor must not absorb the continuations: {svg}"
    );
    assert!(
        svg.contains("width=\"30.00\" height=\"20.00\" fill=\"#ff0000\""),
        "the block cursor covers the recorded span: {svg}"
    );
    assert!(rendered.evidence().cursor_drawn());
}

/// Round-3: a clipped width-2 lead fits its last column in SVG.
///
/// A width-2 glyph in the final column keeps its recorded width but paints
/// only the remaining column: the joint run stays 20px, the cursor covers
/// 10px, and nothing overflows the grid.
#[test]
fn clipped_width2_lead_fits_its_last_column_in_svg() {
    let parts = span_parts(
        2,
        vec![
            span_cell("a", 1, SnapshotUnderline::None),
            span_cell("漢", 2, SnapshotUnderline::None),
        ],
        shown_cursor(1, SnapshotCursorShape::Block),
    );
    let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
    let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (20, 20));
    let svg = rendered.svg();
    assert!(
        svg.contains(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">a<"
        ),
        "the first column keeps its own text: {svg}"
    );
    assert!(
        svg.contains(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">漢<"
        ),
        "the clipped lead fits its last column without overflowing: {svg}"
    );
    assert!(
        svg.contains(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">漢<"
        ),
        "the cursor redraw fits the glyph into its column: {svg}"
    );
    assert!(
        svg.contains("width=\"10.00\" height=\"20.00\" fill=\"#ff0000\""),
        "the block cursor covers the clipped column only: {svg}"
    );
    assert!(
        !svg.contains("width=\"20.00\" height=\"20.00\" fill=\"#ff0000\""),
        "the cursor must not use the recorded width past the row end: {svg}"
    );
}

/// Round-3: a zero-width lead paints nothing but keeps its column in SVG.
///
/// A lone combining mark owns its column with width zero: no text element,
/// no decoration, and the neighbor keeps its own column (`b` at x=10).
#[test]
fn zero_width_lead_paints_nothing_in_svg() {
    let parts = span_parts(
        2,
        vec![
            span_cell("́", 0, SnapshotUnderline::Single),
            span_cell("b", 1, SnapshotUnderline::None),
        ],
        hidden_cursor(),
    );
    let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
    let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (20, 20));
    let svg = rendered.svg();
    assert!(
        svg.contains(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" \
             xml:space=\"preserve\">b<"
        ),
        "the neighbor fits exactly one column: {svg}"
    );
    assert!(
        svg.contains("x=\"10.00\""),
        "the neighbor starts in the second column: {svg}"
    );
    assert!(
        !svg.contains("<text x=\"0.00\""),
        "the zero-width lead draws no text in its column: {svg}"
    );
    assert!(
        !svg.contains("text-decoration"),
        "the zero-width underline paints nothing: {svg}"
    );
}

/// F7: same-paint leads keep their own cell geometry in SVG.
///
/// Every cell shares one paint, so the old joint run merged each row into a
/// single text (`AAAB` / `ABAA`). Each lead must instead emit its own text
/// with its own `x` and recorded span width: `B` sits at pixel 30 in the
/// first row and pixel 10 in the second, and the shared underline survives
/// on every split text.
#[test]
fn same_paint_spans_keep_cell_geometry() {
    let cells = vec![
        span_cell("A", 1, SnapshotUnderline::Single),
        span_cell("A", 1, SnapshotUnderline::Single),
        span_cell("A", 1, SnapshotUnderline::Single),
        span_cell("B", 1, SnapshotUnderline::Single),
        span_cell("A", 1, SnapshotUnderline::Single),
        span_cell("B", 1, SnapshotUnderline::Single),
        span_cell("A", 1, SnapshotUnderline::Single),
        span_cell("A", 1, SnapshotUnderline::Single),
    ];
    let parts = SnapshotParts {
        cols: 4,
        rows: 2,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells,
        palette: resolved_palette(),
        default_foreground: Rgb::new(192, 192, 192),
        default_background: Rgb::new(0, 0, 0),
        default_cursor: Rgb::new(9, 9, 9),
        cursor: hidden_cursor(),
        title: None,
        metadata: SnapshotMetadata::default(),
    };
    let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
    let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (40, 40));
    let svg = rendered.svg();
    assert_eq!(
        svg.matches(
            "textLength=\"10.00\" lengthAdjust=\"spacingAndGlyphs\" xml:space=\"preserve\">B<"
        )
        .count(),
        2,
        "each B keeps its own single-column text: {svg}"
    );
    assert!(
        svg.contains("x=\"30.00\""),
        "the first-row B starts in the fourth column: {svg}"
    );
    assert!(
        svg.contains("x=\"10.00\""),
        "the second-row B starts in the second column: {svg}"
    );
    assert_eq!(
        svg.matches("text-decoration=\"underline\"").count(),
        8,
        "splitting the run keeps the shared underline on every cell: {svg}"
    );
    assert!(
        !svg.contains(">AAAB<"),
        "the first row must not merge into one text: {svg}"
    );
    assert!(
        !svg.contains(">ABAA<"),
        "the second row must not merge into one text: {svg}"
    );
    assert!(!rendered.evidence().cursor_drawn());
}

/// F8: thin cursors stay inside a 1px cell in both renderers.
///
/// At 1px cells the 2px thin-cursor thickness overflows the cell in SVG
/// (the underline starts a pixel above the cell, the bar runs a pixel
/// wide of it) while the PNG path already clamps to the cell bounds. Both
/// shapes must draw exactly the cell's own pixel in SVG too.
#[test]
fn thin_cursor_stays_inside_small_cell() {
    use tui_test::SnapshotCursor;
    for shape in [SnapshotCursorShape::Underline, SnapshotCursorShape::Bar] {
        let texts = [" "; 9];
        let parts = SnapshotParts {
            cols: 3,
            rows: 3,
            scope: SnapshotScope::Viewport,
            viewport_origin: 0,
            cells: texts.iter().map(|text| cell(text)).collect(),
            palette: resolved_palette(),
            default_foreground: Rgb::new(192, 192, 192),
            default_background: Rgb::new(0, 0, 0),
            default_cursor: Rgb::new(9, 9, 9),
            cursor: SnapshotCursor::new(1, 1, true, true, shape, Rgb::new(255, 0, 0), None),
            title: None,
            metadata: SnapshotMetadata::default(),
        };
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated()
            .with_cell_width(1.0)
            .with_cell_height(1.0)
            .with_baseline(0.8);
        let rendered =
            tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
        assert_eq!((rendered.width(), rendered.height()), (3, 3));
        assert!(
            rendered.svg().contains(
                "<rect x=\"1.00\" y=\"1.00\" width=\"1.00\" height=\"1.00\" fill=\"#ff0000\"/>"
            ),
            "the {shape:?} cursor fills exactly its 1px cell: {}",
            rendered.svg()
        );
        assert!(rendered.evidence().cursor_drawn());
        #[cfg(feature = "recording-raster")]
        {
            use tui_test::render_snapshot_png;
            let rendered_png =
                render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
            assert_eq!((rendered_png.width(), rendered_png.height()), (3, 3));
            let decoder = png::Decoder::new(rendered_png.png());
            let mut reader = decoder.read_info().expect("PNG header decodes");
            let mut pixels = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut pixels).expect("PNG frame decodes");
            assert_eq!((info.width, info.height), (3, 3));
            let red: Vec<(u32, u32)> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, px)| px[0] == 255 && px[1] == 0 && px[2] == 0 && px[3] == 255)
                .map(|(i, _)| ((i as u32) % 3, (i as u32) / 3))
                .collect();
            assert!(
                !red.is_empty(),
                "the {shape:?} cursor paints in PNG too: {red:?}"
            );
            assert!(
                red.iter().all(|&(x, y)| x == 1 && y == 1),
                "the {shape:?} cursor stays inside its cell in both renderers: {red:?}"
            );
        }
    }
}

/// U03-H: invalid metrics and scale values fail without a panic.
#[test]
fn invalid_metrics_and_scale_fail_without_panic() {
    use tui_test::render::style::Padding;
    let bad = [
        SnapshotRenderOptions::default().with_cell_width(0.0),
        SnapshotRenderOptions::default().with_cell_width(-10.0),
        SnapshotRenderOptions::default().with_cell_width(f32::NAN),
        SnapshotRenderOptions::default().with_cell_width(f32::INFINITY),
        SnapshotRenderOptions::default().with_cell_height(0.0),
        SnapshotRenderOptions::default().with_font_size(0.0),
        SnapshotRenderOptions::default().with_font_size(f32::NAN),
        SnapshotRenderOptions::default().with_baseline(-1.0),
        SnapshotRenderOptions::default().with_baseline(22.0),
        SnapshotRenderOptions::default().with_baseline(f32::NAN),
        SnapshotRenderOptions::default().with_scale(0.0),
        SnapshotRenderOptions::default().with_scale(-2.0),
        SnapshotRenderOptions::default().with_scale(f64::NAN),
        SnapshotRenderOptions::default().with_scale(f64::INFINITY),
        SnapshotRenderOptions::default().with_scale(f64::MAX),
        SnapshotRenderOptions::default().with_border_width(-1.0),
        SnapshotRenderOptions::default().with_border_width(f32::NAN),
        SnapshotRenderOptions::default().with_border_radius(-4.0),
        SnapshotRenderOptions::default().with_canvas_padding(Padding::Uniform(10_001)),
    ];
    let snapshot = snapshot_of(1, 1, &[" "]);
    for options in bad {
        let error = tui_test::render_snapshot_svg(&snapshot, &options)
            .expect_err("invalid metrics must fail");
        assert!(
            !error.to_string().is_empty(),
            "typed errors carry a message"
        );
    }
}

/// U03-I (SVG half): default `Style` values keep their previous layout.
///
/// A default offline render of a 1-by-1 grid reports the recorded live
/// screenshot dimensions: an 88-by-125 panel with decoration.
#[test]
fn default_style_values_keep_previous_layout() {
    let snapshot = snapshot_of(1, 1, &[" "]);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    assert_eq!((rendered.width(), rendered.height()), (88, 125));
    assert!(
        rendered.svg().contains(r#"viewBox="0 0 88 125""#),
        "default geometry is unchanged"
    );
}

/// U03-C (SVG half): bytes and path wrappers save the same document.
#[test]
fn svg_bytes_and_path_wrappers_agree() {
    let snapshot = snapshot_of(2, 1, &["a", " "]);
    let options = SnapshotRenderOptions::default();
    let rendered = tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
    let dir = std::env::temp_dir();
    let path = dir.join(format!("u03-svg-{}.svg", std::process::id()));
    let evidence =
        tui_test::write_snapshot_svg(&path, &snapshot, &options).expect("SVG path writes");
    let saved = std::fs::read_to_string(&path).expect("SVG file reads back");
    std::fs::remove_file(&path).ok();
    assert_eq!(saved, rendered.svg());
    assert_eq!(evidence, *rendered.evidence());
}

/// U03-A (SVG half): a live capture renders through the same functions.
///
/// The offline render of a live Viewport capture must equal the live
/// screenshot SVG byte for byte: same grid conversion, same resolved
/// colors, same cursor projection, same title, same layout.
#[cfg(unix)]
#[test]
fn live_capture_and_screenshot_share_the_svg_path() {
    use tui_test::{OpenOptions, Operation, RunOptions, Session, SnapshotScope};

    let session = Session::new(format!("u03-svg-{}", std::process::id()));
    let defaults = OpenOptions::default();
    session
        .run(RunOptions {
            backend: defaults.backend,
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "printf 'hello-U03'; sleep 30".to_string()],
            profile: defaults.profile,
            style: defaults.style,
            cols: 16,
            rows: 2,
            cwd: defaults.cwd,
            env: defaults.env,
            wait_ready: Some(false),
            restart: defaults.restart,
            timeouts: defaults.timeouts,
            recording: defaults.recording,
        })
        .expect("session starts");
    let deadline = Instant::now() + Duration::from_secs(15);
    let marker = loop {
        let text = match session.execute(Operation::Text { full: false }) {
            Ok(tui_test::OperationResult::Text(text)) => text,
            other => panic!("text operation gave {other:?}"),
        };
        if text.contains("hello-U03") {
            break text;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for marker; got {text:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(marker.contains("hello-U03"));

    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("live capture works");
    let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");

    let path = std::env::temp_dir().join(format!("u03-live-{}.svg", std::process::id()));
    session
        .execute(Operation::Screenshot {
            full: false,
            path: Some(path.to_string_lossy().into_owned()),
            zoom: None,
            background: None,
        })
        .expect("live screenshot works");
    session.close().expect("session closes");
    let live = std::fs::read_to_string(&path).expect("live SVG reads back");
    std::fs::remove_file(&path).ok();
    assert_eq!(live, rendered.svg());
}

#[cfg(feature = "recording-raster")]
mod raster_tests {
    use super::*;
    use tui_test::{render_snapshot_png, write_snapshot_png};

    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(bytes);
        let mut reader = decoder.read_info().expect("PNG header decodes");
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).expect("PNG frame decodes");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        (info.width, info.height, pixels)
    }

    /// U03-B: offline PNG rendering starts no session machinery.
    #[test]
    fn offline_png_renders_without_a_session() {
        let snapshot = snapshot_of(2, 1, &["x", " "]);
        let rendered = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
            .expect("offline PNG renders");
        let (width, height, _) = decode_png(rendered.png());
        assert_eq!((width, height), (98, 125));
        assert_eq!((rendered.width(), rendered.height()), (98, 125));
    }

    /// U03-C: bytes and path wrappers decode to equal pixels.
    #[test]
    fn png_bytes_and_path_wrappers_decode_equal() {
        let snapshot = snapshot_of(3, 1, &["a", "x", " "]);
        let options = SnapshotRenderOptions::default();
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        let dir = std::env::temp_dir();
        let path = dir.join(format!("u03-png-{}.png", std::process::id()));
        let evidence = write_snapshot_png(&path, &snapshot, &options).expect("PNG path writes");
        let saved = std::fs::read(&path).expect("PNG file reads back");
        std::fs::remove_file(&path).ok();
        assert_eq!(decode_png(&saved), decode_png(rendered.png()));
        assert_eq!(evidence, *rendered.evidence());
    }

    /// Extent of one RGB color in a row band: the painted geometry the
    /// round-3 span tests assert, not just the decoded dimensions.
    fn band_extent(
        pixels: &[u8],
        width: u32,
        height: u32,
        y0: u32,
        y1: u32,
        color: [u8; 3],
    ) -> (u32, u32, u32) {
        let mut min_x = width;
        let mut max_x = 0;
        let mut count = 0;
        for y in y0..y1.min(height) {
            for x in 0..width {
                let i = (y * width + x) as usize * 4;
                if pixels[i..i + 3] == color {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    count += 1;
                }
            }
        }
        (min_x, max_x, count)
    }

    /// Round-3 (PNG half): a recorded width-3 underline paints three columns.
    ///
    /// Same grid as `recorded_width3_span_paints_three_columns_in_svg` but
    /// with the cursor hidden, so the underline is the only painted decor:
    /// it fills 30px, not 20px. Underlines need no font lookup, so this
    /// geometry holds on any host font.
    #[test]
    fn recorded_width3_underline_paints_three_columns_in_png() {
        let parts = span_parts(
            4,
            vec![
                span_cell(" ", 3, SnapshotUnderline::Single),
                span_cell("", 0, SnapshotUnderline::None),
                span_cell("", 0, SnapshotUnderline::None),
                span_cell("x", 1, SnapshotUnderline::None),
            ],
            hidden_cursor(),
        );
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (40, 20));
        let (width, height, pixels) = decode_png(rendered.png());
        assert_eq!((width, height), (40, 20));
        // The default-foreground underline is one pixel tall at scale 1.
        assert_eq!(
            band_extent(&pixels, width, height, 17, 20, [192, 192, 192]),
            (0, 29, 30),
            "the underline covers all three columns"
        );
    }

    /// Round-3 (PNG half): a block cursor covers a width-3 span.
    ///
    /// The blank lead redraws no glyph, so the cursor fill is the only red
    /// in the image: 30x20 with no font lookup involved.
    #[test]
    fn recorded_width3_cursor_covers_three_columns_in_png() {
        let parts = span_parts(
            4,
            vec![
                span_cell(" ", 3, SnapshotUnderline::None),
                span_cell("", 0, SnapshotUnderline::None),
                span_cell("", 0, SnapshotUnderline::None),
                span_cell("x", 1, SnapshotUnderline::None),
            ],
            shown_cursor(0, SnapshotCursorShape::Block),
        );
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (40, 20));
        assert!(rendered.evidence().cursor_drawn());
        let (width, height, pixels) = decode_png(rendered.png());
        assert_eq!((width, height), (40, 20));
        assert_eq!(
            band_extent(&pixels, width, height, 0, 20, [255, 0, 0]),
            (0, 29, 600),
            "the block cursor covers the recorded span"
        );
    }

    /// Round-3 (PNG half): a clipped width-2 lead paints one column.
    ///
    /// The lead keeps its recorded width of two but the grid ends after one
    /// column, so an underline cursor covers 10px and nothing overflows the
    /// 20px panel. The lead holds a narrow `W` with a recorded width of two
    /// rather than a CJK glyph, so no host CJK font is needed: the renderer
    /// cannot tell the two apart, and both take the same clipping path.
    #[test]
    fn clipped_width2_lead_paints_one_column_in_png() {
        let parts = span_parts(
            2,
            vec![
                span_cell("a", 1, SnapshotUnderline::None),
                span_cell("W", 2, SnapshotUnderline::None),
            ],
            shown_cursor(1, SnapshotCursorShape::Underline),
        );
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (20, 20));
        assert!(rendered.evidence().cursor_drawn());
        let (width, height, pixels) = decode_png(rendered.png());
        assert_eq!((width, height), (20, 20));
        // An underline cursor is two pixels tall at scale 1 and redraws no
        // glyph, so this geometry needs no font lookup either.
        assert_eq!(
            band_extent(&pixels, width, height, 0, 20, [255, 0, 0]),
            (10, 19, 20),
            "the cursor covers the clipped column only"
        );
    }

    /// Round-3 (PNG half): a zero-width lead paints nothing.
    ///
    /// Same grid as `zero_width_lead_paints_nothing_in_svg`: the underline
    /// band stays background, while the neighbor glyph still lands in the
    /// second column.
    #[test]
    fn zero_width_lead_paints_nothing_in_png() {
        let parts = span_parts(
            2,
            vec![
                span_cell("́", 0, SnapshotUnderline::Single),
                span_cell("b", 1, SnapshotUnderline::None),
            ],
            hidden_cursor(),
        );
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (20, 20));
        let (width, height, pixels) = decode_png(rendered.png());
        assert_eq!((width, height), (20, 20));
        let gray = [192, 192, 192];
        assert_eq!(
            band_extent(&pixels, width, height, 17, 20, gray),
            (20, 0, 0),
            "the zero-width underline paints nothing"
        );
        let (_, _, glyphs) = band_extent(&pixels, width, height, 0, 17, gray);
        assert!(glyphs > 0, "the neighbor glyph still renders");
        let (min_x, _, _) = band_extent(&pixels, width, height, 0, 17, gray);
        assert!(
            min_x >= 10,
            "no glyph pixels leak into the zero-width column"
        );
    }

    /// U03-D: hidden and clipped cursors stay hidden in PNG output.
    #[test]
    fn hidden_and_clipped_cursors_stay_hidden_in_png() {
        let options = SnapshotRenderOptions::default();
        let hidden = {
            let mut parts = snapshot_parts(2, 1, &[" ", " "]);
            parts.cursor = SnapshotCursor::new(
                0,
                0,
                true,
                false,
                SnapshotCursorShape::Block,
                Rgb::new(200, 30, 30),
                None,
            );
            TerminalSnapshot::try_new(parts).expect("test snapshot is valid")
        };
        let shown = {
            let mut parts = snapshot_parts(2, 1, &[" ", " "]);
            parts.cursor = SnapshotCursor::new(
                0,
                0,
                true,
                true,
                SnapshotCursorShape::Block,
                Rgb::new(200, 30, 30),
                None,
            );
            TerminalSnapshot::try_new(parts).expect("test snapshot is valid")
        };
        let clipped = {
            let mut parts = snapshot_parts(2, 1, &[" ", " "]);
            parts.cursor = SnapshotCursor::new(
                7,
                7,
                false,
                true,
                SnapshotCursorShape::Block,
                Rgb::new(200, 30, 30),
                None,
            );
            TerminalSnapshot::try_new(parts).expect("test snapshot is valid")
        };
        let hidden_render = render_snapshot_png(&hidden, &options).expect("hidden renders");
        let shown_render = render_snapshot_png(&shown, &options).expect("shown renders");
        let clipped_render = render_snapshot_png(&clipped, &options).expect("clipped renders");
        assert!(!hidden_render.evidence().cursor_drawn());
        assert!(shown_render.evidence().cursor_drawn());
        assert!(!clipped_render.evidence().cursor_drawn());
        assert_ne!(
            decode_png(hidden_render.png()),
            decode_png(shown_render.png()),
            "a drawn cursor changes pixels"
        );
        assert_eq!(
            decode_png(hidden_render.png()),
            decode_png(clipped_render.png()),
            "hidden and clipped cursors both draw nothing"
        );
    }

    /// R14 (PNG half): the drawn cursor uses the captured cursor color.
    ///
    /// A block cursor over a blank cell fills it with the captured color
    /// (red); the differing default (green) must not appear anywhere.
    #[test]
    fn drawn_cursor_uses_captured_cursor_color_in_png() {
        let mut parts = snapshot_parts(2, 1, &[" ", " "]);
        parts.default_cursor = Rgb::new(0, 255, 0);
        parts.cursor = SnapshotCursor::new(
            0,
            0,
            true,
            true,
            SnapshotCursorShape::Block,
            Rgb::new(255, 0, 0),
            None,
        );
        let snapshot = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert!(rendered.evidence().cursor_drawn());
        let (width, height, pixels) = decode_png(rendered.png());
        assert_eq!((width, height), (20, 20));
        let center = (10 * width + 5) as usize * 4;
        assert_eq!(
            &pixels[center..center + 4],
            &[255, 0, 0, 255],
            "cursor cell must be the captured red"
        );
        assert!(
            !pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel == &[0, 255, 0, 255]),
            "default cursor green must not leak into any pixel"
        );
    }

    /// R15 (PNG half): subpixel logical cells fail before allocation, at any
    /// scale, so no backend can disagree about the output size.
    #[test]
    fn subpixel_cells_fail_without_rendering_in_png() {
        let snapshot = snapshot_of(1, 1, &[" "]);
        for scale in [1.0, 40.0] {
            let options = SnapshotRenderOptions::default()
                .with_cell_width(0.25)
                .with_cell_height(0.25)
                .with_font_size(0.2)
                .with_baseline(0.2)
                .with_scale(scale);
            let error =
                render_snapshot_png(&snapshot, &options).expect_err("subpixel cells must fail");
            assert!(
                matches!(error, tui_test::RenderError::CellTooSmall { .. }),
                "scale {scale}: unexpected error {error}"
            );
        }
    }

    /// U03-G: a 3-column, 2-row grid with 10-by-20 cells gives a 30-by-40
    /// image with no decoration. Blank cells need no font lookup, so the
    /// geometry gate stays independent of host fonts.
    #[test]
    fn undecorated_grid_gives_exact_decoded_pixels() {
        let snapshot = snapshot_of(3, 2, &[" "; 6]);
        let options = undecorated().with_cell_width(10.0).with_cell_height(20.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (30, 40));
        let (width, height, _) = decode_png(rendered.png());
        assert_eq!((width, height), (30, 40));
    }

    /// Round-2 (PNG half): fractional metrics share one output size across
    /// PNG and SVG.
    ///
    /// Same grid as `fractional_metrics_share_one_svg_size`: the decoded
    /// PNG, the PNG evidence, the SVG evidence, and the SVG document must
    /// all agree. At 2x the shared size is exactly 61x81.
    #[test]
    fn fractional_metrics_share_one_png_size() {
        for scale in [1.0, 1.5, 2.0] {
            let snapshot = snapshot_of(3, 2, &[" "; 6]);
            let options = undecorated()
                .with_cell_width(10.1)
                .with_cell_height(20.1)
                .with_scale(scale);
            let png = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
            let svg =
                tui_test::render_snapshot_svg(&snapshot, &options).expect("offline SVG renders");
            let (width, height, _) = decode_png(png.png());
            assert_eq!(
                (width, height),
                (png.width(), png.height()),
                "scale {scale}: decoded PNG must match the PNG evidence"
            );
            assert_eq!(
                (svg.width(), svg.height()),
                (width, height),
                "scale {scale}: SVG and PNG must share one size"
            );
            assert_eq!(
                (svg.evidence().width(), svg.evidence().height()),
                (width, height),
                "scale {scale}: SVG evidence must match the shared size"
            );
        }
        let snapshot = snapshot_of(3, 2, &[" "; 6]);
        let options = undecorated()
            .with_cell_width(10.1)
            .with_cell_height(20.1)
            .with_scale(2.0);
        let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (61, 81));
        let (width, height, _) = decode_png(rendered.png());
        assert_eq!((width, height), (61, 81));
    }

    /// U03-I (PNG half): default options keep the recorded screenshot size.
    #[test]
    fn default_options_keep_recorded_png_size() {
        let snapshot = snapshot_of(1, 1, &[" "]);
        let rendered = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
            .expect("offline PNG renders");
        assert_eq!((rendered.width(), rendered.height()), (88, 125));
        let (width, height, _) = decode_png(rendered.png());
        assert_eq!((width, height), (88, 125));
    }

    /// U03-A (PNG half): a live capture renders through the same renderer.
    ///
    /// The offline PNG of a live Viewport capture must decode to the same
    /// pixels as the live screenshot PNG: same frame conversion, same
    /// `GridRenderer` path at the same size.
    #[cfg(unix)]
    #[test]
    fn live_capture_and_screenshot_share_the_png_path() {
        use tui_test::{OpenOptions, Operation, RunOptions, Session, SnapshotScope};

        let session = Session::new(format!("u03-png-{}", std::process::id()));
        let defaults = OpenOptions::default();
        session
            .run(RunOptions {
                backend: defaults.backend,
                program: "sh".to_string(),
                args: vec!["-c".to_string(), "printf 'hello-U03'; sleep 30".to_string()],
                profile: defaults.profile,
                style: defaults.style,
                cols: 16,
                rows: 2,
                cwd: defaults.cwd,
                env: defaults.env,
                wait_ready: Some(false),
                restart: defaults.restart,
                timeouts: defaults.timeouts,
                recording: defaults.recording,
            })
            .expect("session starts");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let text = match session.execute(Operation::Text { full: false }) {
                Ok(tui_test::OperationResult::Text(text)) => text,
                other => panic!("text operation gave {other:?}"),
            };
            if text.contains("hello-U03") {
                break;
            }
            if Instant::now() >= deadline {
                panic!("timed out waiting for marker; got {text:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let snapshot = session
            .capture_snapshot(SnapshotScope::Viewport)
            .expect("live capture works");
        let rendered = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
            .expect("offline PNG renders");

        let path = std::env::temp_dir().join(format!("u03-live-{}.png", std::process::id()));
        session
            .execute(Operation::Screenshot {
                full: false,
                path: Some(path.to_string_lossy().into_owned()),
                zoom: None,
                background: None,
            })
            .expect("live screenshot works");
        session.close().expect("session closes");
        let live = std::fs::read(&path).expect("live PNG reads back");
        std::fs::remove_file(&path).ok();
        assert_eq!(decode_png(&live), decode_png(rendered.png()));
    }

    /// F9: underline and strike bands stay inside 1px and 2px cells.
    ///
    /// At scale 1 the ordinary underline sits three pixels above the cell
    /// bottom, so a 1px cell's band painted two rows above its owner (row 2
    /// underlined row 0); strike-through escapes the same way. Every band
    /// is clamped into its owning cell, and a block cursor repaints the
    /// covered cell's bands in the contrasting color without leaking
    /// either. Blank cells need no font lookup, so this holds on any host
    /// font.
    #[test]
    fn decorations_stay_inside_small_cells() {
        fn decorated_blank() -> SnapshotCell {
            SnapshotCell::new(
                " ".to_string(),
                1,
                SnapshotColor::Rgb(255, 0, 0),
                SnapshotColor::Default,
                Attrs::STRIKE,
                SnapshotUnderline::Single,
                SnapshotColor::Default,
                None,
                None,
            )
            .expect("test cell is valid")
        }

        fn snapshot_with(cursor: SnapshotCursor) -> TerminalSnapshot {
            let parts = SnapshotParts {
                cols: 1,
                rows: 3,
                scope: SnapshotScope::Viewport,
                viewport_origin: 0,
                cells: vec![decorated_blank(), cell(" "), decorated_blank()],
                palette: resolved_palette(),
                default_foreground: Rgb::new(192, 192, 192),
                default_background: Rgb::new(0, 0, 0),
                default_cursor: Rgb::new(9, 9, 9),
                cursor,
                title: None,
                metadata: SnapshotMetadata::default(),
            };
            TerminalSnapshot::try_new(parts).expect("test snapshot is valid")
        }

        fn block_cursor(row: u32) -> SnapshotCursor {
            SnapshotCursor::new(
                0,
                row,
                true,
                true,
                SnapshotCursorShape::Block,
                Rgb::new(0, 0, 255),
                None,
            )
        }

        // A decoration pixel is any pixel the red bands touched: the bands
        // are antialiased, so a fractional strike edge blends toward black
        // instead of landing on exact red.
        fn is_decoration(pixel: &[u8]) -> bool {
            pixel[3] == 255 && pixel[0] > 0 && pixel[1] == 0 && pixel[2] == 0
        }

        fn decoration_rows(pixels: &[u8], width: u32, height: u32) -> Vec<u32> {
            let mut rows = Vec::new();
            for y in 0..height {
                let touched = (0..width).any(|x| {
                    let i = (y * width + x) as usize * 4;
                    is_decoration(&pixels[i..i + 4])
                });
                if touched {
                    rows.push(y);
                }
            }
            rows
        }

        fn is_cursor(pixel: &[u8]) -> bool {
            pixel == [0, 0, 255, 255]
        }

        for cell_height in [1.0f32, 2.0] {
            let options = undecorated()
                .with_cell_width(3.0)
                .with_cell_height(cell_height)
                .with_baseline(0.8 * cell_height);
            let row_px = cell_height as u32;
            let owner = |y: u32| y / row_px == 0 || y / row_px == 2;

            let snapshot = snapshot_with(hidden_cursor());
            let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
            assert!(!rendered.evidence().cursor_drawn());
            let (width, height, pixels) = decode_png(rendered.png());
            assert_eq!((width, height), (3, 3 * row_px));
            let rows = decoration_rows(&pixels, width, height);
            assert!(
                rows.iter().all(|&y| owner(y)),
                "height {cell_height}: hidden cursor leaves no decoration outside rows 0 and 2: {rows:?}"
            );
            for owning in [0, 2] {
                assert!(
                    rows.iter().any(|&y| y / row_px == owning),
                    "height {cell_height}: hidden cursor keeps row {owning}'s band in its cell: {rows:?}"
                );
            }

            let snapshot = snapshot_with(block_cursor(1));
            let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
            assert!(rendered.evidence().cursor_drawn());
            let (width, height, pixels) = decode_png(rendered.png());
            assert_eq!((width, height), (3, 3 * row_px));
            let rows = decoration_rows(&pixels, width, height);
            assert!(
                rows.iter().all(|&y| owner(y)),
                "height {cell_height}: a block cursor on row 1 keeps decorations in rows 0 and 2: {rows:?}"
            );
            for owning in [0, 2] {
                assert!(
                    rows.iter().any(|&y| y / row_px == owning),
                    "height {cell_height}: row {owning}'s band survives the neighboring cursor: {rows:?}"
                );
            }
            let cursor_pixels = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| is_cursor(pixel.as_slice()))
                .map(|(i, _)| (i as u32) / width)
                .collect::<Vec<_>>();
            assert_eq!(
                cursor_pixels.len() as u32,
                3 * row_px,
                "height {cell_height}: the block cursor fills exactly row 1"
            );
            assert!(
                cursor_pixels.iter().all(|&y| y / row_px == 1),
                "height {cell_height}: the block cursor stays inside row 1: {cursor_pixels:?}"
            );

            let snapshot = snapshot_with(block_cursor(2));
            let rendered = render_snapshot_png(&snapshot, &options).expect("offline PNG renders");
            assert!(rendered.evidence().cursor_drawn());
            let (width, height, pixels) = decode_png(rendered.png());
            assert_eq!((width, height), (3, 3 * row_px));
            let rows = decoration_rows(&pixels, width, height);
            assert!(
                !rows.is_empty() && rows.iter().all(|&y| y / row_px == 0),
                "height {cell_height}: the covered row 2 shows no decoration, row 0 keeps its band: {rows:?}"
            );
            let cursor_count = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| is_cursor(pixel.as_slice()))
                .count() as u32;
            if row_px == 1 {
                assert_eq!(
                    cursor_count, 0,
                    "height {cell_height}: the repaint covers the 1px cursor cell"
                );
            } else {
                assert!(
                    cursor_count < 3 * row_px,
                    "height {cell_height}: the repaint takes back part of the cursor cell"
                );
            }
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .all(|(i, pixel)| !is_cursor(pixel) || (i as u32) / width / row_px == 2),
                "height {cell_height}: cursor pixels stay inside row 2"
            );
        }
    }
}
