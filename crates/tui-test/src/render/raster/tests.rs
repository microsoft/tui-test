#[cfg(feature = "recording-font-jetbrains-mono-styles")]
use super::font::{FontSystem, GlyphKey};
use super::{FrameRenderer, GridRenderer, RgbaFrame};
use crate::profile::Profile;
use crate::record::frames::Frame;
use crate::render::style::{CanvasPadding, Style};
use crate::render::svg::{RenderColors, RenderState};
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::cell::{Attrs, Color, EmuCell, CONTINUATION};
use crate::terminal::emu::Emulator;
use std::time::Duration;

fn cell(character: &str, attrs: Attrs) -> EmuCell {
    EmuCell {
        ch: character.into(),
        fg: Some(Color::Rgb(220, 220, 220)),
        attrs,
        ..EmuCell::blank()
    }
}

fn frame(grid: Vec<Vec<EmuCell>>) -> Frame {
    frame_with_cursor(grid, None)
}

fn frame_with_cursor(grid: Vec<Vec<EmuCell>>, cursor: Option<(u16, usize)>) -> Frame {
    let emulator = AlacrittyEmu::new(
        grid.first().map_or(1, Vec::len) as u16,
        grid.len().max(1) as u16,
        &Profile::default(),
    );
    Frame {
        grid,
        title: None,
        duration: Duration::ZERO,
        render_state: RenderState::capture(&emulator),
        cursor,
    }
}

#[test]
fn repeated_renders_are_byte_identical() {
    let frame = frame(vec![vec![cell("x", Attrs::empty())]]);
    let mut renderer = GridRenderer::new(1, 1);
    let first = renderer.render(&frame).unwrap();
    let second = renderer.render(&frame).unwrap();
    assert_eq!(first.as_raw(), second.as_raw());
    assert_eq!(renderer.pixel_size().1 % 2, 0);
}

#[test]
fn scaled_renderers_multiply_output_dimensions() {
    let standard = GridRenderer::new(80, 30);
    let hidpi = GridRenderer::with_scale(80, 30, 2);
    assert_eq!(
        hidpi.pixel_size(),
        (standard.pixel_size().0 * 2, standard.pixel_size().1 * 2)
    );
}

#[test]
fn fractional_zoom_shrinks_output_without_changing_grid_dimensions() {
    let standard = GridRenderer::new(80, 30);
    let half = GridRenderer::with_zoom(80, 30, 0.5, Style::default()).unwrap();
    assert_eq!(
        half.pixel_size(),
        (
            standard.pixel_size().0.div_ceil(2),
            standard.pixel_size().1.div_ceil(2)
        )
    );

    let mut half = half;
    half.render(&frame(vec![vec![EmuCell::blank(); 80]; 30]))
        .unwrap();
}

