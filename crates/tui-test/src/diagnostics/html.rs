use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    ArtifactFile, FailureObservation, FailureReport, ScreenSnapshotDetails, TIMELINE_LIMIT,
};
use crate::api::CellColor;
use crate::render::svg::{self, RenderColors};
use crate::terminal::cell::{Attrs, EmuCell, CONTINUATION};

use super::markdown::{explanation, render as markdown, Explanation};

#[derive(Serialize)]
pub(super) struct Timeline {
    schema_version: u32,
    failure_screen_sequence: u64,
    geometry: Geometry,
    frames: Vec<Frame>,
    truncated: bool,
}

impl Timeline {
    pub(super) fn is_truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Serialize)]
struct Geometry {
    grid_x: f32,
    grid_y: f32,
    cell_width: f32,
    cell_height: f32,
}

#[derive(Serialize)]
struct Frame {
    #[serde(flatten)]
    screen: ScreenSnapshotDetails,
    /// Each grid entry indexes this dictionary, including blank/continuation cells.
    cells: Vec<FrameCell>,
    grid: Vec<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    omission: Option<String>,
}

#[derive(Serialize)]
struct FrameCell {
    char: String,
    width: u8,
    fg: CellColor,
    bg: CellColor,
    underline_color: CellColor,
    underline_style: &'static str,
    flags: Vec<&'static str>,
    resolved_fg: String,
    resolved_bg: String,
    resolved_underline_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_id: Option<String>,
}

impl FrameCell {
    fn capture(cell: &EmuCell, width: u8, colors: &dyn RenderColors) -> Self {
        let style = svg::style_of(cell, colors);
        Self {
            char: cell.ch.to_string(),
            link: cell.uri().map(str::to_string),
            link_id: cell
                .hyperlink
                .as_ref()
                .and_then(|link| link.id.as_ref())
                .map(ToString::to_string),
            width,
            fg: crate::engine::cell_color(cell.fg),
            bg: crate::engine::cell_color(cell.bg),
            underline_color: crate::engine::cell_color(cell.underline_color),
            underline_style: cell.underline.name(),
            flags: [
                (Attrs::BOLD, "bold"),
                (Attrs::DIM, "dim"),
                (Attrs::ITALIC, "italic"),
                (Attrs::INVERSE, "inverse"),
                (Attrs::INVISIBLE, "invisible"),
                (Attrs::STRIKE, "strike"),
                (Attrs::BLINK, "blink"),
            ]
            .into_iter()
            .filter_map(|(flag, name)| cell.has(flag).then_some(name))
            .collect(),
            resolved_fg: style.fg.to_hex(),
            resolved_bg: svg::bg_of(cell, colors).to_hex(),
            resolved_underline_color: cell
                .underline_color
                .map_or(style.fg, |color| colors.resolve(Some(color), true))
                .to_hex(),
        }
    }
}

pub(super) fn timeline(observation: &FailureObservation) -> Result<Timeline, serde_json::Error> {
    timeline_with_limit(observation, TIMELINE_LIMIT)
}

