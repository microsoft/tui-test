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
pub fn matches(cell: Color, expected: &Expected) -> bool {
    match expected {
        Expected::Ansi256(n) => match cell {
            Color::Default => *n == 0,
            Color::Idx(i) => i == *n,
            Color::Rgb(r, g, b) => rgb_to_ansi256(r, g, b) == *n,
        },
        Expected::Hex(er, eg, eb) | Expected::Rgb(er, eg, eb) => {
            let (er, eg, eb) = (*er, *eg, *eb);
            match cell {
                Color::Default => er == 0 && eg == 0 && eb == 0,
                Color::Idx(i) => {
                    let (r, g, b) = ansi256_to_rgb(i);
                    r == er && g == eg && b == eb
                }
                Color::Rgb(r, g, b) => r == er && g == eg && b == eb,
            }
        }
    }
}

/// Render a cell's color in the same space as the expected value, for messages.
pub fn describe_cell(cell: Color, expected: &Expected) -> String {
    match expected {
        Expected::Ansi256(_) => match cell {
            Color::Default => "0".to_string(),
            Color::Idx(i) => i.to_string(),
            Color::Rgb(r, g, b) => rgb_to_ansi256(r, g, b).to_string(),
        },
        _ => match cell {
            Color::Default => "#000000".to_string(),
            Color::Idx(i) => {
                let (r, g, b) = ansi256_to_rgb(i);
                format!("#{r:02x}{g:02x}{b:02x}")
            }
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        },
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
        assert!(matches(Color::Idx(9), &Expected::Ansi256(9)));
        assert!(!matches(Color::Idx(2), &Expected::Ansi256(9)));
        assert!(matches(Color::Default, &Expected::Ansi256(0)));
        assert!(matches(Color::Rgb(255, 0, 0), &Expected::Rgb(255, 0, 0)));
    }

    #[test]
    fn ansi256_cube_roundtrip() {
        let (r, g, b) = ansi256_to_rgb(196);
        assert_eq!((r, g, b), (255, 0, 0));
    }
}
