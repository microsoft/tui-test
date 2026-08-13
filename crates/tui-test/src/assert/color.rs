//! Color parsing and comparison for `expect --fg/--bg`.

use super::super::terminal::cell::Color;
use crate::terminal::emu::Emulator;

/// The spelling of [`Expected::Default`], on the command line and in messages.
pub const DEFAULT: &str = "default";

#[derive(Debug, Clone)]
pub enum Expected {
    /// The terminal's default color, i.e. the cell set no color of its own.
    Default,
    Ansi256(u8),
    Hex(u8, u8, u8),
    Rgb(u8, u8, u8),
}

impl Expected {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case(DEFAULT) {
            return Ok(Expected::Default);
        }
        if let Some(hex) = s.strip_prefix('#') {
            let (r, g, b) = parse_hex(hex).map_err(|_| invalid(s))?;
            return Ok(Expected::Hex(r, g, b));
        }
        if s.contains(',') {
            let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
            let parsed: Result<Vec<u8>, _> = parts.iter().map(|p| p.parse::<u8>()).collect();
            match parsed.ok().as_deref() {
                Some([r, g, b]) => return Ok(Expected::Rgb(*r, *g, *b)),
                _ => return Err(invalid(s)),
            }
        }
        let n: u8 = s.parse().map_err(|_| invalid(s))?;
        Ok(Expected::Ansi256(n))
    }

    pub fn describe(&self) -> String {
        match self {
            Expected::Default => DEFAULT.to_string(),
            Expected::Ansi256(n) => n.to_string(),
            Expected::Hex(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Expected::Rgb(r, g, b) => format!("{r},{g},{b}"),
        }
    }
}

/// A consistent, enumerated error for any unparseable color value.
fn invalid(got: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "color must be \"{DEFAULT}\", ansi256 (0-255), hex (#rrggbb), or rgb (r,g,b) (got: \"{got}\")"
    )
}

fn parse_hex(hex: &str) -> anyhow::Result<(u8, u8, u8)> {
    if hex.len() != 6 {
        anyhow::bail!("hex color must be 6 digits");
    }
    let r = u8::from_str_radix(&hex[0..2], 16)?;
    let g = u8::from_str_radix(&hex[2..4], 16)?;
    let b = u8::from_str_radix(&hex[4..6], 16)?;
    Ok((r, g, b))
}

/// Does a cell's resolved color match the expected color?
///
/// A cell that set no color of its own matches `default`, and also matches the
/// concrete RGB value the terminal currently uses for that foreground or
/// background. It never matches an ANSI index because it did not select one.
///
/// A concrete `#rrggbb` is resolved through the session profile, the same table
/// the screenshot renderer draws with. These used to be two separate hardcoded
/// tables that disagreed on every ANSI slot, so `expect --fg "#800000"` passed
/// on a cell a screenshot painted `#e88388`.
pub fn matches(
    cell: Option<Color>,
    expected: &Expected,
    colors: &dyn Emulator,
    is_fg: bool,
) -> bool {
    match expected {
        Expected::Default => cell.is_none(),
        Expected::Ansi256(n) => cell.is_some_and(|cell| cell.to_index() == *n),
        Expected::Hex(er, eg, eb) | Expected::Rgb(er, eg, eb) => {
            let got = colors.resolve(cell, is_fg);
            (got.r, got.g, got.b) == (*er, *eg, *eb)
        }
    }
}

/// Render a cell's color in the same space as the expected value, for messages.
pub fn describe_cell(
    cell: Option<Color>,
    expected: &Expected,
    colors: &dyn Emulator,
    is_fg: bool,
) -> String {
    match expected {
        Expected::Default => cell
            .map(|cell| cell.to_index().to_string())
            .unwrap_or_else(|| DEFAULT.to_string()),
        Expected::Ansi256(_) => cell
            .map(|cell| cell.to_index().to_string())
            .unwrap_or_else(|| DEFAULT.to_string()),
        _ => colors.resolve(cell, is_fg).to_hex(),
    }
}

pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return (232 + ((r as i32 - 8) * 24 / 247)) as u8;
    }
    let cube = |v: u8| -> i32 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v as i32 - 35) / 40
        }
    };
    (16 + 36 * cube(r) + 6 * cube(g) + cube(b)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Colors, Profile};
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::Color;
    use crate::terminal::emu::Emulator;

    /// A real emulator, so these exercise the same resolution path a session
    /// uses rather than a stand-in that could drift from it.
    fn emu_with(colors: Colors) -> AlacrittyEmu {
        AlacrittyEmu::new(
            10,
            2,
            &Profile {
                colors,
                ..Default::default()
            },
        )
    }

    #[test]
    fn parse_forms() {
        assert!(matches!(
            Expected::parse("9").unwrap(),
            Expected::Ansi256(9)
        ));
        assert!(matches!(
            Expected::parse("#ff0000").unwrap(),
            Expected::Hex(255, 0, 0)
        ));
        assert!(matches!(
            Expected::parse("255,0,0").unwrap(),
            Expected::Rgb(255, 0, 0)
        ));
    }

    #[test]
    fn matches_palette_and_default() {
        let c = emu_with(Colors::default());
        let idx = |i| Some(Color::from_index(i));
        assert!(matches(idx(9), &Expected::Ansi256(9), &c, true));
        assert!(!matches(idx(2), &Expected::Ansi256(9), &c, true));
        assert!(matches(idx(196), &Expected::Ansi256(196), &c, true));
        assert!(matches(
            Some(Color::Rgb(255, 0, 0)),
            &Expected::Rgb(255, 0, 0),
            &c,
            true
        ));
    }

    #[test]
    fn default_color_matches_its_resolved_foreground_or_background() {
        let c = emu_with(Colors::default());
        assert!(!matches(None, &Expected::Ansi256(0), &c, true));
        assert!(matches(None, &Expected::Hex(192, 192, 192), &c, true));
        assert!(matches(None, &Expected::Hex(0, 0, 0), &c, false));
        assert!(!matches(None, &Expected::Hex(0, 0, 0), &c, true));
        assert!(matches(None, &Expected::Default, &c, true));
        assert_eq!(
            describe_cell(None, &Expected::Hex(0, 0, 0), &c, true),
            "#c0c0c0"
        );
    }

    #[test]
    fn default_color_assertions_follow_runtime_changes() {
        let mut c = emu_with(Colors::default());
        c.process(b"\x1b]10;#010203\x07\x1b]11;#040506\x07");
        assert!(matches(None, &Expected::Hex(1, 2, 3), &c, true));
        assert!(matches(None, &Expected::Hex(4, 5, 6), &c, false));
    }

    #[test]
    fn a_colored_cell_is_not_default() {
        let c = emu_with(Colors::default());
        let red = Some(Color::from_index(1));
        assert!(!matches(red, &Expected::Default, &c, true));
        assert!(matches(red, &Expected::Ansi256(1), &c, true));
        assert!(matches!(
            Expected::parse("default").unwrap(),
            Expected::Default
        ));
        assert!(matches!(
            Expected::parse("DEFAULT").unwrap(),
            Expected::Default
        ));
        assert_eq!(describe_cell(red, &Expected::Default, &c, true), "1");
    }

    /// The regression test for the bug this module used to carry: the color a
    /// screenshot paints and the color an assertion matches are now the same
    /// value for every slot, because both come from the profile.
    #[test]
    fn an_assertion_matches_the_color_a_screenshot_paints() {
        let colors = emu_with(Colors::default());
        for index in 0u8..=255 {
            let cell = Some(Color::from_index(index));
            let painted = colors.resolve(cell, true);
            assert!(
                matches(
                    cell,
                    &Expected::Hex(painted.r, painted.g, painted.b),
                    &colors,
                    true
                ),
                "slot {index} paints {} but does not match it",
                painted.to_hex()
            );
        }
    }

    /// An assertion compares against what the terminal is *currently*
    /// showing, so a program that recolors a slot changes what matches.
    ///
    /// This is the other half of the screenshot test: both read the same
    /// state, so a colour a screenshot paints is a colour an assertion
    /// matches, at every point in a session rather than only at the start.
    #[test]
    fn an_assertion_follows_a_color_a_program_set() {
        use crate::terminal::emu::Emulator;
        let mut emu = emu_with(Colors::default());
        let red = Some(Color::from_index(1));
        let configured = Colors::default().red;

        assert!(matches(
            red,
            &Expected::Hex(configured.r, configured.g, configured.b),
            &emu,
            true
        ));

        emu.process(b"\x1b]4;1;#22c55e\x07");
        assert!(
            matches(red, &Expected::Hex(0x22, 0xc5, 0x5e), &emu, true),
            "the assertion follows the colour the program set"
        );
        assert!(
            !matches(
                red,
                &Expected::Hex(configured.r, configured.g, configured.b),
                &emu,
                true
            ),
            "the configured colour is no longer what slot 1 shows"
        );
        assert!(
            matches(red, &Expected::Ansi256(1), &emu, true),
            "the index is unaffected: it names a slot, not a colour"
        );

        emu.process(b"\x1b]104;1\x07");
        assert!(
            matches(
                red,
                &Expected::Hex(configured.r, configured.g, configured.b),
                &emu,
                true
            ),
            "a reset restores the configured colour"
        );
    }

    /// A profile's palette is what an assertion compares against, so two
    /// profiles genuinely disagree rather than sharing one hardcoded table.
    #[test]
    fn a_recolored_profile_moves_what_an_assertion_matches() {
        let colors = emu_with(Colors {
            red: crate::profile::Rgb::new(1, 2, 3),
            ..Default::default()
        });
        let red = Some(Color::from_index(1));
        assert!(matches(red, &Expected::Hex(1, 2, 3), &colors, true));
        assert!(!matches(red, &Expected::Hex(128, 0, 0), &colors, true));
        assert!(
            matches(red, &Expected::Ansi256(1), &colors, true),
            "the index is unaffected by the palette"
        );
    }
}
