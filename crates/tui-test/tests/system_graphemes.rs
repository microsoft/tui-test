//! U05-R13: mandatory combining sequences on the system font backend.
//!
//! The pinned path shapes every grapheme with one face; the ordinary
//! offline, live screenshot, and recording paths share that shaping through
//! the system catalog instead of scalar-count rejection. Each required
//! combining sequence (`e` plus U+0301, `a` plus U+0308) renders on all
//! three paths. Synthetic snapshots cover the offline path with no child,
//! PTY, session, or daemon; live content is produced by the child (`printf`
//! at startup or via submit, then `sleep`), so no PTY echo path can mangle
//! the combining bytes, and each live assertion polls to a known marker
//! before rendering.

#![cfg(feature = "recording-raster")]

use tui_test::profile::{Profile, Rgb};
use tui_test::terminal::cell::Attrs;
use tui_test::{
    SnapshotCell, SnapshotColor, SnapshotCursor, SnapshotCursorShape, SnapshotMetadata,
    SnapshotParts, SnapshotRenderOptions, SnapshotScope, SnapshotUnderline, TerminalSnapshot,
};

const E_ACUTE: &str = "e\u{301}";
const A_DIAERESIS: &str = "a\u{308}";

fn resolved_palette() -> [Rgb; 256] {
    let profile = Profile::default();
    std::array::from_fn(|index| profile.colors.rgb(index as u8))
}

fn styled_cell(text: &str, attrs: Attrs) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        1,
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

fn plain_cell(text: &str) -> SnapshotCell {
    styled_cell(text, Attrs::empty())
}

fn blank_cell() -> SnapshotCell {
    plain_cell(" ")
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

fn snapshot_of(cells: Vec<SnapshotCell>) -> TerminalSnapshot {
    let cols = cells.len() as u32;
    TerminalSnapshot::try_new(SnapshotParts {
        cols,
        rows: 1,
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
    })
    .expect("test snapshot is valid")
}

/// Ordinary offline rendering without pinned fonts: zero chrome, so the
/// image is exactly the grid and the first cell is its left half.
fn undecorated_options() -> SnapshotRenderOptions {
    use tui_test::render::style::Padding;
    SnapshotRenderOptions::default()
        .with_canvas_padding(Padding::Uniform(0))
        .with_content_padding(Padding::Uniform(0))
        .with_title_bar(false)
        .with_title_controls(false)
        .with_shadow(false)
        .with_border_width(0.0)
        .with_border_radius(0.0)
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

fn ink_offsets(pixels: &[u8], image_width: u32) -> Vec<(u32, u32)> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel != &&[0, 0, 0, 255])
        .map(|(index, _)| {
            let index = index as u32;
            (index % image_width, index / image_width)
        })
        .collect()
}

/// Ordinary offline render of one combining cell plus a blank: the mark is
/// really drawn (the image differs from the bare base) and all ink stays in
/// the combining cell's half of the grid.
fn check_ordinary_offline_combining(sequence: &str, base: &str, attrs: Attrs) {
    assert_eq!(sequence.chars().count(), 2);
    let snapshot = snapshot_of(vec![styled_cell(sequence, attrs), blank_cell()]);
    let rendered = tui_test::render_snapshot_png(&snapshot, &undecorated_options())
        .expect("ordinary offline renders the combining sequence");
    let (width, height, pixels) = decode_png(rendered.png());
    assert_eq!(width % 2, 0, "two grid columns split the image evenly");
    let cell_width = width / 2;
    let ink = ink_offsets(&pixels, width);
    assert!(!ink.is_empty(), "combined cell holds ink");
    assert!(
        ink.iter().all(|(x, _)| *x < cell_width),
        "shaped ink stays in its cell"
    );
    let base_snapshot = snapshot_of(vec![styled_cell(base, attrs), blank_cell()]);
    let base = tui_test::render_snapshot_png(&base_snapshot, &undecorated_options())
        .expect("ordinary offline renders the bare base");
    let (base_width, base_height, base_pixels) = decode_png(base.png());
    assert_eq!((base_width, base_height), (width, height));
    assert_ne!(pixels, base_pixels, "combining mark changes the image");
}

/// U05-R13 (offline): `e` plus U+0301 renders on the system backend.
#[test]
fn ordinary_offline_renders_e_acute() {
    check_ordinary_offline_combining(E_ACUTE, "e", Attrs::empty());
}

/// U05-R13 (offline): `a` plus U+0308 renders on the system backend in the
/// italic slot.
#[test]
fn ordinary_offline_renders_a_diaeresis_italic() {
    check_ordinary_offline_combining(A_DIAERESIS, "a", Attrs::ITALIC);
}

/// U05-R13 (offline): `a` plus CGJ renders on the system backend. CGJ
/// needs no standalone coverage on either backend: it rides along in the
/// shaping buffer and the base still draws.
#[test]
fn ordinary_offline_renders_cgj() {
    let sequence = "a\u{34f}";
    let snapshot = snapshot_of(vec![styled_cell(sequence, Attrs::empty()), blank_cell()]);
    let rendered = tui_test::render_snapshot_png(&snapshot, &undecorated_options())
        .expect("ordinary offline renders base plus CGJ");
    let (width, _, pixels) = decode_png(rendered.png());
    let cell_width = width / 2;
    let ink = ink_offsets(&pixels, width);
    assert!(!ink.is_empty(), "base plus CGJ holds ink");
    assert!(
        ink.iter().all(|(x, _)| *x < cell_width),
        "CGJ ink stays in its cell"
    );
}

