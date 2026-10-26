//! U05: mandatory grapheme rendering through one shaping path.
//!
//! The pinned cell, cursor-repaint, and title paths share a single shaper
//! call per grapheme (rustybuzz), with one face selected per grapheme in
//! fallback order. These tests cover the required U05 behaviors over the
//! P02 corpus: each Unicode case renders (U05-A), cell and cursor repaint
//! shape equally (U05-B), wide-cell clipping stays in the permitted region
//! (U05-C), missing glyphs fail (U05-D), single-scalar cells keep the
//! verbatim path (U05-E), every positive case gives glyph evidence and an
//! image (U05-F), and the named negative errors without image success
//! (U05-G).
//!
//! Font bytes come from the repo assets read as files, so host fonts
//! cannot leak into strict output:
//!
//! - JetBrains Mono static TTFs (OFL, see
//!   `assets/jetbrains-mono/SOURCE`): regular, bold, italic, bold-italic.
//!   Covers ASCII, box drawing, and the combining marks U+0301 and U+0308.
//! - `assets/noto-sans-jp/NotoSansJP-subset.ttf` (OFL-1.1, see
//!   `assets/noto-sans-jp/SOURCE`): a development-time subset keeping only
//!   U+6771 (東) plus `.notdef`, for the CJK corpus case. Nothing fetches
//!   fonts at test time.
//! - U+1F980 (🦀) is in none of these faces and is the named negative
//!   missing-glyph case.

#![cfg(feature = "recording-raster")]

use tui_test::profile::Rgb;
use tui_test::render::style::Padding;
use tui_test::terminal::cell::Attrs;
use tui_test::{
    render_snapshot_png, PinnedFace, PinnedFontError, PinnedFontSet, RenderError, SnapshotCell,
    SnapshotColor, SnapshotCursor, SnapshotCursorShape, SnapshotMetadata, SnapshotParts,
    SnapshotRenderOptions, SnapshotScope, SnapshotUnderline, TerminalSnapshot,
};

const CELL_W: u32 = 10;
const CELL_H: u32 = 21;

const E_ACUTE: &str = "e\u{301}";
const A_DIAERESIS: &str = "a\u{308}";

fn regular_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-Regular.ttf").to_vec()
}

fn bold_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-Bold.ttf").to_vec()
}

fn italic_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-Italic.ttf").to_vec()
}

fn bold_italic_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-BoldItalic.ttf").to_vec()
}

fn cjk_bytes() -> Vec<u8> {
    include_bytes!("../assets/noto-sans-jp/NotoSansJP-subset.ttf").to_vec()
}

/// Every style slot the corpus needs, plus the CJK face in the regular
/// slot after JetBrains Mono. No title is drawn, but the pair mirrors the
/// U04 convention of explicit style faces.
fn corpus_set() -> PinnedFontSet {
    let face = |bytes: Vec<u8>, bold: bool, italic: bool| {
        PinnedFace::new(bytes, 0, bold, italic).expect("bundled face parses")
    };
    PinnedFontSet::new(vec![
        face(regular_bytes(), false, false),
        face(bold_bytes(), true, false),
        face(italic_bytes(), false, true),
        face(bold_italic_bytes(), true, true),
        face(cjk_bytes(), false, false),
    ])
    .expect("corpus set builds")
}

fn resolved_palette() -> [Rgb; 256] {
    let profile = tui_test::profile::Profile::default();
    std::array::from_fn(|index| profile.colors.rgb(index as u8))
}

fn cell(text: &str, width: u16, attrs: Attrs) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        width,
        SnapshotColor::Default,
        SnapshotColor::Default,
        attrs,
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("test cell is valid")
}

fn plain(text: &str, width: u16) -> SnapshotCell {
    cell(text, width, Attrs::empty())
}

fn blank() -> SnapshotCell {
    plain(" ", 1)
}

