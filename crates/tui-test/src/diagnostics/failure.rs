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
        operation: impl Into<String>,
        reason: FailureReason,
        summary: impl Into<String>,
    ) -> Self {
        let mut truncated = false;
        Self {
            schema_version: FAILURE_SCHEMA_VERSION,
            operation: bounded(&operation.into(), 256, &mut truncated),
            reason,
            summary: bounded(&summary.into(), 4096, &mut truncated),
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
            .map(|comparison| ComparisonDiagnostics {
                kind: bounded(&comparison.kind, 128, truncated),
                expected: comparison
                    .expected
                    .as_ref()
                    .map(|value| bounded(value, 1024, truncated)),
                actual: comparison
                    .actual
                    .as_ref()
                    .map(|value| bounded(value, 1024, truncated)),
            });
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

pub(crate) fn failure_reason(
    error: &TuiTestError,
    observation: Option<&FailureObservation>,
) -> FailureReason {
    if let Some(observation) = observation {
        if observation.process.cancelled {
            return FailureReason::Cancelled;
        }
        if observation.process.exit_code.is_some() {
            return FailureReason::SessionExited;
        }
    }
    if let Some(locator) = error
        .report
        .as_ref()
        .and_then(|details| details.locator.as_ref())
    {
        return match locator.failure_reason {
            Some(LocatorFailureReason::Ambiguous) => FailureReason::LocatorAmbiguous,
            Some(LocatorFailureReason::OutsideViewport)
            | Some(LocatorFailureReason::MatchedNoCells) => FailureReason::MatchNotActionable,
            _ => FailureReason::LocatorNoMatch,
        };
    }
    match error.kind {
        ErrorKind::Internal => FailureReason::InternalFailure,
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
    }
}

pub(crate) fn merge_failure_details(target: &mut FailureReport, source: FailureReport) {
    target.reason = source.reason;
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

#[cfg(test)]
mod tests {
    use super::*;

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
