//! Key-name → escape-sequence mapping.

const ESC: &str = "\u{1b}";
const CSI: &str = "\u{1b}[";

#[derive(Default, Clone, Copy)]
struct Mods {
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Mods {
    fn any(&self) -> bool {
        self.ctrl || self.alt || self.shift
    }
    /// 1 + shift + 2*alt + 4*ctrl
    fn param(&self) -> u32 {
        1 + self.shift as u32 + 2 * self.alt as u32 + 4 * self.ctrl as u32
    }
}

fn named(key: &str, m: Mods) -> Option<String> {
    let p = m.param();
    let modified = m.any();
    let seq = match key.to_ascii_lowercase().as_str() {
        "home" => {
            if modified {
                format!("{CSI}1;{p}H")
            } else {
                format!("{CSI}H")
            }
        }
        "end" => {
            if modified {
                format!("{CSI}1;{p}F")
            } else {
                format!("{CSI}F")
            }
        }
        "up" => {
            if modified {
                format!("{CSI}1;{p}A")
            } else {
                format!("{CSI}A")
            }
        }
        "down" => {
            if modified {
                format!("{CSI}1;{p}B")
            } else {
                format!("{CSI}B")
            }
        }
        "right" => {
            if modified {
                format!("{CSI}1;{p}C")
            } else {
                format!("{CSI}C")
            }
        }
        "left" => {
            if modified {
                format!("{CSI}1;{p}D")
            } else {
                format!("{CSI}D")
            }
        }
        "pageup" => {
            if modified {
                format!("{CSI}5;{p}~")
            } else {
                format!("{CSI}5~")
            }
        }
        "pagedown" => {
            if modified {
                format!("{CSI}6;{p}~")
            } else {
                format!("{CSI}6~")
            }
        }
        "insert" => {
            if modified {
                format!("{CSI}2;{p}~")
            } else {
                format!("{CSI}2~")
            }
        }
        "delete" => {
            if modified {
                format!("{CSI}3;{p}~")
            } else {
                format!("{CSI}3~")
            }
        }
        "backspace" => {
            if modified && p != 1 {
                format!("{CSI}27;{p};127~")
            } else {
                "\u{7f}".to_string()
            }
        }
        "tab" => {
            if modified && p != 1 {
                format!("{CSI}27;{p};9~")
            } else {
                "\t".to_string()
            }
        }
        "enter" | "return" => {
            if modified && p != 1 {
                format!("{CSI}27;{p};13~")
            } else {
                "\r".to_string()
            }
        }
        "space" => {
            if modified && p != 1 {
                format!("{CSI}27;{p};32~")
            } else {
                " ".to_string()
            }
        }
        "escape" | "esc" => {
            if modified && p != 1 {
                format!("{CSI}27;{p};27~")
            } else {
                ESC.to_string()
            }
        }
        "f1" => {
            if modified {
                format!("{CSI}1;{p}P")
            } else {
                format!("{ESC}OP")
            }
        }
        "f2" => {
            if modified {
                format!("{CSI}1;{p}Q")
            } else {
                format!("{ESC}OQ")
            }
        }
        "f3" => {
            if modified {
                format!("{CSI}1;{p}R")
            } else {
                format!("{ESC}OR")
            }
        }
        "f4" => {
            if modified {
                format!("{CSI}1;{p}S")
            } else {
                format!("{ESC}OS")
            }
        }
        "f5" => {
            if modified {
                format!("{CSI}15;{p}~")
            } else {
                format!("{CSI}15~")
            }
        }
        "f6" => {
            if modified {
                format!("{CSI}17;{p}~")
            } else {
                format!("{CSI}17~")
            }
        }
        "f7" => {
            if modified {
                format!("{CSI}18;{p}~")
            } else {
                format!("{CSI}18~")
            }
        }
        "f8" => {
            if modified {
                format!("{CSI}19;{p}~")
            } else {
                format!("{CSI}19~")
            }
        }
        "f9" => {
            if modified {
                format!("{CSI}20;{p}~")
            } else {
                format!("{CSI}20~")
            }
        }
        "f10" => {
            if modified {
                format!("{CSI}21;{p}~")
            } else {
                format!("{CSI}21~")
            }
        }
        "f11" => {
            if modified {
                format!("{CSI}23;{p}~")
            } else {
                format!("{CSI}23~")
            }
        }
        "f12" => {
            if modified {
                format!("{CSI}24;{p}~")
            } else {
                format!("{CSI}24~")
            }
        }
        _ => return None,
    };
    Some(seq)
}

fn char_combo(ch: char, m: Mods) -> String {
    let mut c = ch;
    if m.shift && c.is_ascii_alphabetic() {
        c = c.to_ascii_uppercase();
    }
    if m.ctrl {
        let code = (c.to_ascii_uppercase() as u32) & 0xff;
        if (0x40..=0x5f).contains(&code) {
            let ctrl_char = char::from_u32(code - 0x40).unwrap_or(c);
            return if m.alt {
                format!("{ESC}{ctrl_char}")
            } else {
                ctrl_char.to_string()
            };
        }
    }
    if m.alt {
        return format!("{ESC}{c}");
    }
    c.to_string()
}

/// Translate a single `press` token such as `Enter`, `Ctrl+C`, `:` into bytes.
pub fn token_to_seq(token: &str) -> anyhow::Result<String> {
    if token.is_empty() {
        return Ok(String::new());
    }
    if let Some(seq) = named(token, Mods::default()) {
        return Ok(seq);
    }
    if token.contains('+') && token.len() > 1 {
        let parts: Vec<&str> = token.split('+').collect();
        let (key, mods) = parts.split_last().unwrap();
        let mut m = Mods::default();
        for part in mods {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => m.ctrl = true,
                "alt" | "option" | "meta" => m.alt = true,
                "shift" => m.shift = true,
                other => anyhow::bail!("unknown modifier: {other}"),
            }
        }
        if let Some(seq) = named(key, m) {
            return Ok(seq);
        }
        let chars: Vec<char> = key.chars().collect();
        if chars.len() == 1 {
            return Ok(char_combo(chars[0], m));
        }
        anyhow::bail!("invalid key: '{key}'");
    }
    Ok(token.to_string())
}

/// Translate a sequence of `press` tokens into a single byte string.
pub fn tokens_to_seq(tokens: &[String]) -> anyhow::Result<String> {
    let mut out = String::new();
    for token in tokens {
        out.push_str(&token_to_seq(token)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys() {
        assert_eq!(token_to_seq("Enter").unwrap(), "\r");
        assert_eq!(token_to_seq("Escape").unwrap(), "\u{1b}");
        assert_eq!(token_to_seq("Up").unwrap(), "\u{1b}[A");
        assert_eq!(token_to_seq("F5").unwrap(), "\u{1b}[15~");
    }

    #[test]
    fn ctrl_combos() {
        assert_eq!(token_to_seq("Ctrl+C").unwrap(), "\u{3}");
        assert_eq!(token_to_seq("Control+a").unwrap(), "\u{1}");
    }

    #[test]
    fn literals() {
        assert_eq!(token_to_seq(":").unwrap(), ":");
        assert_eq!(token_to_seq("w").unwrap(), "w");
    }

    #[test]
    fn sequence() {
        let toks: Vec<String> = ["Escape", ":", "w", "q", "Enter"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(tokens_to_seq(&toks).unwrap(), "\u{1b}:wq\r");
    }
}