#[test]
fn adjacent_background_cells_are_seamless_at_fractional_zoom() {
    let backgrounds = [
        Color::Rgb(125, 86, 244),
        Color::Rgb(125, 86, 244),
        Color::Rgb(125, 86, 244),
        Color::Rgb(236, 106, 94),
        Color::Rgb(236, 106, 94),
        Color::Rgb(236, 106, 94),
        Color::Rgb(244, 191, 79),
        Color::Rgb(244, 191, 79),
        Color::Rgb(244, 191, 79),
        Color::Rgb(97, 197, 84),
        Color::Rgb(97, 197, 84),
        Color::Rgb(97, 197, 84),
    ];
    let rows = 5;
    let grid = vec![
        backgrounds
            .iter()
            .copied()
            .map(|background| EmuCell {
                bg: Some(background),
                ..EmuCell::blank()
            })
            .collect::<Vec<_>>();
        rows
    ];

    for zoom in [1.02, 1.25] {
        let mut renderer =
            GridRenderer::with_zoom(backgrounds.len() as u16, rows, zoom, Style::default())
                .unwrap();
        let image = renderer.render(&frame(grid.clone())).unwrap();
        let (panel_width, panel_height) =
            crate::render::svg::pixel_size(backgrounds.len() as u16, rows, &Style::default());
        let panel_width = super::scaled_dimension(panel_width, zoom, "test width").unwrap();
        let panel_height = super::scaled_dimension(panel_height, zoom, "test height").unwrap();
        let origin_x = (image.dimensions().0 - panel_width) as f32 / 2.0;
        let origin_y = (image.dimensions().1 - panel_height) as f32 / 2.0;
        let scale = zoom as f32;

        for row in 0..rows {
            let top = grid_y(origin_y, row, scale);
            let bottom = grid_y(origin_y, row + 1, scale);
            for (column, background) in backgrounds.iter().copied().enumerate() {
                let left = grid_x(origin_x, column, scale);
                let right = grid_x(origin_x, column + 1, scale);
                let expected = color_to_pixel(background);
                for y in top..bottom {
                    for x in left..right {
                        assert_eq!(
                            pixel_at(&image, x, y),
                            expected,
                            "unexpected pixel at cell ({column}, {row}), ({x}, {y}), zoom {zoom}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn block_cursor_is_aligned_with_background_cells_at_fractional_zoom() {
    let background = Color::Rgb(97, 197, 84);
    let mut row = vec![
        EmuCell {
            bg: Some(background),
            ..EmuCell::blank()
        };
        3
    ];
    row.push(EmuCell::blank());

    for zoom in [1.02, 1.25] {
        let content = frame_with_cursor(vec![row.clone()], Some((3, 0)));
        let cursor = content
            .render_state
            .color(crate::profile::ColorSlot::Cursor);
        let mut renderer = GridRenderer::with_zoom(4, 1, zoom, Style::default()).unwrap();
        let image = renderer.render(&content).unwrap();
        let (panel_width, panel_height) = crate::render::svg::pixel_size(4, 1, &Style::default());
        let panel_width = super::scaled_dimension(panel_width, zoom, "test width").unwrap();
        let panel_height = super::scaled_dimension(panel_height, zoom, "test height").unwrap();
        let origin_x = (image.dimensions().0 - panel_width) as f32 / 2.0;
        let origin_y = (image.dimensions().1 - panel_height) as f32 / 2.0;
        let scale = zoom as f32;
        let top = grid_y(origin_y, 0, scale);
        let bottom = grid_y(origin_y, 1, scale);
        let background_left = grid_x(origin_x, 2, scale);
        let cursor_left = grid_x(origin_x, 3, scale);
        let cursor_right = grid_x(origin_x, 4, scale);

        for y in top..bottom {
            for x in background_left..cursor_left {
                assert_eq!(
                    pixel_at(&image, x, y),
                    color_to_pixel(background),
                    "background gap before cursor at ({x}, {y}), zoom {zoom}"
                );
            }
            for x in cursor_left..cursor_right {
                assert_eq!(
                    pixel_at(&image, x, y),
                    [cursor.r, cursor.g, cursor.b, 255],
                    "misaligned cursor pixel at ({x}, {y}), zoom {zoom}"
                );
            }
        }
    }
}

#[test]
fn invalid_zoom_is_rejected() {
    for zoom in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(GridRenderer::with_zoom(1, 1, zoom, Style::default()).is_err());
    }
}

#[test]
fn smaller_terminal_is_centered_on_the_recording_canvas() {
    let mut renderer = GridRenderer::new(4, 3);
    let mut content_cell = cell(" ", Attrs::empty());
    content_cell.bg = Some(Color::Rgb(1, 2, 3));
    let content = frame(vec![vec![content_cell; 2]]);
    let expected_background = content.render_state.resolve(None, false);
    let image = renderer.render(&content).unwrap();
    let (width, height) = image.dimensions();
    let (panel_width, panel_height) = crate::render::svg::pixel_size(2, 1, &Style::default());
    let origin_x = (width - panel_width) / 2;
    let origin_y = (height - panel_height) / 2;

    assert_eq!(
        Style::default().canvas_background,
        crate::profile::Rgb::new(104, 103, 170)
    );
    assert_eq!(
        pixel_at(&image, 0, 0),
        [
            Style::default().canvas_background.r,
            Style::default().canvas_background.g,
            Style::default().canvas_background.b,
            255
        ]
    );
    assert_eq!(
        pixel_at(&image, origin_x + panel_width / 2, origin_y + 10),
        [217, 217, 232, 255]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + panel_width / 2,
            origin_y + Style::default().header_height() as u32 - 1
        ),
        [0, 0, 0, 255]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + (crate::render::svg::MARGIN_X + 5.0) as u32,
            origin_y + (Style::default().header_height() / 2.0) as u32
        ),
        [105, 17, 10, 255]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + panel_width / 2,
            origin_y + Style::default().header_height() as u32 + 1
        ),
        [
            expected_background.r,
            expected_background.g,
            expected_background.b,
            255
        ]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + panel_width / 2,
            origin_y
                + (Style::default().header_height() + crate::render::svg::CONTENT_PADDING_TOP)
                    as u32
                + 1
        ),
        [1, 2, 3, 255]
    );
    let canvas = pixel_at(&image, 0, 0);
    for shadow in [
        pixel_at(&image, origin_x - 1, origin_y + panel_height / 2),
        pixel_at(&image, origin_x + panel_width, origin_y + panel_height / 2),
    ] {
        assert!(
            shadow[..3]
                .iter()
                .zip(&canvas[..3])
                .all(|(shadow, canvas)| shadow < canvas),
            "the shadow darkens the canvas: {shadow:?} vs {canvas:?}"
        );
    }
}

#[test]
fn bold_and_italic_change_the_rasterized_glyph() {
    let mut renderer = GridRenderer::new(1, 1);
    let regular = renderer
        .render(&frame(vec![vec![cell("M", Attrs::empty())]]))
        .unwrap();
    let bold = renderer
        .render(&frame(vec![vec![cell("M", Attrs::BOLD)]]))
        .unwrap();
    let italic = renderer
        .render(&frame(vec![vec![cell("M", Attrs::ITALIC)]]))
        .unwrap();
    assert_ne!(regular.as_raw(), bold.as_raw());
    assert_ne!(regular.as_raw(), italic.as_raw());
}

#[cfg(feature = "recording-font-jetbrains-mono-styles")]
#[test]
fn bundled_styles_do_not_need_synthetic_bold_or_italic() {
    let mut fonts = FontSystem::new(&Style::default().font);
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let glyph = fonts
            .resolve(GlyphKey {
                character: 'M',
                bold,
                italic,
            })
            .unwrap();
        assert!(!glyph.synthetic_bold);
        assert!(!glyph.synthetic_italic);
    }
}

