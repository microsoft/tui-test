//! Color parsing and comparison for `expect --fg/--bg`.

use super::super::terminal::cell::Color;

#[derive(Debug, Clone)]
pub enum Expected {
    Ansi256(u8),
    Hex(u8, u8, u8),
    Rgb(u8, u8, u8),
}

impl Expected {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
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
            Expected::Ansi256(n) => n.to_string(),
            Expected::Hex(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Expected::Rgb(r, g, b) => format!("{r},{g},{b}"),
        }
    }
}

/// A consistent, enumerated error for any unparseable color value.
fn invalid(got: &str) -> anyhow::Error {
    anyhow::anyhow!("color must be ansi256 (0-255), hex (#rrggbb), or rgb (r,g,b) (got: \"{got}\")")
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
/// A cell using the terminal default never matches: the default is whatever
/// the viewer's theme picks, so no concrete value can be claimed for it.
pub fn matches(cell: Option<Color>, expected: &Expected) -> bool {
    let Some(cell) = cell else {
        return false;
    };
    match expected {
        Expected::Ansi256(n) => cell.to_index() == *n,
        Expected::Hex(er, eg, eb) | Expected::Rgb(er, eg, eb) => {
            let rgb = match cell {
                Color::Rgb(r, g, b) => (r, g, b),
                c => ansi256_to_rgb(c.to_index()),
            };
            rgb == (*er, *eg, *eb)
        }
    }
}

/// Render a cell's color in the same space as the expected value, for messages.
pub fn describe_cell(cell: Option<Color>, expected: &Expected) -> String {
    let Some(cell) = cell else {
        return "default".to_string();
    };
    match expected {
        Expected::Ansi256(_) => cell.to_index().to_string(),
        _ => {
            let (r, g, b) = match cell {
                Color::Rgb(r, g, b) => (r, g, b),
                c => ansi256_to_rgb(c.to_index()),
            };
            format!("#{r:02x}{g:02x}{b:02x}")
        }
    }
}

const ANSI16: [(u8, u8, u8); 16] = [
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

pub fn ansi256_to_rgb(n: u8) -> (u8, u8, u8) {
    match n {
        0..=15 => ANSI16[n as usize],
        16..=231 => {
            let i = n as u16 - 16;
            let r = (i / 36) % 6;
            let g = (i / 6) % 6;
            let b = i % 6;
            let conv = |c: u16| -> u8 {
                if c == 0 {
                    0
                } else {
                    (c * 40 + 55) as u8
                }
            };
            (conv(r), conv(g), conv(b))
        }
        232..=255 => {
            let v = (n as u16 - 232) * 10 + 8;
            (v as u8, v as u8, v as u8)
        }
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
    use crate::terminal::cell::Color;

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
        let idx = |i| Some(Color::from_index(i));
        assert!(matches(idx(9), &Expected::Ansi256(9)));
        assert!(!matches(idx(2), &Expected::Ansi256(9)));
        assert!(matches(idx(196), &Expected::Ansi256(196)));
        assert!(matches(
            Some(Color::Rgb(255, 0, 0)),
            &Expected::Rgb(255, 0, 0)
        ));
    }

    /// A default-colored cell used to match `--fg 0`, claiming it was black
    /// when the theme actually paints it light gray.
    #[test]
    fn default_color_matches_nothing() {
        assert!(!matches(None, &Expected::Ansi256(0)));
        assert!(!matches(None, &Expected::Hex(0, 0, 0)));
        assert_eq!(describe_cell(None, &Expected::Ansi256(0)), "default");
    }

    #[test]
    fn ansi256_cube_roundtrip() {
        let (r, g, b) = ansi256_to_rgb(196);
        assert_eq!((r, g, b), (255, 0, 0));
    }
}
