//! Text/regex search over the terminal grid, including scoped and normalized
//! selectors. Match offsets are mapped back to terminal cells.

use regex::Regex;

use crate::api::{
    LocatorQuery, LocatorSelector, MatchOccurrence, StyleSelector, TextAnchor, TextMatch,
    TextPosition, TextSelector, TextSpan, TextStyle, WhitespaceMode,
};

use super::cell::EmuCell;

pub enum Pattern {
    Text(String),
    Regex(Regex),
}

impl Pattern {
    pub fn new(text: &str, is_regex: bool) -> anyhow::Result<Self> {
        if is_regex {
            Ok(Pattern::Regex(Regex::new(text)?))
        } else {
            Ok(Pattern::Text(text.to_string()))
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Pattern::Text(text) => text.clone(),
            Pattern::Regex(regex) => regex.as_str().to_string(),
        }
    }

    pub fn matches(&self, haystack: &str) -> bool {
        match self {
            Pattern::Text(text) => haystack.contains(text.as_str()),
            Pattern::Regex(regex) => regex.is_match(haystack),
        }
    }

    fn ranges(&self, chars: &[char]) -> Vec<(usize, usize)> {
        match self {
            Pattern::Text(text) => {
                let needle: Vec<char> = text.chars().collect();
                text_ranges(chars, &needle)
            }
            Pattern::Regex(regex) => {
                let block: String = chars.iter().collect();
                regex
                    .find_iter(&block)
                    .filter(|matched| !matched.is_empty())
                    .map(|matched| {
                        let start = block[..matched.start()].chars().count();
                        (start, start + matched.as_str().chars().count())
                    })
                    .collect()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchedCell {
    pub x: usize,
    pub y: usize,
    pub cell: EmuCell,
}

#[derive(Debug, Clone)]
pub struct LocatedMatch {
    pub value: TextMatch,
    pub cells: Vec<MatchedCell>,
    pub(crate) source_start: usize,
    pub(crate) source_end: usize,
}

struct FlatGrid {
    chars: Vec<char>,
    sources: Vec<usize>,
    width: usize,
}

/// Locate the matches selected by `selector`.
pub fn locate(rows: &[Vec<EmuCell>], selector: &TextSelector) -> anyhow::Result<Vec<LocatedMatch>> {
    locate_text_within(rows, selector, None)
}

pub fn locate_query<F>(
    rows: &[Vec<EmuCell>],
    query: &LocatorQuery,
    style_matches: &mut F,
) -> anyhow::Result<Vec<LocatedMatch>>
where
    F: FnMut(&EmuCell, &TextStyle) -> bool,
{
    let allowed = match query.within.as_deref() {
        Some(parent) => {
            let parents = locate_query(rows, parent, style_matches)?;
            if parents.is_empty() {
                return Ok(Vec::new());
            }
            Some(
                parents
                    .into_iter()
                    .map(|matched| (matched.source_start, matched.source_end))
                    .collect::<Vec<_>>(),
            )
        }
        None => None,
    };
    match &query.selector {
        LocatorSelector::Text(selector) => locate_text_within(rows, selector, allowed.as_deref()),
        LocatorSelector::Style(selector) => {
            locate_style_within(rows, selector, allowed.as_deref(), style_matches)
        }
    }
}

fn locate_text_within(
    rows: &[Vec<EmuCell>],
    selector: &TextSelector,
    allowed: Option<&[(usize, usize)]>,
) -> anyhow::Result<Vec<LocatedMatch>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let flat = flatten(rows, selector.whitespace);
    let Some((start, end)) = scope(&flat, selector)? else {
        return Ok(Vec::new());
    };
    let pattern = selector_pattern(&selector.text, selector.regex, selector.whitespace)?;
    let ranges: Vec<_> = pattern
        .ranges(&flat.chars)
        .into_iter()
        .filter(|(match_start, match_end)| *match_start >= start && *match_end <= end)
        .filter(|range| {
            allowed.is_none_or(|allowed| {
                source_range(&flat, *range).is_some_and(|(source_start, source_end)| {
                    allowed.iter().any(|(allowed_start, allowed_end)| {
                        source_start >= *allowed_start && source_end <= *allowed_end
                    })
                })
            })
        })
        .collect();
    let selected = select(ranges, &selector.occurrence, &pattern.describe())?;
    Ok(selected
        .into_iter()
        .filter_map(|range| materialize(rows, &flat, range))
        .collect())
}

fn locate_style_within<F>(
    rows: &[Vec<EmuCell>],
    selector: &StyleSelector,
    allowed: Option<&[(usize, usize)]>,
    style_matches: &mut F,
) -> anyhow::Result<Vec<LocatedMatch>>
where
    F: FnMut(&EmuCell, &TextStyle) -> bool,
{
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let flat = flatten(rows, WhitespaceMode::Exact);
    let mut ranges = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let mut start = None;
        let mut containing_regions = Vec::new();
        for (x, cell) in row.iter().enumerate() {
            let position = x + y * flat.width;
            let cell_regions = match allowed {
                Some(regions) => regions
                    .iter()
                    .enumerate()
                    .filter_map(|(index, (region_start, region_end))| {
                        (position >= *region_start && position < *region_end).then_some(index)
                    })
                    .collect::<Vec<_>>(),
                None => vec![0],
            };
            if !cell_regions.is_empty() && style_matches(cell, &selector.style) {
                if start.is_none() {
                    start = Some(position);
                    containing_regions = cell_regions;
                    continue;
                }
                containing_regions.retain(|region| cell_regions.contains(region));
                if containing_regions.is_empty() {
                    ranges.push((
                        start.replace(position).expect("style run already started"),
                        position,
                    ));
                    containing_regions = cell_regions;
                }
            } else if let Some(start) = start.take() {
                ranges.push((start, position));
                containing_regions.clear();
            }
        }
        if let Some(start) = start {
            ranges.push((start, y * flat.width + row.len()));
        }
    }
    let selected = select(ranges, &selector.occurrence, "style")?;
    Ok(selected
        .into_iter()
        .filter_map(|range| materialize(rows, &flat, range))
        .collect())
}

fn source_range(flat: &FlatGrid, (start, end): (usize, usize)) -> Option<(usize, usize)> {
    if start >= end {
        return None;
    }
    Some((
        *flat.sources.get(start)?,
        flat.sources.get(end - 1)?.saturating_add(1),
    ))
}

/// Compatibility helper for the simple text waits and mouse text lookup.
pub fn find(
    rows: &[Vec<EmuCell>],
    pattern: &Pattern,
    strict: bool,
) -> anyhow::Result<Option<Vec<MatchedCell>>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let flat = flatten(rows, WhitespaceMode::Exact);
    let occurrence = if strict {
        MatchOccurrence::Unique
    } else {
        MatchOccurrence::First
    };
    let selected = select(
        pattern.ranges(&flat.chars),
        &occurrence,
        &pattern.describe(),
    )?;
    Ok(selected
        .into_iter()
        .next()
        .and_then(|range| materialize(rows, &flat, range))
        .map(|matched| matched.cells))
}

fn selector_pattern(
    text: &str,
    regex: bool,
    whitespace: WhitespaceMode,
) -> anyhow::Result<Pattern> {
    let text = if !regex && whitespace == WhitespaceMode::Normalize {
        normalize(text)
    } else {
        text.to_string()
    };
    Pattern::new(&text, regex)
}

fn flatten(rows: &[Vec<EmuCell>], whitespace: WhitespaceMode) -> FlatGrid {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let source = rows.iter().enumerate().flat_map(|(y, row)| {
        (0..width).map(move |x| {
            (
                x + y * width,
                row.get(x)
                    .and_then(|cell| cell.ch.chars().next())
                    .unwrap_or(' '),
            )
        })
    });
    let mut chars = Vec::new();
    let mut sources = Vec::new();
    let mut pending_space = None;
    for (position, ch) in source {
        if whitespace == WhitespaceMode::Normalize && ch.is_whitespace() {
            if !chars.is_empty() && pending_space.is_none() {
                pending_space = Some(position);
            }
            continue;
        }
        if let Some(position) = pending_space.take() {
            chars.push(' ');
            sources.push(position);
        }
        chars.push(ch);
        sources.push(position);
    }
    FlatGrid {
        chars,
        sources,
        width,
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn scope(flat: &FlatGrid, selector: &TextSelector) -> anyhow::Result<Option<(usize, usize)>> {
    let start = match &selector.scope.after {
        Some(anchor) => match anchor_range(flat, anchor, selector.whitespace, "after")? {
            Some((_, end)) => end,
            None => return Ok(None),
        },
        None => 0,
    };
    let end = match &selector.scope.before {
        Some(anchor) => match anchor_range(flat, anchor, selector.whitespace, "before")? {
            Some((start, _)) => start,
            None => return Ok(None),
        },
        None => flat.chars.len(),
    };
    Ok((start <= end).then_some((start, end)))
}

fn anchor_range(
    flat: &FlatGrid,
    anchor: &TextAnchor,
    whitespace: WhitespaceMode,
    name: &str,
) -> anyhow::Result<Option<(usize, usize)>> {
    let pattern = selector_pattern(&anchor.text, anchor.regex, whitespace)?;
    let ranges = select(
        pattern.ranges(&flat.chars),
        &anchor.occurrence,
        &format!("{name} anchor '{}'", pattern.describe()),
    )?;
    if ranges.len() > 1 {
        anyhow::bail!("{name} anchor must select one match");
    }
    Ok(ranges.into_iter().next())
}

fn select(
    ranges: Vec<(usize, usize)>,
    occurrence: &MatchOccurrence,
    description: &str,
) -> anyhow::Result<Vec<(usize, usize)>> {
    let count = ranges.len();
    match occurrence {
        MatchOccurrence::Any => Ok(ranges),
        MatchOccurrence::Unique if count > 1 => anyhow::bail!(
            "unique match expected one occurrence of '{description}', but found {count}"
        ),
        MatchOccurrence::Unique | MatchOccurrence::First => {
            Ok(ranges.into_iter().next().into_iter().collect())
        }
        MatchOccurrence::Last => Ok(ranges.into_iter().last().into_iter().collect()),
        MatchOccurrence::Nth(index) => Ok(ranges.into_iter().nth(*index).into_iter().collect()),
    }
}

fn materialize(
    rows: &[Vec<EmuCell>],
    flat: &FlatGrid,
    (start, end): (usize, usize),
) -> Option<LocatedMatch> {
    let (source_start, source_end) = source_range(flat, (start, end))?;
    let mut cells = Vec::new();
    for position in source_start..source_end {
        let y = position / flat.width;
        let x = position % flat.width;
        if let Some(cell) = rows.get(y).and_then(|row| row.get(x)) {
            cells.push(MatchedCell {
                x,
                y,
                cell: cell.clone(),
            });
        }
    }
    let first = cells.first()?;
    let last = cells.last()?;
    let mut spans = Vec::new();
    for cell in &cells {
        match spans.last_mut() {
            Some(TextSpan { row, end, .. })
                if *row as usize == cell.y && *end as usize == cell.x =>
            {
                *end = end.saturating_add(1);
            }
            _ => spans.push(TextSpan {
                row: cell.y.min(u16::MAX as usize) as u16,
                start: cell.x.min(u16::MAX as usize) as u16,
                end: cell.x.saturating_add(1).min(u16::MAX as usize) as u16,
            }),
        }
    }
    Some(LocatedMatch {
        value: TextMatch {
            text: flat.chars[start..end].iter().collect(),
            start: TextPosition {
                row: first.y.min(u16::MAX as usize) as u16,
                column: first.x.min(u16::MAX as usize) as u16,
            },
            end: TextPosition {
                row: last.y.min(u16::MAX as usize) as u16,
                column: last.x.saturating_add(1).min(u16::MAX as usize) as u16,
            },
            spans,
        },
        cells,
        source_start,
        source_end,
    })
}

fn text_ranges(haystack: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if haystack[index..index + needle.len()] == *needle {
            ranges.push((index, index + needle.len()));
            index += needle.len();
        } else {
            index += 1;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{TextScope, WhitespaceMode};
    use crate::terminal::cell::Attrs;

    fn grid(lines: &[&str]) -> Vec<Vec<EmuCell>> {
        lines
            .iter()
            .map(|line| {
                line.chars()
                    .map(|ch| EmuCell {
                        ch: ch.to_string().into(),
                        ..EmuCell::blank()
                    })
                    .collect()
            })
            .collect()
    }

    fn locate_query_text(
        rows: &[Vec<EmuCell>],
        query: &LocatorQuery,
    ) -> anyhow::Result<Vec<LocatedMatch>> {
        locate_query(rows, query, &mut |cell, style| {
            style
                .bold
                .is_none_or(|expected| expected == cell.has(Attrs::BOLD))
        })
    }

    #[test]
    fn normalizes_whitespace_and_preserves_locations() {
        let mut selector = TextSelector::new("hello world");
        selector.whitespace = WhitespaceMode::Normalize;
        let found = locate(&grid(&["  hello", "    world  "]), &selector).unwrap();
        assert_eq!(found[0].value.text, "hello world");
        assert_eq!(found[0].value.start, TextPosition { row: 0, column: 2 });
        assert_eq!(found[0].value.end, TextPosition { row: 1, column: 9 });
    }

    #[test]
    fn scopes_a_match_after_an_anchor() {
        let mut selector = TextSelector::new("Save");
        selector.occurrence = MatchOccurrence::First;
        selector.scope = TextScope {
            after: Some(TextAnchor {
                text: "Settings".into(),
                regex: false,
                occurrence: MatchOccurrence::Unique,
            }),
            before: None,
        };
        let found = locate(&grid(&["Save", "Settings", "Save"]), &selector).unwrap();
        assert_eq!(found[0].value.start, TextPosition { row: 2, column: 0 });
    }

    #[test]
    fn selects_any_last_and_nth_occurrences() {
        let rows = grid(&["item item item"]);
        let mut selector = TextSelector::new("item");
        selector.occurrence = MatchOccurrence::Any;
        assert_eq!(locate(&rows, &selector).unwrap().len(), 3);
        selector.occurrence = MatchOccurrence::Last;
        assert_eq!(locate(&rows, &selector).unwrap()[0].value.start.column, 10);
        selector.occurrence = MatchOccurrence::Nth(1);
        assert_eq!(locate(&rows, &selector).unwrap()[0].value.start.column, 5);
    }

    #[test]
    fn unique_reports_ambiguous_text() {
        let mut selector = TextSelector::new("same");
        selector.occurrence = MatchOccurrence::Unique;
        let error = locate(&grid(&["same same"]), &selector).unwrap_err();
        assert!(error.to_string().contains("found 2"));
    }

    #[test]
    fn selectors_find_all_occurrences_by_default() {
        let found = locate(&grid(&["same same"]), &TextSelector::new("same")).unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn child_locators_match_only_inside_parent_regions() {
        let mut parent = TextSelector::new("Settings Save");
        parent.whitespace = WhitespaceMode::Normalize;
        let child = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("Save")),
            within: Some(Box::new(LocatorQuery::text(parent))),
        };

        let found =
            locate_query_text(&grid(&["Settings", "  Save", "Save outside"]), &child).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start, TextPosition { row: 1, column: 2 });
    }

    #[test]
    fn child_locators_honor_parent_occurrence_selection() {
        let rows = grid(&["panel: Save", "panel: Save"]);
        let mut parent = TextSelector::new("panel: Save");
        parent.occurrence = MatchOccurrence::Last;
        let child = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("Save")),
            within: Some(Box::new(LocatorQuery::text(parent))),
        };

        let found = locate_query_text(&rows, &child).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start, TextPosition { row: 1, column: 7 });
    }

    #[test]
    fn locator_regions_compose_across_multiple_levels() {
        let outer = TextSelector::new("panel [Save]");
        let middle = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("[Save]")),
            within: Some(Box::new(LocatorQuery::text(outer))),
        };
        let inner = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("Save")),
            within: Some(Box::new(middle)),
        };

        let found = locate_query_text(&grid(&["panel [Save]", "Save outside"]), &inner).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start, TextPosition { row: 0, column: 7 });
    }

    #[test]
    fn child_occurrences_are_selected_across_all_parent_regions() {
        let rows = grid(&["[a]", "[a]"]);
        let parent = TextSelector::new("[a]");
        let mut child = TextSelector::new("a");
        let query = |child| LocatorQuery {
            selector: LocatorSelector::Text(child),
            within: Some(Box::new(LocatorQuery::text(parent.clone()))),
        };
        assert_eq!(
            locate_query_text(&rows, &query(child.clone()))
                .unwrap()
                .len(),
            2
        );

        child.occurrence = MatchOccurrence::Nth(1);
        assert_eq!(
            locate_query_text(&rows, &query(child.clone())).unwrap()[0]
                .value
                .start,
            TextPosition { row: 1, column: 1 }
        );
        child.occurrence = MatchOccurrence::Unique;
        assert!(locate_query_text(&rows, &query(child))
            .unwrap_err()
            .to_string()
            .contains("found 2"));
    }

    #[test]
    fn child_matches_cannot_span_separate_parent_regions() {
        let rows = grid(&["ab  ", "  ab"]);
        let parent = TextSelector::new("ab");
        let mut child = TextSelector::new("b a");
        child.whitespace = WhitespaceMode::Normalize;
        let query = LocatorQuery {
            selector: LocatorSelector::Text(child),
            within: Some(Box::new(LocatorQuery::text(parent))),
        };
        assert!(locate_query_text(&rows, &query).unwrap().is_empty());
    }

    #[test]
    fn text_and_style_stages_chain_in_both_directions() {
        let mut rows = grid(&["plain BOLD end"]);
        for cell in &mut rows[0][6..10] {
            cell.attrs.insert(Attrs::BOLD);
        }

        let style = StyleSelector::from(TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        });
        let styled_text = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("OL")),
            within: Some(Box::new(LocatorQuery::style(style.clone()))),
        };
        let found = locate_query_text(&rows, &styled_text).unwrap();
        assert_eq!(found[0].value.start.column, 7);

        let text_style = LocatorQuery {
            selector: LocatorSelector::Style(style),
            within: Some(Box::new(LocatorQuery::text("BOLD"))),
        };
        let found = locate_query_text(&rows, &text_style).unwrap();
        assert_eq!(found[0].value.text, "BOLD");
    }

    #[test]
    fn style_runs_do_not_merge_adjacent_parent_matches() {
        let mut rows = grid(&["XX"]);
        for cell in &mut rows[0] {
            cell.attrs.insert(Attrs::BOLD);
        }
        let parent = LocatorQuery::text("X");
        let query = |occurrence| LocatorQuery {
            selector: LocatorSelector::Style(StyleSelector {
                style: TextStyle {
                    bold: Some(true),
                    ..TextStyle::default()
                },
                occurrence,
                ..StyleSelector::default()
            }),
            within: Some(Box::new(parent.clone())),
        };

        let found = locate_query_text(&rows, &query(MatchOccurrence::Any)).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].value.text, "X");
        assert_eq!(
            locate_query_text(&rows, &query(MatchOccurrence::Nth(1))).unwrap()[0]
                .value
                .start
                .column,
            1
        );
        assert!(locate_query_text(&rows, &query(MatchOccurrence::Unique))
            .unwrap_err()
            .to_string()
            .contains("found 2"));
    }
}
