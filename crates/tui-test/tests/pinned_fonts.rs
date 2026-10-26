//! U04: pinned-font policy for strict offline rendering.
//!
//! The pinned policy loads only explicitly specified font bytes: no system
//! fonts and no ambient font-selection variables. These tests cover the
//! required U04 behaviors: host-font independence (U04-A, corpus C06),
//! digest sensitivity (U04-B), typed failures (U04-C), cache isolation
//! (U04-D), pixel determinism (U04-E), and unchanged system-font behavior
//! (U04-F). The font bytes come from the bundled JetBrains Mono assets read
//! as files, so system font configuration cannot leak into the inputs.

#![cfg(feature = "recording-raster")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tui_test::profile::{Profile, Rgb};
use tui_test::render::style::Padding;
use tui_test::terminal::cell::Attrs;
use tui_test::{
    render_snapshot_png, write_snapshot_png, CaptureBackground, CursorPhase, PinnedFace,
    PinnedFontError, PinnedFontSet, RenderError, SnapshotCell, SnapshotColor, SnapshotCursor,
    SnapshotCursorShape, SnapshotMetadata, SnapshotParts, SnapshotRenderOptions, SnapshotScope,
    SnapshotUnderline, TerminalSnapshot, RASTER_POLICY_PINNED_V1, RASTER_POLICY_SYSTEM_V1,
    RENDER_CONTRACT_VERSION,
};

fn regular_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-Regular.ttf").to_vec()
}

fn bold_bytes() -> Vec<u8> {
    include_bytes!("../assets/jetbrains-mono/JetBrainsMono-Bold.ttf").to_vec()
}

fn regular_face() -> PinnedFace {
    PinnedFace::new(regular_bytes(), 0, false, false).expect("bundled regular face parses")
}

fn bold_face() -> PinnedFace {
    PinnedFace::new(bold_bytes(), 0, true, false).expect("bundled bold face parses")
}

/// Regular plus bold: the pair every titled snapshot below needs, because
/// the title bar always draws in the bold slot.
fn titled_pair() -> PinnedFontSet {
    PinnedFontSet::new(vec![regular_face(), bold_face()]).expect("pinned pair builds")
}

fn resolved_palette() -> [Rgb; 256] {
    let profile = Profile::default();
    std::array::from_fn(|index| profile.colors.rgb(index as u8))
}

fn plain_cell(text: &str, width: u16) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        width,
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

fn bold_cell(text: &str) -> SnapshotCell {
    SnapshotCell::new(
        text.to_string(),
        1,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::BOLD,
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("test cell is valid")
}

fn shown_cursor() -> SnapshotCursor {
    SnapshotCursor::new(
        0,
        0,
        true,
        true,
        SnapshotCursorShape::Block,
        Rgb::new(9, 9, 9),
        Some(true),
    )
}

fn snapshot_with(cells: Vec<SnapshotCell>, title: Option<&str>) -> TerminalSnapshot {
    let cols = 3;
    let rows = 2;
    assert_eq!(cells.len(), (cols * rows) as usize);
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
        cursor: shown_cursor(),
        title: title.map(str::to_string),
        metadata: SnapshotMetadata::default(),
    })
    .expect("test snapshot is valid")
}

fn ascii_snapshot(title: Option<&str>) -> TerminalSnapshot {
    snapshot_with(
        vec![
            plain_cell("a", 1),
            plain_cell("b", 1),
            plain_cell("c", 1),
            plain_cell(" ", 1),
            plain_cell("x", 1),
            plain_cell("y", 1),
        ],
        title,
    )
}

