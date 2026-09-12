use super::*;
use crate::diagnostics::{
    CellMismatch, LocatorStageDiagnostics, LocatorStageMode, OccurrenceSource,
};

const MAX_STAGES: usize = 128;
const MAX_SAMPLE: usize = 64;
const MAX_STAGE_BYTES: usize = 8 * 1024;

fn bound_stage(stage: &mut LocatorStageDiagnostics) -> bool {
    let mut truncated = false;
    while serde_json::to_vec(stage)
        .expect("diagnostic stage contains only serializable values")
        .len()
        > MAX_STAGE_BYTES
    {
        truncated = true;
        if !stage.candidates.is_empty() {
            stage.candidates.pop();
            stage.candidates_truncated = true;
        } else if !stage.mismatches.is_empty() {
            stage.mismatches.pop();
            stage.mismatches_truncated = true;
        } else {
            stage.selector = None;
            break;
        }
    }
    truncated
}

#[derive(Default)]
pub(super) struct Trace {
    pub enabled: bool,
    pub stages: Vec<LocatorStageDiagnostics>,
    pub truncated: bool,
}

#[derive(Default)]
pub(super) struct NodeStats {
    pub input: usize,
    pub raw: usize,
    pub styled: usize,
    pub selected: Vec<TextMatch>,
    pub selected_count: usize,
    pub mismatches: Vec<CellMismatch>,
    pub mismatches_truncated: bool,
    pub reason: Option<LocatorFailureReason>,
}

impl NodeStats {
    pub fn mismatch(&mut self, mut mismatch: CellMismatch) {
        for value in [
            &mut mismatch.grapheme,
            &mut mismatch.expected,
            &mut mismatch.actual,
        ] {
            self.mismatches_truncated |= truncate(value);
        }
        if self.mismatches.len() < MAX_SAMPLE {
            self.mismatches.push(mismatch);
        } else {
            self.mismatches_truncated = true;
        }
    }
}

impl Trace {
    pub fn record(
        &mut self,
        query: &LocatorQuery,
        path: &str,
        require_one: bool,
        stats: NodeStats,
    ) -> Option<usize> {
        if !self.enabled {
            return None;
        }
        if let Some(stage) = self
            .stages
            .iter_mut()
            .find(|stage| stage.expression_path == path)
        {
            stage.evaluations += 1;
            stage.input_candidate_count = stage.input_candidate_count.saturating_add(stats.input);
            stage.raw_candidate_count = stage.raw_candidate_count.saturating_add(stats.raw);
            stage.style_candidate_count = stage.style_candidate_count.saturating_add(stats.styled);
            stage.selected_count = stage.selected_count.saturating_add(stats.selected_count);
            stage.failure_reason = stats.reason;
            stage.candidates.extend(stats.selected);
            stage.mismatches.extend(stats.mismatches);
            stage.mismatches_truncated |= stats.mismatches_truncated;
            stage.candidates_truncated |= stats.selected_count > MAX_SAMPLE;
            stage.truncate();
            self.truncated |= bound_stage(stage);
            return Some(stage.stage_index);
        }
        if self.stages.len() == MAX_STAGES {
            self.truncated = true;
            return None;
        }
        let index = self.stages.len();
        let parent_filter = query.within.is_some() && query.direction == LocatorDirection::Within;
        let mode = match &query.selector {
            LocatorSelector::Text(_) => LocatorStageMode::Text,
            LocatorSelector::Style(_) if parent_filter => LocatorStageMode::ParentStyleFilter,
            LocatorSelector::Style(_) => LocatorStageMode::ContiguousStyleRuns,
            LocatorSelector::Link(_) if parent_filter => LocatorStageMode::ParentLinkFilter,
            LocatorSelector::Link(_) => LocatorStageMode::ContiguousLinkRuns,
            LocatorSelector::And { .. } => LocatorStageMode::Intersection,
            LocatorSelector::Or { .. } => LocatorStageMode::Union,
            LocatorSelector::Filter { .. } => LocatorStageMode::ContainmentFilter,
        };
        // Branch structure is identified by paths, not repeated subtrees in every stage.
        let selector = if query.selector.children().is_empty() {
            let mut selector = query.selector.clone();
            match &mut selector {
                LocatorSelector::Text(text) => {
                    self.truncated |= truncate(&mut text.text);
                    for anchor in [&mut text.scope.after, &mut text.scope.before]
                        .into_iter()
                        .flatten()
                    {
                        self.truncated |= truncate(&mut anchor.text);
                    }
                }
                LocatorSelector::Link(link) => {
                    self.truncated |= truncate(&mut link.uri);
                }
                LocatorSelector::Style(selector) => {
                    for value in [
                        &mut selector.style.foreground,
                        &mut selector.style.background,
                        &mut selector.style.underline_color,
                        &mut selector.style.underline_style,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        self.truncated |= truncate(value);
                    }
                }
                _ => {}
            }
            Some(selector)
        } else {
            None
        };
        let default = require_one && query.occurrence == MatchOccurrence::Any;
        let mut stage = LocatorStageDiagnostics {
            stage_index: index,
            expression_path: path.into(),
            evaluations: 1,
            failure_reason: stats.reason,
            mode,
            selector,
            direction: query.direction,
            requested_occurrence: query.occurrence.clone(),
            effective_occurrence: if default {
                MatchOccurrence::Unique
            } else {
                query.occurrence.clone()
            },
            occurrence_source: if default {
                OccurrenceSource::ActionDefault
            } else {
                OccurrenceSource::Explicit
            },
            input_candidate_count: stats.input,
            raw_candidate_count: stats.raw,
            style_candidate_count: stats.styled,
            selected_count: stats.selected_count,
            candidates_truncated: stats.selected_count > MAX_SAMPLE,
            candidates: stats.selected,
            mismatches: stats.mismatches,
            mismatches_truncated: stats.mismatches_truncated,
        };
        stage.truncate();
        self.truncated |= bound_stage(&mut stage);
        self.stages.push(stage);
        Some(index)
    }
}

