//! How a screenshot or recording is drawn.
//!
//! Every value here was a constant in [`crate::render::svg`], which the raster
//! path reads too, so one struct threaded through that renderer reaches SVG,
//! APNG, GIF and MP4 alike. The defaults reproduce those constants exactly:
//! rendering with [`Style::default`] has to produce the same bytes as before
//! this existed, and a test holds that.
//!
//! Configured under a profile's recording section:
//!
//! ```toml
//! [profiles.docs.recording.style]
//! font-size = 18
//! background = "#1d1f21"
//! padding = 32
//!
//! [profiles.docs.recording.style.font]
//! family = "Cascadia Code"
//! bold = "Cascadia Code Bold"
//!
//! [profiles.docs.recording.style.window]
//! title-bar = false
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::Rgb;

/// The font stack used when nothing names one.
///
/// A list rather than a single family because an SVG is read by whatever
/// renders it, which may have none of these installed; the raster path picks
/// the first it can actually load.
pub const DEFAULT_FONT_FAMILY: &str =
    "'Cascadia Code','JetBrains Mono','Fira Code',Menlo,Consolas,'DejaVu Sans Mono',monospace";

/// The grid font size that the cell geometry was originally drawn around.
const DEFAULT_FONT_SIZE: f32 = 17.0;
/// Cell width and height as a fraction of the font size.
///
/// The renderer used to hold 10x21 for a 17px font as three independent
/// constants, so changing the size alone tore the layout. Keeping them as
/// ratios means one knob moves the whole grid, and at 17px they multiply back
/// to exactly 10.0 and 21.0 in `f32`, so existing output is unchanged.
const CELL_W_RATIO: f32 = 10.0 / DEFAULT_FONT_SIZE;
const CELL_H_RATIO: f32 = 21.0 / DEFAULT_FONT_SIZE;

/// Font families, one per style the terminal can ask for.
///
/// A single family covers all four when a face carries its own bold and
/// italic. They are separate because many terminal fonts ship as siblings
/// rather than as one family with weights — Berkeley Mono and the Nerd Font
/// patches among them — and because a reader may want a different italic than
/// the one the family provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FontFamilies {
    /// The family for unstyled text, and the fallback for the other three.
    pub family: String,
    pub bold: Option<String>,
    pub italic: Option<String>,
    pub bold_italic: Option<String>,
    /// Extra font files or directories to load, on top of the system fonts and
    /// whatever `fonts/` directories tui-test finds.
    ///
    /// A relative path is resolved against the config file that named it, so a
    /// repository can carry its own fonts and a checkout renders the same
    /// wherever it sits. Naming a family that no loaded face provides is not
    /// an error: it falls back, exactly as an unavailable system font does.
    pub files: Vec<PathBuf>,
}

impl Default for FontFamilies {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT_FAMILY.to_string(),
            bold: None,
            italic: None,
            bold_italic: None,
            files: Vec::new(),
        }
    }
}

impl FontFamilies {
    /// The family to draw with, falling back to `family` for any variant that
    /// names none. Bold italic falls through bold then italic first, so
    /// setting only one of them still applies to the combination.
    pub fn resolve(&self, bold: bool, italic: bool) -> &str {
        let pick = match (bold, italic) {
            (true, true) => self
                .bold_italic
                .as_deref()
                .or(self.bold.as_deref())
                .or(self.italic.as_deref()),
            (true, false) => self.bold.as_deref(),
            (false, true) => self.italic.as_deref(),
            (false, false) => None,
        };
        pick.unwrap_or(&self.family)
    }

    /// Every family named, so a font loader knows which ones to look for.
    pub fn named(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.family.as_str()).chain(
            [
                self.bold.as_deref(),
                self.italic.as_deref(),
                self.bold_italic.as_deref(),
            ]
            .into_iter()
            .flatten(),
        )
    }

    /// Resolve relative `files` against the directory holding the config that
    /// named them, so a repository can carry its own fonts.
    pub fn resolve_paths(&mut self, config_dir: &Path) {
        for file in &mut self.files {
            if file.is_relative() {
                *file = config_dir.join(&*file);
            }
        }
    }
}

/// Directories tui-test looks in for fonts, nearest first.
///
/// The same places a config file is looked for, each with a `fonts/`
/// subdirectory: a project can carry fonts next to its `tui-test.toml`, and
/// `~/.tui-test/fonts` covers a font a user wants everywhere without
/// installing it system-wide. Missing directories are skipped, so none of
/// these has to exist.
pub fn font_search_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![cwd.join("fonts")];
    if std::env::var_os("TUI_TEST_HOME").is_none() {
        if let Some(config_home) = dirs::config_dir() {
            dirs.push(config_home.join("tui-test").join("fonts"));
        }
    }
    let home = crate::config::home_dir().join("fonts");
    if !dirs.contains(&home) {
        dirs.push(home);
    }
    dirs
}

