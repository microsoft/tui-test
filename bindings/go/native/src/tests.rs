use super::*;

fn text(value: &str) -> TuiString {
    TuiString {
        data: value.as_ptr(),
        len: value.len(),
    }
}
unsafe fn message(result: *mut TuiResult) -> String {
    unsafe { (*result).error_message.required().unwrap() }
}

#[test]
fn missing_sessions_retain_structured_errors_and_owned_messages() {
    for _ in 0..128 {
        let result = unsafe { tui_state(text("go-abi-missing-session")) };
        assert_eq!(unsafe { (*result).error_kind }, 3);
        assert!(unsafe { message(result) }.contains("no active session"));
        unsafe { tui_result_free(result) };
    }
    unsafe { tui_result_free(std::ptr::null_mut()) };
}

#[test]
fn pointer_options_reject_null_and_preserve_validation() {
    let session = text("go-abi-pointer-options");
    let options = TuiOpenOptions {
        cols: TuiOptionalU64 {
            present: true,
            value: 65_536,
        },
        ..Default::default()
    };
    let results = unsafe {
        [
            tui_open_ptr(session, std::ptr::null()),
            tui_run_ptr(session, std::ptr::null(), text(""), std::ptr::null(), 0),
            tui_open_ptr(session, &options),
            tui_run_ptr(session, &options, text(""), std::ptr::null(), 0),
        ]
    };
    for result in results {
        assert_eq!(unsafe { (*result).error_kind }, 2);
        unsafe { tui_result_free(result) };
    }
}
#[test]
fn panic_is_contained_and_next_call_succeeds() {
    let result = boundary(|| panic!("intentional boundary test"));
    assert_eq!(unsafe { (*result).error_kind }, 5);
    assert!(unsafe { message(result) }.contains("panicked"));
    unsafe { tui_result_free(result) };
    let result = unsafe { tui_close(text("go-abi-missing-session")) };
    assert_eq!(unsafe { (*result).error_kind }, 0);
    unsafe { tui_result_free(result) };
}
#[test]
fn invalid_utf8_and_absent_required_inputs_report_usage() {
    let bytes = [0xff];
    for session in [
        TuiString::default(),
        TuiString {
            data: bytes.as_ptr(),
            len: 1,
        },
        TuiString {
            data: std::ptr::null(),
            len: 1,
        },
    ] {
        let result = unsafe { tui_state(session) };
        assert_eq!(unsafe { (*result).error_kind }, 2);
        unsafe { tui_result_free(result) };
    }
}
#[test]
fn input_conversion_preserves_explicit_zero_false_and_empty_values() {
    let options = TuiOpenOptions {
        cols: TuiOptionalU64 {
            present: true,
            value: 0,
        },
        wait_ready: TuiOptionalBool {
            present: true,
            value: false,
        },
        cwd: text(""),
        ..Default::default()
    };
    let converted = unsafe { input::open(options).unwrap() };
    assert_eq!(converted.cols, 0);
    assert_eq!(converted.rows, 30);
    assert_eq!(converted.wait_ready, Some(false));
    assert_eq!(converted.cwd, Some(String::new()));
    let absent = unsafe { input::open(TuiOpenOptions::default()).unwrap() };
    assert_eq!(absent.cols, 80);
    assert_eq!(absent.wait_ready, None);
    assert_eq!(absent.cwd, None);
}
#[test]
fn output_buffers_preserve_nuls_unicode_and_absence() {
    let expected = "a\0界🙂";
    let result = output::operation(OperationResult::Text(expected.into()));
    assert_eq!(unsafe { (*result).text.required().unwrap() }, expected);
    unsafe { tui_result_free(result) };
    let result = output::operation(OperationResult::Title(None));
    assert!(unsafe { (*result).text.data.is_null() });
    unsafe { tui_result_free(result) };
    let result = output::operation(OperationResult::Title(Some(String::new())));
    assert!(!unsafe { (*result).text.data.is_null() });
    assert_eq!(unsafe { (*result).text.len }, 0);
    unsafe { tui_result_free(result) };
}
#[test]
fn locator_stages_preserve_parent_selection_and_strict_action_semantics() {
    let stages = [
        TuiLocatorStage {
            text: text("parent"),
            occurrence: 4,
            index: 2,
            ..Default::default()
        },
        TuiLocatorStage {
            text: text("child"),
            direction: 1,
            style: TuiTextStyle {
                bold: TuiOptionalBool {
                    present: true,
                    value: false,
                },
                ..Default::default()
            },
            ..Default::default()
        },
    ];
    let query = TuiQuery {
        stages: stages.as_ptr(),
        len: stages.len(),
    };
    let converted = unsafe { input::query(query, true).unwrap() };
    assert_eq!(converted.occurrence, MatchOccurrence::Unique);
    assert_eq!(converted.direction, LocatorDirection::After);
    assert_eq!(converted.style.bold, Some(false));
    assert_eq!(
        converted.within.unwrap().occurrence,
        MatchOccurrence::Nth(2)
    );
    assert_eq!(
        unsafe { input::query(query, false).unwrap() }.occurrence,
        MatchOccurrence::Any
    );
}
#[test]
fn match_output_owns_nested_spans() {
    let result = output::operation(OperationResult::Matches(vec![TextMatch {
        text: "界".into(),
        start: TextPosition { row: 3, column: 2 },
        end: TextPosition { row: 3, column: 4 },
        spans: vec![TextSpan {
            row: 3,
            start: 2,
            end: 4,
        }],
    }]));
    let matches = unsafe { input::slice((*result).matches, (*result).matches_len).unwrap() };
    assert_eq!(unsafe { matches[0].text.required().unwrap() }, "界");
    let spans = unsafe { input::slice(matches[0].spans, matches[0].spans_len).unwrap() };
    assert_eq!(spans[0].end, 4);
    unsafe { tui_result_free(result) };
}
