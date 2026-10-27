//! Terminal snapshot serialization + on-disk `.snap` comparison.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Map, Value};

use super::super::terminal::cell::{display_width, truncate_to_columns, Attrs, Color, EmuCell};

pub enum SnapshotStatus {
    Passed,
    Written,
    Updated,
    Failed { expected: String, actual: String },
}

/// Write policy for [`compare_with_policy`].
///
/// The policy selects whether a comparison may write a baseline file.
/// It never selects ambient behavior: the new API reads no update,
/// `BLESS`, or `INSTA` environment variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotPolicy {
    /// Compare only. A missing baseline returns [`PolicyOutcome::Missing`]
    /// and different content returns [`PolicyOutcome::Different`]; no file
    /// or directory is made.
    #[default]
    CheckOnly,
    /// Write the baseline only when no file exists yet. The write uses an
    /// atomic create-new step (a temporary file plus a hard link) that fails
    /// when the file already exists, so a concurrent winner is never replaced.
    /// Different content returns [`PolicyOutcome::Different`]; nothing is
    /// replaced.
    CreateMissing,
    /// Write the baseline when it is missing and replace it when it differs.
    /// Every write goes through an atomic temporary-file-plus-rename step, so
    /// a failed write keeps the previous baseline. Unlike legacy [`compare`],
    /// a replace installs a regular file at the baseline path: an existing
    /// baseline symlink is replaced, never followed, and its target is left
    /// untouched.
    Replace,
}

/// Structured outcome of [`compare_with_policy`].
///
/// This is a new result type. The exhaustive [`SnapshotStatus`] enum from the
/// recorded commit is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyOutcome {
    /// The saved baseline and the compared content match after trimming
    /// surrounding whitespace on both sides. This is
    /// comparison-after-trimming, not byte identity: `"  one\n"` equals
    /// `"one"`, and the stored bytes keep their recorded trimmed form. No
    /// write happens under any policy.
    Equal,
    /// No baseline file exists and the policy wrote nothing.
    Missing,
    /// A baseline file exists whose trimmed content differs, and the policy
    /// wrote nothing.
    Different { expected: String, actual: String },
    /// The policy wrote a baseline where none existed.
    Created,
    /// The policy replaced an existing baseline with different bytes.
    Replaced,
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
    if prev.underline_color != cur.underline_color {
        m.insert("underline_color".into(), color_value(cur.underline_color));
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
    // The URI, not a boolean: a snapshot that only recorded "this is a link"
    // would pass when the link silently started pointing somewhere else,
    // which is the whole thing worth asserting about a link. The `id=` is
    // deliberately not recorded, because it is an anchor for joining a
    // wrapped link rather than anything a reader can check, and the ghostty
    // backend cannot report it at all.
    if prev.uri() != cur.uri() {
        m.insert("link".into(), json!(cur.uri().unwrap_or_default()));
    }
    m
}

fn baseline() -> EmuCell {
    EmuCell::blank()
}