/// The window chrome drawn around the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct WindowStyle {
    /// Draw the title bar. With it off the grid keeps its own margin but the
    /// bar, its divider and the traffic lights are all gone, and the panel is
    /// shorter by that much.
    pub title_bar: bool,
    /// Draw the three traffic lights. Ignored when the title bar is off,
    /// since they live on it.
    pub traffic_lights: bool,
    pub background: Rgb,
    pub foreground: Rgb,
    pub divider: Rgb,
}

impl Default for WindowStyle {
    fn default() -> Self {
        Self {
            title_bar: true,
            traffic_lights: true,
            background: Rgb::new(217, 217, 232),
            foreground: Rgb::new(65, 65, 69),
            divider: Rgb::new(0, 0, 0),
        }
    }
}

/// A border drawn around the terminal panel.
///
/// Off by default: the panel has never had one, and a zero width keeps it that
/// way rather than drawing a hairline nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct BorderStyle {
    pub width: f32,
    pub color: Rgb,
    /// Corner radius of the panel, which the border follows.
    pub radius: f32,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self {
            width: 0.0,
            color: Rgb::new(0, 0, 0),
            radius: 8.0,
        }
    }
}

/// The drop shadow under the panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ShadowStyle {
    pub enabled: bool,
    pub color: Rgb,
    /// Layers as `(spread, offset-y, alpha)`, drawn largest first.
    ///
    /// There is no blur: the softness comes from stacking rounded rectangles
    /// that each grow a little and fade a little, which every SVG renderer can
    /// draw and the raster path can reproduce exactly.
    pub layers: Vec<(f32, f32, u8)>,
}

impl Default for ShadowStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: Rgb::new(8, 8, 18),
            layers: vec![
                (7.0, 5.0, 18),
                (5.0, 4.0, 20),
                (3.0, 3.0, 22),
                (1.0, 2.0, 24),
            ],
        }
    }
}

/// Everything about how output is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Style {
    pub font: FontFamilies,
    /// Grid font size in pixels. Cell width and height follow it.
    pub font_size: f32,
    /// Title bar font size, smaller than the grid so the chrome does not
    /// compete with the terminal content.
    pub title_font_size: f32,
    /// The area around the panel.
    pub background: Rgb,
    pub padding: u32,
    pub window: WindowStyle,
    pub border: BorderStyle,
    pub shadow: ShadowStyle,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font: FontFamilies::default(),
            font_size: DEFAULT_FONT_SIZE,
            title_font_size: 13.0,
            background: Rgb::new(104, 103, 170),
            padding: 24,
            window: WindowStyle::default(),
            border: BorderStyle::default(),
            shadow: ShadowStyle::default(),
        }
    }
}

impl Style {
    pub fn cell_width(&self) -> f32 {
        self.font_size * CELL_W_RATIO
    }

    pub fn cell_height(&self) -> f32 {
        self.font_size * CELL_H_RATIO
    }

    /// Where a glyph sits inside its cell, measured from the cell's top.
    pub fn baseline(&self) -> f32 {
        (self.cell_height() - self.font_size) / 2.0 + self.font_size * 0.78
    }

    /// The height the title bar occupies, zero when it is not drawn.
    pub fn header_height(&self) -> f32 {
        if self.window.title_bar {
            34.0
        } else {
            0.0
        }
    }

    /// The height of the line under the title bar.
    pub fn divider_height(&self) -> f32 {
        if self.window.title_bar {
            1.0
        } else {
            0.0
        }
    }

