//! Terminal profiles: the settings a session runs with.
//!
//! A profile is chosen when a session opens and fixed for its lifetime. It is
//! read from a TOML file so a project can commit the terminal its tests expect,
//! rather than depending on whatever the machine happens to default to.
//!
//! # Colors are resolved here, not by the emulator
//!
//! A terminal grid stores color *indices*, not colors: a cell painted with
//! `SGR 31` records palette slot 1, and what that looks like is the viewer's
//! choice. Nothing in the emulator needs a palette — xterm.js's `theme` option
//! is inert in a headless terminal, and alacritty has no palette at all.
//!
//! shell-use has to make that choice twice: once to draw a screenshot, and once
//! to answer `expect --fg "#rrggbb"`. Those answers have to agree. They used to
//! come from two separate hardcoded tables that disagreed on all sixteen ANSI
//! slots, so `expect --fg "#800000"` passed on a cell the screenshot painted
//! `#e88388`. [`Colors`] is the single table both now read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::terminal::cell::NamedColor;

/// Rows of scrollback a profile retains when it does not say otherwise.
///
/// The emulators do not agree on their own defaults (alacritty 10,000,
/// xterm.js 1,000), so this is always set explicitly rather than inherited.
pub const DEFAULT_SCROLLBACK: usize = 10_000;

/// The file a profile is read from, under the config directory.
pub const CONFIG_FILE: &str = "shell-use.toml";

/// The profile used when none is named.
pub const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Parse `#rgb` or `#rrggbb`. The leading `#` is optional so a TOML value
    /// that lost it to a stray quote still reads sensibly.
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s.trim().trim_start_matches('#');
        let read = |i: usize, n: usize| -> Result<u8, String> {
            u8::from_str_radix(&hex[i..i + n], 16)
                .map(|v| if n == 1 { v * 17 } else { v })
                .map_err(|_| format!("invalid hex color {s:?}"))
        };
        match hex.len() {
            3 => Ok(Rgb::new(read(0, 1)?, read(1, 1)?, read(2, 1)?)),
            6 => Ok(Rgb::new(read(0, 2)?, read(2, 2)?, read(4, 2)?)),
            _ => Err(format!("color must be #rgb or #rrggbb (got {s:?})")),
        }
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Rgb::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// The colors a session paints with.
///
/// Only the sixteen ANSI slots are configurable. Indices 16-255 are the xterm
/// color cube and gray ramp, which are defined by the spec rather than by a
/// theme, so [`Colors::rgb`] computes them instead of storing them. A config
/// that could override them would let two sessions disagree about what
/// `--fg 196` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    /// The color text takes when a cell set none of its own.
    pub foreground: Rgb,
    /// The color an unpainted cell takes.
    pub background: Rgb,
    /// Tracked for `OSC 12`; nothing draws a cursor yet.
    pub cursor: Rgb,

    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
    pub bright_black: Rgb,
    pub bright_red: Rgb,
    pub bright_green: Rgb,
    pub bright_yellow: Rgb,
    pub bright_blue: Rgb,
    pub bright_magenta: Rgb,
    pub bright_cyan: Rgb,
    pub bright_white: Rgb,
}

impl Default for Colors {
    /// The classic VGA/xterm palette, which is what `TERM=xterm-256color`
    /// promises and what the assertion layer already compared against.
    fn default() -> Self {
        Colors {
            foreground: Rgb::new(192, 192, 192),
            background: Rgb::new(0, 0, 0),
            cursor: Rgb::new(192, 192, 192),

            black: Rgb::new(0, 0, 0),
            red: Rgb::new(128, 0, 0),
            green: Rgb::new(0, 128, 0),
            yellow: Rgb::new(128, 128, 0),
            blue: Rgb::new(0, 0, 128),
            magenta: Rgb::new(128, 0, 128),
            cyan: Rgb::new(0, 128, 128),
            white: Rgb::new(192, 192, 192),
            bright_black: Rgb::new(128, 128, 128),
            bright_red: Rgb::new(255, 0, 0),
            bright_green: Rgb::new(0, 255, 0),
            bright_yellow: Rgb::new(255, 255, 0),
            bright_blue: Rgb::new(0, 0, 255),
            bright_magenta: Rgb::new(255, 0, 255),
            bright_cyan: Rgb::new(0, 255, 255),
            bright_white: Rgb::new(255, 255, 255),
        }
    }
}