#[test]
fn supported_unicode_renders_and_missing_unicode_is_reported_when_absent() {
    let mut renderer = GridRenderer::new(1, 1);
    renderer
        .render(&frame(vec![vec![cell("é", Attrs::empty())]]))
        .unwrap();
    if let Err(error) = renderer.render(&frame(vec![vec![cell("\u{10fffd}", Attrs::empty())]])) {
        assert!(error.to_string().contains("U+10FFFD"));
    }
}

#[test]
fn unsupported_emoji_sequences_are_reported_instead_of_misrendered() {
    let mut renderer = GridRenderer::new(2, 1);
    let error = renderer
        .render(&frame(vec![vec![
            cell("👩‍💻", Attrs::empty()),
            EmuCell::blank(),
        ]]))
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("U+1F469"));
    assert!(message.contains("U+200D"));
    assert!(message.contains("U+1F4BB"));
}

#[test]
fn cjk_uses_a_system_fallback_or_reports_the_missing_glyph() {
    let mut renderer = GridRenderer::new(2, 1);
    let grid = vec![vec![
        cell("界", Attrs::empty()),
        cell(CONTINUATION, Attrs::empty()),
    ]];
    if let Err(error) = renderer.render(&frame(grid)) {
        assert!(error.to_string().contains("U+754C"));
    }
}

