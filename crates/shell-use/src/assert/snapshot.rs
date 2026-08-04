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
        None => Value::String(crate::assert::color::DEFAULT.to_string()),
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
    // The style, not a boolean: a curly underline and a single one are
    // different renderings, and a snapshot that only recorded "underlined"
    // would pass when one silently became the other.
    if prev.underline != cur.underline {
        m.insert("underline".into(), json!(cur.underline.name()));
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

    /// A snapshot records the palette *slot* a cell chose, never the color
    /// that slot resolves to.
    ///
    /// This is what lets a saved baseline outlive a profile change: the same
    /// screen recorded under two profiles that disagree about what red looks
    /// like still produces the same snapshot, so recoloring a terminal does
    /// not invalidate every snapshot in a suite.
    #[test]
    fn a_snapshot_records_the_slot_rather_than_the_color() {
        let colored = EmuCell {
            ch: "x".into(),
            fg: Some(Color::from_index(1)),
            ..EmuCell::blank()
        };
        let out = serialize(&[vec![colored]], 1, true);
        assert!(
            out.contains("\"fg\": 1"),
            "the slot is recorded, not an rgb value: {out}"
        );
        assert!(
            !out.contains('#'),
            "a palette color must not be resolved into the snapshot: {out}"
        );
    }

    /// A true-color cell names its own color, so that one *is* recorded
    /// literally: no profile can change what `38;2;r;g;b` means.
    #[test]
    fn a_true_color_cell_records_its_own_value() {
        let rgb = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Rgb(0x11, 0x22, 0x33)),
            ..EmuCell::blank()
        };
        assert!(serialize(&[vec![rgb]], 1, true).contains("#112233"));
    }

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

    /// Snapshots recorded a bare "is underlined", so a curly underline turning
    /// single left the snapshot passing. The style name is recorded instead.
    #[test]
    fn a_shift_between_underline_styles_is_recorded() {
        use crate::terminal::cell::UnderlineStyle;
        let styled = |u| EmuCell {
            underline: u,
            ..EmuCell::blank()
        };
        let curly = shift(
            &styled(UnderlineStyle::Single),
            &styled(UnderlineStyle::Curly),
        );
        assert_eq!(curly.get("underline"), Some(&json!("curly")));
        assert_eq!(
            shift(
                &styled(UnderlineStyle::Curly),
                &styled(UnderlineStyle::None)
            )
            .get("underline"),
            Some(&json!("none"))
        );
        assert!(shift(
            &styled(UnderlineStyle::Curly),
            &styled(UnderlineStyle::Curly)
        )
        .is_empty());
    }
}