fn strict_options(set: &PinnedFontSet) -> SnapshotRenderOptions {
    SnapshotRenderOptions::default()
        .with_canvas_padding(Padding::Uniform(0))
        .with_content_padding(Padding::Uniform(0))
        .with_title_bar(false)
        .with_title_controls(false)
        .with_shadow(false)
        .with_border_width(0.0)
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

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Process-wide environment changes stay inside one test at a time: a
/// system-font render running concurrently must never observe a rogue
/// value planted by the host-availability test.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    old: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.old.take() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tui-test-pinned-{name}-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("temp dir builds");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// U04-A (corpus C06): host font availability cannot change strict output.
///
/// The pinned render runs once, then again with the system font path
/// hidden behind an empty fontconfig directory and the ambient family
/// selection pointed at a bogus family — sandboxed to temp-local
/// directories and restored process environment. Both strict images must
/// decode to equal pixels with equal contracts.
#[test]
fn u04_a_pinned_output_survives_changed_host_fonts() {
    let _lock = ENV_LOCK.lock().expect("env lock is available");
    let snapshot = ascii_snapshot(Some("pinned"));
    let set = titled_pair();
    let options = strict_options(&set).with_title_bar(true);
    let baseline = render_snapshot_png(&snapshot, &options).expect("pinned baseline renders");

    let empty_fonts = TempDir::new("fontconfig-empty");
    let _fontconfig = EnvGuard::set(
        "FONTCONFIG_PATH",
        empty_fonts.path().to_str().expect("temp path is UTF-8"),
    );
    let _families = EnvGuard::set("TUI_TEST_RECORDING_FONT_FAMILIES", "NoSuchFamily-XYZ");

    // The rogue environment is real enough to run through: a system render
    // still succeeds while the pinned render must not move.
    let _system = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
        .expect("system render survives the rogue environment");
    let changed = render_snapshot_png(&snapshot, &options).expect("pinned render under rogue env");

    assert_eq!(
        decode_png(baseline.png()),
        decode_png(changed.png()),
        "host font availability must not change strict pinned pixels"
    );
    assert_eq!(baseline.contract(), changed.contract());
    assert_eq!(baseline.contract().raster_policy(), RASTER_POLICY_PINNED_V1);
}

/// U04-B: different font bytes change the reported digest.
///
/// A second face in the fallback order changes the set digest, the
/// contract, and the contract's per-face entries, while the same bytes
/// twice give the same digest and the same decoded pixels.
#[test]
fn u04_b_different_bytes_change_the_digest() {
    assert_ne!(
        regular_face().digest(),
        bold_face().digest(),
        "different bytes must digest differently"
    );
    let single = PinnedFontSet::new(vec![regular_face()]).expect("single-face set builds");
    let pair = titled_pair();
    assert_ne!(single.digest(), pair.digest());
    assert!(is_hex_digest(&single.digest()));
    assert!(is_hex_digest(&pair.digest()));

    let snapshot = ascii_snapshot(None);
    let first = render_snapshot_png(&snapshot, &strict_options(&single))
        .expect("single-face pinned render works without a title");
    let second = render_snapshot_png(&snapshot, &strict_options(&pair))
        .expect("two-face pinned render works");
    assert_ne!(
        first.contract().fonts()[0].digest(),
        second.contract().fonts()[1].digest()
    );
    assert_ne!(first.contract(), second.contract());

    let repeat = render_snapshot_png(&snapshot, &strict_options(&single))
        .expect("repeated pinned render works");
    assert_eq!(first.contract(), repeat.contract());
    assert_eq!(decode_png(first.png()), decode_png(repeat.png()));
}

/// U04-C: missing font, missing styled face, and missing glyph fail clearly.
///
/// Each failure keeps its type through the public render error: no host
/// fallback, no replacement glyph, and no image alongside the error.
#[test]
fn u04_c_missing_font_face_and_glyph_fail_clearly() {
    let empty = PinnedFontSet::new(Vec::new()).expect_err("an empty set must fail");
    assert_eq!(empty, PinnedFontError::EmptySet);
    assert!(
        empty.to_string().contains("never falls back to host fonts"),
        "unexpected message: {empty}"
    );

    // A regular-only set cannot draw the bold slot.
    let regular_only = PinnedFontSet::new(vec![regular_face()]).expect("regular set builds");
    let bold_snapshot = snapshot_with(
        vec![
            bold_cell("b"),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
        ],
        None,
    );
    let missing_face = render_snapshot_png(&bold_snapshot, &strict_options(&regular_only))
        .expect_err("a missing style face must fail");
    match missing_face {
        RenderError::PinnedFont(PinnedFontError::MissingFace { bold, italic }) => {
            assert!(bold);
            assert!(!italic);
        }
        other => panic!("missing face must stay typed, got: {other}"),
    }
    assert!(
        missing_face.to_string().contains("bold"),
        "the message must name the style: {missing_face}"
    );

    // The pinned JetBrains pair holds no crab emoji.
    let crab = snapshot_with(
        vec![
            plain_cell("\u{1F980}", 2),
            plain_cell("", 0),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
        ],
        None,
    );
    let missing_glyph = render_snapshot_png(&crab, &strict_options(&titled_pair()))
        .expect_err("a missing glyph must fail");
    match &missing_glyph {
        RenderError::PinnedFont(PinnedFontError::MissingGlyph { character, .. }) => {
            assert_eq!(*character, '\u{1F980}');
        }
        other => panic!("missing glyph must stay typed, got: {other}"),
    }
    let message = missing_glyph.to_string();
    assert!(message.contains("U+1F980"), "unexpected message: {message}");
    assert!(
        message.contains("no host font or replacement glyph is substituted"),
        "unexpected message: {message}"
    );
}

/// U04-D: previous system-font cache operations cannot change strict output.
///
/// Pinned renders surround system renders — including one at a different
/// scale that exercises the shared raster caches — and the strict pixels
/// and contracts on both sides must match.
#[test]
fn u04_d_system_cache_operations_cannot_change_strict_output() {
    let _lock = ENV_LOCK.lock().expect("env lock is available");
    let snapshot = ascii_snapshot(Some("pinned"));
    let set = titled_pair();
    let options = strict_options(&set).with_title_bar(true);

    let before = render_snapshot_png(&snapshot, &options).expect("first pinned render works");
    let system_before = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
        .expect("first system render works");
    let _scaled = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default().with_scale(2.0))
        .expect("scaled system render works");
    let after = render_snapshot_png(&snapshot, &options).expect("second pinned render works");
    let system_after = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
        .expect("second system render works");

    assert_eq!(
        decode_png(before.png()),
        decode_png(after.png()),
        "system cache operations must not change strict pinned pixels"
    );
    assert_eq!(before.contract(), after.contract());
    assert_eq!(
        decode_png(system_before.png()),
        decode_png(system_after.png()),
        "pinned renders must not change system output either"
    );
}