pub(super) fn sample(matches: &[LocatedMatch]) -> Vec<TextMatch> {
    matches
        .iter()
        .take(MAX_SAMPLE)
        .map(|matched| {
            let mut value = matched.value.clone();
            truncate(&mut value.text);
            value.spans.truncate(256);
            value
        })
        .collect()
}

pub(super) fn truncate(value: &mut String) -> bool {
    if value.len() <= 4096 {
        return false;
    }
    let mut end = 4096;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str("...");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::terminal::{alacritty::AlacrittyEmu, cell::Attrs, emu::Emulator};

    fn rows() -> Vec<Vec<EmuCell>> {
        let mut emu = AlacrittyEmu::new(8, 2, &Profile::default());
        emu.process(b"\x1b[1mA\x1b]8;;test:docs\x07B\x1b[22mC\x1b]8;;\x07");
        emu.viewable_rows()
    }

    fn evaluate(rows: &[Vec<EmuCell>], query: &LocatorQuery) -> LocatorEvaluation {
        let style = |cell: &EmuCell, style: &TextStyle| {
            style.bold.is_none_or(|bold| bold == cell.has(Attrs::BOLD))
        };
        let traced = evaluate_query(rows, query, false, &mut |cell, expected, _, _| {
            CellStyleEvaluation {
                matched: style(cell, expected),
                mismatches: Vec::new(),
            }
        })
        .unwrap();
        let plain = locate_query(rows, query, &mut |cell, expected| style(cell, expected));
        match plain {
            Ok(plain) => assert_eq!(
                plain.iter().map(|m| &m.value).collect::<Vec<_>>(),
                traced.matches.iter().map(|m| &m.value).collect::<Vec<_>>()
            ),
            Err(error) => assert_eq!(
                traced.diagnostics.evaluation_error.as_deref(),
                Some(error.to_string().as_str())
            ),
        }
        traced
    }

    fn stage<'a>(evaluation: &'a LocatorEvaluation, path: &str) -> &'a LocatorStageDiagnostics {
        evaluation
            .diagnostics
            .stages
            .iter()
            .find(|stage| stage.expression_path == path)
            .unwrap()
    }

    #[test]
    fn composed_cell_sets_and_whole_match_refinement_have_distinct_evidence() {
        let rows = rows();
        let bold = LocatorQuery::style(TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        });
        let linked = LocatorQuery::link("test:docs");
        let intersection = evaluate(&rows, &bold.clone().and(linked.clone()));
        assert_eq!(intersection.matches[0].value.text, "B");
        assert_eq!(
            stage(&intersection, "root").mode,
            LocatorStageMode::Intersection
        );
        assert_eq!(stage(&intersection, "root.left").selected_count, 1);
        let union = evaluate(&rows, &bold.clone().or(linked.clone()));
        assert_eq!(union.matches[0].value.text, "ABC");
        let mut whole_link = linked.clone();
        whole_link.within = Some(Box::new(bold.clone()));
        let rejected = evaluate(&rows, &whole_link);
        assert_eq!(
            rejected.diagnostics.failure_reason,
            Some(LocatorFailureReason::LinkFilterRemovedAll)
        );
        let root = stage(&rejected, "root");
        assert_eq!(root.mode, LocatorStageMode::ParentLinkFilter);
        assert_eq!(root.mismatches[0].location.column, 0);
        assert_eq!(root.mismatches[0].property, "link");
        assert_eq!(root.mismatches[0].expected, "test:docs");
        let contained = evaluate(&rows, &bold.filter(Some(linked), None));
        assert_eq!(contained.matches[0].value.text, "AB");
        assert_eq!(
            stage(&contained, "root").mode,
            LocatorStageMode::ContainmentFilter
        );
        assert_eq!(stage(&contained, "root.has").selected_count, 1);
    }

    #[test]
    fn successful_union_and_negative_filters_do_not_inherit_soft_branch_failures() {
        let rows = rows();
        let query = LocatorQuery::text("missing").or(LocatorQuery::text("ABC"));
        assert_eq!(evaluate(&rows, &query).diagnostics.failure_reason, None);
        let query = LocatorQuery::text("ABC").filter(
            Some(LocatorQuery::link("test:docs")),
            Some(LocatorQuery::text("absent")),
        );
        let result = evaluate(&rows, &query);
        assert_eq!(result.matches[0].value.text, "ABC");
        assert_eq!(result.diagnostics.failure_reason, None);
        assert_eq!(stage(&result, "root.has_not").selected_count, 0);
    }

    #[test]
    fn explicit_unique_operand_errors_propagate_with_the_branch_path() {
        let rows = rows();
        let mut ambiguous = LocatorQuery::text(" ");
        ambiguous.occurrence = MatchOccurrence::Unique;
        for query in [
            ambiguous.clone().or(LocatorQuery::text("ABC")),
            LocatorQuery::text("ABC").and(ambiguous.clone()),
            LocatorQuery::text("ABC ").filter(None, Some(ambiguous)),
        ] {
            let result = evaluate(&rows, &query);
            // The candidate-bounded has_not finds only one blank and is valid.
            if let Some(error) = &result.diagnostics.evaluation_error {
                let failed = &result.diagnostics.stages[result.diagnostics.failure_stage.unwrap()];
                assert_eq!(failed.failure_reason, Some(LocatorFailureReason::Ambiguous));
                assert!(error.contains(&failed.expression_path));
                assert!(!failed.expression_path.eq("root"));
            } else {
                assert_eq!(
                    result.diagnostics.failure_reason,
                    Some(LocatorFailureReason::FilterRemovedAll)
                );
            }
        }
    }

    #[test]
    fn occurrence_stays_on_its_operand_and_counts_survive_early_selection() {
        let rows = rows();
        let mut any = LocatorQuery::text(" ");
        any.occurrence = MatchOccurrence::First;
        let result = evaluate(&rows, &any);
        assert_eq!(stage(&result, "root").raw_candidate_count, 13);
        assert_eq!(stage(&result, "root").selected_count, 1);
        any.occurrence = MatchOccurrence::Nth(99);
        assert_eq!(
            evaluate(&rows, &any).diagnostics.failure_reason,
            Some(LocatorFailureReason::NthOutOfRange)
        );
        let mut left = LocatorQuery::text(crate::api::TextSelector {
            text: "[AB]".into(),
            regex: true,
            ..Default::default()
        });
        left.occurrence = MatchOccurrence::First;
        let result = evaluate(&rows, &left.and(LocatorQuery::text("B")));
        assert_eq!(
            result.diagnostics.failure_reason,
            Some(LocatorFailureReason::IntersectionEmpty)
        );
    }

    #[test]
    fn containment_is_bounded_and_repeated_evaluations_are_aggregated() {
        let row = (0..600)
            .map(|_| EmuCell {
                ch: "A".into(),
                ..EmuCell::blank()
            })
            .collect();
        let rows = vec![row];
        let query = LocatorQuery::text("A")
            .filter(Some(LocatorQuery::text("A")), Some(LocatorQuery::text("B")));
        let result = evaluate(&rows, &query);
        assert_eq!(result.matches.len(), 600);
        assert_eq!(result.diagnostics.stages.len(), 4);
        assert_eq!(stage(&result, "root.has").evaluations, 600);
        assert_eq!(stage(&result, "root.has").candidates.len(), MAX_SAMPLE);
        assert!(stage(&result, "root.has").candidates_truncated);
        let rows = vec!["ABC OUT"
            .chars()
            .map(|ch| EmuCell {
                ch: ch.to_string().into(),
                ..EmuCell::blank()
            })
            .collect()];
        let escaping = LocatorQuery::text("OUT");
        let query = LocatorQuery::text("ABC").filter(Some(escaping), None);
        assert_eq!(
            evaluate(&rows, &query).diagnostics.failure_reason,
            Some(LocatorFailureReason::FilterRemovedAll)
        );
    }
}