#[test]
fn frame_palette_and_cursor_state_change_the_pixels() {
    let mut first_emu = AlacrittyEmu::new(1, 1, &Profile::default());
    first_emu.process(b"\x1b]11;#010203\x07\x1b]12;#ff00ff\x07\x1b[6 q");
    let first = Frame {
        grid: vec![vec![EmuCell::blank()]],
        title: None,
        duration: Duration::ZERO,
        render_state: RenderState::capture(&first_emu),
        cursor: Some((0, 0)),
    };

    let mut second_emu = AlacrittyEmu::new(1, 1, &Profile::default());
    second_emu.process(b"\x1b]11;#070809\x07\x1b[?25l");
    let second = Frame {
        grid: vec![vec![EmuCell::blank()]],
        title: None,
        duration: Duration::ZERO,
        render_state: RenderState::capture(&second_emu),
        cursor: None,
    };

    let mut renderer = GridRenderer::new(1, 1);
    let first_pixels = renderer.render(&first).unwrap().into_raw();
    let second_pixels = renderer.render(&second).unwrap().into_raw();
    assert_ne!(first_pixels, second_pixels);

    let width = renderer.pixel_size().0 as usize;
    let x = (Style::default().canvas_padding.left() + super::super::svg::MARGIN_X as u32) as usize;
    let y = (Style::default().canvas_padding.top()
        + Style::default().header_height() as u32
        + super::super::svg::CONTENT_PADDING_TOP as u32) as usize;
    let cursor = (y * width + x) * 4;
    assert_eq!(&first_pixels[cursor..cursor + 3], &[255, 0, 255]);
}

#[test]
fn block_cursor_does_not_reveal_invisible_text() {
    let emulator = AlacrittyEmu::new(1, 1, &Profile::default());
    let render_state = RenderState::capture(&emulator);
    let hidden = Frame {
        grid: vec![vec![cell("M", Attrs::INVISIBLE)]],
        title: None,
        duration: Duration::ZERO,
        render_state: render_state.clone(),
        cursor: Some((0, 0)),
    };
    let blank = Frame {
        grid: vec![vec![EmuCell::blank()]],
        title: None,
        duration: Duration::ZERO,
        render_state,
        cursor: Some((0, 0)),
    };

    let mut renderer = GridRenderer::new(1, 1);
    let hidden_pixels = renderer.render(&hidden).unwrap().into_raw();
    let blank_pixels = renderer.render(&blank).unwrap().into_raw();
    assert_eq!(hidden_pixels, blank_pixels);
}

fn pixel_at(frame: &RgbaFrame, x: u32, y: u32) -> [u8; 4] {
    let (width, _) = frame.dimensions();
    let offset = ((y * width + x) * 4) as usize;
    frame.as_raw()[offset..offset + 4].try_into().unwrap()
}

fn color_to_pixel(color: Color) -> [u8; 4] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b, 255],
        _ => unreachable!("the test uses RGB colors"),
    }
}

fn grid_x(origin_x: f32, column: usize, scale: f32) -> u32 {
    (origin_x
        + (super::super::svg::MARGIN_X + column as f32 * Style::default().cell_width()) * scale)
        .round() as u32
}

fn grid_y(origin_y: f32, row: usize, scale: f32) -> u32 {
    (origin_y
        + (Style::default().header_height()
            + super::super::svg::CONTENT_PADDING_TOP
            + row as f32 * Style::default().cell_height())
            * scale)
        .round() as u32
}