fn timeline_with_limit(
    observation: &FailureObservation,
    limit: usize,
) -> Result<Timeline, serde_json::Error> {
    let sources = observation.history.retained();
    let mut timeline = Timeline {
        schema_version: 1,
        failure_screen_sequence: observation.screen_sequence,
        geometry: Geometry {
            grid_x: svg::CANVAS_PADDING as f32 + svg::MARGIN_X,
            grid_y: svg::CANVAS_PADDING as f32 + svg::HEADER_H + svg::CONTENT_PADDING_TOP,
            cell_width: svg::CELL_W,
            cell_height: svg::CELL_H,
        },
        frames: sources
            .values()
            .map(|source| Frame {
                screen: source.details.clone(),
                cells: Vec::new(),
                grid: Vec::new(),
                svg: None,
                omission: Some(
                    if source.rows.iter().map(Vec::len).sum::<usize>() > 100_000 {
                        "Cell grid and SVG omitted: frame exceeds 100,000 cells."
                    } else {
                        "Cell grid and SVG omitted: timeline byte limit."
                    }
                    .into(),
                ),
            })
            .collect(),
        truncated: false,
    };
    // Omitted frames still retain text and metadata. Reserve their actual encoded
    // size before giving the newest evidence the remaining grid/SVG budget.
    let metadata_bytes = serde_json::to_vec(&timeline)?.len();
    if metadata_bytes > limit {
        return Err(serde::ser::Error::custom(
            "frame text and metadata exceed the timeline byte limit",
        ));
    }
    let mut remaining = limit.saturating_sub(metadata_bytes.max(64 * 1024));
    for (frame, source) in timeline.frames.iter_mut().zip(sources.values()).rev() {
        let reserved = serde_json::to_vec(&frame)?.len();
        if source.rows.iter().map(Vec::len).sum::<usize>() <= 100_000 {
            frame.omission =
                Some("SVG omitted: timeline byte limit. Cell metadata is retained.".into());
            let mut dictionary = HashMap::new();
            for row in source.rows.iter() {
                let mut indices = Vec::with_capacity(row.len());
                for (x, cell) in row.iter().enumerate() {
                    let width = if cell.ch == CONTINUATION {
                        0
                    } else if row.get(x + 1).is_some_and(|next| next.ch == CONTINUATION) {
                        2
                    } else {
                        1
                    };
                    let value = FrameCell::capture(cell, width, &source.render_state);
                    let key = serde_json::to_string(&value)?;
                    let index = *dictionary.entry(key).or_insert_with(|| {
                        let index = frame.cells.len();
                        frame.cells.push(value);
                        index
                    });
                    indices.push(index);
                }
                frame.grid.push(indices);
            }
            if serde_json::to_vec(&frame)?.len() > remaining + reserved {
                frame.cells.clear();
                frame.grid.clear();
                frame.omission = Some("Cell grid and SVG omitted: timeline byte limit.".into());
            } else {
                frame.omission = None;
                let cursor = &frame.screen.cursor;
                frame.svg = Some(svg::render_svg_with_zoom(
                    &source.rows,
                    frame.screen.size.cols,
                    &source.render_state,
                    cursor
                        .visible
                        .then_some((cursor.column, usize::from(cursor.row))),
                    frame.screen.title.as_deref(),
                    1.0,
                    None,
                ));
                if serde_json::to_vec(&frame)?.len() > remaining + reserved {
                    frame.svg = None;
                    frame.omission =
                        Some("SVG omitted: timeline byte limit. Cell metadata is retained.".into());
                }
            }
        }
        remaining =
            remaining.saturating_sub(serde_json::to_vec(&frame)?.len().saturating_sub(reserved));
    }
    timeline.truncated = timeline.frames.iter().any(|frame| frame.omission.is_some());
    Ok(timeline)
}

#[derive(Serialize)]
struct EmbeddedFile {
    name: String,
    bytes: usize,
    data: String,
}

