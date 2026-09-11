//! Text/regex search over the terminal grid, including scoped and normalized
//! selectors. Match offsets are mapped back to terminal cells.

use regex::Regex;

use crate::api::{
    LocatorDirection, LocatorQuery, LocatorSelector, MatchOccurrence, StyleSelector, TextAnchor,
    TextMatch, TextPosition, TextSelector, TextSpan, TextStyle, WhitespaceMode,
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
                let mut byte_offset = 0;
                let mut char_offset = 0;
                regex
                    .find_iter(&block)
                    .filter_map(|matched| {
                        if matched.is_empty() {
                            return None;
                        }
                        char_offset += block[byte_offset..matched.start()].chars().count();
                        let start = char_offset;
                        char_offset += matched.as_str().chars().count();
                        byte_offset = matched.end();
                        Some((start, char_offset))
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
    locate_text_within(rows, selector, None, None)
}

pub fn locate_query<F>(
    rows: &[Vec<EmuCell>],
    query: &LocatorQuery,
    style_matches: &mut F,
) -> anyhow::Result<Vec<LocatedMatch>>
where
    F: FnMut(&EmuCell, &TextStyle) -> bool,
{
    locate_query_within(rows, query, None, style_matches)
}

fn locate_query_within<F>(
    rows: &[Vec<EmuCell>],
    query: &LocatorQuery,
    enclosing: Option<&[(usize, usize)]>,
    style_matches: &mut F,
) -> anyhow::Result<Vec<LocatedMatch>>
where
    F: FnMut(&EmuCell, &TextStyle) -> bool,
{
    let mut parents = match query.within.as_deref() {
        Some(parent) => {
            let mut parents = locate_query_within(rows, parent, enclosing, style_matches)?;
            if parents.is_empty() {
                return Ok(Vec::new());
            }
            parents.sort_by_key(|matched| matched.source_start);
            Some(parents)
        }
        None => None,
    };
    let relative = parents.as_ref().map(|parents| {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        relative_regions(parents, query.direction, width.saturating_mul(rows.len()))
    });
    let allowed = match (relative, enclosing) {
        (Some(relative), Some(enclosing)) => Some(
            relative
                .iter()
                .flat_map(|region| intersect_ranges(std::slice::from_ref(region), enclosing))
                .collect(),
        ),
        (Some(relative), None) => Some(relative),
        (None, Some(enclosing)) => Some(enclosing.to_vec()),
        (None, None) => None,
    };
    let occurrence = query.occurrence.clone();
    let select_early = query.style.is_empty();
    let mut selected_early = false;
    let mut matches = match &query.selector {
        LocatorSelector::Text(selector) => {
            selected_early = select_early;
            locate_text_within(
                rows,
                selector,
                allowed.as_deref(),
                select_early.then_some(&occurrence),
            )?
        }
        LocatorSelector::Style(selector)
            if query.direction == LocatorDirection::Within && parents.is_some() =>
        {
            let mut matches = parents.take().expect("parent matches are present");
            matches
                .retain(|matched| located_match_has_style(matched, &selector.style, style_matches));
            matches
        }
        LocatorSelector::Style(selector) => {
            selected_early = select_early;
            locate_style_within(
                rows,
                selector,
                allowed.as_deref(),
                select_early.then_some(&occurrence),
                style_matches,
            )?
        }
        LocatorSelector::Link(selector)
            if query.direction == LocatorDirection::Within && parents.is_some() =>
        {
            let mut matches = parents.take().expect("parent matches are present");
            matches.retain(|matched| {
                matched
                    .cells
                    .iter()
                    .all(|cell| cell_has_link(&cell.cell, &selector.uri))
            });
            matches
        }
        LocatorSelector::Link(selector) => {
            locate_cells_within(rows, allowed.as_deref(), &mut |cell| {
                cell_has_link(cell, &selector.uri)
            })?
        }
        LocatorSelector::And { left, right } | LocatorSelector::Or { left, right } => {
            let left = locate_query_within(rows, left, allowed.as_deref(), style_matches)?;
            let right = locate_query_within(rows, right, allowed.as_deref(), style_matches)?;
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            let left = coverage(&left, width);
            let right = coverage(&right, width);
            let ranges = if matches!(&query.selector, LocatorSelector::And { .. }) {
                intersect_ranges(&left, &right)
            } else {
                normalize_ranges(left.into_iter().chain(right).collect())
            };
            materialize_runs(rows, &ranges)
        }
        LocatorSelector::Filter {
            input,
            has,
            has_not,
        } => {
            let candidates = locate_query_within(rows, input, allowed.as_deref(), style_matches)?;
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            let mut retained = Vec::new();
            for candidate in candidates {
                let scope = coverage(std::slice::from_ref(&candidate), width);
                let positive = match has {
                    Some(inner) => {
                        !locate_query_within(rows, inner, Some(&scope), style_matches)?.is_empty()
                    }
                    None => true,
                };
                let negative = match has_not {
                    Some(inner) => {
                        locate_query_within(rows, inner, Some(&scope), style_matches)?.is_empty()
                    }
                    None => true,
                };
                if positive && negative {
                    retained.push(candidate);
                }
            }
            retained
        }
    };
    if !query.style.is_empty() {
        matches.retain(|matched| located_match_has_style(matched, &query.style, style_matches));
    }
    if selected_early {
        Ok(matches)
    } else {
        select_items(matches, &occurrence, &query.selector.description())
    }
}

fn located_match_has_style<F>(
    matched: &LocatedMatch,
    style: &TextStyle,
    style_matches: &mut F,
) -> bool
where
    F: FnMut(&EmuCell, &TextStyle) -> bool,
{
    let visible = |cell: &MatchedCell| {
        !cell.cell.ch.is_empty() && !cell.cell.ch.chars().all(char::is_whitespace)
    };
    let has_visible = matched.cells.iter().any(&visible);
    matched
        .cells
        .iter()
        .filter(|cell| !has_visible || visible(cell))
        .all(|cell| style_matches(&cell.cell, style))
}

fn cell_has_link(cell: &EmuCell, uri: &str) -> bool {
    cell.uri().unwrap_or_default() == uri
}

fn normalize_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges.into_iter().filter(|(start, end)| start < end) {
        if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn intersect_ranges(left: &[(usize, usize)], right: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let left = normalize_ranges(left.to_vec());
    let right = normalize_ranges(right.to_vec());
    let (mut i, mut j) = (0, 0);
    let mut result = Vec::new();
    while i < left.len() && j < right.len() {
        let start = left[i].0.max(right[j].0);
        let end = left[i].1.min(right[j].1);
        if start < end {
            result.push((start, end));
        }
        if left[i].1 <= right[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

fn coverage(matches: &[LocatedMatch], width: usize) -> Vec<(usize, usize)> {
    normalize_ranges(
        matches
            .iter()
            .flat_map(|matched| {
                matched.value.spans.iter().map(|span| {
                    let row = span.row as usize * width;
                    (row + span.start as usize, row + span.end as usize)
                })
            })
            .collect(),
    )
}

fn materialize_runs(rows: &[Vec<EmuCell>], ranges: &[(usize, usize)]) -> Vec<LocatedMatch> {
    let flat = flatten(rows, WhitespaceMode::Exact);
    if flat.width == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for &(mut start, end) in ranges {
        while start < end {
            let row_end = end.min((start / flat.width + 1) * flat.width);
            if let Some(matched) = materialize(rows, &flat, (start, row_end)) {
                matches.push(matched);
            }
            start = row_end;
        }
    }
    matches
}

fn locate_cells_within<F>(
    rows: &[Vec<EmuCell>],
    allowed: Option<&[(usize, usize)]>,
    predicate: &mut F,
) -> anyhow::Result<Vec<LocatedMatch>>
where
    F: FnMut(&EmuCell) -> bool,
{
    locate_style_within(
        rows,
        &StyleSelector::default(),
        allowed,
        None,
        &mut |cell, _| predicate(cell),
    )
}

fn relative_regions(
    parents: &[LocatedMatch],
    direction: LocatorDirection,
    source_len: usize,
) -> Vec<(usize, usize)> {
    parents
        .iter()
        .enumerate()
        .filter_map(|(index, parent)| {
            let range = match direction {
                LocatorDirection::Within => (parent.source_start, parent.source_end),
                LocatorDirection::After => (
                    parent.source_end,
                    parents
                        .get(index + 1)
                        .map_or(source_len, |next| next.source_start),
                ),
                LocatorDirection::Before => (
                    index
                        .checked_sub(1)
                        .and_then(|previous| parents.get(previous))
                        .map_or(0, |previous| previous.source_end),
                    parent.source_start,
                ),
            };
            (range.0 <= range.1).then_some(range)
        })
        .collect()
}

fn locate_text_within(
    rows: &[Vec<EmuCell>],
    selector: &TextSelector,
    allowed: Option<&[(usize, usize)]>,
    occurrence: Option<&MatchOccurrence>,
) -> anyhow::Result<Vec<LocatedMatch>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let flat = flatten(rows, selector.whitespace);
    let Some((start, end)) = scope(&flat, selector)? else {
        return Ok(Vec::new());
    };
    let pattern = selector_pattern(&selector.text, selector.regex, selector.whitespace)?;
    let search_regions = match allowed {
        Some(regions) => regions
            .iter()
            .map(|&(a, b)| {
                (
                    start.max(flat.sources.partition_point(|source| *source < a)),
                    end.min(flat.sources.partition_point(|source| *source < b)),
                )
            })
            .collect(),
        None => vec![(start, end)],
    };
    let mut ranges = Vec::new();
    for (start, end) in search_regions {
        if start < end {
            ranges.extend(
                pattern
                    .ranges(&flat.chars[start..end])
                    .into_iter()
                    .map(|(a, b)| (a + start, b + start)),
            );
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let ranges = if let Some(occurrence) = occurrence {
        select(ranges, occurrence, &pattern.describe())?
    } else {
        ranges
    };
    Ok(ranges
        .into_iter()
        .filter_map(|range| materialize(rows, &flat, range))
        .collect())
}

fn locate_style_within<F>(
    rows: &[Vec<EmuCell>],
    selector: &StyleSelector,
    allowed: Option<&[(usize, usize)]>,
    occurrence: Option<&MatchOccurrence>,
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
    let ranges = if let Some(occurrence) = occurrence {
        select(ranges, occurrence, "style")?
    } else {
        ranges
    };
    Ok(ranges
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

/// Resolve the simple lookup used by coordinate-based mouse input.
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
    select_items(ranges, occurrence, description)
}

fn select_items<T>(
    items: Vec<T>,
    occurrence: &MatchOccurrence,
    description: &str,
) -> anyhow::Result<Vec<T>> {
    let count = items.len();
    match occurrence {
        MatchOccurrence::Any => Ok(items),
        MatchOccurrence::Unique if count > 1 => {
            anyhow::bail!("expected '{description}' to match once, but found {count} matches")
        }
        MatchOccurrence::Unique | MatchOccurrence::First => {
            Ok(items.into_iter().next().into_iter().collect())
        }
        MatchOccurrence::Last => Ok(items.into_iter().last().into_iter().collect()),
        MatchOccurrence::Nth(index) => Ok(items.into_iter().nth(*index).into_iter().collect()),
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
                row: cell.y.min(u32::MAX as usize) as u32,
                start: cell.x.min(u16::MAX as usize) as u16,
                end: cell.x.saturating_add(1).min(u16::MAX as usize) as u16,
            }),
        }
    }
    Some(LocatedMatch {
        value: TextMatch {
            text: flat.chars[start..end].iter().collect(),
            start: TextPosition {
                row: first.y.min(u32::MAX as usize) as u32,
                column: first.x.min(u16::MAX as usize) as u16,
            },
            end: TextPosition {
                row: last.y.min(u32::MAX as usize) as u32,
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
    fn regex_ranges_map_utf8_offsets_in_one_pass() {
        let chars = "é界a".chars().collect::<Vec<_>>();
        assert_eq!(
            Pattern::new(".", true).unwrap().ranges(&chars),
            vec![(0, 1), (1, 2), (2, 3)]
        );
    }

    #[test]
    fn locations_preserve_rows_above_u16() {
        let mut rows = vec![Vec::new(); u16::MAX as usize + 2];
        rows[u16::MAX as usize + 1] = vec![EmuCell {
            ch: "X".into(),
            ..EmuCell::blank()
        }];

        let found = locate_query_text(&rows, &LocatorQuery::text("X")).unwrap();
        assert_eq!(found[0].value.start.row, u16::MAX as u32 + 1);
        assert_eq!(found[0].value.end.row, u16::MAX as u32 + 1);
        assert_eq!(found[0].value.spans[0].row, u16::MAX as u32 + 1);
    }

    #[test]
    fn scopes_a_match_after_an_anchor() {
        let mut selector = TextSelector::new("Save");
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
        let mut query = LocatorQuery::text("item");
        assert_eq!(locate_query_text(&rows, &query).unwrap().len(), 3);
        query.occurrence = MatchOccurrence::Last;
        assert_eq!(
            locate_query_text(&rows, &query).unwrap()[0]
                .value
                .start
                .column,
            10
        );
        query.occurrence = MatchOccurrence::Nth(1);
        assert_eq!(
            locate_query_text(&rows, &query).unwrap()[0]
                .value
                .start
                .column,
            5
        );
    }

    #[test]
    fn unique_reports_ambiguous_text() {
        let mut query = LocatorQuery::text("same");
        query.occurrence = MatchOccurrence::Unique;
        let error = locate_query_text(&grid(&["same same"]), &query).unwrap_err();
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
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text(parent))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };

        let found =
            locate_query_text(&grid(&["Settings", "  Save", "Save outside"]), &child).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start, TextPosition { row: 1, column: 2 });
    }

    #[test]
    fn child_locators_honor_parent_occurrence_selection() {
        let rows = grid(&["panel: Save", "panel: Save"]);
        let mut parent = LocatorQuery::text("panel: Save");
        parent.occurrence = MatchOccurrence::Last;
        let child = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("Save")),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(parent)),
            direction: LocatorDirection::Within,
            style: Default::default(),
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
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text(outer))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        let inner = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("Save")),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(middle)),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };

        let found = locate_query_text(&grid(&["panel [Save]", "Save outside"]), &inner).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start, TextPosition { row: 0, column: 7 });
    }

    #[test]
    fn child_occurrences_are_selected_across_all_parent_regions() {
        let rows = grid(&["[a]", "[a]"]);
        let parent = TextSelector::new("[a]");
        let query = |occurrence| LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("a")),
            occurrence,
            within: Some(Box::new(LocatorQuery::text(parent.clone()))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        assert_eq!(
            locate_query_text(&rows, &query(MatchOccurrence::Any))
                .unwrap()
                .len(),
            2
        );

        assert_eq!(
            locate_query_text(&rows, &query(MatchOccurrence::Nth(1))).unwrap()[0]
                .value
                .start,
            TextPosition { row: 1, column: 1 }
        );
        assert!(locate_query_text(&rows, &query(MatchOccurrence::Unique))
            .unwrap_err()
            .to_string()
            .contains("found 2"));
    }

    #[test]
    fn relative_directions_segment_multiple_parent_matches() {
        let rows = grid(&["Section One", "Save 1", "Section Two", "Save 2"]);
        let query = |parent, text, direction, occurrence| LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new(text)),
            occurrence,
            within: Some(Box::new(LocatorQuery::text(parent))),
            direction,
            style: Default::default(),
        };

        let after = locate_query_text(
            &rows,
            &query(
                "Section",
                "Save",
                LocatorDirection::After,
                MatchOccurrence::Any,
            ),
        )
        .unwrap();
        assert_eq!(
            after
                .iter()
                .map(|matched| matched.value.start.row)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );

        let before = locate_query_text(
            &rows,
            &query(
                "Save",
                "Section",
                LocatorDirection::Before,
                MatchOccurrence::Any,
            ),
        )
        .unwrap();
        assert_eq!(
            before
                .iter()
                .map(|matched| matched.value.start.row)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );

        let second = locate_query_text(
            &rows,
            &query(
                "Section",
                "Save",
                LocatorDirection::After,
                MatchOccurrence::Nth(1),
            ),
        )
        .unwrap();
        assert_eq!(second[0].value.start.row, 3);
    }

    #[test]
    fn relative_text_search_can_follow_a_style_locator() {
        let mut rows = grid(&["RED status OK"]);
        for cell in &mut rows[0][..3] {
            cell.attrs.insert(Attrs::BOLD);
        }
        let query = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("OK")),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::style(TextStyle {
                bold: Some(true),
                ..TextStyle::default()
            }))),
            direction: LocatorDirection::After,
            style: Default::default(),
        };

        let found = locate_query_text(&rows, &query).unwrap();
        assert_eq!(found[0].value.start, TextPosition { row: 0, column: 11 });
    }

    #[test]
    fn child_matches_cannot_span_separate_parent_regions() {
        let rows = grid(&["ab  ", "  ab"]);
        let parent = TextSelector::new("ab");
        let mut child = TextSelector::new("b a");
        child.whitespace = WhitespaceMode::Normalize;
        let query = LocatorQuery {
            selector: LocatorSelector::Text(child),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text(parent))),
            direction: LocatorDirection::Within,
            style: Default::default(),
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
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::style(style.clone()))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        let found = locate_query_text(&rows, &styled_text).unwrap();
        assert_eq!(found[0].value.start.column, 7);

        let text_style = LocatorQuery {
            selector: LocatorSelector::Style(style),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text("BOLD"))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };
        let found = locate_query_text(&rows, &text_style).unwrap();
        assert_eq!(found[0].value.text, "BOLD");
    }

    #[test]
    fn nested_style_filters_the_entire_parent_match() {
        let mut rows = grid(&["Warning"]);
        for cell in &mut rows[0][..4] {
            cell.attrs.insert(Attrs::BOLD);
        }
        let query = LocatorQuery {
            selector: LocatorSelector::Style(StyleSelector::from(TextStyle {
                bold: Some(true),
                ..TextStyle::default()
            })),
            occurrence: MatchOccurrence::Any,
            within: Some(Box::new(LocatorQuery::text("Warning"))),
            direction: LocatorDirection::Within,
            style: Default::default(),
        };

        assert!(locate_query_text(&rows, &query).unwrap().is_empty());
        for cell in &mut rows[0] {
            cell.attrs.insert(Attrs::BOLD);
        }
        let found = locate_query_text(&rows, &query).unwrap();
        assert_eq!(found[0].value.text, "Warning");
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
                ..StyleSelector::default()
            }),
            occurrence,
            within: Some(Box::new(parent.clone())),
            direction: LocatorDirection::Within,
            style: Default::default(),
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

    #[test]
    fn style_constraints_filter_before_unique_selection() {
        let mut rows = grid(&["X X"]);
        rows[0][2].attrs.insert(Attrs::BOLD);
        let query = LocatorQuery {
            selector: LocatorSelector::Text(TextSelector::new("X")),
            occurrence: MatchOccurrence::Unique,
            within: None,
            direction: LocatorDirection::Within,
            style: TextStyle {
                bold: Some(true),
                ..TextStyle::default()
            },
        };

        let found = locate_query_text(&rows, &query).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value.start.column, 2);
    }

    /// A blank inside a match is skipped for a color, which it cannot show,
    /// but not for a link, which it can carry. `A B` whose space alone links
    /// somewhere is not a run that links nowhere.
    #[test]
    fn a_link_constraint_covers_the_blanks_inside_a_match() {
        let link = |uri: &str| {
            Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: None,
                uri: uri.into(),
            }))
        };
        let mut rows = grid(&["A B"]);
        rows[0][1].hyperlink = link("https://example.com");

        let locate = locate_query_text;
        let with_link = |link: &str| LocatorQuery {
            within: Some(Box::new(LocatorQuery::text("A B"))),
            ..LocatorQuery::link(link)
        };

        assert!(
            locate(&rows, &with_link("")).unwrap().is_empty(),
            "the linked space means the run does not link nowhere"
        );
        assert!(
            locate(&rows, &with_link("https://example.com"))
                .unwrap()
                .is_empty(),
            "and the unlinked letters mean it is not all one link either"
        );

        for cell in &mut rows[0] {
            cell.hyperlink = link("https://example.com");
        }
        assert_eq!(
            locate(&rows, &with_link("https://example.com"))
                .unwrap()
                .len(),
            1,
            "every cell linked, blanks included, matches"
        );
    }

    /// Asking about a link and an appearance together means the same as asking
    /// about each alone.
    ///
    /// `A B` linked throughout, with the letters bold and the space not — the
    /// usual shape, since a program has no reason to bold a space. The blank
    /// is skipped for bold, which it cannot show, and checked for the link,
    /// which it carries. Treating one policy as the match's own would fail the
    /// combination while passing both halves.
    #[test]
    fn a_link_and_an_appearance_compose() {
        let mut rows = grid(&["A B"]);
        for cell in &mut rows[0] {
            cell.hyperlink = Some(std::sync::Arc::new(crate::terminal::cell::Hyperlink {
                id: None,
                uri: "https://example.com".into(),
            }));
        }
        rows[0][0].attrs.insert(Attrs::BOLD);
        rows[0][2].attrs.insert(Attrs::BOLD);

        let bold = LocatorQuery {
            within: Some(Box::new(LocatorQuery::text("A B"))),
            ..LocatorQuery::style(TextStyle {
                bold: Some(true),
                ..Default::default()
            })
        };
        let linked = LocatorQuery {
            within: Some(Box::new(LocatorQuery::text("A B"))),
            ..LocatorQuery::link("https://example.com")
        };
        let both = LocatorQuery {
            within: Some(Box::new(bold.clone())),
            ..LocatorQuery::link("https://example.com")
        };
        for query in [bold, linked, both] {
            assert_eq!(
                locate_query_text(&rows, &query).unwrap()[0].value.text,
                "A B"
            );
        }
    }

    fn linked_grid() -> Vec<Vec<EmuCell>> {
        let mut rows = grid(&["ABC"]);
        for cell in &mut rows[0][..2] {
            cell.attrs.insert(Attrs::BOLD);
        }
        for cell in &mut rows[0][1..] {
            cell.hyperlink = Some(std::sync::Arc::new(super::super::cell::Hyperlink {
                id: None,
                uri: "test:link".into(),
            }));
        }
        rows
    }

    fn texts(rows: &[Vec<EmuCell>], query: &LocatorQuery) -> Vec<String> {
        locate_query_text(rows, query)
            .unwrap()
            .into_iter()
            .map(|m| m.value.text)
            .collect()
    }

    #[test]
    fn cell_intersection_and_union_regroup_matches() {
        let rows = linked_grid();
        let bold = LocatorQuery::style(TextStyle {
            bold: Some(true),
            ..Default::default()
        });
        let link = LocatorQuery::link("test:link");
        assert_eq!(texts(&rows, &bold.clone().and(link.clone())), ["B"]);
        assert_eq!(texts(&rows, &link.clone().and(bold.clone())), ["B"]);
        assert_eq!(texts(&rows, &bold.clone().or(link.clone())), ["ABC"]);
        assert_eq!(texts(&rows, &link.clone().or(bold)), ["ABC"]);
        assert_eq!(texts(&rows, &link.clone().or(link)), ["BC"]);
        let query = LocatorQuery::text("A").or(LocatorQuery::text("C"));
        assert_eq!(texts(&rows, &query), ["A", "C"]);
        assert_eq!(
            texts(
                &grid(&["AB", "CD"]),
                &LocatorQuery::text("ABCD").or(LocatorQuery::text("absent"))
            ),
            ["AB", "CD"]
        );
    }

    #[test]
    fn containment_preserves_parents_and_differs_from_coverage() {
        let rows = linked_grid();
        let parent = LocatorQuery::text("ABC");
        let link = LocatorQuery::link("test:link");
        assert_eq!(
            texts(&rows, &parent.clone().filter(Some(link.clone()), None)),
            ["ABC"]
        );
        assert!(texts(&rows, &parent.clone().filter(None, Some(link.clone()))).is_empty());
        let whole = LocatorQuery {
            within: Some(Box::new(parent.clone())),
            ..link.clone()
        };
        assert!(texts(&rows, &whole).is_empty());
        assert_eq!(texts(&rows, &parent.clone().and(link)), ["BC"]);
        assert_eq!(
            texts(
                &rows,
                &parent
                    .clone()
                    .filter(Some(parent.clone()), Some(LocatorQuery::text("absent")))
            ),
            ["ABC"]
        );
        assert!(texts(
            &rows,
            &parent.clone().filter(Some(parent.clone()), Some(parent))
        )
        .is_empty());
    }

    #[test]
    fn filters_do_not_search_outside_the_candidate() {
        let rows = grid(&["AAAB"]);
        let parent = LocatorQuery::text("AAB");
        assert_eq!(
            texts(
                &rows,
                &parent.clone().filter(
                    Some(LocatorQuery::text(TextSelector {
                        text: "^AA".into(),
                        regex: true,
                        ..TextSelector::new("")
                    })),
                    None
                )
            ),
            ["AAB"]
        );
        assert!(texts(
            &rows,
            &LocatorQuery::text("AAA").filter(Some(LocatorQuery::text("B")), None)
        )
        .is_empty());
        let after = LocatorQuery {
            within: Some(Box::new(LocatorQuery::text("AAA"))),
            direction: LocatorDirection::After,
            ..LocatorQuery::text("B")
        };
        assert!(texts(&rows, &LocatorQuery::text("AAA").filter(Some(after), None)).is_empty());
    }

    #[test]
    fn composition_respects_selection_and_operand_errors() {
        let rows = grid(&["ABA"]);
        let a = LocatorQuery::text("A");
        let b = LocatorQuery::text("B");
        let mut union = a.clone().or(b.clone());
        union.occurrence = MatchOccurrence::First;
        assert_eq!(texts(&rows, &union), ["ABA"]);
        let first = LocatorQuery {
            occurrence: MatchOccurrence::First,
            ..a.clone()
        };
        assert_eq!(texts(&rows, &first.or(b.clone())), ["AB"]);
        let unique = LocatorQuery {
            occurrence: MatchOccurrence::Unique,
            ..a
        };
        assert!(locate_query_text(&rows, &unique.or(b)).is_err());
    }

    #[test]
    fn set_algebra_agrees_with_small_cell_masks() {
        let rows = grid(&["ABCD"]);
        let query = |mask: u8| {
            (0..4)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| LocatorQuery::text(char::from(b'A' + i).to_string()))
                .reduce(LocatorQuery::or)
                .unwrap_or_else(|| LocatorQuery::text("absent"))
        };
        for a in 0..16 {
            for b in 0..16 {
                for (actual, expected) in [
                    (query(a).and(query(b)), a & b),
                    (query(a).or(query(b)), a | b),
                ] {
                    let found = locate_query_text(&rows, &actual).unwrap();
                    let mask = found
                        .iter()
                        .flat_map(|m| &m.cells)
                        .fold(0u8, |mask, cell| mask | (1 << cell.x));
                    assert_eq!(mask, expected);
                }
            }
        }
    }

    #[test]
    fn normalized_candidates_keep_their_shape_until_composed() {
        let rows = grid(&["A  B"]);
        let normalized = LocatorQuery::text(TextSelector {
            whitespace: WhitespaceMode::Normalize,
            ..TextSelector::new("A B")
        });
        assert_eq!(
            texts(
                &rows,
                &normalized
                    .clone()
                    .filter(Some(LocatorQuery::text("A  B")), None)
            ),
            ["A B"]
        );
        let composed = locate_query_text(&rows, &normalized.and(LocatorQuery::link(""))).unwrap();
        assert_eq!(composed[0].value.text, "A  B");
        assert_eq!(composed[0].value.spans[0].end, 4);
    }

    #[test]
    fn containment_does_not_merge_adjacent_inner_parent_regions() {
        let rows = grid(&["XX"]);
        let inner = LocatorQuery {
            within: Some(Box::new(LocatorQuery::text("X"))),
            ..LocatorQuery::text("XX")
        };
        assert!(texts(&rows, &LocatorQuery::text("XX").filter(Some(inner), None)).is_empty());
    }

    #[test]
    fn composition_preserves_physical_columns_and_missing_cells() {
        let mut rows = grid(&["xyz", "q"]);
        rows[0][0].ch = "\u{754c}".into();
        rows[0][1].ch = "".into();
        rows[0][2].ch = "e\u{301}".into();
        let query = LocatorQuery::link("").or(LocatorQuery::text("absent"));
        let found = locate_query_text(&rows, &query).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].value.spans[0].end, 3);
        assert_eq!(found[1].value.spans[0].end, 1);
        assert_eq!(found[0].cells[2].cell.ch, "e\u{301}");
        assert_eq!(found[0].cells[1].cell.ch, "");
    }
}
