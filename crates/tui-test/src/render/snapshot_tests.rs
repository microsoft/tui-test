use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::api::RecordingFormat;
use crate::profile::Profile;
use crate::record::frames::Frame;
use crate::render::svg::RenderState;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::cell::{Attrs, Color, EmuCell, NamedColor, UnderlineStyle};

use super::encode;
use super::raster::GridRenderer;

const COLS: u16 = 28;
const ROWS: usize = 5;
const UPDATE_ENV: &str = "UPDATE_TUI_TEST_RENDER_SNAPSHOTS";

#[test]
fn single_frame_gif_and_png_renders_match_snapshots() {
    for case in cases() {
        for (format, extension) in [
            (RecordingFormat::Gif, "gif"),
            (RecordingFormat::Apng, "png"),
        ] {
            let output = temporary_output(case.name, extension);
            let frame = Frame {
                grid: case.grid.clone(),
                title: None,
                duration: Duration::from_millis(250),
                render_state: RenderState::capture(&AlacrittyEmu::new(
                    COLS,
                    ROWS as u16,
                    &Profile::default(),
                )),
                cursor: None,
            };
            let mut renderer = GridRenderer::new(COLS, ROWS);
            encode::encode(&output, format, &[frame], &mut renderer).unwrap();
            let actual = std::fs::read(&output).unwrap();
            std::fs::remove_file(output).unwrap();
            assert_snapshot(case.name, extension, &actual);
        }
    }
}

struct SnapshotCase {
    name: &'static str,
    grid: Vec<Vec<EmuCell>>,
}

fn cases() -> [SnapshotCase; 3] {
    [
        SnapshotCase {
            name: "regular",
            grid: regular_grid(),
        },
        SnapshotCase {
            name: "styles",
            grid: styles_grid(),
        },
        SnapshotCase {
            name: "nerd-fonts",
            grid: nerd_font_grid(),
        },
    ]
}