/// Serialize a grid into a boxed text view, optionally followed by attributes.
///
/// The attributes are a JSON object carrying whatever the box itself cannot
/// record exactly: the full window title, and the per-cell style shifts when
/// they are asked for. It is emitted only when there is something to put in
/// it, so a plain snapshot is still just the box.
///
/// A style shift covers foreground, background and underline colors, the SGR
/// attributes, the underline style and the link. The link is not an SGR
/// attribute — `SGR 0` clears every other entry
/// here and leaves it running — but it is carried on a cell the same way, so
/// it is recorded with them rather than behind a second switch a reader has to
/// remember to turn on.
pub fn serialize(
    rows: &[Vec<EmuCell>],
    cols: u16,
    include_style: bool,
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
    if include_style && !shifts.is_empty() {
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

/// Compare serialized snapshot data against the stored baseline under an
/// explicit write policy. Snapshots are resolved under `base`/`__snapshots__`
/// so they land in the client's working directory rather than the daemon's.
///
/// When the saved baseline matches `content` after trimming surrounding
/// whitespace on both sides, this returns [`PolicyOutcome::Equal`]
/// without a write under all policies. [`SnapshotPolicy::CheckOnly`] never
/// makes a file or a directory. [`SnapshotPolicy::CreateMissing`] writes only
/// through an atomic create-new step; when a concurrent writer makes the file
/// first, the winner's file is compared again and this returns `Equal` or
/// `Different` without replacing it. [`SnapshotPolicy::Replace`] writes
/// through an atomic temporary-file-plus-rename step, so a failed write keeps
/// the previous baseline. A replace installs a regular file at the baseline
/// path: unlike legacy [`compare`], which follows a baseline symlink and
/// updates its target, a policy replace swaps the link itself away and never
/// touches the old target.
///
/// No ambient update, `BLESS`, or `INSTA` environment variable is read.
pub fn compare_with_policy(
    base: &Path,
    name: &str,
    content: &str,
    policy: SnapshotPolicy,
) -> std::io::Result<PolicyOutcome> {
    let path = snapshot_path(base, name);
    let trimmed = content.trim();
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let existing = existing.trim();
            if existing == trimmed {
                return Ok(PolicyOutcome::Equal);
            }
            let outcome = PolicyOutcome::Different {
                expected: existing.to_string(),
                actual: trimmed.to_string(),
            };
            match policy {
                SnapshotPolicy::CheckOnly | SnapshotPolicy::CreateMissing => Ok(outcome),
                SnapshotPolicy::Replace => {
                    atomic_replace(base, &path, trimmed)?;
                    Ok(PolicyOutcome::Replaced)
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match policy {
            SnapshotPolicy::CheckOnly => Ok(PolicyOutcome::Missing),
            SnapshotPolicy::CreateMissing => {
                let body = format!("{trimmed}\n");
                match create_new(&path, base, &body) {
                    Ok(()) => Ok(PolicyOutcome::Created),
                    Err(race) if race.kind() == std::io::ErrorKind::AlreadyExists => {
                        // A concurrent writer made the file first. Compare the
                        // winner's file again and never replace it.
                        let existing = std::fs::read_to_string(&path)?;
                        let existing = existing.trim();
                        if existing == trimmed {
                            Ok(PolicyOutcome::Equal)
                        } else {
                            Ok(PolicyOutcome::Different {
                                expected: existing.to_string(),
                                actual: trimmed.to_string(),
                            })
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            SnapshotPolicy::Replace => {
                atomic_replace(base, &path, trimmed)?;
                Ok(PolicyOutcome::Created)
            }
        },
        Err(error) => Err(error),
    }
}

/// Compare a freshly serialized snapshot against the stored one. Snapshots are
/// resolved under `base`/`__snapshots__` so they land in the client's working
/// directory rather than the daemon's.
///
/// This keeps the recorded signature and behavior: a missing baseline is
/// written and reported as `Written`, different content is replaced and
/// reported as `Updated` only when `update` is set, and otherwise reported as
/// `Failed`.
///
/// Link behavior differs from [`compare_with_policy`]: this legacy API writes
/// with `std::fs::write`, which follows an existing baseline symlink and
/// updates its TARGET (the link survives). Only
/// [`compare_with_policy`] with [`SnapshotPolicy::Replace`] uses the safe
/// atomic temporary-file-plus-rename step, which installs a regular file at
/// the baseline path and leaves the link target untouched.
pub fn compare(
    base: &Path,
    name: &str,
    content: &str,
    update: bool,
) -> std::io::Result<SnapshotStatus> {
    let path = snapshot_path(base, name);
    let trimmed = content.trim();
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let existing = existing.trim();
            if existing == trimmed {
                return Ok(SnapshotStatus::Passed);
            }
            if update {
                std::fs::create_dir_all(snapshot_dir(base))?;
                // Legacy follow-link write: when the baseline is a symlink,
                // the new bytes land in its target and the link survives.
                std::fs::write(&path, format!("{trimmed}\n"))?;
                Ok(SnapshotStatus::Updated)
            } else {
                Ok(SnapshotStatus::Failed {
                    expected: existing.to_string(),
                    actual: trimmed.to_string(),
                })
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(snapshot_dir(base))?;
            std::fs::write(&path, format!("{trimmed}\n"))?;
            Ok(SnapshotStatus::Written)
        }
        Err(error) => Err(error),
    }
}

/// Save `body` to `path`, failing when the file already exists.
///
/// The parent directory is made first because no step below makes it. The
/// bytes go to a uniquely named temporary file made with
/// [`std::fs::OpenOptions::create_new`], and only [`std::fs::hard_link`]
/// places the baseline: linking fails when the file already exists, so this
/// call never replaces a baseline that another writer made first, and
/// concurrent readers only ever see a missing or a complete file, never a
/// partial write.
fn create_new(path: &Path, base: &Path, body: &str) -> std::io::Result<()> {
    let dir = snapshot_dir(base);
    std::fs::create_dir_all(&dir)?;
    let stamp = REPLACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(temp_file_name(&path_lossless_name(path), "new", stamp));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    // The temporary file is exclusively owned from here, so a partial write
    // must not leave it behind for a later writer to trip over.
    if let Err(error) = std::io::Write::write_all(&mut file, body.as_bytes()) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);
    let link = std::fs::hard_link(&temp, path);
    // The temporary file is only a vehicle; the linked baseline (or the
    // concurrent winner's file) is what stays.
    let _ = std::fs::remove_file(&temp);
    link
}

/// Counter that keeps temporary replacement names distinct across threads.
static REPLACE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The stamp the next replacement will use, for tests only.
///
/// Integration tests share one process-global counter across threads, so
/// instead of assuming a stamp value they read the stamp in play and occupy
/// exactly that temporary slot. Hidden from documentation; production code
/// never calls this.
#[doc(hidden)]
pub fn __test_only_replace_counter() -> u64 {
    REPLACE_COUNTER.load(Ordering::Relaxed)
}

/// Test-only mid-write fault hook, compiled out of release builds.
///
/// When the environment variable `TUI_TEST_SNAPSHOT_WRITE_FAULT` holds
/// `<dir>:<n>` and `dir` is this call's snapshot directory, the first `n`
/// body bytes travel through the exclusively owned temporary handle and the
/// write then fails with an injected error, exercising partial-write cleanup.
/// The scope is one directory so tests sharing a process stay unaffected.
/// Production code otherwise reads no environment variables.
#[cfg(debug_assertions)]
fn injected_partial_write(dir: &Path) -> Option<usize> {
    let spec = std::env::var("TUI_TEST_SNAPSHOT_WRITE_FAULT").ok()?;
    let (scope, limit) = spec.rsplit_once(':')?;
    (Path::new(scope) == dir)
        .then(|| limit.parse().ok())
        .flatten()
}

/// Save `trimmed` to `path` through a temporary file plus an atomic rename.
///
/// The rename either installs the full new baseline or leaves the previous
/// baseline unchanged, so an explicit write failure keeps the previous bytes.
/// The temporary file lives in the same directory so the rename stays atomic.
fn atomic_replace(base: &Path, path: &Path, trimmed: &str) -> std::io::Result<()> {
    let dir = snapshot_dir(base);
    std::fs::create_dir_all(&dir)?;
    let stamp = REPLACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(temp_file_name(&path_lossless_name(path), "tmp", stamp));
    // Exclusively create the temporary file: when the path already exists (a
    // stale file, a planted symlink, or a concurrent writer's temp), the open
    // fails and the preexisting path is left alone — it is not ours to write
    // through or remove. Writing through the exclusively owned handle (never
    // `std::fs::write`, which follows symlinks) keeps a preexisting symlink
    // from redirecting bytes into the baseline.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    // From here the temporary file is exclusively owned: only a completed
    // rename installs new bytes, and any failure below removes the temp file.
    let body = format!("{trimmed}\n");
    let write = (|| -> std::io::Result<()> {
        #[cfg(debug_assertions)]
        if let Some(limit) = injected_partial_write(&dir) {
            let mut limit = limit.min(body.len());
            while !body.is_char_boundary(limit) {
                limit -= 1;
            }
            std::io::Write::write_all(&mut file, &body.as_bytes()[..limit])?;
            std::io::Write::flush(&mut file)?;
            return Err(std::io::Error::other("injected partial-write fault"));
        }
        std::io::Write::write_all(&mut file, body.as_bytes())?;
        std::io::Write::flush(&mut file)?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if write.is_err() {
        // A failed replacement must not leave a partial baseline behind.
        // The previous baseline is already intact because only a completed
        // rename installs new bytes; drop the owned temporary file.
        let _ = std::fs::remove_file(&temp);
    }
    write
}

/// File stem of `path` for temporary-file naming, without touching the
/// sanitization rules for snapshot names.
fn path_lossless_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("snapshot")
        .to_string()
}

/// Temporary-file name for a baseline stem that stays short at any input length.
///
/// The full stem cannot be embedded: a 250-byte snapshot name makes a valid
/// 255-byte `.snap` file, but `.{stem}.tmp-{pid}-{stamp}` would already be
/// 263 bytes and fail with `ENAMETOOLONG`. The name below carries only a
/// 64-bit FNV-1a hash of the stem plus the process id and a unique stamp, so
/// it is bounded (well under every `NAME_MAX`) for every baseline while
/// staying unique per baseline within a directory. `kind` separates the
/// create-new (`"new"`) and replace (`"tmp"`) writers sharing one directory.
/// The hash is deterministic (fixed FNV offset basis), so tests can derive
/// the exact slot the next writer takes through
/// [`__test_only_temp_file_name`].
fn temp_file_name(stem: &str, kind: &str, stamp: u64) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in stem.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!(".{kind}-{hash:016x}-{}-{stamp}", std::process::id())
}

/// Temporary-file name the writers derive for `stem`, for tests only.
///
/// Integration tests plant files at the exact slot the next writer takes, so
/// the derivation is shared through this seam rather than mirrored.
/// Hidden from documentation; production code never calls this.
#[doc(hidden)]
pub fn __test_only_temp_file_name(stem: &str, kind: &str, stamp: u64) -> String {
    temp_file_name(stem, kind, stamp)
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

    #[test]
    fn underline_colors_record_slots_rgb_and_resets() {
        let colored = |color| EmuCell {
            underline_color: color,
            ..EmuCell::blank()
        };
        let rows = [vec![
            colored(Some(Color::from_index(1))),
            colored(Some(Color::Rgb(1, 2, 3))),
            colored(None),
        ]];
        let serialized = serialize(&rows, 3, true, None);
        let attributes: Value =
            serde_json::from_str(serialized.split_once("╯\n").unwrap().1).unwrap();
        assert_eq!(attributes["colors"]["0,0"]["underline_color"], json!(1));
        assert_eq!(
            attributes["colors"]["1,0"]["underline_color"],
            json!("#010203")
        );
        assert_eq!(
            attributes["colors"]["2,0"]["underline_color"],
            json!("default")
        );
        assert_ne!(
            serialized,
            serialize(&[vec![EmuCell::blank(); 3]], 3, true, None)
        );
        assert_eq!(
            serialize(&rows, 3, false, None),
            serialize(&[vec![EmuCell::blank(); 3]], 3, false, None)
        );
        assert!(shift(&rows[0][0], &rows[0][0]).is_empty());
    }
}
