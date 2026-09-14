use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::{
    AutomaticRecordingMode, LocatorDirection, LocatorSelector, MatchOccurrence, Size, TextMatch,
    TextPosition,
};
use crate::render::svg::RenderState;
use crate::terminal::cell::EmuCell;
use crate::terminal::emu::CursorShape;

mod expectation;
mod failure;
mod input;
pub(crate) mod strings;
pub use expectation::{LocatorExpectation, OperationExpectation};
pub(crate) use failure::{failure_reason, merge_failure_details};
pub use failure::{FailureDetails, LocatorFailure};
pub use input::{InputArguments, InputDetails, MouseTarget};

pub const FAILURE_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SCREEN_HISTORY_LIMIT: u16 = 10;
pub const MAX_SCREEN_HISTORY_LIMIT: u16 = 50;

const MAX_HISTORY_BYTES: usize = 512 * 1024;
const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTEXT_ENTRIES: usize = 16;
const MAX_CONTEXT_KEY_BYTES: usize = 64;
const MAX_CONTEXT_VALUE_BYTES: usize = 256;

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
    Completed,
    TestFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorFailureReason {
    AnchorNotFound,
    AnchorAmbiguous,
    RelativeRegionNoMatch,
    StyleFilterRemovedAll,
    LinkFilterRemovedAll,
    IntersectionEmpty,
    UnionEmpty,
    FilterRemovedAll,
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
    ContiguousLinkRuns,
    ParentLinkFilter,
    Intersection,
    Union,
    ContainmentFilter,
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
    #[serde(default)]
    pub expression_path: String,
    #[serde(default)]
    pub evaluations: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<LocatorFailureReason>,
    pub mode: LocatorStageMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<LocatorSelector>,
    pub direction: LocatorDirection,
    pub requested_occurrence: MatchOccurrence,
    /// Action operations may require a unique match.
    pub effective_occurrence: MatchOccurrence,
    pub occurrence_source: OccurrenceSource,
    pub input_candidate_count: usize,
    pub raw_candidate_count: usize,
    /// Candidates remaining after applying the requested styles.
    pub style_candidate_count: usize,
    pub selected_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<TextMatch>,
    pub candidates_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mismatches: Vec<CellMismatch>,
    pub mismatches_truncated: bool,
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
    pub mismatches_truncated: bool,
}