fn regular_grid() -> Vec<Vec<EmuCell>> {
    let mut grid = blank_grid();
    write_text(
        &mut grid,
        0,
        0,
        "tui-test render snapshot",
        CellStyle::default(),
    );
    write_text(
        &mut grid,
        1,
        0,
        "ANSI color palette",
        CellStyle {
            fg: Some(Color::Named(NamedColor::BrightCyan)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        2,
        0,
        "truecolor background",
        CellStyle {
            fg: Some(Color::Rgb(250, 245, 235)),
            bg: Some(Color::Rgb(96, 48, 128)),
            ..CellStyle::default()
        },
    );
    write_text(&mut grid, 3, 0, "0123456789 -> [] {}", CellStyle::default());
    write_text(
        &mut grid,
        4,
        0,
        "full glyphs: \u{03bb} \u{0416} \u{2260}",
        CellStyle::default(),
    );
    grid
}

fn styles_grid() -> Vec<Vec<EmuCell>> {
    let mut grid = blank_grid();
    write_text(&mut grid, 0, 0, "Regular", CellStyle::default());
    write_text(
        &mut grid,
        1,
        0,
        "Bold",
        CellStyle {
            attrs: Attrs::BOLD,
            fg: Some(Color::Named(NamedColor::BrightYellow)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        2,
        0,
        "Italic",
        CellStyle {
            attrs: Attrs::ITALIC,
            fg: Some(Color::Named(NamedColor::BrightGreen)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        3,
        0,
        "Bold Italic",
        CellStyle {
            attrs: Attrs::BOLD | Attrs::ITALIC,
            fg: Some(Color::Named(NamedColor::BrightMagenta)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        4,
        0,
        "Underline Strike",
        CellStyle {
            attrs: Attrs::STRIKE,
            fg: Some(Color::Rgb(220, 220, 220)),
            underline: UnderlineStyle::Single,
            underline_color: Some(Color::Rgb(90, 180, 255)),
            ..CellStyle::default()
        },
    );
    grid
}

fn nerd_font_grid() -> Vec<Vec<EmuCell>> {
    let mut grid = blank_grid();
    write_text(
        &mut grid,
        0,
        0,
        "Nerd Font:",
        CellStyle {
            fg: Some(Color::Named(NamedColor::BrightBlue)),
            ..CellStyle::default()
        },
    );
    set_cell(
        &mut grid,
        0,
        11,
        "\u{f115}",
        CellStyle {
            fg: Some(Color::Named(NamedColor::BrightYellow)),
            ..CellStyle::default()
        },
    );
    set_cell(
        &mut grid,
        0,
        13,
        "\u{e0b0}",
        CellStyle {
            fg: Some(Color::Rgb(80, 160, 255)),
            bg: Some(Color::Rgb(30, 60, 100)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        1,
        0,
        "\u{250c}\u{2500}\u{2500}\u{2500}\u{2510}  \u{2190} \u{2192}  \u{2588}\u{2593}\u{2592}\u{2591}",
        CellStyle::default(),
    );
    write_text(
        &mut grid,
        2,
        0,
        "private-use vector paths",
        CellStyle {
            attrs: Attrs::ITALIC,
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        3,
        0,
        "Powerline cell fill",
        CellStyle {
            attrs: Attrs::BOLD,
            fg: Some(Color::Rgb(180, 210, 255)),
            bg: Some(Color::Rgb(35, 45, 65)),
            ..CellStyle::default()
        },
    );
    write_text(
        &mut grid,
        4,
        0,
        "deterministic raster output",
        CellStyle::default(),
    );
    grid
}

#[derive(Clone, Copy, Default)]
struct CellStyle {
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: Attrs,
    underline: UnderlineStyle,
    underline_color: Option<Color>,
}

fn blank_grid() -> Vec<Vec<EmuCell>> {
    vec![vec![EmuCell::blank(); usize::from(COLS)]; ROWS]
}

fn write_text(grid: &mut [Vec<EmuCell>], row: usize, column: usize, text: &str, style: CellStyle) {
    for (offset, character) in text.chars().enumerate() {
        set_cell(grid, row, column + offset, &character.to_string(), style);
    }
}

fn set_cell(
    grid: &mut [Vec<EmuCell>],
    row: usize,
    column: usize,
    character: &str,
    style: CellStyle,
) {
    let cell = grid
        .get_mut(row)
        .and_then(|row| row.get_mut(column))
        .unwrap_or_else(|| panic!("snapshot cell {column},{row} exceeds {COLS}x{ROWS}"));
    *cell = EmuCell {
        ch: character.into(),
        fg: style.fg,
        bg: style.bg,
        attrs: style.attrs,
        underline: style.underline,
        underline_color: style.underline_color,
    };
}

fn assert_snapshot(name: &str, extension: &str, actual: &[u8]) {
    let expected_path = snapshot_dir().join(format!("{name}.{extension}"));
    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::create_dir_all(snapshot_dir()).unwrap();
        std::fs::write(expected_path, actual).unwrap();
        return;
    }

    let expected = std::fs::read(&expected_path).unwrap_or_else(|error| {
        panic!(
            "could not read render snapshot {}: {error}; set {UPDATE_ENV}=1 to create it",
            expected_path.display()
        )
    });
    let matches = if extension == "gif" {
        decode_gif(&expected) == decode_gif(actual)
    } else {
        expected == actual
    };
    if !matches {
        let actual_path = failure_dir().join(format!("{name}.actual.{extension}"));
        std::fs::create_dir_all(failure_dir()).unwrap();
        std::fs::write(&actual_path, actual).unwrap();
        panic!(
            "render snapshot {} changed (expected {} bytes, got {}); actual output: {}; \
             set {UPDATE_ENV}=1 to accept it",
            expected_path.display(),
            expected.len(),
            actual.len(),
            actual_path.display()
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DecodedGif {
    dimensions: (u16, u16),
    frames: Vec<DecodedGifFrame>,
}

#[derive(Debug, PartialEq, Eq)]
struct DecodedGifFrame {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    delay: u16,
    pixels: Vec<u8>,
}

fn decode_gif(bytes: &[u8]) -> DecodedGif {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(Cursor::new(bytes)).unwrap();
    let dimensions = (decoder.width(), decoder.height());
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        frames.push(DecodedGifFrame {
            left: frame.left,
            top: frame.top,
            width: frame.width,
            height: frame.height,
            delay: frame.delay,
            pixels: frame.buffer.to_vec(),
        });
    }
    DecodedGif { dimensions, frames }
}

fn snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("render-snapshots")
}

fn failure_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("render-snapshot-failures")
}

fn temporary_output(name: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tui-test-render-snapshot-{}-{name}.{extension}",
        std::process::id()
    ))
}
