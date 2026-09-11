//! How a screenshot or recording is drawn.
//!
//! Every value here was a constant in [`crate::render::svg`], which the raster
//! path reads too, so one struct threaded through that renderer reaches SVG,
//! APNG, GIF and MP4 alike. The defaults reproduce those constants exactly:
//! rendering with [`Style::default`] has to produce the same bytes as before
//! this existed, and a test holds that.
//!
//! Not yet reachable from `tui-test.toml`. The renderers read it, sessions
//! carry it, and it will be configured under a profile's recording section,
//! but nothing deserializes it into a session yet, so every render currently
//! uses [`Style::default`]. Keys are spelled as in the rest of the config
//! file — `font_size`, not `font-size` — matching how
//! [`crate::profile::Colors`] spells `bright_black`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::Rgb;

/// The font stack used when nothing names one.
///
/// A CSS-style list rather than one family because this is written straight
/// into the SVG's `font-family`, where the reader picks the first it has. The
/// raster path does not read it: that path resolves faces through its own
/// catalog, so a family named here moves an SVG and not yet a recording.
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
/// Ceilings that keep a config value from reaching arithmetic it would break.
/// Generous enough that no real recording comes near them.
const MAX_FONT_SIZE: f32 = 1_000.0;
const MAX_LENGTH: f32 = 10_000.0;
const MAX_PADDING: u32 = 10_000;

const CELL_W_RATIO: f32 = 10.0 / DEFAULT_FONT_SIZE;
const CELL_H_RATIO: f32 = 21.0 / DEFAULT_FONT_SIZE;

/// Font families, one per style the terminal can ask for.
///
/// A single family covers all four when a face carries its own bold and
/// italic. They are separate because many terminal fonts ship as siblings
/// rather than as one family with weights — Berkeley Mono and the Nerd Font
/// patches among them — and because a reader may want a different italic than
/// the one the family provides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

/// The window chrome drawn around the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

impl WindowStyle {
    /// The traffic lights to draw, empty when they are turned off or there is
    /// no title bar to hold them.
    pub fn traffic_lights(&self) -> &[Rgb] {
        const LIGHTS: [Rgb; 3] = [
            Rgb::new(236, 106, 94),
            Rgb::new(244, 191, 79),
            Rgb::new(97, 197, 84),
        ];
        if self.title_bar && self.traffic_lights {
            &LIGHTS
        } else {
            &[]
        }
    }
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
#[serde(default, deny_unknown_fields)]
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
///
/// `offset` moves it down, `spread` widens it. The softness comes from
/// stacking rounded rectangles that each grow a little and fade a little,
/// which every SVG renderer can draw and the raster path reproduces exactly.
/// That stack is derived rather than configured: it is a drawing trick, and
/// publishing it as config would freeze it into the file format forever.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowStyle {
    pub enabled: bool,
    pub color: Rgb,
    /// How far below the panel the shadow sits.
    pub offset: f32,
    /// How far beyond the panel's edges it reaches.
    pub spread: f32,
}

impl Default for ShadowStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: Rgb::new(8, 8, 18),
            offset: 5.0,
            spread: 7.0,
        }
    }
}

impl ShadowStyle {
    /// The stacked rectangles, as `(spread, offset, alpha)` drawn largest
    /// first.
    ///
    /// Four layers, each stepping two sevenths of the spread and a fifth of
    /// the offset inward while gaining alpha. Those fractions are what
    /// reproduce the original 7/5/3/1 by 5/4/3/2 stack exactly at the
    /// defaults.
    fn layers(&self) -> [(f32, f32, u8); 4] {
        let spread_step = self.spread * 2.0 / 7.0;
        let offset_step = self.offset / 5.0;
        std::array::from_fn(|index| {
            let step = index as f32;
            (
                self.spread - step * spread_step,
                self.offset - step * offset_step,
                18 + index as u8 * 2,
            )
        })
    }
}

