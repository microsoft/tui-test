//! F1: the public `EmuCell` shape stays compatible with the pre-`width` form.
//!
//! The recorded column width used to travel as a required public field,
//! which broke every downstream struct literal and destructure. Widths now
//! travel in private render data, so the complete old literal and the full
//! destructure below (no `..`) must keep compiling with unchanged types.

use std::sync::Arc;

use tui_test::terminal::cell::{Attrs, Color, EmuCell, Hyperlink, UnderlineStyle};

/// The complete old literal plus the full destructure, with no `..`.
///
/// Any added, removed, or retyped field breaks this at compile time, which
/// is the point: downstream code constructs and destructures the old shape.
#[test]
fn old_complete_emu_cell_shape_compiles() {
    let link = Arc::new(Hyperlink {
        id: None,
        uri: "https://example.com".into(),
    });
    let cell = EmuCell {
        ch: "x".into(),
        fg: Some(Color::Rgb(1, 2, 3)),
        bg: None,
        underline: UnderlineStyle::Single,
        underline_color: Some(Color::Idx(4)),
        attrs: Attrs::BOLD,
        hyperlink: Some(Arc::clone(&link)),
    };
    let EmuCell {
        ch,
        fg,
        bg,
        underline,
        underline_color,
        attrs,
        hyperlink,
    } = cell;
    assert_eq!(ch.as_str(), "x");
    assert_eq!(fg, Some(Color::Rgb(1, 2, 3)));
    assert_eq!(bg, None);
    assert_eq!(underline, UnderlineStyle::Single);
    assert_eq!(underline_color, Some(Color::Idx(4)));
    assert!(attrs.contains(Attrs::BOLD));
    assert_eq!(hyperlink.as_ref(), Some(&link));
    assert_eq!(EmuCell::blank(), EmuCell::default());
}