impl Colors {
    /// The sixteen ANSI slots, in palette order.
    pub fn ansi(&self) -> [Rgb; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }

    /// The name a slot goes by in the config file.
    pub fn slot_name(index: u8) -> Option<&'static str> {
        Some(match NamedColor::from_index(index)? {
            NamedColor::Black => "black",
            NamedColor::Red => "red",
            NamedColor::Green => "green",
            NamedColor::Yellow => "yellow",
            NamedColor::Blue => "blue",
            NamedColor::Magenta => "magenta",
            NamedColor::Cyan => "cyan",
            NamedColor::White => "white",
            NamedColor::BrightBlack => "bright_black",
            NamedColor::BrightRed => "bright_red",
            NamedColor::BrightGreen => "bright_green",
            NamedColor::BrightYellow => "bright_yellow",
            NamedColor::BrightBlue => "bright_blue",
            NamedColor::BrightMagenta => "bright_magenta",
            NamedColor::BrightCyan => "bright_cyan",
            NamedColor::BrightWhite => "bright_white",
        })
    }

    /// Resolve any 256-color index.
    ///
    /// Slots 0-15 come from the profile; everything above comes from the
    /// xterm table, which no profile can move.
    pub fn rgb(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi()[index as usize],
            _ => xterm_color(index),
        }
    }
}

/// A color a program can address.
///
/// `OSC 4` names a palette entry and `OSC 10/11/12` name the three defaults.
/// Emulators number these however they like internally, so each backend
/// translates its own layout and that numbering never reaches here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSlot {
    Indexed(u8),
    Foreground,
    Background,
    Cursor,
}

/// The xterm 256-color table, which is the same in every terminal.
///
/// Slots 0-15 here are the classic VGA colors, and a profile overrides them.
/// The rest is the 6x6x6 color cube and the 24-step gray ramp, which the
/// specification fixes and no profile can move: `--fg 196` has to mean the
/// same thing in every session.
static XTERM_256: [Rgb; 256] = build_xterm_256();

const fn build_xterm_256() -> [Rgb; 256] {
    let mut table = [Rgb::new(0, 0, 0); 256];

    // 0-15: VGA.
    let vga = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let mut i = 0;
    while i < 16 {
        table[i] = Rgb::new(vga[i].0, vga[i].1, vga[i].2);
        i += 1;
    }

    // 16-231: a 6x6x6 cube whose levels step 0, 95, 135, 175, 215, 255.
    let levels = [0u8, 95, 135, 175, 215, 255];
    while i < 232 {
        let n = i - 16;
        table[i] = Rgb::new(levels[(n / 36) % 6], levels[(n / 6) % 6], levels[n % 6]);
        i += 1;
    }

    // 232-255: a gray ramp from 8 to 238 in steps of 10.
    while i < 256 {
        let v = (i - 232) as u8 * 10 + 8;
        table[i] = Rgb::new(v, v, v);
        i += 1;
    }
    table
}

/// The color a slot has when nothing has overridden it.
pub fn xterm_color(index: u8) -> Rgb {
    XTERM_256[index as usize]
}

/// The settings a session runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    /// Rows retained beyond the visible screen.
    pub scrollback: usize,
    pub colors: Colors,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            scrollback: DEFAULT_SCROLLBACK,
            colors: Colors::default(),
        }
    }
}

/// A parsed config file: named profiles, and nothing else.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigFile {
    pub profiles: BTreeMap<String, Profile>,
}

impl ConfigFile {
    pub fn parse(toml_text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(toml_text)?)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
    }

    /// The named profile, or the built-in defaults when nothing is named and
    /// the file defines no `default`.
    pub fn profile(&self, name: Option<&str>) -> anyhow::Result<Profile> {
        match name {
            Some(name) => self.profiles.get(name).copied().ok_or_else(|| {
                let known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
                if known.is_empty() {
                    anyhow::anyhow!("no profile {name:?}; the config file defines none")
                } else {
                    anyhow::anyhow!("no profile {name:?}; found: {}", known.join(", "))
                }
            }),
            None => Ok(self
                .profiles
                .get(DEFAULT_PROFILE)
                .copied()
                .unwrap_or_default()),
        }
    }
}

