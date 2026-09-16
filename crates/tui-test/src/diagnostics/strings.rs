use super::{DiagnosticHint, FailureReason, FailureReport, LocatorFailureReason};
use crate::api::Operation;

pub(crate) fn format_timeout(timeout_ms: u64) -> String {
    if timeout_ms.is_multiple_of(1_000) {
        format!("{}s", timeout_ms / 1_000)
    } else {
        format!("{timeout_ms}ms")
    }
}

pub(crate) fn diagnostic_hints(details: &FailureReport) -> Vec<DiagnosticHint> {
    let mut hints = Vec::new();
    match details.reason {
        FailureReason::LocatorAmbiguous => hints.push(DiagnosticHint {
            code: "choose_occurrence".to_string(),
            message:
                "Narrow the locator or choose first(), last(), or nth() when multiple matches are expected."
                    .to_string(),
        }),
        FailureReason::LocatorNoMatch => {
            if let Some(locator) = &details.locator {
                if locator.failure_reason == Some(LocatorFailureReason::StyleFilterRemovedAll) {
                    hints.push(DiagnosticHint {
                        code: "inspect_style_mismatch".to_string(),
                        message:
                            "The selector matched candidates, but their styles did not match."
                                .to_string(),
                    });
                } else if let Some(stage) = locator.failure_stage {
                    hints.push(DiagnosticHint {
                        code: "inspect_locator_stage".to_string(),
                        message: format!(
                            "Inspect locator stage {stage}; it produced no selected candidates."
                        ),
                    });
                }
            }
        }
        FailureReason::MatchNotActionable => hints.push(DiagnosticHint {
            code: "make_match_visible".to_string(),
            message:
                "The locator matched, but the result was not actionable in the visible viewport."
                    .to_string(),
        }),
        FailureReason::SessionExited => hints.push(DiagnosticHint {
            code: "inspect_process_exit".to_string(),
            message: "Inspect the process exit code and the final recording output.".to_string(),
        }),
        FailureReason::TimedOut => hints.push(DiagnosticHint {
            code: "inspect_last_change".to_string(),
            message:
                "Inspect the retained screen transitions and the last successful operation before the timeout."
                    .to_string(),
        }),
        _ => {}
    }
    hints
}

pub(crate) fn base_error_message(message: &str) -> String {
    message
        .split_once("\n\nTerminal content:\n")
        .map_or(message, |(base, _)| base)
        .to_string()
}

pub(crate) fn diagnostic_operation_name(operation: &Operation) -> &'static str {
    match operation {
        Operation::Open(_) => "open",
        Operation::Run(_) => "run",
        Operation::Restart { .. } => "restart",
        Operation::Close => "close",
        Operation::State => "state",
        Operation::Text { .. } => "text",
        Operation::PackedScreen { .. } => "packed_screen",
        Operation::Cells { .. } => "cells",
        Operation::GetCommand => "get.command",
        Operation::GetOutput => "get.output",
        Operation::GetExitCode => "get.exit_code",
        Operation::GetCwd => "get.cwd",
        Operation::GetCursor => "get.cursor",
        Operation::GetModes => "get.modes",
        Operation::GetColors => "get.colors",
        Operation::GetSize => "get.size",
        Operation::GetTitle => "get.title",
        Operation::GetClipboard => "get.clipboard",
        Operation::GetBellCount => "get.bell_count",
        Operation::GetBellEvents => "get.bell_events",
        Operation::Write { .. } => "write",
        Operation::Submit { .. } => "submit",
        Operation::Key { .. } => "key",
        Operation::Mouse { .. } => "mouse",
        Operation::Resize { .. } => "resize",
        Operation::Signal { .. } => "signal",
        Operation::WaitTitle { .. } => "wait.title",
        Operation::WaitClipboard { .. } => "wait.clipboard",
        Operation::WaitClipboardMatch { .. } => "wait.clipboard_match",
        Operation::WaitIdle { .. } => "wait.idle",
        Operation::WaitCommand { .. } => "wait.command",
        Operation::WaitExit { .. } => "wait.exit",
        Operation::WaitReady { .. } => "wait.ready",
        Operation::WaitBell { .. } => "wait.bell",
        Operation::FindLocator { .. } => "locator.find",
        Operation::WaitLocator { .. } => "locator.wait",
        Operation::ClickLocator { .. } => "locator.click",
        Operation::HighlightLocator { .. } => "locator.highlight",
        Operation::ExpectTitle { .. } => "expect.title",
        Operation::ExpectExitCode { .. } => "expect.exit_code",
        Operation::ExpectMode { .. } => "expect.mode",
        Operation::ExpectColors { .. } => "expect.colors",
        Operation::ExpectCursor { .. } => "expect.cursor",
        Operation::ExpectOutput { .. } => "expect.output",
        Operation::ExpectBellCount { .. } => "expect.bell_count",
        Operation::Snapshot { .. } => "expect.snapshot",
        Operation::Screenshot { .. } => "screenshot",
        Operation::StartRecording { .. } => "record.start",
        Operation::StopRecording => "record.stop",
    }
}

pub(crate) fn operation_timeout(operation: &Operation) -> Option<u64> {
    match operation {
        Operation::WaitTitle { timeout_ms, .. }
        | Operation::WaitClipboard { timeout_ms }
        | Operation::WaitClipboardMatch { timeout_ms, .. }
        | Operation::WaitIdle { timeout_ms }
        | Operation::WaitCommand { timeout_ms }
        | Operation::WaitExit { timeout_ms }
        | Operation::WaitReady { timeout_ms }
        | Operation::WaitBell { timeout_ms }
        | Operation::WaitLocator { timeout_ms, .. }
        | Operation::ClickLocator { timeout_ms, .. }
        | Operation::HighlightLocator { timeout_ms, .. }
        | Operation::ExpectTitle { timeout_ms, .. }
        | Operation::ExpectExitCode { timeout_ms, .. }
        | Operation::ExpectMode { timeout_ms, .. }
        | Operation::ExpectColors { timeout_ms, .. }
        | Operation::ExpectCursor { timeout_ms, .. }
        | Operation::ExpectBellCount { timeout_ms, .. } => *timeout_ms,
        _ => None,
    }
}

pub(crate) fn truncate_diagnostic_value(mut value: String, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value, false);
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str("\n... diagnostic value truncated ...");
    (value, true)
}

pub(crate) fn timeout_message(pattern: &str, timeout_ms: u64, not: bool) -> String {
    format!(
        "timed out after {} waiting for '{pattern}' to be {}",
        format_timeout(timeout_ms),
        if not { "hidden" } else { "visible" }
    )
}

pub(crate) fn title_timeout_message_from_actual(
    actual: Option<&str>,
    pattern: &str,
    timeout_ms: u64,
    not: bool,
) -> String {
    let actual = actual
        .map(|title| format!("'{title}'"))
        .unwrap_or_else(|| "no title set".to_string());
    format!(
        "timed out after {} waiting for the title '{pattern}' to be {}; the title is {actual}",
        format_timeout(timeout_ms),
        if not { "hidden" } else { "visible" },
    )
}