/// Everything about how output is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    /// Reject a style that cannot be drawn.
    ///
    /// Follows the same shape as `resolve_zoom`: finite, positive, and
    /// bounded. Without it a config file reaches the renderers with values
    /// they cannot express — a non-finite size writes a literal `NaN` into the
    /// SVG, a zero or negative one collapses the grid to nothing, and an
    /// enormous padding overflows the canvas arithmetic. Each of those failed
    /// far from the config that caused it, or not at all.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("font_size", self.font_size),
            ("title_font_size", self.title_font_size),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and greater than zero"));
            }
            if value > MAX_FONT_SIZE {
                return Err(format!("{name} must not exceed {MAX_FONT_SIZE}"));
            }
        }
        for (name, value) in [
            ("border.width", self.border.width),
            ("border.radius", self.border.radius),
            ("shadow.offset", self.shadow.offset),
            ("shadow.spread", self.shadow.spread),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("{name} must be finite and not negative"));
            }
            if value > MAX_LENGTH {
                return Err(format!("{name} must not exceed {MAX_LENGTH}"));
            }
        }
        if self.padding > MAX_PADDING {
            return Err(format!("padding must not exceed {MAX_PADDING}"));
        }
        // A positive font size can still be too small to draw with: a
        // subnormal one leaves cells that round to nothing.
        if self.cell_width() < 1.0 || self.cell_height() < 1.0 {
            return Err("font_size is too small to draw a cell".to_string());
        }
        if self.font.family.trim().is_empty() {
            return Err("font.family must name a font".to_string());
        }
        for (name, family) in [
            ("font.bold", &self.font.bold),
            ("font.italic", &self.font.italic),
            ("font.bold_italic", &self.font.bold_italic),
        ] {
            if family.as_deref().is_some_and(|f| f.trim().is_empty()) {
                return Err(format!("{name} must name a font when it is set"));
            }
        }
        Ok(())
    }

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

    /// The shadow's stacked rectangles, empty when it is turned off.
    pub fn shadow_layers(&self) -> Vec<(f32, f32, u8)> {
        if self.shadow.enabled {
            self.shadow.layers().to_vec()
        } else {
            Vec::new()
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
    fn font_files_resolve_against_the_config_that_named_them() {
        // Built by joining rather than written out: a literal "/opt/x" is
        // relative on Windows, which has no drive letter for it, and a
        // literal expectation would compare separators byte for byte.
        let config_dir = std::env::temp_dir().join("config");
        let absolute = std::env::temp_dir().join("Absolute.ttf");
        assert!(absolute.is_absolute(), "the fixture is absolute everywhere");

        let mut font = FontFamilies {
            files: vec![
                PathBuf::from("fonts").join("Berkeley.ttf"),
                absolute.clone(),
            ],
            ..FontFamilies::default()
        };
        font.resolve_paths(&config_dir);
        assert_eq!(
            font.files[0],
            config_dir.join("fonts").join("Berkeley.ttf"),
            "a repository carrying its own font renders the same wherever it sits"
        );
        assert_eq!(font.files[1], absolute, "an absolute path is left alone");
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

    #[test]
    fn the_config_spelling_matches_the_rest_of_the_file() {
        // Colors spells bright_black, so Style spells font_size. One config
        // file should not carry two naming conventions.
        let style: Style = toml::from_str(
            "font_size = 20\npadding = 8\n\
             [font]\nfamily = \"Berkeley Mono\"\nbold_italic = \"Berkeley Mono Oblique\"\n\
             [window]\ntitle_bar = false\n\
             [border]\nwidth = 2\ncolor = \"#ff0000\"\n\
             [shadow]\nenabled = false\noffset = 2\n",
        )
        .unwrap();
        assert_eq!(style.font_size, 20.0);
        assert_eq!(style.font.resolve(true, true), "Berkeley Mono Oblique");
        assert!(!style.window.title_bar);
        assert_eq!(style.border.width, 2.0);
        assert!(!style.shadow.enabled);
        assert_eq!(style.shadow.offset, 2.0);
        assert_eq!(
            style.background,
            Style::default().background,
            "anything unnamed keeps its default"
        );
    }

    #[test]
    fn a_kebab_case_key_is_rejected() {
        let error = toml::from_str::<Style>("font-size = 20\n").unwrap_err();
        assert!(
            error.to_string().contains("font-size"),
            "the file's convention is snake_case: {error}"
        );
    }

    /// The shadow's layer stack is derived, and at the defaults it has to
    /// reproduce the four rectangles the renderer used to hold as a constant.
    #[test]
    fn the_default_shadow_reproduces_the_original_stack() {
        assert_eq!(
            Style::default().shadow_layers(),
            vec![
                (7.0, 5.0, 18),
                (5.0, 4.0, 20),
                (3.0, 3.0, 22),
                (1.0, 2.0, 24)
            ]
        );
    }

    #[test]
    fn a_wider_shadow_scales_every_layer() {
        let style = Style {
            shadow: ShadowStyle {
                spread: 14.0,
                offset: 10.0,
                ..ShadowStyle::default()
            },
            ..Style::default()
        };
        let layers = style.shadow_layers();
        assert_eq!(
            layers[0],
            (14.0, 10.0, 18),
            "the outermost follows the knobs"
        );
        assert_eq!(
            layers[3].0, 2.0,
            "and the innermost steps in proportionally"
        );
    }

    /// A config file is user input, so every value that reaches the renderers
    /// has to be one they can draw.
    #[test]
    fn a_style_that_cannot_be_drawn_is_rejected() {
        let bad = [
            ("font_size = nan", "font_size"),
            ("font_size = inf", "font_size"),
            ("font_size = 0", "font_size"),
            ("font_size = -17", "font_size"),
            ("font_size = 1e10", "font_size"),
            ("font_size = 1e-40", "too small"),
            ("title_font_size = 0", "title_font_size"),
            ("padding = 4294967295", "padding"),
            ("[border]\nwidth = -1", "border.width"),
            ("[border]\nradius = nan", "border.radius"),
            ("[shadow]\noffset = inf", "shadow.offset"),
            ("[shadow]\nspread = -3", "shadow.spread"),
            ("[font]\nfamily = \"\"", "font.family"),
            ("[font]\nbold = \"  \"", "font.bold"),
        ];
        for (source, expected) in bad {
            let style: Style = toml::from_str(source).expect("parses as TOML");
            let error = style
                .validate()
                .expect_err(&format!("{source:?} must be rejected"));
            assert!(
                error.contains(expected),
                "{source:?} should name {expected}, said {error:?}"
            );
        }
        Style::default()
            .validate()
            .expect("the default is drawable");
    }
}