/// Where a config file is looked for, nearest first.
///
/// A project-local file wins so a repository can pin the terminal its tests
/// expect. `SHELL_USE_CONFIG` overrides both, which is also how a test suite
/// pins a config without depending on the working directory.
pub fn search_paths(cwd: &Path) -> Vec<PathBuf> {
    if let Ok(explicit) = std::env::var("SHELL_USE_CONFIG") {
        return vec![PathBuf::from(explicit)];
    }
    vec![
        cwd.join(CONFIG_FILE),
        crate::config::home_dir().join(CONFIG_FILE),
    ]
}

/// Resolve a profile: an explicit file if given, else the first file found on
/// the search path, else the built-in defaults.
///
/// A missing file is not an error — shell-use runs without one. A file that
/// exists but does not parse *is* an error, because silently ignoring it would
/// run the session with settings the user did not ask for.
pub fn resolve(
    explicit_config: Option<&Path>,
    profile_name: Option<&str>,
    cwd: &Path,
) -> anyhow::Result<Profile> {
    if let Some(path) = explicit_config {
        return ConfigFile::load(path)?.profile(profile_name);
    }
    for path in search_paths(cwd) {
        if path.is_file() {
            return ConfigFile::load(&path)?.profile(profile_name);
        }
    }
    match profile_name {
        Some(name) => anyhow::bail!("no profile {name:?}: no config file found"),
        None => Ok(Profile::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_round_trip() {
        for raw in ["#000000", "#ffffff", "#800000", "#c0c0c0"] {
            assert_eq!(Rgb::parse(raw).unwrap().to_hex(), raw);
        }
        assert_eq!(Rgb::parse("#f00").unwrap(), Rgb::new(255, 0, 0));
        assert_eq!(Rgb::parse("800000").unwrap(), Rgb::new(128, 0, 0));
    }

    #[test]
    fn a_bad_color_says_what_it_wanted() {
        for raw in ["", "#12", "#1234567", "nope", "#gggggg"] {
            let err = Rgb::parse(raw).unwrap_err();
            assert!(
                err.contains("color") || err.contains("hex"),
                "{raw:?}: {err}"
            );
        }
    }

    /// A profile that says nothing is the built-in default, so a config file is
    /// never required.
    #[test]
    fn an_empty_config_yields_the_defaults() {
        let cfg = ConfigFile::parse("").unwrap();
        assert_eq!(cfg.profile(None).unwrap(), Profile::default());
        assert_eq!(Profile::default().scrollback, 10_000);
    }

    /// Every field is individually optional, so a profile can set one color
    /// without restating the palette.
    #[test]
    fn a_partial_profile_keeps_the_other_defaults() {
        let cfg = ConfigFile::parse(
            r##"
            [profiles.ci]
            scrollback = 50

            [profiles.ci.colors]
            red = "#ff0000"
            "##,
        )
        .unwrap();
        let p = cfg.profile(Some("ci")).unwrap();
        assert_eq!(p.scrollback, 50);
        assert_eq!(p.colors.red, Rgb::new(255, 0, 0), "the override applies");
        assert_eq!(
            p.colors.green,
            Colors::default().green,
            "an unset slot keeps its default"
        );
        assert_eq!(
            p.colors.background,
            Colors::default().background,
            "an unset default color is untouched"
        );
    }

    #[test]
    fn an_unknown_profile_names_the_ones_that_exist() {
        let cfg = ConfigFile::parse("[profiles.ci]\n[profiles.demo]\n").unwrap();
        let err = cfg.profile(Some("nope")).unwrap_err().to_string();
        assert!(err.contains("ci") && err.contains("demo"), "{err}");
    }

    /// A typo in a key is an error rather than a setting that silently does
    /// nothing.
    #[test]
    fn an_unknown_key_is_rejected() {
        let err = ConfigFile::parse("[profiles.ci]\nscrollbacks = 10\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("scrollbacks"), "{err}");
    }

    /// Above the sixteen configurable slots the palette is spec, not
    /// preference, so profiles cannot disagree about what `--fg 196` means.
    #[test]
    fn the_color_cube_ignores_the_profile() {
        let recolored = Colors {
            red: Rgb::new(1, 2, 3),
            ..Default::default()
        };
        for n in 16u8..=255 {
            assert_eq!(recolored.rgb(n), Colors::default().rgb(n), "index {n}");
        }
        assert_eq!(Colors::default().rgb(196), Rgb::new(255, 0, 0));
        assert_eq!(Colors::default().rgb(232), Rgb::new(8, 8, 8));
        assert_eq!(recolored.rgb(1), Rgb::new(1, 2, 3), "but slot 1 follows it");
    }

    /// Every configurable slot is reachable by the name the file uses, so the
    /// documented key set and the resolver cannot drift apart.
    #[test]
    fn every_ansi_slot_has_a_config_key() {
        for i in 0u8..16 {
            let name = Colors::slot_name(i).unwrap_or_else(|| panic!("slot {i} unnamed"));
            let toml = format!("[profiles.p.colors]\n{name} = \"#010203\"\n");
            let p = ConfigFile::parse(&toml)
                .unwrap()
                .profile(Some("p"))
                .unwrap();
            assert_eq!(
                p.colors.rgb(i),
                Rgb::new(1, 2, 3),
                "setting {name:?} must move slot {i}"
            );
        }
        assert_eq!(Colors::slot_name(16), None, "only 0-15 are configurable");
    }

    /// The 16 configurable slots come from the profile; the rest come from the
    /// xterm table, which is the same in every terminal.
    #[test]
    fn only_the_ansi_slots_follow_the_profile() {
        let recolored = Colors {
            red: Rgb::new(1, 2, 3),
            ..Default::default()
        };
        assert_eq!(recolored.rgb(1), Rgb::new(1, 2, 3), "slot 1 follows it");
        for index in 16u8..=255 {
            assert_eq!(
                recolored.rgb(index),
                xterm_color(index),
                "slot {index} is fixed by the specification"
            );
        }
    }

    /// Spot-check the static table against the values the specification
    /// defines, so a typo in 256 entries cannot pass unnoticed.
    #[test]
    fn the_xterm_table_matches_the_specification() {
        assert_eq!(xterm_color(0), Rgb::new(0, 0, 0), "VGA black");
        assert_eq!(xterm_color(1), Rgb::new(128, 0, 0), "VGA red");
        assert_eq!(xterm_color(15), Rgb::new(255, 255, 255), "VGA bright white");
        assert_eq!(
            xterm_color(16),
            Rgb::new(0, 0, 0),
            "the cube starts at black"
        );
        assert_eq!(xterm_color(196), Rgb::new(255, 0, 0), "cube red");
        assert_eq!(
            xterm_color(231),
            Rgb::new(255, 255, 255),
            "the cube ends white"
        );
        assert_eq!(xterm_color(232), Rgb::new(8, 8, 8), "the ramp starts at 8");
        assert_eq!(
            xterm_color(255),
            Rgb::new(238, 238, 238),
            "the ramp ends at 238"
        );
    }

    /// A project-local file wins over the user's, so a repository can pin the
    /// terminal its tests expect. `SHELL_USE_CONFIG` overrides both.
    #[test]
    fn the_search_order_puts_the_project_first() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let old = std::env::var_os("SHELL_USE_CONFIG");
        std::env::remove_var("SHELL_USE_CONFIG");
        let cwd = Path::new("/tmp/some-project");
        let result = std::panic::catch_unwind(|| {
            let paths = search_paths(cwd);
            assert_eq!(paths.len(), 2);
            assert_eq!(paths[0], cwd.join(CONFIG_FILE), "the project file is first");
            assert!(
                paths[1].ends_with(CONFIG_FILE) && paths[1] != paths[0],
                "the user file is second: {:?}",
                paths[1]
            );

            std::env::set_var("SHELL_USE_CONFIG", "/tmp/pinned.toml");
            let pinned = search_paths(cwd);
            assert_eq!(
                pinned,
                vec![PathBuf::from("/tmp/pinned.toml")],
                "an explicit config replaces the search entirely"
            );
        });
        std::env::remove_var("SHELL_USE_CONFIG");
        if let Some(value) = old {
            std::env::set_var("SHELL_USE_CONFIG", value);
        }
        result.unwrap();
    }

    /// Running without a config file is normal, so a missing one is not an
    /// error. A file that exists but does not parse is, because ignoring it
    /// would silently run with settings nobody asked for.
    #[test]
    fn a_missing_config_defaults_but_a_broken_one_fails() {
        let dir = std::env::temp_dir().join(format!("su-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let missing = dir.join("absent.toml");
        assert!(
            resolve(Some(&missing), None, &dir).is_err(),
            "named-but-absent is an error"
        );

        let broken = dir.join("broken.toml");
        std::fs::write(&broken, "[profiles.ci]\nscrollback = \"lots\"\n").unwrap();
        let err = resolve(Some(&broken), None, &dir).unwrap_err().to_string();
        assert!(
            err.contains("broken.toml"),
            "the error names the file: {err}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
