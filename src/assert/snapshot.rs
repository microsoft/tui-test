//! Terminal snapshot serialization + on-disk `.snap` comparison.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::super::terminal::cell::{Attrs, Color, EmuCell};

pub enum SnapshotStatus {
    Passed,
    Written,
    Updated,
    Failed { expected: String, actual: String },
}

fn snapshot_dir(base: &Path) -> PathBuf {
    base.join("__snapshots__")
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if " /\\<>:\"'|?*".contains(c) { '-' } else { c })
        .collect()
}

fn snapshot_path(base: &Path, name: &str) -> PathBuf {
    snapshot_dir(base).join(format!("{}.snap", sanitize(name)))
}

fn color_value(c: Option<Color>) -> Value {
    match c {
        None => Value::String("default".to_string()),
        Some(Color::Rgb(r, g, b)) => Value::String(format!("#{r:02x}{g:02x}{b:02x}")),
        Some(c) => json!(c.to_index()),
    }
}

fn shift(prev: &EmuCell, cur: &EmuCell) -> Map<String, Value> {
    let mut m = Map::new();
    if prev.fg != cur.fg {
        m.insert("fg".into(), color_value(cur.fg));
    }
    if prev.bg != cur.bg {
        m.insert("bg".into(), color_value(cur.bg));
    }
    for (attr, key) in [
        (Attrs::BOLD, "bold"),
        (Attrs::DIM, "dim"),
        (Attrs::ITALIC, "italic"),
        (Attrs::INVERSE, "inverse"),
        (Attrs::INVISIBLE, "invisible"),
        (Attrs::STRIKE, "strike"),
        (Attrs::BLINK, "blink"),
    ] {
        if prev.has(attr) != cur.has(attr) {
            m.insert(key.into(), json!(cur.has(attr)));
        }
    }
    if prev.underline.is_underlined() != cur.underline.is_underlined() {
        m.insert("underline".into(), json!(cur.underline.is_underlined()));
    }
    m
}

fn baseline() -> EmuCell {
    EmuCell::blank()
}

/// Serialize a grid into a boxed text view plus (optionally) a color shift map.
pub fn serialize(rows: &[Vec<EmuCell>], cols: u16, include_colors: bool) -> String {
    let mut lines = Vec::with_capacity(rows.len());
    let mut shifts = Map::new();
    let mut prev = baseline();
    for (y, row) in rows.iter().enumerate() {
        let mut line = String::with_capacity(cols as usize);
        for (x, cell) in row.iter().enumerate() {
            // A continuation contributes nothing, exactly as in
            // `rows_to_strings`: the wide char to its left already spans this
            // column, so giving it a filler widens the row past the box.
            line.push_str(&cell.ch);
            let s = shift(&prev, cell);
            if !s.is_empty() {
                shifts.insert(format!("{x},{y}"), Value::Object(s));
            }
            prev = cell.clone();
        }
        lines.push(line);
    }

    let view = box_view(&lines.join("\n"), cols);
    if include_colors && !shifts.is_empty() {
        format!(
            "{view}\n{}",
            serde_json::to_string_pretty(&Value::Object(shifts)).unwrap_or_default()
        )
    } else {
        view
    }
}

fn box_view(view: &str, width: u16) -> String {
    let bar = "─".repeat(width as usize);
    let top = format!("╭{bar}╮");
    let bottom = format!("╰{bar}╯");
    let mut out = vec![top];
    for line in view.split('\n') {
        out.push(format!("│{line}│"));
    }
    out.push(bottom);
    out.join("\n")
}

/// Compare a freshly serialized snapshot against the stored one. Snapshots are
/// resolved under `base`/`__snapshots__` so they land in the client's working
/// directory rather than the daemon's.
pub fn compare(
    base: &Path,
    name: &str,
    content: &str,
    update: bool,
) -> std::io::Result<SnapshotStatus> {
    let path = snapshot_path(base, name);
    let trimmed = content.trim();
    if !path.exists() {
        std::fs::create_dir_all(snapshot_dir(base))?;
        std::fs::write(&path, format!("{trimmed}\n"))?;
        return Ok(SnapshotStatus::Written);
    }
    let existing = std::fs::read_to_string(&path)?;
    let existing = existing.trim();
    if existing == trimmed {
        return Ok(SnapshotStatus::Passed);
    }
    if update {
        std::fs::write(&path, format!("{trimmed}\n"))?;
        return Ok(SnapshotStatus::Updated);
    }
    Ok(SnapshotStatus::Failed {
        expected: existing.to_string(),
        actual: trimmed.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::cell::CONTINUATION;

    fn cell(s: &str) -> EmuCell {
        EmuCell {
            ch: s.into(),
            ..EmuCell::blank()
        }
    }

    /// A wide char spans two columns on its own. Rendering a filler for the
    /// continuation pushed every later column right and left the content line
    /// one column wider than the frame drawn around it, so any snapshot
    /// holding a wide char was written misaligned and compared against that.
    #[test]
    fn a_wide_char_does_not_overflow_the_frame() {
        let rows = vec![vec![
            cell("你"),
            cell(CONTINUATION),
            cell("b"),
            cell(" "),
            cell(" "),
            cell(" "),
        ]];
        assert_eq!(serialize(&rows, 6, false), "╭──────╮\n│你b   │\n╰──────╯");
    }
}