#[cfg(unix)]
mod live_tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tui_test::{OpenOptions, Operation, OperationResult, RunOptions, Session};

    /// The shell prints `sequence` via octal escapes (portable across
    /// `/bin/sh` implementations: no `\x` hex assumption), then sleeps.
    fn run_options_with_printf(printf_arg: &str) -> RunOptions {
        let defaults = OpenOptions::default();
        RunOptions {
            backend: defaults.backend,
            program: "sh".to_string(),
            args: vec!["-c".to_string(), format!("printf '{printf_arg}'; sleep 30")],
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
        }
    }

    fn wait_for_marker(session: &Session, marker: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let text = match session.execute(Operation::Text { full: false }) {
                Ok(OperationResult::Text(text)) => text,
                other => panic!("text operation gave {other:?}"),
            };
            if text.contains(marker) {
                return text;
            }
            if Instant::now() >= deadline {
                panic!("timed out waiting for marker; got {text:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Live PNG screenshot of one combining sequence: the snapshot proves
    /// the terminal holds the two-scalar grapheme in one cell, and the
    /// screenshot operation (system-backend rasterizer) renders it to a
    /// non-blank image instead of failing with missing glyphs.
    fn check_live_screenshot_combining(name: &str, sequence: &str, printf_arg: &str) {
        let session = Session::new(format!("u05-r13-shot-{name}-{}", std::process::id()));
        session
            .run(run_options_with_printf(printf_arg))
            .expect("session starts");
        wait_for_marker(&session, sequence);
        let snapshot = session
            .capture_snapshot(SnapshotScope::Viewport)
            .expect("live capture works");
        assert!(
            snapshot.cells().iter().any(|cell| cell.text() == sequence),
            "one live cell holds the combining sequence"
        );
        let path =
            std::env::temp_dir().join(format!("u05-r13-shot-{name}-{}.png", std::process::id()));
        let result = session
            .execute(Operation::Screenshot {
                full: false,
                path: Some(path.to_string_lossy().into_owned()),
                zoom: None,
                background: None,
            })
            .expect("live screenshot works");
        session.close().expect("session closes");
        let OperationResult::Screenshot(tui_test::ScreenshotResult::Path(saved)) = result else {
            panic!("screenshot operation gave {result:?}");
        };
        assert_eq!(saved, path.to_string_lossy());
        let bytes = std::fs::read(&path).expect("live PNG reads back");
        std::fs::remove_file(&path).ok();
        let (width, height, pixels) = decode_png(&bytes);
        assert!(width > 0 && height > 0);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != &[0, 0, 0, 255]),
            "live screenshot of the combining sequence is not blank"
        );
    }

    /// U05-R13 (live screenshot): `e` plus U+0301.
    #[test]
    fn live_screenshot_renders_e_acute() {
        check_live_screenshot_combining("e-acute", E_ACUTE, "e\\314\\201");
    }

    /// U05-R13 (live screenshot): `a` plus U+0308.
    #[test]
    fn live_screenshot_renders_a_diaeresis() {
        check_live_screenshot_combining("a-diaeresis", A_DIAERESIS, "a\\314\\210");
    }

    /// Recording export of one combining sequence: the submitted `printf`
    /// puts the two-scalar grapheme on the grid, and stopping the recording
    /// (system-backend rasterizer) exports an animated PNG instead of
    /// failing with missing glyphs.
    fn check_recording_combining(name: &str, sequence: &str, printf_arg: &str) {
        let session = Session::new(format!("u05-r13-rec-{name}-{}", std::process::id()));
        session.open(OpenOptions::default()).expect("open terminal");
        let path =
            std::env::temp_dir().join(format!("u05-r13-rec-{name}-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&path);
        session
            .execute(Operation::StartRecording {
                path: path.to_string_lossy().into_owned(),
                format: None,
                fps: Some(30),
                speed: Some(1.0),
                idle_time_limit: Some(5.0),
                zoom: Some(0.5),
                background: None,
            })
            .expect("start recording");
        session
            .execute(Operation::Submit {
                data: Some(format!("printf '{printf_arg}'")),
            })
            .expect("submit command");
        session
            .execute(Operation::WaitCommand {
                timeout_ms: Some(30_000),
            })
            .expect("wait for command");
        wait_for_marker(&session, sequence);
        let OperationResult::Recording(recorded) = session
            .execute(Operation::StopRecording)
            .expect("stop recording")
        else {
            panic!("unexpected recording result");
        };
        assert_eq!(recorded, path.to_string_lossy());
        session.close().expect("close terminal");
        let bytes = std::fs::read(&path).expect("read apng");
        std::fs::remove_file(&path).ok();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(
            bytes.windows(4).any(|window| window == b"acTL"),
            "recording exports animation frames"
        );
    }

    /// U05-R13 (recording): `e` plus U+0301.
    #[test]
    fn recording_renders_e_acute() {
        check_recording_combining("e-acute", E_ACUTE, "e\\314\\201");
    }

    /// U05-R13 (recording): `a` plus U+0308.
    #[test]
    fn recording_renders_a_diaeresis() {
        check_recording_combining("a-diaeresis", A_DIAERESIS, "a\\314\\210");
    }
}