/// Each axis can pass its own bound while the area is enormous. A grid this
/// size at the largest allowed font and padding is under u32::MAX on both
/// axes and still tens of gigabytes of pixmap, which the process would
/// otherwise only discover by touching the pages.
#[test]
fn an_enormous_canvas_is_refused_before_it_is_allocated() {
    let huge = Style {
        font_size: 999.0,
        canvas_padding: CanvasPadding::Uniform(10_000),
        ..Style::default()
    };
    let Err(error) = GridRenderer::with_zoom(500, 200, 1.0, huge) else {
        panic!("a canvas that large must not be allocated");
    };
    assert!(
        error.to_string().contains("pixels"),
        "the error says how big it was: {error}"
    );

    GridRenderer::with_zoom(
        120,
        40,
        2.0,
        Style {
            font_size: 40.0,
            ..Style::default()
        },
    )
    .map(|_| ())
    .expect("a genuinely large recording still renders");
}

/// A screenshot and a recording of the same terminal under the same config
/// have to describe the same canvas. They are not byte-identical in size:
/// `pixel_size` rounds each axis up to an even number because video encoders
/// demand it, and an SVG has no such constraint. That rounding is the only
/// licensed difference, so this pins it — the two renderers compute their
/// geometry separately, and nothing else would catch them drifting apart.
#[test]
fn both_renderers_agree_on_size_for_the_same_style() {
    use crate::render::style::WindowStyle;

    let cases = [
        ("default", Style::default()),
        (
            "no chrome",
            Style {
                window: WindowStyle {
                    title_bar: false,
                    ..WindowStyle::default()
                },
                ..Style::default()
            },
        ),
        (
            "padded",
            Style {
                canvas_padding: CanvasPadding::Uniform(40),
                ..Style::default()
            },
        ),
        (
            "large font",
            Style {
                font_size: 30.0,
                ..Style::default()
            },
        ),
    ];

    for (name, style) in cases {
        let rows = vec![vec![EmuCell::blank(); 8]; 3];
        let svg = crate::render::svg::render_svg(
            &rows,
            8,
            &Profile::default(),
            None,
            Some("t"),
            &style,
            1.0,
        );
        // The root dimensions can be fractional; the raster canvas is whole
        // pixels, so it takes the ceiling before rounding up to even.
        let attr = |key: &str| -> f64 {
            let at = svg.find(&format!("{key}=\"")).expect("dimension attribute");
            let rest = &svg[at + key.len() + 2..];
            rest[..rest.find('"').unwrap()].parse().expect("a number")
        };

        let renderer =
            GridRenderer::with_zoom(8, 3, 1.0, style).expect("the raster canvas is buildable");
        let even = |value: f64| {
            let whole = value.ceil() as u32;
            whole + whole % 2
        };
        assert_eq!(
            (even(attr("width")), even(attr("height"))),
            renderer.dimensions(),
            "{name}: the recording is the screenshot's canvas rounded up to even"
        );
    }
}

/// Every other raster test builds its renderer with `Style::default()`, so a
/// `GridRenderer` that ignored its style entirely would keep them all green.
#[test]
fn the_raster_canvas_is_drawn_from_its_style() {
    let style = Style {
        font_size: 34.0,
        canvas_padding: CanvasPadding::Uniform(40),
        canvas_background: crate::profile::Rgb::new(1, 2, 3),
        ..Style::default()
    };
    let mut renderer = GridRenderer::with_zoom(4, 2, 1.0, style.clone()).unwrap();
    let plain = GridRenderer::new(4, 2);

    let (panel_width, panel_height) = crate::render::svg::pixel_size(4, 2, &style);
    assert_eq!(
        renderer.dimensions(),
        (
            panel_width + style.canvas_padding.horizontal().unwrap(),
            panel_height + style.canvas_padding.vertical().unwrap()
        ),
        "the canvas is the styled panel plus the styled padding on every side"
    );
    assert_ne!(
        renderer.dimensions(),
        plain.dimensions(),
        "a larger font and padding grow the canvas"
    );

    let image = renderer
        .render(&frame(vec![vec![EmuCell::blank(); 4]; 2]))
        .unwrap();
    assert_eq!(
        pixel_at(&image, 1, 1),
        color_to_pixel(Color::Rgb(1, 2, 3)),
        "the configured background is painted into the padding"
    );
}