fn continuation() -> SnapshotCell {
    SnapshotCell::new(
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
    .expect("continuation cell is valid")
}

fn hidden_cursor() -> SnapshotCursor {
    SnapshotCursor::new(
        0,
        0,
        false,
        true,
        SnapshotCursorShape::Block,
        Rgb::new(9, 9, 9),
        Some(true),
    )
}

fn block_cursor_at(col: u32, row: u32) -> SnapshotCursor {
    SnapshotCursor::new(
        col,
        row,
        true,
        true,
        SnapshotCursorShape::Block,
        Rgb::new(9, 9, 9),
        Some(true),
    )
}

fn snapshot_with_cells(
    cols: u32,
    rows: u32,
    cells: Vec<SnapshotCell>,
    cursor: SnapshotCursor,
) -> TerminalSnapshot {
    assert_eq!(cells.len() as u32, cols * rows);
    TerminalSnapshot::try_new(SnapshotParts {
        cols,
        rows,
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
    })
    .expect("test snapshot is valid")
}

fn strict_options(set: &PinnedFontSet) -> SnapshotRenderOptions {
    SnapshotRenderOptions::default()
        .with_canvas_padding(Padding::Uniform(0))
        .with_content_padding(Padding::Uniform(0))
        .with_title_bar(false)
        .with_title_controls(false)
        .with_shadow(false)
        .with_border_width(0.0)
        .with_border_radius(0.0)
        .with_background(Some(tui_test::CaptureBackground::Color(Rgb::new(0, 0, 0))))
        .with_pinned_fonts(set.clone())
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().expect("PNG header decodes");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG frame decodes");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    (info.width, info.height, pixels)
}

/// Pixel offsets with a color different from `background`.
fn ink_offsets(pixels: &[u8], image_width: u32, background: [u8; 4]) -> Vec<(u32, u32)> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel != &&background)
        .map(|(index, _)| {
            let index = index as u32;
            (index % image_width, index / image_width)
        })
        .collect()
}

/// Pixel offsets of ink inside one cell rect, relative to the rect origin.
fn cell_ink(
    pixels: &[u8],
    image_width: u32,
    background: [u8; 4],
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) -> Vec<(u32, u32)> {
    ink_offsets(pixels, image_width, background)
        .into_iter()
        .filter(|(x, y)| *x >= left && *x < left + width && *y >= top && *y < top + height)
        .map(|(x, y)| (x - left, y - top))
        .collect()
}

const BLACK: [u8; 4] = [0, 0, 0, 255];
const CURSOR_FILL: [u8; 4] = [9, 9, 9, 255];

/// Render one row of `cols` cells with a hidden cursor. With zero chrome
/// the image is exactly the grid: `cols * CELL_W` by `rows * CELL_H`.
fn render_row(cells: Vec<SnapshotCell>, set: &PinnedFontSet) -> (u32, u32, Vec<u8>) {
    let cols = cells.len() as u32;
    let snapshot = snapshot_with_cells(cols, 1, cells, hidden_cursor());
    let rendered =
        render_snapshot_png(&snapshot, &strict_options(set)).expect("corpus row renders");
    assert!(rendered.evidence().width() > 0);
    let (width, height, pixels) = decode_png(rendered.png());
    assert_eq!((width, height), (cols * CELL_W, CELL_H));
    assert_eq!(
        (rendered.evidence().width(), rendered.evidence().height()),
        (width, height)
    );
    (width, height, pixels)
}

/// U05-A: ASCII text renders through the verbatim path with ink in its
/// cell and no shaped-grapheme evidence.
#[test]
fn u05_a_ascii_renders() {
    assert_eq!(E_ACUTE.chars().count(), 2);
    assert_eq!(A_DIAERESIS.chars().count(), 2);
    let set = corpus_set();
    let (width, _, pixels) = render_row(vec![plain("a", 1), blank(), blank()], &set);
    let ink = cell_ink(&pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
    assert!(!ink.is_empty(), "ASCII cell holds ink");
    // Only the first cell holds text: all image ink is its ink.
    assert_eq!(ink.len(), ink_offsets(&pixels, width, BLACK).len());
}

/// U05-A: box-drawing characters render through the verbatim path.
#[test]
fn u05_a_box_drawing_renders() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        3,
        1,
        vec![plain("─", 1), plain("│", 1), plain("┌", 1)],
        hidden_cursor(),
    );
    let rendered =
        render_snapshot_png(&snapshot, &strict_options(&set)).expect("box drawing renders");
    assert!(rendered.evidence().shaped_graphemes().is_empty());
    assert!(!rendered.png().is_empty());
    let (width, _, pixels) = decode_png(rendered.png());
    for (index, _) in ["─", "│", "┌"].iter().enumerate() {
        let left = index as u32 * CELL_W;
        let ink = cell_ink(&pixels, width, BLACK, left, 0, CELL_W, CELL_H);
        assert!(!ink.is_empty(), "box cell {index} holds ink");
    }
}

