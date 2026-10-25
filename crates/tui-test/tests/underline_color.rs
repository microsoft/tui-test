//! U11 acceptance: explicit underline colors in the shared cell paint.
//!
//! Live content is produced by the child (`printf` at startup, then `sleep`),
//! so no PTY echo path can mangle escape sequences. The cursor is hidden in
//! every underline test, so only underline bars can move pixels between
//! renders that are otherwise identical.

#[cfg(feature = "recording-raster")]
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

fn underlined_cell(
    text: &str,
    underline: SnapshotUnderline,
    underline_color: SnapshotColor,
) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        1,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        underline,
        underline_color,
        None,
        None,
    )
    .expect("test cell is valid")
}

fn snapshot_of(cells: Vec<SnapshotCell>, palette: [Rgb; 256]) -> TerminalSnapshot {
    let cols = cells.len() as u32;
    TerminalSnapshot::try_new(SnapshotParts {
        cols,
        rows: 1,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells,
        palette,
        default_foreground: Rgb::new(192, 192, 192),
        default_background: Rgb::new(0, 0, 0),
        default_cursor: Rgb::new(9, 9, 9),
        cursor: hidden_cursor(),
        title: None,
        metadata: SnapshotMetadata::default(),
    })
    .expect("test snapshot is valid")
}

#[cfg(feature = "recording-raster")]
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

/// U11-C (SVG half): a missing underline color keeps the previous
/// foreground-color behavior, with no extra decoration property.
///
/// An explicit color rides on `text-decoration-color`; without one the
/// document keeps the bare `text-decoration` output bytes.
#[test]
fn missing_underline_color_keeps_svg_without_decoration_color() {
    let plain = snapshot_of(
        vec![underlined_cell(
            "u",
            SnapshotUnderline::Single,
            SnapshotColor::Default,
        )],
        resolved_palette(),
    );
    let rendered = tui_test::render_snapshot_svg(&plain, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    assert!(rendered.svg().contains(r#"text-decoration="underline""#));
    assert!(
        !rendered.svg().contains("text-decoration-color"),
        "no explicit color means no color property: {}",
        rendered.svg()
    );

    let colored = snapshot_of(
        vec![underlined_cell(
            "u",
            SnapshotUnderline::Single,
            SnapshotColor::Rgb(255, 0, 0),
        )],
        resolved_palette(),
    );
    let rendered = tui_test::render_snapshot_svg(&colored, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    let expected = Rgb::new(255, 0, 0).to_hex();
    assert!(
        rendered
            .svg()
            .contains(&format!("text-decoration-color=\"{expected}\"")),
        "an explicit underline color is named in the SVG: {}",
        rendered.svg()
    );
}

#[cfg(feature = "recording-raster")]
mod raster_tests {
    use super::*;
    use tui_test::render_snapshot_png;

    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(bytes);
        let mut reader = decoder.read_info().expect("PNG header decodes");
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).expect("PNG frame decodes");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        (info.width, info.height, pixels)
    }

    fn render(snapshot: &TerminalSnapshot) -> (u32, u32, Vec<u8>) {
        let rendered = render_snapshot_png(snapshot, &undecorated()).expect("offline PNG renders");
        decode_png(rendered.png())
    }

    /// U11-A: an RGB underline-color change changes the underline pixels.
    ///
    /// The two snapshots are identical except for the underline color, so
    /// any decoded difference must come from the underline bars.
    #[test]
    fn rgb_underline_color_change_alters_underline_pixels() {
        let red = snapshot_of(
            vec![
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(255, 0, 0),
                ),
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(255, 0, 0),
                ),
            ],
            resolved_palette(),
        );
        let blue = snapshot_of(
            vec![
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(0, 0, 255),
                ),
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(0, 0, 255),
                ),
            ],
            resolved_palette(),
        );
        assert_ne!(
            render(&red),
            render(&blue),
            "a different RGB underline color must repaint the underline"
        );
    }

    /// U11-B: an indexed underline color uses the captured palette.
    ///
    /// Moving the palette entry repaints the underline, and the indexed
    /// render decodes equal to the same color given as RGB.
    #[test]
    fn indexed_underline_color_uses_the_captured_palette() {
        let cells = || {
            vec![
                underlined_cell("u", SnapshotUnderline::Single, SnapshotColor::Indexed(1)),
                underlined_cell("u", SnapshotUnderline::Single, SnapshotColor::Indexed(1)),
            ]
        };
        let mut red_palette = resolved_palette();
        red_palette[1] = Rgb::new(255, 0, 0);
        let mut blue_palette = resolved_palette();
        blue_palette[1] = Rgb::new(0, 0, 255);
        let red = snapshot_of(cells(), red_palette);
        let blue = snapshot_of(cells(), blue_palette);
        assert_ne!(
            render(&red),
            render(&blue),
            "a moved palette entry must repaint the indexed underline"
        );

        let rgb = snapshot_of(
            vec![
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(255, 0, 0),
                ),
                underlined_cell(
                    "u",
                    SnapshotUnderline::Single,
                    SnapshotColor::Rgb(255, 0, 0),
                ),
            ],
            red_palette,
        );
        assert_eq!(
            render(&red),
            render(&rgb),
            "indexed underline colors resolve through the snapshot palette"
        );
    }

    /// U11-C: a missing underline color keeps the foreground-color behavior.
    ///
    /// A default underline color decodes pixel-identical to the same
    /// foreground color given explicitly.
    #[test]
    fn missing_underline_color_matches_explicit_foreground() {
        let foreground = Rgb::new(192, 192, 192);
        let missing = snapshot_of(
            vec![underlined_cell(
                "u",
                SnapshotUnderline::Single,
                SnapshotColor::Default,
            )],
            resolved_palette(),
        );
        let explicit = snapshot_of(
            vec![underlined_cell(
                "u",
                SnapshotUnderline::Single,
                SnapshotColor::Rgb(foreground.r, foreground.g, foreground.b),
            )],
            resolved_palette(),
        );
        assert_eq!(
            render(&missing),
            render(&explicit),
            "no underline color must paint exactly like the foreground color"
        );
    }

    /// U11-D: live and offline images use the same underline-color rule.
    ///
    /// The offline PNG of a live Viewport capture must decode to the same
    /// pixels as the live screenshot PNG: same frame conversion, same
    /// `GridRenderer` path at the same size.
    #[cfg(unix)]
    #[test]
    fn live_and_offline_share_the_underline_color_rule() {
        use tui_test::{OpenOptions, Operation, RunOptions, Session, SnapshotScope};

        let session = Session::new(format!("u11-{}", std::process::id()));
        let defaults = OpenOptions::default();
        session
            .run(RunOptions {
                backend: defaults.backend,
                program: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf '\\x1b[?25l\\x1b[4m\\x1b[58;2;255;0;0mU11D\\x1b[0m done'; sleep 30"
                        .to_string(),
                ],
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
            if text.contains("done") {
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

        let path = std::env::temp_dir().join(format!("u11-live-{}.png", std::process::id()));
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
}