impl EmbeddedFile {
    fn new(name: &str, bytes: &[u8]) -> Self {
        Self {
            name: name.into(),
            bytes: bytes.len(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }
}

pub(super) fn render(
    details: &FailureReport,
    timeline: &Timeline,
    files: &[ArtifactFile],
    errors: &[String],
    directory: &Path,
) -> io::Result<String> {
    html_with_limit(
        details,
        timeline,
        files,
        errors,
        directory,
        super::HTML_LIMIT,
    )
}

fn html_with_limit(
    details: &FailureReport,
    timeline: &Timeline,
    files: &[ArtifactFile],
    errors: &[String],
    directory: &Path,
    limit: usize,
) -> io::Result<String> {
    #[derive(Serialize)]
    struct Payload<'a> {
        details: &'a FailureReport,
        timeline: &'a Timeline,
        files: &'a [ArtifactFile],
        errors: &'a [String],
        explanation: Explanation,
        attachments: Vec<EmbeddedFile>,
    }
    let mut attachments = Vec::new();
    let mut encoded_bytes = 0;
    for file in files
        .iter()
        .filter(|file| file.status == super::ArtifactFileStatus::Written)
    {
        let length = file
            .bytes
            .ok_or_else(|| io::Error::other("artifact has no recorded length"))?;
        if length > limit as u64 {
            return Err(io::Error::other(
                "embedded report exceeds the HTML byte limit",
            ));
        }
        encoded_bytes += (length as usize).div_ceil(3) * 4;
        if encoded_bytes > limit {
            return Err(io::Error::other(
                "embedded report exceeds the HTML byte limit",
            ));
        }
        let mut bytes = vec![0; length as usize];
        let mut source = std::fs::File::open(directory.join(&file.path))?;
        source.read_exact(&mut bytes)?;
        let extra = source.read(&mut [0])?;
        let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
        if extra != 0 || file.sha256.as_deref() != Some(hash.as_str()) {
            return Err(io::Error::other(format!(
                "{} changed before it could be embedded",
                file.path
            )));
        }
        attachments.push(EmbeddedFile::new(&file.path, &bytes));
    }
    if let Some(frame) = timeline
        .frames
        .iter()
        .find(|frame| frame.screen.sequence == timeline.failure_screen_sequence)
    {
        if !attachments.iter().any(|file| file.name == "current.txt") {
            attachments.push(EmbeddedFile::new(
                "current.txt",
                frame.screen.text.as_bytes(),
            ));
        }
        if let Some(svg) = frame
            .svg
            .as_ref()
            .filter(|_| !attachments.iter().any(|file| file.name == "current.svg"))
        {
            attachments.push(EmbeddedFile::new("current.svg", svg.as_bytes()));
        }
    }
    if !attachments.iter().any(|file| file.name == "timeline.json") {
        attachments.push(EmbeddedFile::new(
            "timeline.json",
            &serde_json::to_vec(timeline)?,
        ));
    }
    let markdown_name = details.artifact_name("md");
    if !attachments.iter().any(|file| file.name == markdown_name) {
        attachments.push(EmbeddedFile::new(
            &markdown_name,
            markdown(details, files).as_bytes(),
        ));
    }
    // The HTML cannot contain its own hash. Its embedded manifest is the snapshot
    // before the HTML write; a separately exported manifest adds the final hash.
    let mut sensitivity = super::sensitivity(details, files);
    sensitivity.contains_visual_output |= !timeline.frames.is_empty();
    let manifest = super::FailureArtifactManifest {
        details: details.clone(),
        files: files.to_vec(),
        errors: errors.to_vec(),
        sensitivity,
    };
    attachments.push(EmbeddedFile::new(
        &details.artifact_name("json"),
        &serde_json::to_vec_pretty(&manifest)?,
    ));
    let payload = Payload {
        details,
        timeline,
        files,
        errors,
        explanation: explanation(details),
        attachments,
    };
    let shell = include_str!("../../assets/trace-viewer/report.html")
        .replace(
            "/* REPORT_CSS */",
            include_str!("../../assets/trace-viewer/report.min.css"),
        )
        .replace(
            "/* REPORT_JS */",
            include_str!("../../assets/trace-viewer/report.min.js"),
        );
    let (prefix, suffix) = shell
        .split_once("null /* REPORT_DATA */")
        .ok_or_else(|| io::Error::other("report template has no data marker"))?;
    // Count without retaining JSON, then allocate one exact-sized output buffer.
    let mut size = BoundedSize {
        bytes: prefix.len() + suffix.len(),
        limit,
    };
    payload.serialize(&mut serde_json::Serializer::with_formatter(
        &mut size,
        HtmlJsonFormatter,
    ))?;
    let mut output = Vec::with_capacity(size.bytes);
    output.extend_from_slice(prefix.as_bytes());
    payload.serialize(&mut serde_json::Serializer::with_formatter(
        &mut output,
        HtmlJsonFormatter,
    ))?;
    output.extend_from_slice(suffix.as_bytes());
    drop(payload);
    String::from_utf8(output).map_err(io::Error::other)
}

struct BoundedSize {
    bytes: usize,
    limit: usize,
}

impl IoWrite for BoundedSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(io::Error::other(
                "embedded report exceeds the HTML byte limit",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HtmlJsonFormatter;

impl serde_json::ser::Formatter for HtmlJsonFormatter {
    fn write_string_fragment<W: IoWrite + ?Sized>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            let escaped = match character {
                '&' => b"\\u0026",
                '<' => b"\\u003c",
                '>' => b"\\u003e",
                '\u{2028}' => b"\\u2028",
                '\u{2029}' => b"\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..index])?;
            writer.write_all(escaped)?;
            start = index + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_fixture::{fixture, HOSTILE};
    use crate::diagnostics::*;
    use crate::profile::Profile;

    use crate::terminal::cell::{Color, UnderlineStyle};

    use super::render as html;

    #[test]
    fn timeline_preserves_grid_coordinates_palette_unicode_and_pinned_svg() {
        let (_, observation) = fixture();
        let timeline = timeline(&observation).unwrap();
        let frame = timeline.frames.last().unwrap();
        assert_eq!(frame.screen.sequence, observation.screen_sequence);
        assert_eq!(frame.svg.as_deref(), Some(observation.svg().as_str()));
        assert_eq!(frame.grid.len(), 5);
        assert!(frame.grid.iter().all(|row| row.len() == 24));
        let cell = |x| &frame.cells[frame.grid[0][x]];
        assert_eq!(cell(0).fg, CellColor::Indexed(1));
        assert_eq!(cell(0).resolved_fg, "#112233");
        assert_eq!(cell(1).char, "\u{4f60}");
        assert_eq!(cell(1).width, 2);
        assert_eq!(cell(2).char, "");
        assert_eq!(cell(2).width, 0);
        assert_eq!(cell(3).char, "e\u{301}");
        assert_eq!(cell(3).width, 1);
        assert_eq!(frame.cells[frame.grid[1][0]].char, " ");
        assert!(
            frame.cells.len() < 12,
            "blank cells must be dictionary-compressed"
        );
        assert_eq!(
            serde_json::to_vec(&timeline).unwrap(),
            serde_json::to_vec(&super::timeline(&observation).unwrap()).unwrap()
        );
    }

    #[test]
    fn cell_metadata_exposes_all_attributes_and_resolved_colors() {
        let cell = EmuCell {
            ch: "x".into(),
            fg: Some(Color::Rgb(100, 150, 200)),
            bg: Some(Color::Rgb(20, 40, 60)),
            underline: UnderlineStyle::Curly,
            underline_color: Some(Color::Rgb(1, 2, 3)),
            attrs: Attrs::all(),
            hyperlink: None,
        };
        let value = FrameCell::capture(&cell, 1, &Profile::default());
        assert_eq!(value.flags.len(), 7);
        assert_eq!(value.underline_style, "curly");
        assert_eq!(value.underline_color, CellColor::Rgb(1, 2, 3));
        assert_eq!(value.resolved_underline_color, "#010203");
        assert_eq!(value.resolved_bg, "#6496c8");
        assert_eq!(value.resolved_fg, "#0c1824");
    }

    #[test]
    fn html_embeds_inert_json_and_markdown_preserves_multiline_evidence() {
        let (details, observation) = fixture();
        let html = html(
            &details,
            &timeline(&observation).unwrap(),
            &[],
            &[],
            &std::env::temp_dir(),
        )
        .unwrap();
        let embedded = html
            .split_once("<script id=\"report-data\" type=\"application/json\">")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        let parsed: serde_json::Value = serde_json::from_str(embedded).unwrap();
        assert_eq!(parsed["details"]["summary"], HOSTILE);
        assert!(!embedded.contains('<'));
        assert_eq!(html.matches("<script").count(), 2);
        assert!(html.contains("connect-src 'none'"));
        assert!(!html.contains("<script src="));
        let markdown = markdown(&details, &[]);
        assert!(markdown.contains(&format!("````text\n{HOSTILE}\n````")));
        assert!(markdown.contains("999 (not retained)"));
        assert!(markdown.contains("[3](#screen-3)"));
        assert!(markdown.contains("## Assertion checkpoints"));
        assert!(markdown.contains("viewport"));
    }

    #[test]
    fn standalone_html_embeds_large_recordings_without_omitting_other_evidence() {
        let root =
            std::env::temp_dir().join(format!("tui-test-portable-report-{}", std::process::id()));
        let directory = allocate_artifact_directory(&root).unwrap();
        let (mut details, observation) = fixture();
        let cast = format!(
            "{{\"version\":2,\"width\":20,\"height\":4}}\n[0.01,\"o\",\"{}\"]\n",
            "x".repeat(9 * 1024 * 1024)
        )
        .into_bytes();
        let temporary_path = recording_temp_path(&directory);
        std::fs::write(&temporary_path, &cast).unwrap();
        let reference = write_failure_artifact(
            &FailureArtifactOptions {
                directory: root.clone(),
                include_recording: true,
                ..FailureArtifactOptions::default()
            },
            ArtifactInputs {
                details: &mut details,
                observation: &observation,
                recording: Some(PreparedRecording {
                    temporary_path,
                    bytes: cast.len() as u64,
                    sha256: format!("sha256:{:x}", Sha256::digest(&cast)),
                }),
            },
            directory,
        );
        assert_eq!(
            reference.status,
            FailureArtifactStatus::Written,
            "{:?}",
            reference.errors
        );
        let html = std::fs::read_to_string(reference.report_html.unwrap()).unwrap();
        assert!(html.len() > 12 * 1024 * 1024);
        assert!(html.len() <= crate::diagnostics::HTML_LIMIT);
        let embedded = html
            .split_once("<script id=\"report-data\" type=\"application/json\">")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        let payload: serde_json::Value = serde_json::from_str(embedded).unwrap();
        let attachments = payload["attachments"].as_array().unwrap();
        for name in [
            "current.svg",
            "current.txt",
            "timeline.json",
            "failure.md",
            "session.cast",
        ] {
            let file = attachments
                .iter()
                .find(|file| file["name"] == name)
                .unwrap();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(file["data"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                decoded,
                std::fs::read(Path::new(&reference.directory).join(name)).unwrap()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_frame_gets_the_timeline_byte_budget_first() {
        let (_, observation) = fixture();
        let full = timeline(&observation).unwrap();
        let last_bytes = serde_json::to_vec(full.frames.last().unwrap())
            .unwrap()
            .len();
        let limit = 64 * 1024 + last_bytes;
        let limited = timeline_with_limit(&observation, limit).unwrap();
        assert!(limited.frames.last().unwrap().svg.is_some());
        assert!(limited.frames[0].omission.is_some());
        assert!(serde_json::to_vec(&limited).unwrap().len() <= limit);
    }

    #[test]
    fn omitted_frames_still_consume_the_timeline_byte_budget() {
        let (_, mut observation) = fixture();
        observation.history = ScreenHistory::new(50);
        let rows: Vec<Vec<_>> = (0..40)
            .map(|y| {
                (0..80)
                    .map(|x| EmuCell {
                        ch: "x".into(),
                        fg: Some(Color::Rgb(x, y, 0)),
                        ..EmuCell::blank()
                    })
                    .collect()
            })
            .collect();
        for time in 0..32 {
            observation.screen_sequence = observation.history.capture(
                rows.clone(),
                80,
                Some(format!("frame {time}")),
                (0, 0),
                false,
                crate::terminal::emu::CursorShape::Block,
                time,
                observation.render_state.clone(),
            );
            observation.history.pin_current();
        }
        let full = timeline(&observation).unwrap();
        let last_bytes = serde_json::to_vec(full.frames.last().unwrap())
            .unwrap()
            .len();
        let limit = 64 * 1024 + last_bytes * 2;
        let limited = timeline_with_limit(&observation, limit).unwrap();
        assert!(limited.frames.last().unwrap().svg.is_some());
        assert!(limited.is_truncated());
        assert!(
            serde_json::to_vec(&limited).unwrap().len() <= limit,
            "omitted frame metadata must be included in the byte budget"
        );
    }

    #[test]
    fn oversized_frame_text_fails_the_timeline_budget_explicitly() {
        let (_, mut observation) = fixture();
        std::sync::Arc::make_mut(observation.history.entries.back_mut().unwrap())
            .details
            .text = "x".repeat(64 * 1024);
        let error = timeline_with_limit(&observation, 64 * 1024)
            .err()
            .expect("oversized text must not be embedded past the timeline limit");
        assert!(error.to_string().contains("frame text and metadata"));
    }

    #[test]
    fn artifact_modes_export_exactly_the_requested_files() {
        let root =
            std::env::temp_dir().join(format!("tui-test-report-modes-{}", std::process::id()));
        for mode in [
            FailureArtifactMode::All,
            FailureArtifactMode::Html,
            FailureArtifactMode::Text,
            FailureArtifactMode::None,
        ] {
            let (mut details, observation) = fixture();
            let directory = allocate_artifact_directory(&root).unwrap();
            let reference = write_failure_artifact(
                &FailureArtifactOptions {
                    directory: root.clone(),
                    mode,
                    include_recording: false,
                },
                ArtifactInputs {
                    details: &mut details,
                    observation: &observation,
                    recording: None,
                },
                directory.clone(),
            );
            let mut names: Vec<_> = std::fs::read_dir(&directory)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            let expected: &[&str] = match mode {
                FailureArtifactMode::All => &[
                    "current.svg",
                    "current.txt",
                    "failure.html",
                    "failure.json",
                    "failure.md",
                    "timeline.json",
                ],
                FailureArtifactMode::Html => &["failure.html"],
                FailureArtifactMode::Text => &["current.txt", "failure.json", "failure.md"],
                FailureArtifactMode::None => &[],
            };
            assert_eq!(names, expected, "{mode:?}");
            if mode != FailureArtifactMode::None {
                assert_eq!(reference.status, FailureArtifactStatus::Written);
            }
            if let Some(path) = reference.report_html {
                let html = std::fs::read_to_string(path).unwrap();
                let embedded = html
                    .split_once("<script id=\"report-data\" type=\"application/json\">")
                    .unwrap()
                    .1
                    .split_once("</script>")
                    .unwrap()
                    .0;
                let data: serde_json::Value = serde_json::from_str(embedded).unwrap();
                assert_eq!(data["details"]["summary"], HOSTILE);
                for name in [
                    "current.txt",
                    "current.svg",
                    "failure.json",
                    "failure.md",
                    "timeline.json",
                ] {
                    assert!(data["attachments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|file| file["name"] == name));
                }
                let manifest = data["attachments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|file| file["name"] == "failure.json")
                    .unwrap();
                let manifest = base64::engine::general_purpose::STANDARD
                    .decode(manifest["data"].as_str().unwrap())
                    .unwrap();
                let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
                assert_eq!(
                    manifest["sensitivity"]["contains_visual_output"], true,
                    "{mode:?}"
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn html_write_errors_are_explicit_with_or_without_a_json_sidecar() {
        let root =
            std::env::temp_dir().join(format!("tui-test-html-errors-{}", std::process::id()));
        for mode in [FailureArtifactMode::All, FailureArtifactMode::Html] {
            let (mut details, observation) = fixture();
            let directory = allocate_artifact_directory(&root).unwrap();
            std::fs::create_dir(directory.join("failure.html")).unwrap();
            let reference = write_failure_artifact(
                &FailureArtifactOptions {
                    directory: root.clone(),
                    mode,
                    include_recording: false,
                },
                ArtifactInputs {
                    details: &mut details,
                    observation: &observation,
                    recording: None,
                },
                directory,
            );
            assert!(reference.report_html.is_none());
            assert!(reference
                .errors
                .iter()
                .any(|error| error.contains("failure.html")));
            assert_eq!(
                reference.status,
                if mode == FailureArtifactMode::All {
                    FailureArtifactStatus::Partial
                } else {
                    FailureArtifactStatus::Failed
                }
            );
            assert_eq!(
                reference.manifest.is_some(),
                mode == FailureArtifactMode::All
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timeline_reports_omissions_instead_of_substituting_frames() {
        let (_, observation) = fixture();
        let limited = timeline_with_limit(&observation, 64 * 1024).unwrap();
        assert!(limited.is_truncated());
        assert!(serde_json::to_vec(&limited).unwrap().len() <= 64 * 1024);
        assert!(limited.frames.iter().all(|frame| frame.svg.is_none()
            && frame.grid.is_empty()
            && frame.omission.as_deref().unwrap().contains("byte limit")));
        assert_eq!(
            limited.frames.last().unwrap().screen.sequence,
            observation.screen_sequence
        );
        assert_eq!(
            limited.frames.last().unwrap().screen.text,
            observation.text()
        );
        let mut large = observation;
        std::sync::Arc::make_mut(large.history.entries.back_mut().unwrap()).rows =
            std::sync::Arc::new(vec![vec![EmuCell::blank(); 100_001]]);
        let limited = timeline(&large).unwrap();
        assert!(limited
            .frames
            .last()
            .unwrap()
            .omission
            .as_deref()
            .unwrap()
            .contains("100,000"));
    }

    #[test]
    fn embedded_json_escapes_keys_and_values_and_enforces_the_output_limit() {
        let value = serde_json::json!({ "<>&\u{2028}\u{2029}": HOSTILE });
        let mut encoded = Vec::new();
        value
            .serialize(&mut serde_json::Serializer::with_formatter(
                &mut encoded,
                HtmlJsonFormatter,
            ))
            .unwrap();
        let encoded = String::from_utf8(encoded).unwrap();
        assert!(!encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            value
        );
        let (details, observation) = fixture();
        let timeline = timeline(&observation).unwrap();
        let directory = std::env::temp_dir();
        let full = html(&details, &timeline, &[], &[], &directory).unwrap();
        assert!(
            html_with_limit(&details, &timeline, &[], &[], &directory, full.len() - 1)
                .unwrap_err()
                .to_string()
                .contains("HTML byte limit")
        );
        assert_eq!(
            html_with_limit(&details, &timeline, &[], &[], &directory, full.len()).unwrap(),
            full
        );
    }

    #[test]
    fn a_full_recording_fits_the_html_report_budget() {
        let root =
            std::env::temp_dir().join(format!("tui-test-html-memory-{}", std::process::id()));
        let directory = allocate_artifact_directory(&root).unwrap();
        let length = 64 * 1024 * 1024;
        let chunk = vec![b'x'; 64 * 1024];
        let mut recording = std::fs::File::create(directory.join("session.cast")).unwrap();
        let mut digest = Sha256::new();
        for _ in 0..length / chunk.len() {
            recording.write_all(&chunk).unwrap();
            digest.update(&chunk);
        }
        drop(recording);
        let file = ArtifactFile {
            kind: "recording".into(),
            path: "session.cast".into(),
            status: ArtifactFileStatus::Written,
            bytes: Some(length as u64),
            sha256: Some(format!("sha256:{:x}", digest.finalize())),
            reason: None,
        };
        let (details, observation) = fixture();
        let timeline = timeline(&observation).unwrap();
        let report = html(&details, &timeline, &[file], &[], &directory).unwrap();
        assert!(report.len() > length * 4 / 3);
        assert!(report.len() <= crate::diagnostics::HTML_LIMIT);
        std::fs::remove_dir_all(&directory).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