/// U05-A: `e` plus U+0301 shapes with one face and draws its mark: the
/// image differs from the bare base and all ink stays in the cell.
#[test]
fn u05_a_e_acute_renders() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(2, 1, vec![plain(E_ACUTE, 1), blank()], hidden_cursor());
    let rendered =
        render_snapshot_png(&snapshot, &strict_options(&set)).expect("e plus acute renders");
    assert_eq!(
        rendered.evidence().shaped_graphemes(),
        &[E_ACUTE.to_string()]
    );
    let (width, _, pixels) = decode_png(rendered.png());
    let ink = cell_ink(&pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
    assert!(!ink.is_empty(), "combined cell holds ink");
    assert_eq!(
        ink.len(),
        ink_offsets(&pixels, width, BLACK).len(),
        "shaped ink stays in its cell"
    );
    // The mark is really drawn: dropping it would give the base image.
    let (_, _, base_pixels) = render_row(vec![plain("e", 1), blank()], &set);
    assert_ne!(pixels, base_pixels, "combining mark changes the image");
}

/// U05-A: `a` plus U+0308 in the italic slot shapes and keeps its ink in
/// the cell.
#[test]
fn u05_a_a_diaeresis_italic_renders() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        2,
        1,
        vec![cell(A_DIAERESIS, 1, Attrs::ITALIC), blank()],
        hidden_cursor(),
    );
    let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
        .expect("italic a plus diaeresis renders");
    assert_eq!(
        rendered.evidence().shaped_graphemes(),
        &[A_DIAERESIS.to_string()]
    );
    let (width, _, pixels) = decode_png(rendered.png());
    let ink = cell_ink(&pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
    assert!(!ink.is_empty(), "italic combined cell holds ink");
    assert_eq!(
        ink.len(),
        ink_offsets(&pixels, width, BLACK).len(),
        "shaped ink stays in its cell"
    );
    let base = snapshot_with_cells(
        2,
        1,
        vec![cell("a", 1, Attrs::ITALIC), blank()],
        hidden_cursor(),
    );
    let base = render_snapshot_png(&base, &strict_options(&set)).expect("italic base renders");
    let (_, _, base_pixels) = decode_png(base.png());
    assert_ne!(pixels, base_pixels, "combining mark changes the image");
}

/// U05-A: the two-column character 東 with its continuation cell renders
/// from the CJK face across both columns.
#[test]
fn u05_a_cjk_with_continuation_renders() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        3,
        1,
        vec![plain("東", 2), continuation(), blank()],
        hidden_cursor(),
    );
    let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
        .expect("CJK cell with continuation renders");
    assert!(rendered.evidence().shaped_graphemes().is_empty());
    let (width, _, pixels) = decode_png(rendered.png());
    let ink = cell_ink(&pixels, width, BLACK, 0, 0, 2 * CELL_W, CELL_H);
    assert!(!ink.is_empty(), "wide cell holds ink");
    assert_eq!(
        ink.len(),
        ink_offsets(&pixels, width, BLACK).len(),
        "wide ink stays in its two columns"
    );
}

/// U05-A: bold, italic, and bold-italic text shape in their style slots.
#[test]
fn u05_a_styles_render() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        3,
        1,
        vec![
            cell(E_ACUTE, 1, Attrs::BOLD),
            cell(A_DIAERESIS, 1, Attrs::ITALIC),
            cell("─", 1, Attrs::BOLD | Attrs::ITALIC),
        ],
        hidden_cursor(),
    );
    let rendered =
        render_snapshot_png(&snapshot, &strict_options(&set)).expect("styled cells render");
    assert_eq!(
        rendered.evidence().shaped_graphemes(),
        &[E_ACUTE.to_string(), A_DIAERESIS.to_string()]
    );
    let (width, _, pixels) = decode_png(rendered.png());
    for index in 0..3 {
        let ink = cell_ink(&pixels, width, BLACK, index * CELL_W, 0, CELL_W, CELL_H);
        assert!(!ink.is_empty(), "styled cell {index} holds ink");
    }
}

