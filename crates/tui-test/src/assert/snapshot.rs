//! Terminal snapshot serialization + on-disk `.snap` comparison.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::super::terminal::cell::{display_width, truncate_to_columns, Attrs, Color, EmuCell};

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

/// Serialize a grid into a boxed text view, optionally followed by attributes.
///
/// The attributes are a JSON object carrying whatever the box itself cannot
/// record exactly: the full window title, and the color shifts when they are
/// asked for. It is emitted only when there is something to put in it, so a
/// plain snapshot is still just the box.
pub fn serialize(
    rows: &[Vec<EmuCell>],
    cols: u16,
    include_colors: bool,
    title: Option<&str>,
) -> String {
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

    let view = box_view(&lines.join("\n"), cols, title);
    let mut attributes = Map::new();
    // The border shows a title shortened to fit, which is readable but lossy:
    // two long titles differing only past the cut would otherwise record as
    // the same snapshot and pass for each other. The full one is recorded here
    // so the baseline stays exact however narrow the frame is.
    if let Some(title) = title {
        attributes.insert("title".to_string(), Value::String(title.to_string()));
    }
    if include_colors && !shifts.is_empty() {
        attributes.insert("colors".to_string(), Value::Object(shifts));
    }
    if attributes.is_empty() {
        view
    } else {
        format!(
            "{view}\n{}",
            serde_json::to_string_pretty(&Value::Object(attributes)).unwrap_or_default()
        )
    }
}

/// Frame the view, putting the window title in the top border when there is
/// one.
///
/// The title rides in the border rather than on a line of its own so that a
/// snapshot taken without a title is byte-identical to one taken before titles
/// were recorded at all, which keeps every stored baseline valid. A title too
/// long for the border is truncated so the frame stays rectangular.
fn box_view(view: &str, width: u16, title: Option<&str>) -> String {
    let width = width as usize;
    let bar = "─".repeat(width);
    // `╭─ title ───╮`: one leading dash, the spaced title, then at least one
    // trailing dash. A title with no room for even one character is dropped
    // rather than allowed to push the corner out of line.
    let label = title.and_then(|title| {
        let room = width.checked_sub(4).filter(|room| *room > 0)?;
        Some(format!(" {} ", truncate_to_columns(title, room)))
    });
    let top = match label {
        Some(label) => format!(
            "╭─{label}{}╮",
            "─".repeat(width - 1 - display_width(&label))
        ),
        None => format!("╭{bar}╮"),
    };
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

    /// Just the framed view, dropping any attributes recorded after it.
    fn box_of(serialized: &str) -> String {
        match serialized.split_once("╯\n") {
            Some((frame, _)) => format!("{frame}╯"),
            None => serialized.to_string(),
        }
    }

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
        let out = serialize(&[vec![colored]], 1, true, None);
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
        assert!(serialize(&[vec![rgb]], 1, true, None).contains("#112233"));
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
        assert_eq!(
            serialize(&rows, 6, false, None),
            "╭──────╮\n│你b   │\n╰──────╯"
        );
    }

    /// The window title rides in the top border.
    ///
    /// It goes there rather than on a line of its own so that the frame keeps
    /// its shape and a snapshot taken without a title is byte-identical to one
    /// taken before titles were recorded, which is what keeps stored baselines
    /// valid.
    #[test]
    fn the_title_rides_in_the_top_border() {
        let rows = vec![vec![cell("a"); 20]];
        let bare = serialize(&rows, 20, false, None);
        let titled = serialize(&rows, 20, false, Some("vim"));

        assert!(
            bare.starts_with("╭────────────────────╮"),
            "no title leaves the border untouched: {bare}"
        );
        assert!(
            titled.starts_with("╭─ vim ──────────────╮"),
            "the title is set into the border: {titled}"
        );
        assert_eq!(
            bare.lines().skip(1).collect::<Vec<_>>(),
            box_of(&titled).lines().skip(1).collect::<Vec<_>>(),
            "and nothing below the border changes"
        );
    }

    /// The full title is recorded even when the border shows a short one.
    ///
    /// The border has to fit, so a long title is cut to size there. A snapshot
    /// is an assertion rather than a picture: two titles differing only past
    /// the cut would record identically and pass for each other, so the exact
    /// one is kept alongside the frame.
    #[test]
    fn the_full_title_is_recorded_even_when_the_border_cannot_show_it() {
        let rows = vec![vec![cell("a"); 12]];
        let long = "building module A, step 3";
        let other = "building module B, step 7";
        let out = serialize(&rows, 12, false, Some(long));

        assert!(
            box_of(&out).contains('…'),
            "the border shows a shortened title: {out}"
        );
        assert!(
            out.contains(&format!(r#""title": "{long}""#)),
            "and the exact one is recorded: {out}"
        );
        assert_ne!(
            out,
            serialize(&rows, 12, false, Some(other)),
            "two titles that shorten alike still record differently"
        );
    }

    /// Colors keep their own key, so a snapshot can carry both.
    #[test]
    fn attributes_hold_the_title_and_the_colors_apart() {
        let rows = vec![vec![
            cell("a"),
            EmuCell {
                fg: Some(Color::from_index(1)),
                ..EmuCell::blank()
            },
        ]];
        let out = serialize(&rows, 2, true, Some("t"));
        let attributes: Value =
            serde_json::from_str(out.split_once("╯\n").expect("a frame then attributes").1)
                .expect("attributes parse as json");
        assert_eq!(attributes["title"], json!("t"));
        assert!(
            attributes["colors"].is_object(),
            "colors stay under their own key: {out}"
        );
    }

    /// Every border line stays the same width whatever the title.
    ///
    /// A title wider than the frame would otherwise push the corner out and
    /// produce a snapshot that never matches and cannot be read.
    #[test]
    fn a_title_never_changes_the_frame_width() {
        let rows = vec![vec![cell("a"); 10]];
        // Measured in columns, not characters. A CJK title is half as many
        // characters as columns, so a character count would report a square
        // frame while the drawn one is four columns out.
        for title in [
            "",
            "x",
            "fits",
            "a title far wider than the frame",
            "你好世界你好世界",
            "🚀 build",
            "e\u{301}clair",
        ] {
            let out = serialize(&rows, 10, false, Some(title));
            let widths: Vec<usize> = box_of(&out).lines().map(display_width).collect();
            assert!(
                widths.iter().all(|w| *w == 12),
                "title {title:?} bent the frame: {widths:?}\n{out}"
            );
        }
    }

    /// A frame with no room for a title keeps its plain border rather than
    /// losing a corner to make space.
    #[test]
    fn a_frame_too_narrow_for_a_title_stays_plain() {
        let rows = vec![vec![cell("a"); 3]];
        assert_eq!(
            box_of(&serialize(&rows, 3, false, Some("title"))),
            serialize(&rows, 3, false, None),
            "three columns cannot hold a title, so none is drawn"
        );
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
