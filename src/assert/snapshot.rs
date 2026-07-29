//! Terminal snapshot serialization + on-disk `.snap` comparison.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::super::terminal::cell::{Color, EmuCell};

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

fn color_value(c: Color) -> Value {
    match c {
        Color::Default => Value::String("default".to_string()),
        Color::Idx(i) => json!(i),
        Color::Rgb(r, g, b) => Value::String(format!("#{r:02x}{g:02x}{b:02x}")),
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
    if prev.bold != cur.bold {
        m.insert("bold".into(), json!(cur.bold));
    }
    if prev.dim != cur.dim {
        m.insert("dim".into(), json!(cur.dim));
    }
    if prev.italic != cur.italic {
        m.insert("italic".into(), json!(cur.italic));
    }
    if prev.underline != cur.underline {
        m.insert("underline".into(), json!(cur.underline));
    }
    if prev.inverse != cur.inverse {
        m.insert("inverse".into(), json!(cur.inverse));
    }
    if prev.invisible != cur.invisible {
        m.insert("invisible".into(), json!(cur.invisible));
    }
    if prev.strike != cur.strike {
        m.insert("strike".into(), json!(cur.strike));
    }
    m
}

fn baseline() -> EmuCell {
    EmuCell {
        ch: String::new(),
        fg: Color::Default,
        bg: Color::Default,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        inverse: false,
        invisible: false,
        strike: false,
    }
}

/// Serialize a grid into a boxed text view plus (optionally) a color shift map.
pub fn serialize(rows: &[Vec<EmuCell>], cols: u16, include_colors: bool) -> String {
    let mut lines = Vec::with_capacity(rows.len());
    let mut shifts = Map::new();
    let mut prev = baseline();
    for (y, row) in rows.iter().enumerate() {
        let mut line = String::with_capacity(cols as usize);
        for (x, cell) in row.iter().enumerate() {
            line.push_str(if cell.ch.is_empty() { " " } else { &cell.ch });
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