impl CellStyleEvaluation {
    pub(crate) fn reject(&mut self, limit: usize, capture: impl FnOnce() -> CellMismatch) -> bool {
        self.matched = false;
        if self.mismatches.len() == limit {
            self.mismatches_truncated = true;
            return false;
        }
        self.mismatches.push(capture());
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatorDiagnostics {
    pub search_scope: String,
    pub viewport_origin_y: u32,
    pub stages: Vec<LocatorStageDiagnostics>,
    /// Candidates before the final occurrence is selected.
    pub final_candidate_count: usize,
    #[serde(default)]
    pub stages_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation_error: Option<String>,
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
    #[serde(default)]
    pub is_assertion: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expectation: Option<OperationExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<InputDetails>,
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
    #[serde(default)]
    pub dropped_checkpoint_count: u64,
    pub screens: Vec<ScreenSnapshotDetails>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<ScreenSnapshotDetails>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeouts: Option<crate::api::EffectiveTimeouts>,
    pub tui_test_version: String,
    pub backend: String,
    pub target_os: String,
    pub target_arch: String,
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
pub struct FailureReport {
    pub schema_version: u32,
    pub signature: String,
    pub operation: OperationDiagnostics,
    pub reason: FailureReason,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<TraceOutcome>,
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

impl FailureReport {
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
            outcome: None,
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
        }
        self.signature = format!("sha256:{:x}", hasher.finalize());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureArtifactMode {
    #[default]
    All,
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
            mode: FailureArtifactMode::All,
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionContext {
    pub operation_name: Option<String>,
    pub artifact: Option<FailureArtifactOptions>,
    pub diagnostic_context: BTreeMap<String, String>,
    pub retention: DiagnosticRetentionOptions,
    pub trace: Option<TraceOptions>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceMode {
    #[default]
    Off,
    On,
    OnFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TraceOptions {
    pub mode: TraceMode,
    pub directory: PathBuf,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            mode: TraceMode::Off,
            directory: PathBuf::from(".tui-test/traces"),
        }
    }
}

impl TraceOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.directory.as_os_str().is_empty() {
            return Err("trace directory must not be empty".into());
        }
        Ok(())
    }
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
    pub report_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline: Option<String>,
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
    #[serde(default)]
    pub contains_input: bool,
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
    pub details: FailureReport,
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
    pub cursor_position: (u16, u16),
    pub cursor_visible: bool,
    pub cursor_shape: CursorShape,
    pub screen_sequence: u64,
    pub captured_ms: u64,
    pub last_visual_change_ms: u64,
    pub history: ScreenHistory,
    pub process: ProcessDiagnostics,
    pub runtime: RuntimeDiagnostics,
}

impl FailureObservation {
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
            screen_history: self.history.snapshot(),
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

#[derive(Debug, Clone)]
pub(crate) struct ScreenHistory {
    limit: u16,
    dropped_screen_count: u64,
    dropped_row_count: u64,
    next_sequence: u64,
    entries: VecDeque<std::sync::Arc<ScreenFrame>>,
    checkpoints: VecDeque<std::sync::Arc<ScreenFrame>>,
    checkpoint_bytes: usize,
    dropped_checkpoint_count: u64,
    bytes: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ScreenFrame {
    details: ScreenSnapshotDetails,
    rows: std::sync::Arc<Vec<Vec<EmuCell>>>,
    render_state: RenderState,
}

impl ScreenHistory {
    pub(crate) fn new(limit: u16) -> Self {
        Self {
            limit: limit.min(MAX_SCREEN_HISTORY_LIMIT),
            dropped_screen_count: 0,
            dropped_row_count: 0,
            next_sequence: 1,
            entries: VecDeque::new(),
            checkpoints: VecDeque::new(),
            checkpoint_bytes: 0,
            dropped_checkpoint_count: 0,
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
        render_state: RenderState,
    ) -> u64 {
        let text = crate::assert::snapshot::serialize(&rows, cols, false, title.as_deref());
        if let Some(last) = self.entries.back_mut() {
            if last.rows.as_ref() == &rows
                && last.details.title == title
                && last.details.size.cols == cols
                && last.details.size.rows == rows.len().min(u16::MAX as usize) as u16
                && last.details.cursor.column == cursor.0
                && last.details.cursor.row == cursor.1
                && last.details.cursor.visible == cursor_visible
                && last.details.cursor.shape == cursor_shape_name(cursor_shape)
                && last.render_state == render_state
            {
                let last = std::sync::Arc::make_mut(last);
                last.details.last_seen_ms = elapsed_ms;
                last.details.repeat_count = last.details.repeat_count.saturating_add(1);
                return last.details.sequence;
            }
        }

        let mut changes = match self.entries.back() {
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
        if self
            .entries
            .back()
            .is_some_and(|previous| previous.render_state != render_state)
        {
            changes.push("palette".to_string());
        }
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
        self.entries.push_back(std::sync::Arc::new(ScreenFrame {
            details,
            rows: std::sync::Arc::new(rows),
            render_state,
        }));
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
            dropped_checkpoint_count: self.dropped_checkpoint_count,
            screens: self
                .entries
                .iter()
                .map(|entry| entry.details.clone())
                .collect(),
            checkpoints: self
                .checkpoints
                .iter()
                .map(|entry| entry.details.clone())
                .collect(),
        }
    }

    pub(crate) fn pin_current(&mut self) {
        let Some(frame) = self.entries.back() else {
            return;
        };
        if self
            .checkpoints
            .back()
            .is_some_and(|last| last.details.sequence == frame.details.sequence)
        {
            return;
        }
        let bytes = estimate_screen_bytes(&frame.details, &frame.rows);
        if self.limit == 0 || bytes > MAX_CHECKPOINT_BYTES {
            self.dropped_checkpoint_count = self.dropped_checkpoint_count.saturating_add(1);
            return;
        }
        self.checkpoint_bytes += bytes;
        self.checkpoints.push_back(frame.clone());
        while self.checkpoints.len() > MAX_OPERATION_HISTORY
            || self.checkpoint_bytes > MAX_CHECKPOINT_BYTES
        {
            if let Some(old) = self.checkpoints.pop_front() {
                self.checkpoint_bytes -= estimate_screen_bytes(&old.details, &old.rows);
                self.dropped_checkpoint_count = self.dropped_checkpoint_count.saturating_add(1);
            }
        }
    }

    #[cfg(test)]
    fn retained(&self) -> BTreeMap<u64, &std::sync::Arc<ScreenFrame>> {
        self.checkpoints
            .iter()
            .chain(&self.entries)
            .map(|frame| (frame.details.sequence, frame))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn frames(&self) -> Vec<std::sync::Arc<ScreenFrame>> {
        self.retained()
            .values()
            .map(|frame| (*frame).clone())
            .collect()
    }
}

fn estimate_screen_bytes(details: &ScreenSnapshotDetails, rows: &[Vec<EmuCell>]) -> usize {
    details.text.len()
        + std::mem::size_of::<RenderState>()
        + details.title.as_ref().map_or(0, String::len)
        + rows
            .iter()
            .flatten()
            .map(|cell| {
                cell.ch.len()
                    + std::mem::size_of::<EmuCell>()
                    + cell.hyperlink.as_ref().map_or(0, |link| {
                        link.uri.len() + link.id.as_ref().map_or(0, |id| id.len())
                    })
            })
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

pub(crate) fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
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
    fn screen_history_deduplicates_and_retains_style_changes() {
        let mut history = ScreenHistory::new(3);
        let emu = AlacrittyEmu::new(1, 1, &Profile::default());
        let plain = vec![vec![EmuCell::blank()]];
        let first = history.capture(
            plain.clone(),
            1,
            None,
            (0, 0),
            true,
            CursorShape::Block,
            1,
            RenderState::capture(&emu),
        );
        let repeated = history.capture(
            plain.clone(),
            1,
            None,
            (0, 0),
            true,
            CursorShape::Block,
            2,
            RenderState::capture(&emu),
        );
        assert_eq!(first, repeated);
        assert_eq!(history.snapshot().screens[0].repeat_count, 2);

        let mut styled = plain;
        styled[0][0].attrs.insert(Attrs::BOLD);
        let second = history.capture(
            styled,
            1,
            None,
            (0, 0),
            true,
            CursorShape::Block,
            3,
            RenderState::capture(&emu),
        );
        assert_ne!(first, second);
        let screens = history.snapshot().screens;
        assert_eq!(screens.len(), 2);
        assert_eq!(screens[1].changes, vec!["style"]);
    }

    #[test]
    fn observations_share_history_grids_but_freeze_frame_metadata() {
        use std::sync::Arc;
        let mut emu = AlacrittyEmu::new(80, 24, &Profile::default());
        let mut history = ScreenHistory::new(32);
        for index in 0..32 {
            emu.process(format!("\x1b[H{index:02}").as_bytes());
            history.capture(
                emu.viewable_rows(),
                80,
                None,
                emu.cursor(),
                false,
                CursorShape::Block,
                index,
                RenderState::capture(&emu),
            );
            history.pin_current();
        }
        let (saved, allocation) = crate::test_allocations::measure(|| history.clone());
        assert!(
            allocation.peak < 16 * 1024,
            "peak bytes: {}",
            allocation.peak
        );
        let frame = saved.entries.back().unwrap();
        assert!(Arc::ptr_eq(frame, history.entries.back().unwrap()));
        assert!(Arc::ptr_eq(frame, history.checkpoints.back().unwrap()));
        history.capture(
            emu.viewable_rows(),
            80,
            None,
            emu.cursor(),
            false,
            CursorShape::Block,
            100,
            RenderState::capture(&emu),
        );
        let latest = history.entries.back().unwrap();
        assert!(!Arc::ptr_eq(frame, latest));
        assert!(Arc::ptr_eq(&frame.rows, &latest.rows));
        assert_eq!(frame.details.last_seen_ms, 31);
        assert_eq!(latest.details.last_seen_ms, 100);
        emu.process(b"\x1b[HLATER");
        history.capture(
            emu.viewable_rows(),
            80,
            None,
            emu.cursor(),
            false,
            CursorShape::Block,
            101,
            RenderState::capture(&emu),
        );
        assert!(!saved
            .snapshot()
            .screens
            .last()
            .unwrap()
            .text
            .contains("LATER"));
        assert!(history
            .snapshot()
            .screens
            .last()
            .unwrap()
            .text
            .contains("LATER"));
    }

    #[test]
    fn failure_details_round_trip() {
        let mut details = FailureReport::new(
            "locator.expect",
            Some(25),
            FailureReason::LocatorNoMatch,
            "missing",
        );
        details.finish_signature();
        let encoded = serde_json::to_string(&details).unwrap();
        let decoded: FailureReport = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, details);
    }

    #[test]
    fn checkpoints_survive_sample_eviction_and_remain_bounded() {
        let emu = AlacrittyEmu::new(1, 1, &Profile::default());
        let mut history = ScreenHistory::new(1);
        for sequence in 1..=40 {
            history.capture(
                vec![vec![EmuCell {
                    ch: sequence.to_string().into(),
                    ..EmuCell::blank()
                }]],
                1,
                None,
                (0, 0),
                false,
                CursorShape::Block,
                sequence,
                RenderState::capture(&emu),
            );
            history.pin_current();
        }
        let snapshot = history.snapshot();
        assert_eq!(snapshot.screens.len(), 1);
        assert_eq!(snapshot.checkpoints.len(), MAX_OPERATION_HISTORY);
        assert_eq!(snapshot.dropped_screen_count, 39);
        assert_eq!(snapshot.dropped_checkpoint_count, 8);
        assert_eq!(snapshot.checkpoints[0].sequence, 9);
        assert!(history.checkpoint_bytes <= MAX_CHECKPOINT_BYTES);
        let mut disabled = ScreenHistory::new(0);
        for sequence in 1..=3 {
            disabled.capture(
                vec![vec![EmuCell::blank()]],
                1,
                Some(sequence.to_string()),
                (0, 0),
                false,
                CursorShape::Block,
                sequence,
                RenderState::capture(&emu),
            );
            disabled.pin_current();
        }
        assert_eq!(disabled.snapshot().screens.len(), 1);
        assert!(disabled.checkpoints.is_empty());
        assert_eq!(disabled.snapshot().dropped_checkpoint_count, 3);

        let mut large = ScreenHistory::new(1);
        for sequence in 1..=16 {
            let mut rows = vec![vec![EmuCell::blank(); 450]; 50];
            rows[0][0].ch = sequence.to_string().into();
            large.capture(
                rows,
                450,
                None,
                (0, 0),
                false,
                CursorShape::Block,
                sequence,
                RenderState::capture(&emu),
            );
            large.pin_current();
            assert!(large.checkpoint_bytes <= MAX_CHECKPOINT_BYTES);
        }
        assert!(
            large.checkpoints.len() < 16,
            "the byte budget must evict before the count budget"
        );
        assert!(large.snapshot().dropped_checkpoint_count > 0);
    }

    #[test]
    fn palette_only_changes_create_distinct_pinned_frames() {
        let mut emu = AlacrittyEmu::new(1, 1, &Profile::default());
        let mut history = ScreenHistory::new(1);
        let rows = emu.viewable_rows();
        history.capture(
            rows.clone(),
            1,
            None,
            (0, 0),
            false,
            CursorShape::Block,
            1,
            RenderState::capture(&emu),
        );
        history.pin_current();
        emu.process(b"\x1b]10;#abcdef\x07");
        history.capture(
            rows,
            1,
            None,
            (0, 0),
            false,
            CursorShape::Block,
            2,
            RenderState::capture(&emu),
        );
        let snapshot = history.snapshot();
        assert_eq!(snapshot.screens.len(), 1);
        assert_eq!(snapshot.checkpoints.len(), 1);
        assert!(snapshot.screens[0].changes.contains(&"palette".to_string()));
        assert_ne!(
            history.frames()[0].render_state,
            history.frames()[1].render_state
        );
    }
}