/// U04-E: equal pinned inputs give equal decoded pixels on each platform.
///
/// Two independent builds from the same bytes render the same snapshot to
/// equal decoded RGBA with equal contracts.
#[test]
fn u04_e_equal_pinned_inputs_give_equal_decoded_pixels() {
    let snapshot = ascii_snapshot(Some("pinned"));
    let first = render_snapshot_png(&snapshot, &strict_options(&titled_pair()))
        .expect("first pinned render works");
    let second = render_snapshot_png(&snapshot, &strict_options(&titled_pair()))
        .expect("second pinned render works");
    let (first_width, first_height, first_pixels) = decode_png(first.png());
    let (second_width, second_height, second_pixels) = decode_png(second.png());
    assert_eq!((first_width, first_height), (second_width, second_height));
    assert!(!first_pixels.is_empty());
    assert_eq!(first_pixels, second_pixels);
    assert_eq!(first.contract(), second.contract());
    assert_eq!(
        first.contract().cursor_phase(),
        CursorPhase::Shown,
        "a visible in-capture cursor uses the shown phase"
    );
    assert!(first.evidence().cursor_drawn());
    assert_eq!(first.contract().blink(), Some(true));
}

/// U04-F: existing font selection keeps its recorded behavior.
///
/// Default options select no pinned fonts, so the rasterizer keeps the
/// system-font path with a `system-v1` contract; the remaining evidence
/// comes from the untouched suites running green.
#[test]
fn u04_f_recorded_font_selection_is_unchanged() {
    let _lock = ENV_LOCK.lock().expect("env lock is available");
    assert!(
        SnapshotRenderOptions::default().pinned_fonts().is_none(),
        "pinned fonts stay opt-in"
    );
    let snapshot = ascii_snapshot(Some("recorded"));
    let rendered = render_snapshot_png(&snapshot, &SnapshotRenderOptions::default())
        .expect("system render keeps working");
    assert_eq!(rendered.contract().raster_policy(), RASTER_POLICY_SYSTEM_V1);
    assert!(
        rendered.contract().fonts().is_empty(),
        "system faces are host inputs, never hashed evidence"
    );
    assert_eq!(
        rendered.contract().contract_version(),
        RENDER_CONTRACT_VERSION
    );
    assert!(!decode_png(rendered.png()).2.is_empty());
}