/// The border has to reach real pixels, not just the SVG text.
#[test]
fn a_border_is_stroked_onto_the_raster_canvas() {
    use crate::render::style::BorderStyle;

    let border = BorderStyle {
        width: 4.0,
        color: crate::profile::Rgb::new(255, 0, 0),
        radius: 0.0,
    };
    let style = Style {
        border,
        canvas_padding: CanvasPadding::Uniform(10),
        ..Style::default()
    };
    let mut renderer = GridRenderer::with_zoom(6, 2, 1.0, style.clone()).unwrap();
    let image = renderer
        .render(&frame(vec![vec![EmuCell::blank(); 6]; 2]))
        .unwrap();

    let (panel_width, _) = crate::render::svg::pixel_size(6, 2, &style);
    // The panel is centered, so its left edge sits one padding in. Two pixels
    // further is the middle of a four-wide stroke.
    let middle_of_stroke = (image.dimensions().0 - panel_width) / 2 + 2;
    assert_eq!(
        pixel_at(&image, middle_of_stroke, image.dimensions().1 / 2),
        color_to_pixel(Color::Rgb(255, 0, 0)),
        "the configured border color is painted along the panel edge"
    );

    let mut plain = GridRenderer::with_zoom(
        6,
        2,
        1.0,
        Style {
            canvas_padding: CanvasPadding::Uniform(10),
            ..Style::default()
        },
    )
    .unwrap();
    let unbordered = plain
        .render(&frame(vec![vec![EmuCell::blank(); 6]; 2]))
        .unwrap();
    assert_ne!(
        pixel_at(&unbordered, middle_of_stroke, unbordered.dimensions().1 / 2),
        color_to_pixel(Color::Rgb(255, 0, 0)),
        "and asking for no border leaves that edge alone"
    );
}

/// The raster path centres the window on its canvas, which is the same thing
/// as "at the gap" only while every gap is equal.
#[test]
fn the_raster_window_sits_at_its_own_gaps() {
    use crate::render::style::PaddingSides;

    let style = Style {
        canvas_padding: CanvasPadding::Sides(PaddingSides {
            top: 10,
            right: 20,
            bottom: 60,
            left: 30,
        }),
        canvas_background: crate::profile::Rgb::new(1, 2, 3),
        // Off, so a tinted pixel means the panel rather than its shadow.
        shadow: crate::render::style::ShadowStyle {
            enabled: false,
            ..crate::render::style::ShadowStyle::default()
        },
        ..Style::default()
    };
    let mut renderer = GridRenderer::with_zoom(6, 2, 1.0, style.clone()).unwrap();
    let (panel_width, panel_height) = crate::render::svg::pixel_size(6, 2, &style);

    assert_eq!(
        renderer.dimensions(),
        (panel_width + 30 + 20, panel_height + 10 + 60),
        "each axis grows by its own two gaps"
    );

    let image = renderer
        .render(&frame(vec![vec![EmuCell::blank(); 6]; 2]))
        .unwrap();
    let canvas = color_to_pixel(Color::Rgb(1, 2, 3));

    // One pixel inside the left gap is canvas; one pixel past it is the panel.
    assert_eq!(pixel_at(&image, 29, panel_height / 2 + 10), canvas);
    assert_ne!(
        pixel_at(&image, 31, panel_height / 2 + 10),
        canvas,
        "the window starts at the left gap, not at the midpoint of the canvas"
    );
    // The bottom gap is wider than the top, so the row below the panel is
    // still canvas while the matching row above it is too.
    assert_eq!(pixel_at(&image, panel_width / 2 + 30, 9), canvas);
    assert_eq!(
        pixel_at(&image, panel_width / 2 + 30, image.dimensions().1 - 2),
        canvas
    );
}
