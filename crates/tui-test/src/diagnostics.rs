use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::{
    AutomaticRecordingMode, LocatorDirection, LocatorSelector, MatchOccurrence, Size, TextMatch,
    TextPosition,
};
use crate::render::svg::RenderState;
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::CursorShape;

pub const FAILURE_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SCREEN_HISTORY_LIMIT: u16 = 10;
pub const MAX_SCREEN_HISTORY_LIMIT: u16 = 50;

const FAILURE_JSON_LIMIT: usize = 2 * 1024 * 1024;
const REPORT_LIMIT: usize = 1024 * 1024;
const SCREEN_TEXT_LIMIT: usize = 1024 * 1024;
const SCREEN_SVG_LIMIT: usize = 8 * 1024 * 1024;
pub(crate) const RECORDING_COPY_LIMIT: u64 = 64 * 1024 * 1024;
const ARTIFACT_TOTAL_LIMIT: u64 = 80 * 1024 * 1024;
const MAX_HISTORY_BYTES: usize = 512 * 1024;
const MAX_CONTEXT_ENTRIES: usize = 16;
const MAX_CONTEXT_KEY_BYTES: usize = 64;
const MAX_CONTEXT_VALUE_BYTES: usize = 256;
const MAX_CANDIDATES: usize = 64;
const MAX_MISMATCHES: usize = 64;
const MAX_OPERATION_HISTORY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    TimedOut,
    SessionExited,
    Cancelled,
    LocatorNoMatch,
    LocatorAmbiguous,
    UnexpectedMatch,
    MatchNotActionable,
    ScalarMismatch,
    SnapshotMismatch,
    EmulatorFault,
    InternalFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorFailureReason {
    AnchorNotFound,
    AnchorAmbiguous,
    RelativeRegionNoMatch,
    StyleFilterRemovedAll,
    NthOutOfRange,
    OutsideViewport,
    MatchedNoCells,
    NoMatch,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceSource {
    Explicit,
    ActionDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorStageMode {
    Text,
    ContiguousStyleRuns,
    ParentStyleFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationDiagnostics {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    pub elapsed_ms: u64,
    pub started_screen_sequence: u64,
    pub failed_screen_sequence: u64,
}

impl OperationDiagnostics {
    pub fn pending(name: impl Into<String>, timeout_ms: Option<u64>) -> Self {
        Self {
            name: name.into(),
            timeout_ms,
            elapsed_ms: 0,
            started_screen_sequence: 0,
            failed_screen_sequence: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatorStageDiagnostics {
    pub stage_index: usize,
    pub mode: LocatorStageMode,
    pub selector: LocatorSelector,
    pub direction: LocatorDirection,
    pub requested_occurrence: MatchOccurrence,
    pub effective_occurrence: MatchOccurrence,
    pub occurrence_source: OccurrenceSource,
    pub input_candidate_count: usize,
    pub raw_candidate_count: usize,
    pub style_candidate_count: usize,
    pub selected_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<TextMatch>,
    pub candidates_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mismatches: Vec<CellMismatch>,
    pub mismatches_truncated: bool,
}

impl LocatorStageDiagnostics {
    pub(crate) fn truncate(&mut self) {
        if self.candidates.len() > MAX_CANDIDATES {
            self.candidates.truncate(MAX_CANDIDATES);
            self.candidates_truncated = true;
        }
        if self.mismatches.len() > MAX_MISMATCHES {
            self.mismatches.truncate(MAX_MISMATCHES);
            self.mismatches_truncated = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellMismatch {
    pub location: TextPosition,
    pub grapheme: String,
    pub property: String,
    pub operator: String,
    pub expected: String,
    pub actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellStyleEvaluation {
    pub matched: bool,
    pub mismatches: Vec<CellMismatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatorDiagnostics {
    pub search_scope: String,
    pub viewport_origin_y: u32,
    pub stages: Vec<LocatorStageDiagnostics>,
    pub final_candidate_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<TextMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<LocatorFailureReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationTransition {
    pub elapsed_ms: u64,
    pub screen_sequence: u64,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_index: Option<usize>,
    pub stage_counts: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEvent {
    pub sequence: u64,
    pub name: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub result: String,
    pub screen_before: u64,
    pub screen_at_return: u64,
    pub safe_summary: String,
}

#[derive(Debug)]
pub(crate) struct OperationHistory {
    next_sequence: u64,
    entries: VecDeque<OperationEvent>,
}

#[derive(Debug)]
pub(crate) struct PendingOperation {
    sequence: u64,
    name: String,
    started_ms: u64,
    screen_before: u64,
    safe_summary: String,
}

impl OperationHistory {
    pub(crate) fn new() -> Self {
        Self {
            next_sequence: 1,
            entries: VecDeque::new(),
        }
    }

    pub(crate) fn begin(
        &mut self,
        name: String,
        started_ms: u64,
        screen_before: u64,
        safe_summary: String,
    ) -> PendingOperation {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        PendingOperation {
            sequence,
            name,
            started_ms,
            screen_before,
            safe_summary,
        }
    }

    pub(crate) fn finish(
        &mut self,
        pending: PendingOperation,
        ended_ms: u64,
        screen_at_return: u64,
        result: impl Into<String>,
    ) {
        self.entries.push_back(OperationEvent {
            sequence: pending.sequence,
            name: pending.name,
            started_ms: pending.started_ms,
            ended_ms,
            result: result.into(),
            screen_before: pending.screen_before,
            screen_at_return,
            safe_summary: pending.safe_summary,
        });
        while self.entries.len() > MAX_OPERATION_HISTORY {
            self.entries.pop_front();
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<OperationEvent> {
        self.entries.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorDiagnostics {
    pub column: u16,
    pub row: u16,
    pub visible: bool,
    pub shape: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSnapshotDetails {
    pub sequence: u64,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub repeat_count: u64,
    pub changes: Vec<String>,
    pub size: Size,
    pub cursor: CursorDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenHistoryDetails {
    pub limit: u16,
    pub dropped_screen_count: u64,
    pub dropped_row_count: u64,
    pub screens: Vec<ScreenSnapshotDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDiagnostics {
    pub size: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub cursor: CursorDiagnostics,
    pub last_visual_change_ms: u64,
    pub unchanged_for_ms: u64,
    pub screen_history: ScreenHistoryDetails,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_error: Option<String>,
    pub cancelled: bool,
    pub ready: bool,
    pub command_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_command_exit: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDiagnostics {
    pub tui_test_version: String,
    pub backend: String,
    pub target_os: String,
    pub target_arch: String,
    pub terminal_profile_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStatus {
    Disabled,
    Unavailable,
    Live,
    Copied,
    Omitted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingDiagnostics {
    pub mode: AutomaticRecordingMode,
    pub status: RecordingStatus,
    pub failure_offset_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_committed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub ephemeral: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticHint {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonDiagnostics {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureDetails {
    pub schema_version: u32,
    pub signature: String,
    pub operation: OperationDiagnostics,
    pub reason: FailureReason,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<LocatorDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<ComparisonDiagnostics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evaluation_transitions: Vec<EvaluationTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_operations: Vec<OperationEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<RecordingDiagnostics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<DiagnosticHint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, String>,
    pub truncated: bool,
}

impl FailureDetails {
    pub fn new(
        operation: impl Into<String>,
        timeout_ms: Option<u64>,
        reason: FailureReason,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: FAILURE_SCHEMA_VERSION,
            signature: String::new(),
            operation: OperationDiagnostics::pending(operation, timeout_ms),
            reason,
            summary: summary.into(),
            locator: None,
            comparison: None,
            evaluation_transitions: Vec::new(),
            recent_operations: Vec::new(),
            terminal: None,
            process: None,
            runtime: None,
            recording: None,
            hints: Vec::new(),
            context: BTreeMap::new(),
            truncated: false,
        }
    }

    pub(crate) fn finish_signature(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.operation.name.as_bytes());
        hasher.update([self.reason as u8]);
        if let Some(locator) = &self.locator {
            hasher.update(locator.search_scope.as_bytes());
            for stage in &locator.stages {
                hasher.update([stage.mode as u8, stage.direction as u8]);
                hasher.update(format!("{:?}", stage.effective_occurrence).as_bytes());
            }
            if let Some(reason) = locator.failure_reason {
                hasher.update([reason as u8]);
            }
        }
        if let Some(runtime) = &self.runtime {
            hasher.update(runtime.backend.as_bytes());
            hasher.update(runtime.terminal_profile_fingerprint.as_bytes());
        }
        self.signature = format!("sha256:{:x}", hasher.finalize());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureArtifactMode {
    #[default]
    Bundle,
    Json,
    Svg,
    Text,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FailureArtifactOptions {
    pub directory: PathBuf,
    pub mode: FailureArtifactMode,
    pub include_recording: bool,
}

impl Default for FailureArtifactOptions {
    fn default() -> Self {
        Self {
            directory: PathBuf::new(),
            mode: FailureArtifactMode::Bundle,
            include_recording: false,
        }
    }
}

impl FailureArtifactOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.mode != FailureArtifactMode::None && self.directory.as_os_str().is_empty() {
            return Err("failure artifact directory must not be empty".to_string());
        }
        Ok(())
    }

    pub(crate) fn wants_text(&self) -> bool {
        matches!(
            self.mode,
            FailureArtifactMode::Bundle | FailureArtifactMode::Text
        )
    }

    pub(crate) fn wants_svg(&self) -> bool {
        matches!(
            self.mode,
            FailureArtifactMode::Bundle | FailureArtifactMode::Svg
        )
    }

    pub(crate) fn wants_json(&self) -> bool {
        !matches!(self.mode, FailureArtifactMode::None)
    }

    pub(crate) fn wants_report(&self) -> bool {
        matches!(self.mode, FailureArtifactMode::Bundle)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionContext {
    pub operation_name: Option<String>,
    pub artifact: Option<FailureArtifactOptions>,
    pub diagnostic_context: BTreeMap<String, String>,
    pub retention: DiagnosticRetentionOptions,
}

impl ExecutionContext {
    pub fn with_operation(mut self, operation_name: impl Into<String>) -> Self {
        self.operation_name = Some(operation_name.into());
        self
    }

    pub fn sanitized_context(&self) -> BTreeMap<String, String> {
        self.diagnostic_context
            .iter()
            .take(MAX_CONTEXT_ENTRIES)
            .map(|(key, value)| {
                let key = truncate_utf8(key, MAX_CONTEXT_KEY_BYTES);
                let value = if value.len() <= MAX_CONTEXT_VALUE_BYTES {
                    value.clone()
                } else {
                    format!("{}...", truncate_utf8(value, MAX_CONTEXT_VALUE_BYTES))
                };
                (key, value)
            })
            .collect()
    }
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticRetentionOptions {
    #[serde(alias = "screen-history-limit")]
    pub screen_history_limit: u16,
}

impl Default for DiagnosticRetentionOptions {
    fn default() -> Self {
        Self {
            screen_history_limit: DEFAULT_SCREEN_HISTORY_LIMIT,
        }
    }
}

impl DiagnosticRetentionOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.screen_history_limit > MAX_SCREEN_HISTORY_LIMIT {
            return Err(format!(
                "screen history limit must be at most {MAX_SCREEN_HISTORY_LIMIT}"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureArtifactStatus {
    Written,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureArtifactRef {
    pub status: FailureArtifactStatus,
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFileStatus {
    Written,
    Omitted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactFile {
    pub kind: String,
    pub path: String,
    pub status: ArtifactFileStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitivityDetails {
    pub contains_locator_operands: bool,
    pub contains_terminal_output: bool,
    pub contains_terminal_title: bool,
    pub contains_visual_output: bool,
    pub contains_recording_output: bool,
    pub contains_assertion_operands: bool,
    pub contains_snapshot_evidence: bool,
    pub contains_diagnostic_context: bool,
    pub contains_user_supplied_values: bool,
    pub permissions: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureArtifactManifest {
    #[serde(flatten)]
    pub details: FailureDetails,
    pub sensitivity: SensitivityDetails,
    pub files: Vec<ArtifactFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FailureObservation {
    pub rows: Vec<Vec<EmuCell>>,
    pub cols: u16,
    pub title: Option<String>,
    pub cursor: Option<(u16, usize)>,
    pub cursor_position: (u16, u16),
    pub cursor_visible: bool,
    pub cursor_shape: CursorShape,
    pub render_state: RenderState,
    pub screen_sequence: u64,
    pub output_revision: u64,
    pub captured_ms: u64,
    pub last_visual_change_ms: u64,
    pub history: ScreenHistoryDetails,
    pub process: ProcessDiagnostics,
    pub runtime: RuntimeDiagnostics,
}

impl FailureObservation {
    pub(crate) fn text(&self) -> String {
        crate::assert::snapshot::serialize(&self.rows, self.cols, false, self.title.as_deref())
    }

    pub(crate) fn svg(&self) -> String {
        crate::render::svg::render_svg_with_zoom(
            &self.rows,
            self.cols,
            &self.render_state,
            self.cursor,
            self.title.as_deref(),
            1.0,
        )
    }

    pub(crate) fn terminal(&self) -> TerminalDiagnostics {
        TerminalDiagnostics {
            size: Size {
                cols: self.cols,
                rows: self.rows.len().min(u16::MAX as usize) as u16,
            },
            title: self.title.clone(),
            cursor: CursorDiagnostics {
                column: self.cursor_position.0,
                row: self.cursor_position.1,
                visible: self.cursor_visible,
                shape: cursor_shape_name(self.cursor_shape).to_string(),
            },
            last_visual_change_ms: self.last_visual_change_ms,
            unchanged_for_ms: self.captured_ms.saturating_sub(self.last_visual_change_ms),
            screen_history: self.history.clone(),
        }
    }
}

pub(crate) fn cursor_shape_name(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Block => "block",
        CursorShape::Underline => "underline",
        CursorShape::Bar => "bar",
    }
}

#[derive(Debug)]
pub(crate) struct ScreenHistory {
    limit: u16,
    dropped_screen_count: u64,
    dropped_row_count: u64,
    next_sequence: u64,
    entries: VecDeque<ScreenEntry>,
    bytes: usize,
}

#[derive(Debug)]
struct ScreenEntry {
    details: ScreenSnapshotDetails,
    rows: Vec<Vec<EmuCell>>,
}

impl ScreenHistory {
    pub(crate) fn new(limit: u16) -> Self {
        Self {
            limit: limit.min(MAX_SCREEN_HISTORY_LIMIT),
            dropped_screen_count: 0,
            dropped_row_count: 0,
            next_sequence: 1,
            entries: VecDeque::new(),
            bytes: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture(
        &mut self,
        rows: Vec<Vec<EmuCell>>,
        cols: u16,
        title: Option<String>,
        cursor: (u16, u16),
        cursor_visible: bool,
        cursor_shape: CursorShape,
        elapsed_ms: u64,
    ) -> u64 {
        let text = crate::assert::snapshot::serialize(&rows, cols, false, title.as_deref());
        if let Some(last) = self.entries.back_mut() {
            if last.rows == rows
                && last.details.title == title
                && last.details.size.cols == cols
                && last.details.size.rows == rows.len().min(u16::MAX as usize) as u16
                && last.details.cursor.column == cursor.0
                && last.details.cursor.row == cursor.1
                && last.details.cursor.visible == cursor_visible
                && last.details.cursor.shape == cursor_shape_name(cursor_shape)
            {
                last.details.last_seen_ms = elapsed_ms;
                last.details.repeat_count = last.details.repeat_count.saturating_add(1);
                return last.details.sequence;
            }
        }

        let changes = match self.entries.back() {
            None => vec!["initial".to_string()],
            Some(previous) => screen_changes(
                &previous.rows,
                &rows,
                &previous.details,
                &title,
                cols,
                cursor,
                cursor_visible,
                cursor_shape,
            ),
        };
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        let details = ScreenSnapshotDetails {
            sequence,
            first_seen_ms: elapsed_ms,
            last_seen_ms: elapsed_ms,
            repeat_count: 1,
            changes,
            size: Size {
                cols,
                rows: rows.len().min(u16::MAX as usize) as u16,
            },
            cursor: CursorDiagnostics {
                column: cursor.0,
                row: cursor.1,
                visible: cursor_visible,
                shape: cursor_shape_name(cursor_shape).to_string(),
            },
            title,
            text,
        };
        self.bytes = self
            .bytes
            .saturating_add(estimate_screen_bytes(&details, &rows));
        self.entries.push_back(ScreenEntry { details, rows });
        self.evict();
        sequence
    }

    fn evict(&mut self) {
        while self.entries.len() > usize::from(self.limit.max(1))
            || (self.bytes > MAX_HISTORY_BYTES && self.entries.len() > 1)
        {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self
                .bytes
                .saturating_sub(estimate_screen_bytes(&entry.details, &entry.rows));
            self.dropped_screen_count = self.dropped_screen_count.saturating_add(1);
            self.dropped_row_count = self
                .dropped_row_count
                .saturating_add(entry.rows.len() as u64);
        }
    }

    pub(crate) fn current_sequence(&self) -> u64 {
        self.entries
            .back()
            .map_or(0, |entry| entry.details.sequence)
    }

    pub(crate) fn snapshot(&self) -> ScreenHistoryDetails {
        ScreenHistoryDetails {
            limit: self.limit,
            dropped_screen_count: self.dropped_screen_count,
            dropped_row_count: self.dropped_row_count,
            screens: self
                .entries
                .iter()
                .map(|entry| entry.details.clone())
                .collect(),
        }
    }
}

fn estimate_screen_bytes(details: &ScreenSnapshotDetails, rows: &[Vec<EmuCell>]) -> usize {
    details.text.len()
        + details.title.as_ref().map_or(0, String::len)
        + rows
            .iter()
            .flatten()
            .map(|cell| cell.ch.len() + std::mem::size_of::<EmuCell>())
            .sum::<usize>()
}

#[allow(clippy::too_many_arguments)]
fn screen_changes(
    previous_rows: &[Vec<EmuCell>],
    rows: &[Vec<EmuCell>],
    previous: &ScreenSnapshotDetails,
    title: &Option<String>,
    cols: u16,
    cursor: (u16, u16),
    cursor_visible: bool,
    cursor_shape: CursorShape,
) -> Vec<String> {
    let mut changes = Vec::new();
    if previous_rows != rows {
        let text_changed = previous_rows.len() != rows.len()
            || previous_rows.iter().zip(rows).any(|(previous_row, row)| {
                previous_row.len() != row.len()
                    || previous_row
                        .iter()
                        .zip(row)
                        .any(|(previous_cell, cell)| previous_cell.ch != cell.ch)
            });
        let style_changed = previous_rows.iter().zip(rows).any(|(previous_row, row)| {
            previous_row.iter().zip(row).any(|(previous_cell, cell)| {
                previous_cell.fg != cell.fg
                    || previous_cell.bg != cell.bg
                    || previous_cell.underline != cell.underline
                    || previous_cell.underline_color != cell.underline_color
                    || previous_cell.attrs != cell.attrs
            })
        });
        if text_changed {
            changes.push("text".to_string());
        }
        if style_changed {
            changes.push("style".to_string());
        }
    }
    if previous.title != *title {
        changes.push("title".to_string());
    }
    if previous.size.cols != cols || previous.size.rows != rows.len().min(u16::MAX as usize) as u16
    {
        changes.push("size".to_string());
    }
    if previous.cursor.column != cursor.0
        || previous.cursor.row != cursor.1
        || previous.cursor.visible != cursor_visible
        || previous.cursor.shape != cursor_shape_name(cursor_shape)
    {
        changes.push("cursor".to_string());
    }
    if changes.is_empty() {
        changes.push("visual".to_string());
    }
    changes
}

pub(crate) struct ArtifactInputs<'a> {
    pub details: &'a mut FailureDetails,
    pub observation: &'a FailureObservation,
    pub recording: Option<PreparedRecording>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedRecording {
    pub temporary_path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

pub(crate) fn allocate_artifact_directory(base: &Path) -> io::Result<PathBuf> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    fs::create_dir_all(base)?;
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    for _ in 0..100 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!(
            "failure-{epoch_ms}-p{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to allocate a unique failure artifact directory",
    ))
}

pub(crate) fn recording_temp_path(directory: &Path) -> PathBuf {
    directory.join("session.cast.tmp")
}

pub(crate) fn write_failure_artifact(
    options: &FailureArtifactOptions,
    inputs: ArtifactInputs<'_>,
    directory: PathBuf,
) -> FailureArtifactRef {
    let mut reference = FailureArtifactRef {
        status: FailureArtifactStatus::Failed,
        directory: directory.to_string_lossy().into_owned(),
        manifest: None,
        report: None,
        screen_text: None,
        screen_svg: None,
        recording: None,
        errors: Vec::new(),
    };
    if !options.wants_json() {
        return reference;
    }

    let mut files = Vec::new();
    let mut total = 0u64;
    if options.wants_text() {
        let screen_text = inputs.observation.text();
        write_optional_file(
            &directory,
            "screen_text",
            "current.txt",
            screen_text.as_bytes(),
            SCREEN_TEXT_LIMIT as u64,
            &mut total,
            &mut files,
            &mut reference.errors,
        );
        if files
            .last()
            .is_some_and(|file| file.status == ArtifactFileStatus::Written)
        {
            reference.screen_text =
                Some(directory.join("current.txt").to_string_lossy().into_owned());
        }
    }

    if options.wants_svg() {
        let cell_count = inputs.observation.rows.iter().map(Vec::len).sum::<usize>();
        if cell_count > 100_000 {
            files.push(ArtifactFile {
                kind: "screen_svg".to_string(),
                path: "current.svg".to_string(),
                status: ArtifactFileStatus::Omitted,
                bytes: None,
                sha256: None,
                reason: Some("size_limit".to_string()),
            });
        } else {
            let svg = inputs.observation.svg();
            write_optional_file(
                &directory,
                "screen_svg",
                "current.svg",
                svg.as_bytes(),
                SCREEN_SVG_LIMIT as u64,
                &mut total,
                &mut files,
                &mut reference.errors,
            );
        }
        if files
            .last()
            .is_some_and(|file| file.status == ArtifactFileStatus::Written)
        {
            reference.screen_svg =
                Some(directory.join("current.svg").to_string_lossy().into_owned());
        }
    }

    if let Some(recording) = inputs.recording {
        let final_path = directory.join("session.cast");
        let status = if recording.bytes > RECORDING_COPY_LIMIT
            || total.saturating_add(recording.bytes) > ARTIFACT_TOTAL_LIMIT
        {
            let _ = fs::remove_file(&recording.temporary_path);
            if let Some(details) = inputs.details.recording.as_mut() {
                details.status = RecordingStatus::Omitted;
                details.path = None;
                details.reason = Some("size_limit".to_string());
            }
            ArtifactFile {
                kind: "recording".to_string(),
                path: "session.cast".to_string(),
                status: ArtifactFileStatus::Omitted,
                bytes: Some(recording.bytes),
                sha256: Some(recording.sha256),
                reason: Some("size_limit".to_string()),
            }
        } else {
            match fs::rename(&recording.temporary_path, &final_path) {
                Ok(()) => {
                    total = total.saturating_add(recording.bytes);
                    reference.recording = Some(final_path.to_string_lossy().into_owned());
                    if let Some(details) = inputs.details.recording.as_mut() {
                        details.status = RecordingStatus::Copied;
                        details.path = Some("session.cast".to_string());
                        details.reason = None;
                    }
                    ArtifactFile {
                        kind: "recording".to_string(),
                        path: "session.cast".to_string(),
                        status: ArtifactFileStatus::Written,
                        bytes: Some(recording.bytes),
                        sha256: Some(recording.sha256),
                        reason: None,
                    }
                }
                Err(error) => {
                    let _ = fs::remove_file(&recording.temporary_path);
                    reference
                        .errors
                        .push(format!("failed to commit session.cast: {error}"));
                    if let Some(details) = inputs.details.recording.as_mut() {
                        details.status = RecordingStatus::Failed;
                        details.path = None;
                        details.reason = Some(error.to_string());
                    }
                    ArtifactFile {
                        kind: "recording".to_string(),
                        path: "session.cast".to_string(),
                        status: ArtifactFileStatus::Failed,
                        bytes: Some(recording.bytes),
                        sha256: Some(recording.sha256),
                        reason: Some(error.to_string()),
                    }
                }
            }
        };
        files.push(status);
    } else if options.include_recording {
        let recording = inputs.details.recording.as_ref();
        let failed = recording.is_some_and(|recording| recording.status == RecordingStatus::Failed);
        files.push(ArtifactFile {
            kind: "recording".to_string(),
            path: "session.cast".to_string(),
            status: if failed {
                ArtifactFileStatus::Failed
            } else {
                ArtifactFileStatus::Omitted
            },
            bytes: recording.and_then(|recording| recording.bytes),
            sha256: None,
            reason: Some(
                recording
                    .and_then(|recording| recording.reason.clone())
                    .unwrap_or_else(|| "recording unavailable".to_string()),
            ),
        });
    }

    if options.wants_report() {
        let report = render_report(inputs.details, &files);
        write_optional_file(
            &directory,
            "report",
            "report.md",
            report.as_bytes(),
            REPORT_LIMIT as u64,
            &mut total,
            &mut files,
            &mut reference.errors,
        );
        if files
            .last()
            .is_some_and(|file| file.status == ArtifactFileStatus::Written)
        {
            reference.report = Some(directory.join("report.md").to_string_lossy().into_owned());
        }
    }

    let sensitivity = sensitivity(inputs.details, &files);
    let manifest = FailureArtifactManifest {
        details: inputs.details.clone(),
        sensitivity,
        files,
        errors: reference.errors.clone(),
    };
    let json = match serde_json::to_vec_pretty(&manifest) {
        Ok(json) if json.len() <= FAILURE_JSON_LIMIT => json,
        Ok(_) => {
            reference
                .errors
                .push("failure.json exceeded the 2 MiB limit".to_string());
            return reference;
        }
        Err(error) => {
            reference
                .errors
                .push(format!("failed to serialize failure.json: {error}"));
            return reference;
        }
    };
    if total.saturating_add(json.len() as u64) > ARTIFACT_TOTAL_LIMIT {
        reference
            .errors
            .push("failure.json would exceed the total artifact limit".to_string());
        return reference;
    }
    let manifest_path = directory.join("failure.json");
    match write_atomic(&manifest_path, &json) {
        Ok(()) => {
            reference.manifest = Some(manifest_path.to_string_lossy().into_owned());
            reference.status = if reference.errors.is_empty()
                && manifest
                    .files
                    .iter()
                    .all(|file| file.status == ArtifactFileStatus::Written)
            {
                FailureArtifactStatus::Written
            } else {
                FailureArtifactStatus::Partial
            };
        }
        Err(error) => reference
            .errors
            .push(format!("failed to commit failure.json: {error}")),
    }
    reference
}

#[allow(clippy::too_many_arguments)]
fn write_optional_file(
    directory: &Path,
    kind: &str,
    name: &str,
    bytes: &[u8],
    limit: u64,
    total: &mut u64,
    files: &mut Vec<ArtifactFile>,
    errors: &mut Vec<String>,
) {
    let length = bytes.len() as u64;
    if length > limit || total.saturating_add(length) > ARTIFACT_TOTAL_LIMIT {
        files.push(ArtifactFile {
            kind: kind.to_string(),
            path: name.to_string(),
            status: ArtifactFileStatus::Omitted,
            bytes: Some(length),
            sha256: Some(format!("sha256:{:x}", Sha256::digest(bytes))),
            reason: Some("size_limit".to_string()),
        });
        return;
    }
    let path = directory.join(name);
    match write_atomic(&path, bytes) {
        Ok(()) => {
            *total = total.saturating_add(length);
            files.push(ArtifactFile {
                kind: kind.to_string(),
                path: name.to_string(),
                status: ArtifactFileStatus::Written,
                bytes: Some(length),
                sha256: Some(format!("sha256:{:x}", Sha256::digest(bytes))),
                reason: None,
            });
        }
        Err(error) => {
            errors.push(format!("failed to write {name}: {error}"));
            files.push(ArtifactFile {
                kind: kind.to_string(),
                path: name.to_string(),
                status: ArtifactFileStatus::Failed,
                bytes: Some(length),
                sha256: Some(format!("sha256:{:x}", Sha256::digest(bytes))),
                reason: Some(error.to_string()),
            });
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
    ));
    fs::write(&temporary, bytes)?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn sensitivity(details: &FailureDetails, files: &[ArtifactFile]) -> SensitivityDetails {
    let has_recording = files
        .iter()
        .any(|file| file.kind == "recording" && file.status == ArtifactFileStatus::Written);
    let has_locator = details.locator.is_some();
    let has_terminal = details.terminal.is_some();
    let has_context = !details.context.is_empty();
    SensitivityDetails {
        contains_locator_operands: has_locator,
        contains_terminal_output: has_terminal,
        contains_terminal_title: details
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.title.is_some()),
        contains_visual_output: files
            .iter()
            .any(|file| file.kind == "screen_svg" && file.status == ArtifactFileStatus::Written),
        contains_recording_output: has_recording,
        contains_assertion_operands: has_locator || details.comparison.is_some(),
        contains_snapshot_evidence: details.reason == FailureReason::SnapshotMismatch,
        contains_diagnostic_context: has_context,
        contains_user_supplied_values: has_locator
            || has_terminal
            || has_recording
            || has_context
            || details.comparison.is_some(),
        permissions: if cfg!(unix) {
            "user_only"
        } else {
            "platform_default"
        }
        .to_string(),
    }
}

fn render_report(details: &FailureDetails, files: &[ArtifactFile]) -> String {
    let mut report = String::new();
    report.push_str("# Terminal failure diagnostic\n\n");
    report.push_str(&format!("**Diagnosis:** {}\n\n", details.summary));
    report.push_str("| Field | Value |\n| --- | --- |\n");
    report.push_str(&format!("| Operation | `{}` |\n", details.operation.name));
    report.push_str(&format!(
        "| Reason | `{}` |\n",
        failure_reason_code(details.reason)
    ));
    report.push_str(&format!(
        "| Elapsed | {} ms |\n",
        details.operation.elapsed_ms
    ));
    if let Some(timeout) = details.operation.timeout_ms {
        report.push_str(&format!("| Timeout | {timeout} ms |\n"));
    }

    if let Some(runtime) = &details.runtime {
        report.push_str(&format!("| Backend | `{}` |\n", runtime.backend));
        report.push_str(&format!("| tui-test | `{}` |\n", runtime.tui_test_version));
    }
    if let Some(process) = &details.process {
        report.push_str(&format!("| Process | `{}` |\n", process.state));
        if let Some(code) = process.exit_code {
            report.push_str(&format!("| Exit code | `{code}` |\n"));
        }
    }

    if let Some(locator) = &details.locator {
        report.push_str("\n## Locator evaluation\n\n");
        report.push_str(
            "| Stage | Selector | Direction | Occurrence | Raw | Style | Selected |\n| --- | --- | --- | --- | ---: | ---: | ---: |\n",
        );
        for stage in &locator.stages {
            report.push_str(&format!(
                "| {} | `{}` | `{:?}` | `{:?}` | {} | {} | {} |\n",
                stage.stage_index,
                stage.selector.description().replace('`', "'"),
                stage.direction,
                stage.effective_occurrence,
                stage.raw_candidate_count,
                stage.style_candidate_count,
                stage.selected_count,
            ));
        }
        if let (Some(stage), Some(reason)) = (locator.failure_stage, locator.failure_reason) {
            report.push_str(&format!(
                "\nFailure occurred at stage {stage}: `{reason:?}`.\n"
            ));
        }
        let mismatches = locator
            .stages
            .iter()
            .flat_map(|stage| stage.mismatches.iter())
            .collect::<Vec<_>>();
        if !mismatches.is_empty() {
            report.push_str("\n### Style mismatches\n\n");
            report.push_str("| Cell | Property | Expected | Actual |\n| --- | --- | --- | --- |\n");
            for mismatch in mismatches.into_iter().take(16) {
                report.push_str(&format!(
                    "| {},{} | `{}` | `{}` | `{}` |\n",
                    mismatch.location.column,
                    mismatch.location.row,
                    mismatch.property,
                    mismatch.expected.replace('`', "'"),
                    mismatch.actual.replace('`', "'"),
                ));
            }
        }
    }

    if let Some(comparison) = &details.comparison {
        report.push_str("\n## Expected versus observed\n\n");
        report.push_str(&format!("Comparison: `{}`\n\n", comparison.kind));
        if let Some(expected) = &comparison.expected {
            report.push_str(&format!("- Expected: `{}`\n", expected.replace('`', "'")));
        }
        if let Some(actual) = &comparison.actual {
            report.push_str(&format!("- Actual: `{}`\n", actual.replace('`', "'")));
        }
    }

    if !details.evaluation_transitions.is_empty() {
        report.push_str("\n## What changed while waiting\n\n");
        report
            .push_str("| Time | Screen | Outcome | Stage counts |\n| ---: | ---: | --- | --- |\n");
        for transition in &details.evaluation_transitions {
            report.push_str(&format!(
                "| {} ms | {} | `{}` | `{:?}` |\n",
                transition.elapsed_ms,
                transition.screen_sequence,
                transition.outcome,
                transition.stage_counts
            ));
        }
    }

    if let Some(terminal) = &details.terminal {
        report.push_str("\n## Terminal state\n\n");
        report.push_str(&format!(
            "The screen was unchanged for {} ms before failure. {} distinct screens were retained.\n",
            terminal.unchanged_for_ms,
            terminal.screen_history.screens.len()
        ));
        if !terminal.screen_history.screens.is_empty() {
            report.push_str(
                "\n| Screen | First seen | Last seen | Changes | Preview |\n| ---: | ---: | ---: | --- | --- |\n",
            );
            for screen in &terminal.screen_history.screens {
                let preview = screen
                    .text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("<blank>")
                    .chars()
                    .take(120)
                    .collect::<String>()
                    .replace('|', "\\|")
                    .replace('`', "'");
                report.push_str(&format!(
                    "| {} | {} ms | {} ms | `{}` | {} |\n",
                    screen.sequence,
                    screen.first_seen_ms,
                    screen.last_seen_ms,
                    screen.changes.join(","),
                    preview
                ));
            }
        }
    }

    if !details.recent_operations.is_empty() {
        report.push_str("\n## Recent operations\n\n");
        report.push_str(
            "| Operation | Result | Started | Ended | Summary |\n| --- | --- | ---: | ---: | --- |\n",
        );
        for operation in &details.recent_operations {
            report.push_str(&format!(
                "| `{}` | `{}` | {} ms | {} ms | {} |\n",
                operation.name,
                operation.result,
                operation.started_ms,
                operation.ended_ms,
                operation.safe_summary.replace('|', "\\|")
            ));
        }
    }

    if !details.hints.is_empty() {
        report.push_str("\n## Next actions\n\n");
        for hint in &details.hints {
            report.push_str(&format!("- **{}:** {}\n", hint.code, hint.message));
        }
    }

    report.push_str("\n## Evidence\n\n");
    for file in files {
        let note = file
            .reason
            .as_deref()
            .map_or(String::new(), |reason| format!(" ({reason})"));
        report.push_str(&format!("- `{}`: `{:?}`{}\n", file.path, file.status, note));
    }
    report
}

fn failure_reason_code(reason: FailureReason) -> &'static str {
    match reason {
        FailureReason::TimedOut => "timed_out",
        FailureReason::SessionExited => "session_exited",
        FailureReason::Cancelled => "cancelled",
        FailureReason::LocatorNoMatch => "locator_no_match",
        FailureReason::LocatorAmbiguous => "locator_ambiguous",
        FailureReason::UnexpectedMatch => "unexpected_match",
        FailureReason::MatchNotActionable => "match_not_actionable",
        FailureReason::ScalarMismatch => "scalar_mismatch",
        FailureReason::SnapshotMismatch => "snapshot_mismatch",
        FailureReason::EmulatorFault => "emulator_fault",
        FailureReason::InternalFailure => "internal_failure",
    }
}

pub(crate) fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn profile_fingerprint(profile: &crate::profile::Profile) -> String {
    let bytes = serde_json::to_vec(profile).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{Attrs, EmuCell};
    use crate::terminal::emu::Emulator;

    #[test]
    fn context_is_bounded_without_splitting_utf8() {
        let mut context = ExecutionContext::default();
        context
            .diagnostic_context
            .insert("test".to_string(), "x".repeat(300));
        let values = context.sanitized_context();
        assert!(values["test"].ends_with("..."));
        assert!(values["test"].len() <= MAX_CONTEXT_VALUE_BYTES + 3);
    }

    #[test]
    fn artifact_directories_do_not_overwrite() {
        let root =
            std::env::temp_dir().join(format!("tui-test-failure-artifact-{}", std::process::id()));
        let first = allocate_artifact_directory(&root).unwrap();
        let second = allocate_artifact_directory(&root).unwrap();
        assert_ne!(first, second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn screen_history_deduplicates_and_retains_style_changes() {
        let mut history = ScreenHistory::new(3);
        let plain = vec![vec![EmuCell::blank()]];
        let first = history.capture(plain.clone(), 1, None, (0, 0), true, CursorShape::Block, 1);
        let repeated = history.capture(plain.clone(), 1, None, (0, 0), true, CursorShape::Block, 2);
        assert_eq!(first, repeated);
        assert_eq!(history.snapshot().screens[0].repeat_count, 2);

        let mut styled = plain;
        styled[0][0].attrs.insert(Attrs::BOLD);
        let second = history.capture(styled, 1, None, (0, 0), true, CursorShape::Block, 3);
        assert_ne!(first, second);
        let screens = history.snapshot().screens;
        assert_eq!(screens.len(), 2);
        assert_eq!(screens[1].changes, vec!["style"]);
    }

    #[test]
    fn failure_details_round_trip() {
        let mut details = FailureDetails::new(
            "locator.expect",
            Some(25),
            FailureReason::LocatorNoMatch,
            "missing",
        );
        details.finish_signature();
        let encoded = serde_json::to_string(&details).unwrap();
        let decoded: FailureDetails = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, details);
    }

    #[test]
    fn recording_status_reflects_commit_failure() {
        let root =
            std::env::temp_dir().join(format!("tui-test-recording-commit-{}", std::process::id()));
        let directory = allocate_artifact_directory(&root).unwrap();
        fs::create_dir(directory.join("session.cast")).unwrap();
        let temporary_path = recording_temp_path(&directory);
        fs::write(&temporary_path, b"cast").unwrap();

        let emu = AlacrittyEmu::new(1, 1, &Profile::default());
        let rows = emu.viewable_rows();
        let observation = FailureObservation {
            rows: rows.clone(),
            cols: 1,
            title: None,
            cursor: None,
            cursor_position: (0, 0),
            cursor_visible: false,
            cursor_shape: CursorShape::Block,
            render_state: RenderState::capture(&emu),
            screen_sequence: 1,
            output_revision: 1,
            captured_ms: 1,
            last_visual_change_ms: 1,
            history: ScreenHistoryDetails {
                limit: 1,
                dropped_screen_count: 0,
                dropped_row_count: 0,
                screens: Vec::new(),
            },
            process: ProcessDiagnostics {
                pid: None,
                state: "running".to_string(),
                exit_code: None,
                status_error: None,
                cancelled: false,
                ready: false,
                command_running: false,
                last_command_exit: None,
            },
            runtime: RuntimeDiagnostics {
                tui_test_version: "test".to_string(),
                backend: "alacritty".to_string(),
                target_os: std::env::consts::OS.to_string(),
                target_arch: std::env::consts::ARCH.to_string(),
                terminal_profile_fingerprint: "sha256:test".to_string(),
            },
        };
        let mut details = FailureDetails::new(
            "locator.expect",
            Some(1),
            FailureReason::LocatorNoMatch,
            "missing",
        );
        details.recording = Some(RecordingDiagnostics {
            mode: AutomaticRecordingMode::OnFailure,
            status: RecordingStatus::Live,
            failure_offset_ms: 1,
            last_committed_ms: Some(1),
            path: None,
            bytes: Some(4),
            reason: None,
            ephemeral: false,
        });
        let reference = write_failure_artifact(
            &FailureArtifactOptions {
                directory: root.clone(),
                mode: FailureArtifactMode::Json,
                include_recording: true,
            },
            ArtifactInputs {
                details: &mut details,
                observation: &observation,
                recording: Some(PreparedRecording {
                    temporary_path,
                    bytes: 4,
                    sha256: format!("sha256:{:x}", Sha256::digest(b"cast")),
                }),
            },
            directory,
        );
        assert_eq!(details.recording.unwrap().status, RecordingStatus::Failed);
        assert!(reference.recording.is_none());
        let _ = fs::remove_dir_all(root);
    }
}
