#[cfg(feature = "recording-font-jetbrains-mono-styles")]
use super::font::{FontSystem, GlyphKey};
use super::{FrameRenderer, GridRenderer, RgbaFrame, CANVAS_BACKGROUND, CANVAS_PADDING};
use crate::profile::Profile;
use crate::record::frames::Frame;
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
        cursor: None,
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
fn smaller_terminal_is_centered_on_the_recording_canvas() {
    let mut renderer = GridRenderer::new(4, 3);
    let mut content_cell = cell(" ", Attrs::empty());
    content_cell.bg = Some(Color::Rgb(1, 2, 3));
    let content = frame(vec![vec![content_cell; 2]]);
    let expected_background = content.render_state.resolve(None, false);
    let image = renderer.render(&content).unwrap();
    let (width, height) = image.dimensions();
    let (panel_width, panel_height) = crate::render::svg::pixel_size(2, 1);
    let origin_x = (width - panel_width) / 2;
    let origin_y = (height - panel_height) / 2;

    assert_eq!(CANVAS_BACKGROUND, crate::profile::Rgb::new(104, 103, 170));
    assert_eq!(
        pixel_at(&image, 0, 0),
        [
            CANVAS_BACKGROUND.r,
            CANVAS_BACKGROUND.g,
            CANVAS_BACKGROUND.b,
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
            origin_y + crate::render::svg::HEADER_H as u32 - 1
        ),
        [0, 0, 0, 255]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + (crate::render::svg::MARGIN_X + 5.0) as u32,
            origin_y + (crate::render::svg::HEADER_H / 2.0) as u32
        ),
        [105, 17, 10, 255]
    );
    assert_eq!(
        pixel_at(
            &image,
            origin_x + panel_width / 2,
            origin_y + crate::render::svg::HEADER_H as u32 + 1
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
                + (crate::render::svg::HEADER_H + crate::render::svg::CONTENT_PADDING_TOP) as u32
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
    let mut fonts = FontSystem::new();
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
    let x = (CANVAS_PADDING + super::super::svg::MARGIN_X as u32) as usize;
    let y = (CANVAS_PADDING
        + super::super::svg::HEADER_H as u32
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
