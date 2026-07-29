//! Text/regex search over the terminal grid. Maps a flat match range back to
//! grid cells.

use regex::Regex;

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
            Pattern::Text(t) => t.clone(),
            Pattern::Regex(r) => r.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchedCell {
    pub x: usize,
    pub y: usize,
    pub cell: EmuCell,
}

/// Find the first match of `pattern` in the grid. Returns `Ok(None)` when there
/// is no match, and `Err` on a strict-mode violation (multiple matches).
pub fn find(
    rows: &[Vec<EmuCell>],
    pattern: &Pattern,
    strict: bool,
) -> anyhow::Result<Option<Vec<MatchedCell>>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let chars: Vec<char> = rows
        .iter()
        .flat_map(|row| {
            (0..width).map(move |x| {
                row.get(x)
                    .map(|c| {
                        if c.ch.is_empty() {
                            ' '
                        } else {
                            c.ch.chars().next().unwrap_or(' ')
                        }
                    })
                    .unwrap_or(' ')
            })
        })
        .collect();

    let (index, length) = match pattern {
        Pattern::Text(text) => {
            let needle: Vec<char> = text.chars().collect();
            if needle.is_empty() {
                return Ok(None);
            }
            let occurrences = count_occurrences(&chars, &needle);
            if occurrences == 0 {
                return Ok(None);
            }
            if occurrences > 1 && strict {
                anyhow::bail!(
                    "strict mode expected one match for '{}', but found {}",
                    text,
                    occurrences
                );
            }
            let first = first_occurrence(&chars, &needle).unwrap();
            (first, needle.len())
        }
        Pattern::Regex(re) => {
            let block: String = chars.iter().collect();
            let matches: Vec<_> = re.find_iter(&block).collect();
            if matches.is_empty() {
                return Ok(None);
            }
            if matches.len() > 1 && strict {
                anyhow::bail!(
                    "strict mode expected one match for '{}', but found {}",
                    re.as_str(),
                    matches.len()
                );
            }
            let m = &matches[0];
            let start = block[..m.start()].chars().count();
            let len = m.as_str().chars().count();
            (start, len)
        }
    };

    let mut cells = Vec::with_capacity(length);
    for (y, row) in rows.iter().enumerate() {
        for x in 0..width {
            let pos = x + y * width;
            if pos >= index && pos < index + length {
                if let Some(cell) = row.get(x) {
                    cells.push(MatchedCell {
                        x,
                        y,
                        cell: cell.clone(),
                    });
                }
            }
        }
    }
    Ok(Some(cells))
}

fn count_occurrences(haystack: &[char], needle: &[char]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if haystack[i..i + needle.len()] == *needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

fn first_occurrence(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == *needle)
}