/// U05-A: a wide character at the last column boundary renders in its one
/// surviving column.
#[test]
fn u05_a_wide_at_last_column_renders() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        3,
        1,
        vec![blank(), blank(), plain("東", 1)],
        hidden_cursor(),
    );
    let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
        .expect("wide char at last column renders");
    let (width, height, pixels) = decode_png(rendered.png());
    assert_eq!((width, height), (3 * CELL_W, CELL_H));
    let ink = cell_ink(&pixels, width, BLACK, 2 * CELL_W, 0, CELL_W, CELL_H);
    assert!(!ink.is_empty(), "edge wide cell holds ink");
}

/// U05-A: a cursor over a combined cell and over a wide cell renders.
#[test]
fn u05_a_cursor_over_combined_and_wide_renders() {
    let set = corpus_set();
    for (cells, cursor) in [
        (
            vec![plain(E_ACUTE, 1), blank(), blank()],
            block_cursor_at(0, 0),
        ),
        (
            vec![plain("東", 2), continuation(), blank()],
            block_cursor_at(0, 0),
        ),
    ] {
        let snapshot = snapshot_with_cells(3, 1, cells, cursor);
        let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
            .expect("cursor over cell renders");
        assert!(rendered.evidence().cursor_drawn());
        assert!(!rendered.png().is_empty());
    }
}

/// U05-B: cell rendering and cursor repaint use equal shaping results.
/// The ink positions relative to the cell origin agree between the plain
/// cell and the block-cursor repaint; only the colors change.
#[test]
fn u05_b_cell_and_cursor_repaint_shape_equally() {
    let set = corpus_set();
    for (cells, cursor_col) in [
        (vec![plain(E_ACUTE, 1), blank(), blank()], 0),
        (vec![blank(), plain("東", 2), continuation()], 1),
    ] {
        let plain_snapshot = snapshot_with_cells(3, 1, cells.clone(), hidden_cursor());
        let plain_rendered =
            render_snapshot_png(&plain_snapshot, &strict_options(&set)).expect("plain renders");
        let (width, _, plain_pixels) = decode_png(plain_rendered.png());
        let cursor_snapshot = snapshot_with_cells(3, 1, cells, block_cursor_at(cursor_col, 0));
        let cursor_rendered = render_snapshot_png(&cursor_snapshot, &strict_options(&set))
            .expect("cursor repaint renders");
        let (_, _, cursor_pixels) = decode_png(cursor_rendered.png());
        let span = if cursor_col == 1 { 2 } else { 1 };
        let left = cursor_col * CELL_W;
        let plain_ink = cell_ink(&plain_pixels, width, BLACK, left, 0, span * CELL_W, CELL_H);
        let cursor_ink = cell_ink(
            &cursor_pixels,
            width,
            CURSOR_FILL,
            left,
            0,
            span * CELL_W,
            CELL_H,
        );
        assert!(!plain_ink.is_empty(), "plain cell holds ink");
        // Same coverage up to blend quantization: the repaint draws black
        // on the cursor fill while the plain cell draws gray on black, so
        // the faintest fringe pixels can round to the fill color and drop
        // out of the binary ink set. Every repaint pixel is a plain pixel,
        // and every plain pixel neighbors a repaint pixel.
        for pixel in &cursor_ink {
            assert!(
                plain_ink.contains(pixel),
                "repaint pixel {pixel:?} is a plain pixel"
            );
        }
        for pixel in &plain_ink {
            let (x, y) = *pixel;
            let near = cursor_ink
                .iter()
                .any(|(cx, cy)| cx.abs_diff(x) <= 1 && cy.abs_diff(y) <= 1);
            assert!(near, "plain pixel {pixel:?} neighbors a repaint pixel");
        }
        assert_eq!(
            plain_rendered.evidence().shaped_graphemes(),
            cursor_rendered.evidence().shaped_graphemes(),
            "both renders record the same shaping"
        );
    }
}

