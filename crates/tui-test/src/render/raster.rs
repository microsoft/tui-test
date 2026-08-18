mod draw;
mod font;

#[cfg(test)]
mod tests {
    use super::draw::{
        draw_glyph, fill_circle, fill_rect, fill_rounded_rect, format_glyph_sequence,
        is_default_ignorable, unpremultiply, unsupported_grapheme,
    };
    use super::font::{FontSystem, GlyphKey};

    #[test]
    fn drawing_and_font_primitives_are_available() {
        let mut pixmap = tiny_skia::Pixmap::new(32, 32).unwrap();
        fill_rect(&mut pixmap, 0.0, 0.0, 8.0, 8.0, (255, 0, 0));
        fill_circle(&mut pixmap, 16.0, 8.0, 4.0, (0, 255, 0));
        fill_rounded_rect(&mut pixmap, 0.0, 16.0, 12.0, 12.0, 3.0, (0, 0, 255));

        let mut fonts = FontSystem::new();
        let glyph = fonts
            .resolve(GlyphKey {
                character: '\u{e0b0}',
                bold: false,
                italic: false,
            })
            .expect("bundled Nerd Font contains the Powerline separator");
        draw_glyph(
            &mut pixmap,
            glyph,
            16.0,
            16.0,
            12.0,
            12.0,
            28.0,
            (255, 255, 255),
            1.0,
        );

        assert!(pixmap.data().chunks_exact(4).any(|pixel| pixel[3] != 0));
        assert!(is_default_ignorable('\u{fe0f}'));
        assert!(unsupported_grapheme("a\u{0301}"));
        assert_eq!(format_glyph_sequence("x"), "\"x\" (U+0078)");

        let mut pixel = [10, 20, 30, 0];
        unpremultiply(&mut pixel);
        assert_eq!(pixel, [0, 0, 0, 0]);
    }
}
