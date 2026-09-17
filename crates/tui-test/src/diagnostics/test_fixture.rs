use super::*;
use crate::profile::Profile;
use crate::render::svg::RenderState;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::emu::Emulator;

pub(super) const HOSTILE: &str = "</script><script>globalThis.reportPwned=true</script>\n| ``` ` & <img src=x onerror=alert(1)> /* REPORT_JS */ null /* REPORT_DATA */";

fn capture(history: &mut ScreenHistory, emu: &AlacrittyEmu, time: u64) -> u64 {
    history.capture(
        emu.viewable_rows(),
        emu.size().0,
        emu.title(),
        emu.cursor(),
        emu.cursor_visible(),
        Emulator::cursor_shape(emu),
        time,
        RenderState::capture(emu),
    )
}

pub(super) fn fixture() -> (FailureReport, FailureObservation) {
    let mut emu = AlacrittyEmu::new(20, 4, &Profile::default());
    let mut history = ScreenHistory::new(10);
    emu.process(b"\x1b[?25lREADY");
    capture(&mut history, &emu, 10);
    history.pin_current();
    capture(&mut history, &emu, 12);
    emu.process(b"\x1b[2J\x1b[HWORKING");
    capture(&mut history, &emu, 20);
    emu.resize(24, 5);
    emu.process("\x1b]4;1;#112233\x07\x1b[2J\x1b[H\x1b[31mA\x1b[0m\u{4f60}e\u{301}".as_bytes());
    emu.process(b"\x1b]2;</script><img src=x onerror=alert(1)>\x07");
    let sequence = capture(&mut history, &emu, 30);
    capture(&mut history, &emu, 40);
    history.pin_current();
    let observation = FailureObservation {
        rows: emu.viewable_rows(),
        cols: emu.size().0,
        title: emu.title(),
        cursor: None,
        cursor_position: emu.cursor(),
        cursor_visible: emu.cursor_visible(),
        cursor_shape: Emulator::cursor_shape(&emu),
        render_state: RenderState::capture(&emu),
        screen_sequence: sequence,
        output_revision: 3,
        captured_ms: 40,
        last_visual_change_ms: 30,
        history,
        process: ProcessDiagnostics {
            pid: Some(123),
            state: "running".into(),
            exit_code: None,
            status_error: None,
            cancelled: false,
            ready: true,
            command_running: false,
            last_command_exit: Some(0),
        },
        runtime: RuntimeDiagnostics {
            session_name: Some("deployment-wizard".into()),
            shell: Some("pwsh".into()),
            timeouts: Some(crate::api::EffectiveTimeouts {
                text: 5000,
                idle: 5000,
                command: 30000,
                exit: 30000,
                ready: 30000,
            }),
            tui_test_version: "fixture".into(),
            backend: "alacritty".into(),
            target_os: "test".into(),
            target_arch: "test".into(),
        },
    };
    let mut details = FailureReport::new(
        "expect.text",
        Some(20),
        FailureReason::LocatorNoMatch,
        HOSTILE,
    );
    details.operation.failed_screen_sequence = sequence;
    details.operation.started_screen_sequence = 2;
    details.operation.elapsed_ms = 20;
    details.terminal = Some(observation.terminal());
    details.runtime = Some(observation.runtime.clone());
    details.process = Some(observation.process.clone());
    details.context.insert("test".into(), HOSTILE.into());
    details.comparison = Some(ComparisonDiagnostics {
        kind: "text".into(),
        expected: Some(HOSTILE.into()),
        actual: Some("A\u{4f60}e\u{301}".into()),
    });
    for (id, name, screen, result, assertion, started_ms, ended_ms) in [
        (1, "evicted check", 999, "ok", true, 0, 1),
        (2, "ready", 1, "ok", true, 2, 10),
        (3, "ready again", 1, "ok", true, 10, 12),
        (4, "submit", 2, "ok", false, 12, 20),
        (5, "expect.text", sequence, "assertion", true, 20, 40),
    ] {
        details.recent_operations.push(OperationEvent {
            sequence: id,
            name: name.into(),
            started_ms,
            ended_ms,
            result: result.into(),
            screen_before: if id == 5 { 2 } else { 0 },
            screen_at_return: screen,
            safe_summary: name.into(),
            input: None,
            is_assertion: assertion,
            expectation: match id {
                2 | 3 => Some(OperationExpectation::Locator {
                    query: Box::new(crate::api::LocatorQuery::text("READY")),
                    outcome: LocatorExpectation::Visible,
                }),
                5 => {
                    let mut query = crate::api::LocatorQuery::text("A");
                    query.style.foreground = Some("2".into());
                    Some(OperationExpectation::Locator {
                        query: Box::new(query),
                        outcome: LocatorExpectation::Visible,
                    })
                }
                _ => None,
            },
        });
    }
    details.locator = Some(LocatorDiagnostics {
        stages_truncated: false,
        evaluation_error: None,
        search_scope: "full".into(),
        viewport_origin_y: 7,
        final_candidate_count: 0,
        selected: Vec::new(),
        failure_stage: Some(0),
        failure_reason: Some(LocatorFailureReason::StyleFilterRemovedAll),
        stages: vec![LocatorStageDiagnostics {
            expression_path: "root".into(),
            evaluations: 1,
            failure_reason: Some(LocatorFailureReason::StyleFilterRemovedAll),
            stage_index: 0,
            mode: LocatorStageMode::Text,
            selector: Some(crate::api::LocatorQuery::text("A").selector),
            direction: crate::api::LocatorDirection::Within,
            requested_occurrence: crate::api::MatchOccurrence::Any,
            effective_occurrence: crate::api::MatchOccurrence::Any,
            occurrence_source: OccurrenceSource::Explicit,
            input_candidate_count: 0,
            raw_candidate_count: 1,
            style_candidate_count: 0,
            selected_count: 0,
            candidates: Vec::new(),
            candidates_truncated: false,
            mismatches_truncated: false,
            mismatches: vec![CellMismatch {
                location: crate::api::TextPosition { column: 0, row: 7 },
                grapheme: "A".into(),
                property: "foreground".into(),
                operator: "equals".into(),
                expected: "2".into(),
                actual: "1".into(),
                resolved: Some("#112233".into()),
                reason: "color mismatch".into(),
            }],
        }],
    });
    details.finish_signature();
    (details, observation)
}