    pub fn shadow_layers(&self) -> &[(f32, f32, u8)] {
        if self.shadow.enabled {
            &self.shadow.layers
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default has to reproduce the constants the renderer used to hold,
    /// or every existing screenshot and recording shifts.
    #[test]
    fn the_default_reproduces_the_original_geometry() {
        let style = Style::default();
        assert_eq!(style.cell_width(), 10.0);
        assert_eq!(style.cell_height(), 21.0);
        assert_eq!(style.font_size, 17.0);
        assert_eq!(style.baseline(), (21.0 - 17.0) / 2.0 + 17.0 * 0.78);
        assert_eq!(style.header_height(), 34.0);
        assert_eq!(style.divider_height(), 1.0);
        assert_eq!(style.padding, 24);
        assert_eq!(style.border.radius, 8.0);
        assert_eq!(style.border.width, 0.0, "no border was drawn before");
        assert_eq!(style.shadow_layers().len(), 4);
    }

    /// One knob moves the grid: the ratios are what keep a larger font from
    /// overflowing cells sized for the old one.
    #[test]
    fn cell_geometry_follows_the_font_size() {
        let style = Style {
            font_size: 34.0,
            ..Style::default()
        };
        assert_eq!(style.cell_width(), 20.0);
        assert_eq!(style.cell_height(), 42.0);
    }

    #[test]
    fn a_variant_font_falls_back_to_the_family() {
        let plain = FontFamilies::default();
        assert_eq!(plain.resolve(true, true), DEFAULT_FONT_FAMILY);

        let mixed = FontFamilies {
            family: "Regular".into(),
            bold: Some("Bold".into()),
            ..FontFamilies::default()
        };
        assert_eq!(mixed.resolve(false, false), "Regular");
        assert_eq!(mixed.resolve(true, false), "Bold");
        assert_eq!(mixed.resolve(false, true), "Regular");
        assert_eq!(
            mixed.resolve(true, true),
            "Bold",
            "bold italic falls through to bold before giving up on the variant"
        );
    }

    #[test]
    fn turning_the_title_bar_off_removes_its_height() {
        let style = Style {
            window: WindowStyle {
                title_bar: false,
                ..WindowStyle::default()
            },
            ..Style::default()
        };
        assert_eq!(style.header_height(), 0.0);
        assert_eq!(style.divider_height(), 0.0);
    }

    #[test]
    fn a_disabled_shadow_draws_no_layers() {
        let style = Style {
            shadow: ShadowStyle {
                enabled: false,
                ..ShadowStyle::default()
            },
            ..Style::default()
        };
        assert!(style.shadow_layers().is_empty());
    }

    #[test]
    fn the_config_spelling_is_kebab_case() {
        let style: Style = toml::from_str(
            "font-size = 20\npadding = 8\n\
             [font]\nfamily = \"Berkeley Mono\"\nbold-italic = \"Berkeley Mono Oblique\"\n\
             [window]\ntitle-bar = false\n\
             [border]\nwidth = 2\ncolor = \"#ff0000\"\n",
        )
        .unwrap();
        assert_eq!(style.font_size, 20.0);
        assert_eq!(style.font.family, "Berkeley Mono");
        assert_eq!(style.font.resolve(true, true), "Berkeley Mono Oblique");
        assert!(!style.window.title_bar);
        assert_eq!(style.border.width, 2.0);
        assert_eq!(style.border.color, Rgb::new(255, 0, 0));
        assert_eq!(
            style.background,
            Style::default().background,
            "anything unnamed keeps its default"
        );
    }

    #[test]
    fn font_files_resolve_against_the_config_that_named_them() {
        let mut font = FontFamilies {
            files: vec![
                PathBuf::from("fonts/Berkeley.ttf"),
                PathBuf::from("/opt/fonts/Absolute.ttf"),
            ],
            ..FontFamilies::default()
        };
        font.resolve_paths(Path::new("/repo/config"));
        assert_eq!(
            font.files[0],
            PathBuf::from("/repo/config/fonts/Berkeley.ttf"),
            "a repository carrying its own font renders the same wherever it sits"
        );
        assert_eq!(
            font.files[1],
            PathBuf::from("/opt/fonts/Absolute.ttf"),
            "an absolute path is left alone"
        );
    }

    #[test]
    fn every_named_family_is_reported_for_loading() {
        let font = FontFamilies {
            family: "Regular".into(),
            bold: Some("Bold".into()),
            bold_italic: Some("BoldItalic".into()),
            ..FontFamilies::default()
        };
        let named: Vec<&str> = font.named().collect();
        assert_eq!(named, ["Regular", "Bold", "BoldItalic"]);
    }

    /// The project directory comes first so a repository's own fonts win, and
    /// the tui-test home is always searched so a user font needs no install.
    #[test]
    fn fonts_are_searched_beside_the_config() {
        let dirs = font_search_dirs(Path::new("/repo"));
        assert_eq!(dirs[0], PathBuf::from("/repo/fonts"));
        assert!(
            dirs.iter().any(|dir| dir.ends_with("fonts")),
            "every entry is a fonts directory: {dirs:?}"
        );
        assert!(dirs.len() >= 2, "the home directory is always searched");
    }

    #[test]
    fn an_unknown_style_key_is_rejected() {
        let error = toml::from_str::<Style>("font_size = 20\n").unwrap_err();
        assert!(
            error.to_string().contains("font_size"),
            "the snake_case spelling is not silently ignored: {error}"
        );
    }
}
