use super::*;
use std::time::{Duration, Instant};

unsafe fn completed(task: *mut TuiTask) -> *mut TuiResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = unsafe { tasks::tui_task_poll(task) };
        if !result.is_null() {
            assert!(unsafe { tasks::tui_task_poll(task) }.is_null());
            unsafe { tasks::tui_task_free(task) };
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "asynchronous task did not complete"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn typed_start_copies_inputs_and_transfers_result_once() {
    let task = {
        let context = String::from(r#"{"name":"async-owned-input"}"#);
        unsafe { tui_state_start(text(&context)) }
    };
    let result = unsafe { completed(task) };
    assert_eq!(unsafe { error_kind(result) }, 3);
    assert!(unsafe { message(result) }.contains("no active session"));
    unsafe { tui_result_free(result) };
    let invalid = unsafe { tui_state_start(TuiString::default()) };
    let result = unsafe { completed(invalid) };
    assert_eq!(unsafe { error_kind(result) }, 2);
    unsafe { tui_result_free(result) };
}

#[test]
fn asynchronous_panics_are_contained() {
    let task = tasks::tui_panic_probe_start();
    let result = unsafe { completed(task) };
    assert_eq!(unsafe { error_kind(result) }, 5);
    assert!(unsafe { message(result) }.contains("intentional native panic probe"));
    unsafe { tui_result_free(result) };
}

#[test]
fn pending_tasks_can_be_abandoned_without_borrowing_foreign_storage() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let task = tasks::start(|| {
        tasks::execute(move || {
            receiver.recv().unwrap();
            Ok(OperationResult::Text("owned discarded result".into()))
        })
    });
    unsafe { tasks::tui_task_free(task) };
    sender.send(()).unwrap();
    unsafe { tasks::tui_task_free(std::ptr::null_mut()) };
}

fn text(value: &str) -> TuiString {
    TuiString {
        data: value.as_ptr(),
        len: value.len(),
    }
}
unsafe fn message(result: *mut TuiResult) -> String {
    let result = unsafe { result.as_ref() }.expect("native call returned a null result");
    unsafe { result.error_message.required().unwrap() }
}
unsafe fn error_kind(result: *mut TuiResult) -> u32 {
    unsafe { result.as_ref() }
        .expect("native call returned a null result")
        .error_kind
}

#[test]
fn missing_sessions_retain_structured_errors_and_owned_messages() {
    for _ in 0..128 {
        let result = unsafe { tui_state(text(r#"{"name":"go-abi-missing-session"}"#)) };
        assert_eq!(unsafe { error_kind(result) }, 3);
        assert!(unsafe { message(result) }.contains("no active session"));
        unsafe { tui_result_free(result) };
    }
    unsafe { tui_result_free(std::ptr::null_mut()) };
}

#[test]
fn pointer_options_reject_null_and_preserve_validation() {
    let session = text(r#"{"name":"go-abi-pointer-options"}"#);
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
            tui_screenshot_ptr(session, std::ptr::null()),
            tui_screenshot_ptr(
                session,
                &TuiScreenshotOptions {
                    background: text("#000000"),
                    transparent: true,
                    ..Default::default()
                },
            ),
        ]
    };
    for result in results {
        assert_eq!(unsafe { error_kind(result) }, 2);
        unsafe { tui_result_free(result) };
    }
}
#[test]
fn panic_is_contained_and_next_call_succeeds() {
    let result = boundary(|| panic!("intentional boundary test"));
    assert_eq!(unsafe { error_kind(result) }, 5);
    assert!(unsafe { message(result) }.contains("panicked"));
    unsafe { tui_result_free(result) };
    let result = unsafe {
        tui_close(
            text(r#"{"name":"go-abi-missing-session"}"#),
            TuiOptionalBool::default(),
        )
    };
    assert_eq!(unsafe { error_kind(result) }, 0);
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
        assert_eq!(unsafe { error_kind(result) }, 2);
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
    assert_eq!(absent.recording.mode, AutomaticRecordingMode::Disabled);

    let explicit = unsafe {
        input::open(TuiOpenOptions {
            recording_mode: text("always"),
            ..Default::default()
        })
        .unwrap()
    };
    assert_eq!(explicit.recording.mode, AutomaticRecordingMode::Always);
}

#[test]
fn close_is_attempted_when_trace_finalization_fails() {
    let close_attempted = std::cell::Cell::new(false);
    let error = finish_trace_and_close(
        || Err(TuiTestError::internal("trace failed")),
        || {
            close_attempted.set(true);
            Err(TuiTestError::internal("close failed"))
        },
    )
    .unwrap_err();

    assert!(close_attempted.get());
    assert!(error.to_string().contains("trace failed"));

    let result = finish_trace_and_close(
        || Err(TuiTestError::new(ErrorKind::NoSession, "trace missing")),
        || Ok(OperationResult::Unit),
    );
    assert!(matches!(result, Ok(OperationResult::Unit)));

    let error = finish_trace_and_close(
        || Err(TuiTestError::new(ErrorKind::NoSession, "trace missing")),
        || Err(TuiTestError::internal("close failed")),
    )
    .unwrap_err();
    assert_eq!(error.message, "close failed");
}

#[test]
fn c_context_and_owned_context_share_validation() {
    for (field, value) in [
        ("artifact_mode", serde_json::json!("invalid")),
        ("trace_mode", serde_json::json!("invalid")),
        ("screen_history_limit", serde_json::json!(65535)),
        ("trace_directory", serde_json::json!("")),
    ] {
        let mut context = serde_json::json!({"name": "ffi-context", "trace_mode": "on"});
        context[field] = value;
        let owned = serde_json::from_value::<session::SessionInput>(context.clone())
            .unwrap()
            .validate()
            .unwrap_err();
        let encoded = context.to_string();
        let result = unsafe { tui_state(text(&encoded)) };
        assert_eq!(unsafe { error_kind(result) }, owned.kind.exit_code() as u32);
        assert_eq!(unsafe { message(result) }, owned.message);
        unsafe { tui_result_free(result) };
    }

    let context = session::SessionInput {
        name: "ffi-context".into(),
        screen_history_limit: Some(0),
        ..Default::default()
    };
    let (name, validated) = context.clone().validate().unwrap();
    assert_eq!(validated.retention.screen_history_limit, 0);
    let error = session::execute(name, validated, "state", Operation::State).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NoSession);
    assert_eq!(context.screen_history_limit, Some(0));

    let result = unsafe { tui_state(text(r#"{"name":"ffi-context","unknown":true}"#)) };
    assert_eq!(unsafe { error_kind(result) }, 2);
    assert!(unsafe { message(result) }.contains("unknown field"));
    unsafe { tui_result_free(result) };
}

#[test]
fn shared_close_preserves_missing_session_behavior() {
    for failed in [None, Some(false), Some(true)] {
        let (name, context) = session::SessionInput {
            name: "ffi-close-missing".into(),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert!(matches!(
            session::close(name, context, failed),
            Ok(OperationResult::Unit)
        ));
    }
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
fn locator_expressions_preserve_parent_selection() {
    let value = r#"{"nodes":[{"kind":"text","text":"parent","occurrence":{"nth":2}},{"kind":"style","style":{"bold":false},"direction":"after","within":0,"occurrence":"any"}],"root":1}"#;
    let query = TuiQuery { json: text(value) };
    let converted = unsafe { input::query(query).unwrap() };
    assert_eq!(converted.occurrence, MatchOccurrence::Any);
    assert_eq!(converted.direction, LocatorDirection::After);
    let LocatorSelector::Style(style) = converted.selector else {
        panic!("expected style selector");
    };
    assert_eq!(style.style.bold, Some(false));
    assert_eq!(
        converted.within.unwrap().occurrence,
        MatchOccurrence::Nth(2)
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