/// U04-G (R5): the path entry point shares the byte entry point's pinned behavior.
///
/// A regular-only set cannot draw bold text: both `render_snapshot_png` and
/// `write_snapshot_png` must report the same typed `MissingFace` failure and
/// stage no PNG file. With a complete set both entry points succeed with
/// equal evidence and equal decoded pixels.
#[test]
fn u04_g_path_and_bytes_share_pinned_behavior() {
    let regular_only = PinnedFontSet::new(vec![regular_face()]).expect("regular set builds");
    let bold_snapshot = snapshot_with(
        vec![
            bold_cell("b"),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
            plain_cell(" ", 1),
        ],
        None,
    );
    let options = strict_options(&regular_only);
    let byte_err = render_snapshot_png(&bold_snapshot, &options)
        .expect_err("bytes entry point reports the missing bold face");
    assert!(
        matches!(
            byte_err,
            RenderError::PinnedFont(PinnedFontError::MissingFace {
                bold: true,
                italic: false
            })
        ),
        "bytes failure stays typed, got: {byte_err}"
    );
    let dir = TempDir::new("u04-g");
    let missing_path = dir.path().join("missing.png");
    let path_err = write_snapshot_png(&missing_path, &bold_snapshot, &options)
        .expect_err("path entry point reports the same missing bold face");
    assert!(
        matches!(
            path_err,
            RenderError::PinnedFont(PinnedFontError::MissingFace {
                bold: true,
                italic: false
            })
        ),
        "path failure stays typed, got: {path_err}"
    );
    assert!(
        !missing_path.exists(),
        "a pinned failure stages no PNG file"
    );

    let set = titled_pair();
    let snapshot = ascii_snapshot(Some("pinned"));
    let options = strict_options(&set);
    let rendered = render_snapshot_png(&snapshot, &options).expect("bytes render works");
    let ok_path = dir.path().join("ok.png");
    let evidence = write_snapshot_png(&ok_path, &snapshot, &options).expect("path render works");
    assert_eq!(rendered.evidence(), &evidence);
    let file_bytes = std::fs::read(&ok_path).expect("path render writes bytes");
    assert_eq!(
        decode_png(rendered.png()),
        decode_png(&file_bytes),
        "path and byte entry points decode to equal pixels"
    );
}

/// U04-H (R6): the returned contract covers every output-affecting option.
///
/// Each single-option change below changes the decoded image or the raster
/// policy inputs, so the contract must change with it. Canvas padding and
/// the synthetic-style selection previously left the contract equal while
/// the pixels moved.
#[test]
fn u04_h_contract_covers_every_output_setting() {
    let snapshot = ascii_snapshot(Some("pinned"));
    let base_options = strict_options(&titled_pair());
    let base = render_snapshot_png(&snapshot, &base_options).expect("baseline renders");
    let base_pixels = decode_png(base.png());
    let render_contract = |options: &SnapshotRenderOptions| {
        render_snapshot_png(&snapshot, options)
            .expect("option-variant pinned render works")
            .contract()
            .clone()
    };
    let changed = |label: &str, options: SnapshotRenderOptions| {
        let contract = render_contract(&options);
        assert_ne!(
            base.contract(),
            &contract,
            "{label} joins the render contract"
        );
        contract
    };

    let padded_options = base_options
        .clone()
        .with_canvas_padding(Padding::Uniform(12));
    let padded = render_snapshot_png(&snapshot, &padded_options).expect("padded renders");
    assert_ne!(
        decode_png(padded.png()),
        base_pixels,
        "canvas padding moves the decoded pixels"
    );
    assert_ne!(
        base.contract(),
        padded.contract(),
        "canvas padding joins the render contract"
    );

    changed(
        "content padding",
        base_options
            .clone()
            .with_content_padding(Padding::Uniform(4)),
    );
    changed(
        "capture background",
        base_options
            .clone()
            .with_background(Some(CaptureBackground::Transparent)),
    );
    changed("title bar", base_options.clone().with_title_bar(true));
    changed(
        "title controls",
        base_options.clone().with_title_controls(true),
    );
    changed("shadow", base_options.clone().with_shadow(true));
    changed("border width", base_options.clone().with_border_width(4.0));
    changed(
        "border radius",
        base_options.clone().with_border_radius(2.0),
    );
    changed("output scale", base_options.clone().with_scale(2.0));
    changed("cell width", base_options.clone().with_cell_width(12.0));
    changed("cell height", base_options.clone().with_cell_height(23.0));
    changed("font size", base_options.clone().with_font_size(15.0));
    changed("baseline", base_options.clone().with_baseline(12.0));

    let synthetic_set = titled_pair().with_synthetic_style(true);
    let synthetic_options = strict_options(&synthetic_set);
    let synthetic =
        render_snapshot_png(&snapshot, &synthetic_options).expect("synthetic render works");
    assert_ne!(
        base.contract(),
        synthetic.contract(),
        "synthetic-style selection joins the render contract"
    );
}
