use super::font::{FontSystem, GlyphKey};
use super::{FrameRenderer, GridRenderer};
use crate::terminal::cell::{Attrs, Color, EmuCell, CONTINUATION};

fn cell(character: &str, attrs: Attrs) -> EmuCell {
    EmuCell {
        ch: character.into(),
        fg: Some(Color::Rgb(220, 220, 220)),
        attrs,
        ..EmuCell::blank()
    }
}

#[test]
fn repeated_renders_are_byte_identical() {
    let grid = vec![vec![cell("x", Attrs::empty())]];
    let mut renderer = GridRenderer::new(1, 1);
    let first = renderer.render(&grid, 1).unwrap();
    let second = renderer.render(&grid, 1).unwrap();
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
fn bold_and_italic_change_the_rasterized_glyph() {
    let mut renderer = GridRenderer::new(1, 1);
    let regular = renderer
        .render(&[vec![cell("M", Attrs::empty())]], 1)
        .unwrap();
    let bold = renderer.render(&[vec![cell("M", Attrs::BOLD)]], 1).unwrap();
    let italic = renderer
        .render(&[vec![cell("M", Attrs::ITALIC)]], 1)
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
        .render(&[vec![cell("é", Attrs::empty())]], 1)
        .unwrap();
    if let Err(error) = renderer.render(&[vec![cell("\u{10fffd}", Attrs::empty())]], 1) {
        assert!(error.to_string().contains("U+10FFFD"));
    }
}

#[test]
fn unsupported_emoji_sequences_are_reported_instead_of_misrendered() {
    let mut renderer = GridRenderer::new(2, 1);
    let error = renderer
        .render(&[vec![cell("👩‍💻", Attrs::empty())]], 2)
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
    if let Err(error) = renderer.render(&grid, 2) {
        assert!(error.to_string().contains("U+754C"));
    }
}