/// U05-C: wide-cell clipping paints only in the permitted region. With a
/// single non-space cell, every non-background pixel of the whole image
/// must lie inside that cell's rect.
#[test]
fn u05_c_wide_clipping_stays_in_permitted_region() {
    let set = corpus_set();
    // Wide char squeezed into the last column: only that column is permitted.
    let edge = snapshot_with_cells(
        3,
        1,
        vec![blank(), blank(), plain("東", 1)],
        hidden_cursor(),
    );
    let rendered =
        render_snapshot_png(&edge, &strict_options(&set)).expect("edge wide cell renders");
    let (width, _, pixels) = decode_png(rendered.png());
    let all = ink_offsets(&pixels, width, BLACK);
    assert!(!all.is_empty());
    for (x, y) in &all {
        assert!(
            *x >= 2 * CELL_W && *x < 3 * CELL_W && *y < CELL_H,
            "edge ink ({x}, {y}) stays in the last column"
        );
    }
    // Full wide cell with continuation: only its two columns are permitted.
    let full = snapshot_with_cells(
        3,
        1,
        vec![plain("東", 2), continuation(), blank()],
        hidden_cursor(),
    );
    let rendered =
        render_snapshot_png(&full, &strict_options(&set)).expect("full wide cell renders");
    let (width, _, pixels) = decode_png(rendered.png());
    let all = ink_offsets(&pixels, width, BLACK);
    assert!(!all.is_empty());
    for (x, y) in &all {
        assert!(
            *x < 2 * CELL_W && *y < CELL_H,
            "wide ink ({x}, {y}) stays in its two columns"
        );
    }
}

/// U05-D: missing glyphs cause an error; they never give a success
/// result. Covers a missing single scalar, a missing scalar inside a
/// combining sequence, and a sequence divided across faces.
#[test]
fn u05_d_missing_glyphs_fail() {
    let set = corpus_set();
    // 🦀 is in no pinned face.
    let crab = snapshot_with_cells(1, 1, vec![plain("🦀", 1)], hidden_cursor());
    let error = render_snapshot_png(&crab, &strict_options(&set)).expect_err("crab is missing");
    assert!(
        matches!(
            error,
            RenderError::PinnedFont(PinnedFontError::MissingGlyph {
                character: '🦀',
                ..
            })
        ),
        "crab fails as missing glyph, got {error}"
    );
    // Combining sequence with a scalar no face holds.
    let bad_sequence = snapshot_with_cells(1, 1, vec![plain("e\u{10FFFF}", 1)], hidden_cursor());
    let error = render_snapshot_png(&bad_sequence, &strict_options(&set))
        .expect_err("bad sequence is missing");
    assert!(
        matches!(
            error,
            RenderError::PinnedFont(PinnedFontError::MissingGlyph { .. })
        ),
        "bad sequence fails as missing glyph, got {error}"
    );
    // `a` lives in JetBrains Mono, 東 lives in the CJK face: no single
    // face covers the grapheme, so shaping must not divide it.
    let divided = snapshot_with_cells(
        3,
        1,
        vec![plain("a東", 3), blank(), blank()],
        hidden_cursor(),
    );
    let error = render_snapshot_png(&divided, &strict_options(&set))
        .expect_err("divided sequence is missing");
    assert!(
        matches!(
            error,
            RenderError::PinnedFont(PinnedFontError::MissingGlyph {
                character: '東',
                ..
            })
        ),
        "divided sequence names 東, got {error}"
    );
}

/// U05-E: single-scalar cells keep the verbatim path: ASCII, box drawing,
/// styled text, and CJK record no shaped graphemes and render
/// deterministically.
#[test]
fn u05_e_verbatim_cells_keep_recorded_behavior() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(
        4,
        1,
        vec![
            plain("a", 1),
            cell("─", 1, Attrs::BOLD | Attrs::ITALIC),
            plain("東", 2),
            continuation(),
        ],
        hidden_cursor(),
    );
    let options = strict_options(&set);
    let first = render_snapshot_png(&snapshot, &options).expect("verbatim renders");
    let second = render_snapshot_png(&snapshot, &options).expect("verbatim renders again");
    assert!(first.evidence().shaped_graphemes().is_empty());
    assert_eq!(decode_png(first.png()), decode_png(second.png()));
}

