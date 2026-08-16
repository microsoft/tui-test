//! Text/regex search over the terminal grid, including scoped and normalized
//! selectors. Match offsets are mapped back to terminal cells.

use regex::Regex;

use crate::api::{
    MatchOccurrence, TextAnchor, TextMatch, TextPosition, TextSelector, TextSpan, WhitespaceMode,
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
}

struct FlatGrid {
    chars: Vec<char>,
    sources: Vec<usize>,
    width: usize,
}

/// Locate the matches selected by `selector`.
pub fn locate(rows: &[Vec<EmuCell>], selector: &TextSelector) -> anyhow::Result<Vec<LocatedMatch>> {
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
        .collect();
    let selected = select(ranges, &selector.occurrence, &pattern.describe())?;
    Ok(selected
        .into_iter()
        .filter_map(|range| materialize(rows, &flat, range))
        .collect())
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
    if start >= end {
        return None;
    }
    let source_start = *flat.sources.get(start)?;
    let source_end = flat.sources.get(end - 1)?.saturating_add(1);
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
        let error = locate(&grid(&["same same"]), &TextSelector::new("same")).unwrap_err();
        assert!(error.to_string().contains("found 2"));
    }
}
