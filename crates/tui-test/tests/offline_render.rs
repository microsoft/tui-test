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

/// U03-B: offline rendering starts no process, PTY, session, or daemon.
///
/// This test builds its snapshot from validated parts and renders it with no
/// session object anywhere in scope; success means no live machinery ran.
#[test]
fn offline_svg_renders_without_a_session() {
    let snapshot = snapshot_of(2, 1, &["a", " "]);
    let rendered = tui_test::render_snapshot_svg(&snapshot, &SnapshotRenderOptions::default())
        .expect("offline SVG renders");
    assert!(rendered.svg().contains(">a </text>"), "{}", rendered.svg());
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
}