/// U05-F: each positive corpus case gives glyph evidence and an image:
/// the recorded shaped graphemes plus PNG bytes decoding to the evidence
/// dimensions.
#[test]
fn u05_f_positive_cases_give_glyph_evidence_and_image() {
    let set = corpus_set();
    let cases: Vec<(&str, Vec<SnapshotCell>, Vec<String>)> = vec![
        ("ascii", vec![plain("a", 1)], vec![]),
        ("box", vec![plain("─", 1)], vec![]),
        (
            "e-acute",
            vec![plain(E_ACUTE, 1)],
            vec![E_ACUTE.to_string()],
        ),
        (
            "a-diaeresis",
            vec![cell(A_DIAERESIS, 1, Attrs::ITALIC)],
            vec![A_DIAERESIS.to_string()],
        ),
        ("cjk", vec![plain("東", 2), continuation()], vec![]),
        (
            "bold-combined",
            vec![cell(E_ACUTE, 1, Attrs::BOLD)],
            vec![E_ACUTE.to_string()],
        ),
    ];
    for (name, cells, expected) in cases {
        let cols = cells.len() as u32;
        let snapshot = snapshot_with_cells(cols, 1, cells, hidden_cursor());
        let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
            .unwrap_or_else(|error| panic!("positive case {name} renders: {error}"));
        assert_eq!(
            rendered.evidence().shaped_graphemes(),
            &expected,
            "positive case {name} records glyph evidence"
        );
        assert!(
            !rendered.png().is_empty(),
            "positive case {name} gives an image"
        );
        let (width, height, _) = decode_png(rendered.png());
        assert_eq!(
            (rendered.evidence().width(), rendered.evidence().height()),
            (width, height),
            "positive case {name} image matches its evidence"
        );
    }
}

/// U05-G: the named negative missing-glyph case returns an error without
/// an image success result.
#[test]
fn u05_g_missing_glyph_errors_without_image_success() {
    let set = corpus_set();
    let snapshot = snapshot_with_cells(1, 1, vec![plain("🦀", 1)], hidden_cursor());
    let result = render_snapshot_png(&snapshot, &strict_options(&set));
    assert!(
        matches!(
            result,
            Err(RenderError::PinnedFont(
                PinnedFontError::MissingGlyph { .. }
            ))
        ),
        "crab never renders successfully"
    );
}

