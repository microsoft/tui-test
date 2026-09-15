use serde::{Deserialize, Serialize};

use super::strings::truncate_diagnostic_value;
use super::FailureObservation;
use super::{
    CellMismatch, ComparisonDiagnostics, FailureReason, FailureReport, LocatorFailureReason,
    FAILURE_SCHEMA_VERSION,
};
use crate::api::{ErrorKind, TextPosition, TuiTestError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureDetails {
    pub schema_version: u32,
    pub operation: String,
    pub reason: FailureReason,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<LocatorFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<ComparisonDiagnostics>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatorFailure {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<LocatorFailureReason>,
    /// Selector descriptions leading to the failing stage.
    pub selectors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_index: Option<usize>,
    /// Sample candidate starts in terminal grid coordinates.
    pub locations: Vec<TextPosition>,
    pub mismatches: Vec<CellMismatch>,
}

impl FailureDetails {
    pub fn new(
        operation: impl AsRef<str>,
        reason: FailureReason,
        summary: impl AsRef<str>,
    ) -> Self {
        let mut truncated = false;
        Self {
            schema_version: FAILURE_SCHEMA_VERSION,
            operation: bounded(operation.as_ref(), 256, &mut truncated),
            reason,
            summary: bounded(summary.as_ref(), 4096, &mut truncated),
            locator: None,
            comparison: None,
            truncated,
        }
    }
}

impl FailureReport {
    pub(crate) fn failure_details(&self) -> FailureDetails {
        let mut details = FailureDetails::new(&self.operation.name, self.reason, &self.summary);
        let truncated = &mut details.truncated;
        *truncated |= self.truncated;
        details.comparison = self
            .comparison
            .as_ref()
            .map(|comparison| bounded_comparison(comparison, 1024, truncated));
        if let Some(comparison) = &details.comparison {
            if comparison.kind == "snapshot" {
                if let (Some(expected), Some(actual)) = (&comparison.expected, &comparison.actual) {
                    details.summary = format!(
                        "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
                    );
                }
            }
        }
        details.locator = self.locator.as_ref().map(|locator| {
            let stage = locator
                .failure_stage
                .and_then(|index| {
                    locator
                        .stages
                        .iter()
                        .find(|stage| stage.stage_index == index)
                })
                .or_else(|| locator.stages.last());
            let candidates = stage.map_or(locator.selected.as_slice(), |stage| &stage.candidates);
            let mismatches = stage.map_or(&[][..], |stage| stage.mismatches.as_slice());
            *truncated |= candidates.len() > 4
                || mismatches.len() > 4
                || stage
                    .is_some_and(|stage| stage.candidates_truncated || stage.mismatches_truncated);
            let mut selectors = Vec::new();
            for entry in &locator.stages {
                if let Some(selector) = &entry.selector {
                    if selectors.len() == 8 {
                        *truncated = true;
                        break;
                    }
                    selectors.push(bounded(&selector.description(), 256, truncated));
                }
                if stage.is_some_and(|stage| entry.stage_index == stage.stage_index) {
                    break;
                }
            }
            LocatorFailure {
                reason: locator.failure_reason,
                selectors,
                stage_index: locator.failure_stage,
                locations: candidates
                    .iter()
                    .take(4)
                    .map(|candidate| candidate.start)
                    .collect(),
                mismatches: mismatches
                    .iter()
                    .take(4)
                    .map(|mismatch| CellMismatch {
                        location: mismatch.location,
                        grapheme: bounded(&mismatch.grapheme, 64, truncated),
                        property: bounded(&mismatch.property, 64, truncated),
                        operator: bounded(&mismatch.operator, 64, truncated),
                        expected: bounded(&mismatch.expected, 256, truncated),
                        actual: bounded(&mismatch.actual, 256, truncated),
                        resolved: mismatch
                            .resolved
                            .as_ref()
                            .map(|value| bounded(value, 256, truncated)),
                        reason: bounded(&mismatch.reason, 256, truncated),
                    })
                    .collect(),
            }
        });
        details
    }
}

fn bounded(value: &str, limit: usize, truncated: &mut bool) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    *truncated = true;
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

fn bounded_comparison(
    comparison: &ComparisonDiagnostics,
    limit: usize,
    truncated: &mut bool,
) -> ComparisonDiagnostics {
    let mut start = 0;
    if comparison.kind == "snapshot" {
        if let (Some(expected), Some(actual)) = (&comparison.expected, &comparison.actual) {
            if expected.len() > limit || actual.len() > limit {
                let common = expected
                    .chars()
                    .zip(actual.chars())
                    .take_while(|(expected, actual)| expected == actual)
                    .map(|(ch, _)| ch.len_utf8())
                    .sum::<usize>();
                // Keep the differing line, or a little context if that line alone is long.
                start = expected[..common]
                    .rfind('\n')
                    .map_or(0, |index| index + 1)
                    .max(common.saturating_sub(limit / 4));
                while !expected.is_char_boundary(start) {
                    start -= 1;
                }
            }
        }
    }
    let mut excerpt = |value: &String| {
        if start != 0 {
            *truncated = true;
            format!("...{}", bounded(&value[start..], limit - 3, truncated))
        } else {
            bounded(value, limit, truncated)
        }
    };
    let expected = comparison.expected.as_ref().map(&mut excerpt);
    let actual = comparison.actual.as_ref().map(&mut excerpt);
    ComparisonDiagnostics {
        kind: bounded(&comparison.kind, 128, truncated),
        expected,
        actual,
    }
}

pub(crate) fn failure_reason(
    error: &TuiTestError,
    observation: Option<&FailureObservation>,
) -> FailureReason {
    let reason = error.report.as_ref().map_or_else(
        || match error.kind {
            ErrorKind::Internal => FailureReason::InternalFailure,
            ErrorKind::Assertion if error.message.starts_with("session exited") => {
                FailureReason::SessionExited
            }
            ErrorKind::Assertion
                if error.message.contains("timed out") || error.message.contains("timeout") =>
            {
                FailureReason::TimedOut
            }
            ErrorKind::Assertion if error.message.contains("snapshot mismatch") => {
                FailureReason::SnapshotMismatch
            }
            ErrorKind::Assertion => FailureReason::ScalarMismatch,
            ErrorKind::Usage | ErrorKind::NoSession => FailureReason::InternalFailure,
        },
        |report| report.reason,
    );
    if matches!(
        reason,
        FailureReason::TimedOut
            | FailureReason::SessionExited
            | FailureReason::LocatorNoMatch
            | FailureReason::LocatorAmbiguous
            | FailureReason::UnexpectedMatch
            | FailureReason::MatchNotActionable
    ) {
        if let Some(observation) = observation {
            if observation.process.cancelled {
                return FailureReason::Cancelled;
            }
            if observation.process.exit_code.is_some() {
                return FailureReason::SessionExited;
            }
        }
    }
    reason
}

pub(crate) fn merge_failure_details(target: &mut FailureReport, source: FailureReport) {
    let (summary, truncated) = truncate_diagnostic_value(source.summary, 64 * 1024);
    target.summary = summary;
    target.locator = source.locator;
    target.comparison = source.comparison;
    target.evaluation_transitions = source.evaluation_transitions;
    target.hints = source.hints;
    target.truncated |= source.truncated || truncated;
    if source.operation.timeout_ms.is_some() {
        target.operation.timeout_ms = source.operation.timeout_ms;
    }
}

pub(crate) fn comparison_failure(
    operation: &str,
    timeout_ms: Option<u64>,
    reason: FailureReason,
    message: String,
    kind: &str,
    expected: Option<String>,
    actual: Option<String>,
) -> TuiTestError {
    let mut details = FailureReport::new(operation, timeout_ms, reason, message.clone());
    details.comparison = Some(bounded_comparison(
        &ComparisonDiagnostics {
            kind: kind.to_string(),
            expected,
            actual,
        },
        256 * 1024,
        &mut details.truncated,
    ));
    TuiTestError::assertion(message).with_report(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_excerpts_preserve_late_differences_through_both_limits() {
        for prefix in [
            "same row\n".repeat(40),
            "same row\n".repeat(40_000),
            "\u{1f600}".repeat(80_000),
        ] {
            let expected = format!("{prefix}EXPECTED_DIFFERENCE{}", " tail".repeat(400));
            let actual = format!("{prefix}OBSERVED_DIFFERENCE{}", " tail".repeat(400));
            let error = comparison_failure(
                "expect.snapshot",
                None,
                FailureReason::SnapshotMismatch,
                format!(
                    "snapshot mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
                ),
                "snapshot",
                Some(expected),
                Some(actual),
            );
            let report = error.report.unwrap();
            let comparison = report.comparison.as_ref().unwrap();
            assert!(comparison
                .expected
                .as_ref()
                .unwrap()
                .contains("EXPECTED_DIFFERENCE"));
            assert!(comparison
                .actual
                .as_ref()
                .unwrap()
                .contains("OBSERVED_DIFFERENCE"));
            let details = report.failure_details();
            let comparison = details.comparison.as_ref().unwrap();
            assert_ne!(comparison.expected, comparison.actual);
            assert!(comparison.expected.as_ref().unwrap().len() <= 1027);
            assert!(comparison.actual.as_ref().unwrap().len() <= 1027);
            assert!(details.summary.contains("EXPECTED_DIFFERENCE"));
            assert!(details.summary.contains("OBSERVED_DIFFERENCE"));
            assert!(details.summary.len() <= 4096);
            assert!(details.truncated);
        }
    }

    #[test]
    fn comparison_excerpts_handle_end_of_input_and_unicode_boundaries() {
        let prefix = "\u{1f600}".repeat(2000);
        for (expected, actual) in [
            (String::new(), "new".into()),
            ("short".into(), "other".into()),
            (prefix.clone(), format!("{prefix}extra")),
            (format!("{prefix}extra"), prefix.clone()),
            (format!("{prefix}\u{e9}"), format!("{prefix}\u{ea}")),
            (format!("{prefix}\n"), format!("{prefix}\nextra")),
        ] {
            let comparison = ComparisonDiagnostics {
                kind: "snapshot".into(),
                expected: Some(expected.clone()),
                actual: Some(actual.clone()),
            };
            let mut truncated = false;
            let bounded = bounded_comparison(&comparison, 1024, &mut truncated);
            assert_ne!(bounded.expected, bounded.actual);
            if expected.len() <= 1024 && actual.len() <= 1024 {
                assert_eq!(bounded, comparison);
                assert!(!truncated);
            } else {
                assert!(truncated);
            }
        }
        let comparison = ComparisonDiagnostics {
            kind: "exit_code".into(),
            expected: Some("0".into()),
            actual: None,
        };
        assert_eq!(
            bounded_comparison(&comparison, 1024, &mut false),
            comparison
        );
    }

    #[test]
    fn public_failure_contains_only_bounded_actionable_evidence() {
        let mut report = FailureReport::new(
            "expect.output",
            Some(25),
            FailureReason::ScalarMismatch,
            "failure".repeat(10_000),
        );
        report.comparison = Some(ComparisonDiagnostics {
            kind: "output".into(),
            expected: Some("expected".repeat(10_000)),
            actual: Some("observed".repeat(10_000)),
        });
        report
            .context
            .insert("private".into(), "report only".into());
        report.finish_signature();
        let details = report.failure_details();
        let json = serde_json::to_value(&details).unwrap();
        let keys: Vec<_> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "comparison",
                "operation",
                "reason",
                "schema_version",
                "summary",
                "truncated"
            ]
        );
        assert!(serde_json::to_vec(&details).unwrap().len() < 8192);
        assert!(details.truncated);
        assert_eq!(details.operation, "expect.output");
        assert_eq!(
            report
                .comparison
                .as_ref()
                .unwrap()
                .actual
                .as_ref()
                .unwrap()
                .len(),
            80_000
        );
        assert_eq!(
            serde_json::from_value::<FailureDetails>(json).unwrap(),
            details
        );
    }
}