/// U05-H: a width-1 cell holding a space plus a combining mark renders a
/// non-blank image with all ink inside the cell. The space contributes no
/// outline but its advance: dropping the advance-only entry would leave the
/// pen at the cell origin, so the mark would land outside the clip and the
/// render would succeed blank. The mark sits over the space, so its ink is
/// right of the cell origin rather than at the broken left edge. The
/// two-mark cluster below cannot precompose (a space never merges with a
/// mark), so unlike `e` plus U+0301 it exercises the multi-glyph pen path.
#[test]
fn u05_h_advance_only_glyphs_keep_pen_position() {
    let set = corpus_set();
    for sequence in [" \u{301}", " \u{301}\u{308}"] {
        let snapshot =
            snapshot_with_cells(2, 1, vec![plain(sequence, 1), blank()], hidden_cursor());
        let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
            .unwrap_or_else(|error| panic!("{sequence:?} renders, got {error}"));
        assert_eq!(
            rendered.evidence().shaped_graphemes(),
            &[sequence.to_string()]
        );
        let (width, _, pixels) = decode_png(rendered.png());
        let ink = cell_ink(&pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
        assert!(!ink.is_empty(), "{sequence:?} draws ink instead of blank");
        assert_eq!(
            ink.len(),
            ink_offsets(&pixels, width, BLACK).len(),
            "{sequence:?} keeps all ink in its cell"
        );
        let min_x = ink.iter().map(|(x, _)| *x).min().expect("ink is non-empty");
        assert!(
            min_x >= 3,
            "{sequence:?} draws over the space (min_x {min_x}), not at the dropped-pen edge"
        );
    }
}

/// U05-H: cursor repaint of an advance-only cell draws the mark at the same
/// shaper position as the plain cell; only the colors change.
#[test]
fn u05_h_cursor_repaint_keeps_advance_only_positions() {
    let set = corpus_set();
    let sequence = " \u{301}";
    let plain_snapshot =
        snapshot_with_cells(2, 1, vec![plain(sequence, 1), blank()], hidden_cursor());
    let plain_rendered =
        render_snapshot_png(&plain_snapshot, &strict_options(&set)).expect("plain renders");
    let (width, _, plain_pixels) = decode_png(plain_rendered.png());
    let cursor_snapshot = snapshot_with_cells(
        2,
        1,
        vec![plain(sequence, 1), blank()],
        block_cursor_at(0, 0),
    );
    let cursor_rendered =
        render_snapshot_png(&cursor_snapshot, &strict_options(&set)).expect("cursor renders");
    assert!(cursor_rendered.evidence().cursor_drawn());
    let (_, _, cursor_pixels) = decode_png(cursor_rendered.png());
    let plain_ink = cell_ink(&plain_pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
    let cursor_ink = cell_ink(&cursor_pixels, width, CURSOR_FILL, 0, 0, CELL_W, CELL_H);
    assert!(!plain_ink.is_empty(), "plain advance-only cell holds ink");
    for pixel in &cursor_ink {
        assert!(
            plain_ink.contains(pixel),
            "repaint pixel {pixel:?} is a plain pixel"
        );
    }
    for pixel in &plain_ink {
        let (x, y) = *pixel;
        let near = cursor_ink
            .iter()
            .any(|(cx, cy)| cx.abs_diff(x) <= 1 && cy.abs_diff(y) <= 1);
        assert!(near, "plain pixel {pixel:?} neighbors a repaint pixel");
    }
}

/// U05-H: a pinned title containing an advance-only cluster shapes through
/// the same per-grapheme path as cells instead of failing or blanking.
#[test]
fn u05_h_pinned_title_shapes_advance_only_cluster() {
    let set = corpus_set();
    let titled = |title: Option<&str>| {
        TerminalSnapshot::try_new(SnapshotParts {
            cols: 12,
            rows: 1,
            scope: SnapshotScope::Viewport,
            viewport_origin: 0,
            cells: vec![
                plain("a", 1),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ],
            palette: resolved_palette(),
            default_foreground: Rgb::new(192, 192, 192),
            default_background: Rgb::new(0, 0, 0),
            default_cursor: Rgb::new(9, 9, 9),
            cursor: hidden_cursor(),
            title: title.map(str::to_string),
            metadata: SnapshotMetadata::default(),
        })
        .expect("titled snapshot is valid")
    };
    let options = strict_options(&set).with_title_bar(true);
    let marked =
        render_snapshot_png(&titled(Some("a \u{301}")), &options).expect("marked title renders");
    assert_eq!(
        marked.evidence().title().unwrap_or_default(),
        "a \u{301} - 12x1"
    );
    let (_, _, marked_pixels) = decode_png(marked.png());
    let unmarked =
        render_snapshot_png(&titled(Some("a ")), &options).expect("unmarked title renders");
    let (_, _, unmarked_pixels) = decode_png(unmarked.png());
    assert_ne!(
        marked_pixels, unmarked_pixels,
        "the title mark changes the image instead of blanking"
    );
}

/// U05-I: default-ignorables ride along in the shaping buffer. CGJ
/// (U+034F, no standalone cmap entry in the bundled face) and a variation
/// selector (U+FE00) shape with the base and draw exactly the base image;
/// the old cmap preflight rejected both as missing glyphs. A
/// genuinely-unsupported visible scalar next to an ignorable still fails.
#[test]
fn u05_i_default_ignorables_shape_without_coverage() {
    let set = corpus_set();
    let (_, _, base_pixels) = render_row(vec![plain("a", 1), blank()], &set);
    for sequence in ["a\u{34f}", "a\u{fe00}"] {
        assert_eq!(sequence.chars().count(), 2);
        let snapshot =
            snapshot_with_cells(2, 1, vec![plain(sequence, 1), blank()], hidden_cursor());
        let rendered = render_snapshot_png(&snapshot, &strict_options(&set))
            .unwrap_or_else(|error| panic!("{sequence:?} renders, got {error}"));
        assert_eq!(
            rendered.evidence().shaped_graphemes(),
            &[sequence.to_string()]
        );
        let (_, _, pixels) = decode_png(rendered.png());
        assert_eq!(
            pixels, base_pixels,
            "{sequence:?} draws exactly the base image"
        );
    }
    let bad = snapshot_with_cells(
        2,
        1,
        vec![plain("a\u{34f}\u{10FFFF}", 1), blank()],
        hidden_cursor(),
    );
    let error = render_snapshot_png(&bad, &strict_options(&set))
        .expect_err("unsupported scalar next to CGJ still fails");
    assert!(
        matches!(
            error,
            RenderError::PinnedFont(PinnedFontError::MissingGlyph {
                character: '\u{10FFFF}',
                ..
            })
        ),
        "unsupported scalar fails as missing glyph, got {error}"
    );
}

/// U05 synthetic fallback: with a regular-only face and synthetic styles
/// selected, bold-italic combining text shapes, block-cursor repaint of the
/// combined cell works, and a visible title renders. With synthetic styles
/// off, the same styled cell fails as a missing face.
#[test]
fn synthetic_style_fallback() {
    let regular_only = || {
        PinnedFontSet::new(vec![
            PinnedFace::new(regular_bytes(), 0, false, false).expect("bundled face parses")
        ])
        .expect("regular-only set builds")
    };
    let synthetic = regular_only().with_synthetic_style(true);

    // Bold-italic combining text shapes with the regular face.
    let styled = snapshot_with_cells(
        2,
        1,
        vec![cell(E_ACUTE, 1, Attrs::BOLD | Attrs::ITALIC), blank()],
        hidden_cursor(),
    );
    let rendered = render_snapshot_png(&styled, &strict_options(&synthetic))
        .expect("synthetic bold-italic combining cell renders");
    assert_eq!(
        rendered.evidence().shaped_graphemes(),
        &[E_ACUTE.to_string()]
    );
    let (width, _, pixels) = decode_png(rendered.png());
    let ink = cell_ink(&pixels, width, BLACK, 0, 0, CELL_W, CELL_H);
    assert!(!ink.is_empty(), "synthetic styled cell holds ink");
    assert_eq!(
        ink.len(),
        ink_offsets(&pixels, width, BLACK).len(),
        "synthetic styled ink stays in its cell"
    );

    // Block-cursor repaint of the combined cell renders too.
    let repainted = snapshot_with_cells(
        2,
        1,
        vec![cell(E_ACUTE, 1, Attrs::BOLD | Attrs::ITALIC), blank()],
        block_cursor_at(0, 0),
    );
    let cursor_rendered = render_snapshot_png(&repainted, &strict_options(&synthetic))
        .expect("synthetic cursor repaint renders");
    assert!(cursor_rendered.evidence().cursor_drawn());
    assert!(!cursor_rendered.png().is_empty());

    // A visible title renders: titles always use the bold slot, so the
    // regular-only set needs the same synthetic fallback.
    let titled = TerminalSnapshot::try_new(SnapshotParts {
        cols: 12,
        rows: 1,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells: (0..12).map(|_| blank()).collect(),
        palette: resolved_palette(),
        default_foreground: Rgb::new(192, 192, 192),
        default_background: Rgb::new(0, 0, 0),
        default_cursor: Rgb::new(9, 9, 9),
        cursor: hidden_cursor(),
        title: Some(E_ACUTE.to_string()),
        metadata: SnapshotMetadata::default(),
    })
    .expect("titled snapshot is valid");
    let titled_rendered =
        render_snapshot_png(&titled, &strict_options(&synthetic).with_title_bar(true))
            .expect("synthetic title renders");
    assert_eq!(
        titled_rendered.evidence().title().unwrap_or_default(),
        format!("{E_ACUTE} - 12x1").as_str()
    );
    assert!(!titled_rendered.png().is_empty());

    // Without synthetic styles the same styled cell is a missing face.
    let error = render_snapshot_png(&styled, &strict_options(&regular_only()))
        .expect_err("styled combining cell without synthetic fails");
    assert!(
        matches!(
            error,
            RenderError::PinnedFont(PinnedFontError::MissingFace {
                bold: true,
                italic: true
            })
        ),
        "unselected style fails as missing face, got {error}"
    );
}
