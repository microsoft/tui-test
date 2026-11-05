use std::path::PathBuf;
use std::time::Duration;

use super::*;

fn parsed(text: &str) -> Tape {
    parse("test.tape", text).unwrap()
}

fn plan(text: &str) -> Plan {
    compile(&parsed(text)).unwrap()
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tape")
}

#[test]
fn example_compiles_without_side_effects() {
    let tape = parsed(include_str!("../../tests/fixtures/tape/parse.tape"));
    let before = tape.clone();
    let result = compile(&tape).unwrap();
    assert_eq!(tape, before);
    assert!(!result.initially_visible());
    assert!(matches!(
        result.actions().last().unwrap().value,
        Action::Hide
    ));
    assert_eq!(result.output().value, PathBuf::from("hello.gif"));
    assert_eq!(result.requirements()[0].value, "echo");
    assert_eq!(result.environment()[0].value.0, "TAPE_GREETING");
    serde_json::to_string(&result).unwrap();
}

#[test]
fn quotes_are_literal_and_newlines_are_whitespace() {
    let tape = parsed("Type 'one\\n' \"two\\t\"\n`three \"quotes\"` Enter");
    assert_eq!(
        tape.commands()[0].value,
        Command::Type {
            text: "one\\n two\\t three \"quotes\"".into(),
            delay: None,
        }
    );
    assert!(matches!(
        tape.commands()[1].value,
        Command::Key {
            key: Key::Enter,
            ..
        }
    ));
    let tape = parsed("Type \"before\" # comment\nType \"after\"");
    assert_eq!(tape.commands().len(), 2);
    for text in [
        "Type 'unterminated",
        "Type \"line\nbreak\"",
        "Type `missing",
        "Type \"a\\\"b\"",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
}

#[test]
fn locations_use_one_based_lines_and_unicode_columns() {
    let error = parse("name.tape", "# comment\r\nType 'é' Enter\nSleep bad").unwrap_err();
    assert_eq!(error.location.line, 3);
    assert_eq!(error.location.column, 7);
    assert!(error.to_string().starts_with("name.tape:3:7:"));
    let error = parse("name.tape", "Type 'é' !").unwrap_err();
    assert_eq!(error.location.column, 10);
}

#[test]
fn durations_are_exact_and_checked() {
    let tape = parsed(
        "Sleep 1 Sleep .25s Sleep 5ms Sleep 1.5m Sleep .000000001s Sleep 9223372036.854775807s",
    );
    let expected = [
        Duration::from_secs(1),
        Duration::from_millis(250),
        Duration::from_millis(5),
        Duration::from_secs(90),
        Duration::from_nanos(1),
        Duration::from_nanos(i64::MAX as u64),
    ];
    for (command, duration) in tape.commands().iter().zip(expected) {
        assert_eq!(command.value, Command::Sleep(duration));
    }
    for text in [
        "Sleep -1",
        "Sleep 1.2.3",
        "Sleep 9223372036.854775808s",
        "Sleep 999999999999999999999999999999999999999999",
        "Sleep NaN",
        "Sleep 1h",
        "Sleep '1s'",
        "Sleep @1",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
    assert_eq!(
        parsed("Sleep 0").commands()[0].value,
        Command::Sleep(Duration::ZERO)
    );
    assert_eq!(
        parsed("Sleep .0000000009").commands()[0].value,
        Command::Sleep(Duration::ZERO)
    );
    for (fraction, nanos) in [
        ("00000000001666666666", 0),
        ("00000000001666666667", 1),
        ("9999999999999999999999999999999999999999", 59_999_999_999),
    ] {
        assert_eq!(
            parsed(&format!("Sleep .{fraction}m")).commands()[0].value,
            Command::Sleep(Duration::from_nanos(nanos)),
        );
    }
    assert_eq!(
        plan("Output fraction.gif Sleep .00000000001666666667m").actions()[0].value,
        Action::Sleep(Duration::from_nanos(1))
    );
}

#[test]
fn typing_speed_retains_upstream_unit_quirks() {
    for (text, duration) in [
        ("Set TypingSpeed 2", Duration::from_secs(2)),
        ("Set TypingSpeed 3ms", Duration::from_millis(3)),
        ("Set TypingSpeed .1s", Duration::from_millis(100)),
        ("Set TypingSpeed '1m'", Duration::from_millis(1)),
    ] {
        assert_eq!(
            parsed(text).commands()[0].value,
            Command::Set(Setting::TypingSpeed(duration))
        );
    }
    for text in [
        "Set TypingSpeed 1m",
        "Set TypingSpeed '10ms'",
        "Set TypingSpeed -1",
        "Set TypingSpeed nope",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
}

#[test]
fn repeated_keys_scroll_and_modifiers_have_typed_shape() {
    let tape = parsed("Enter@.5m 3 ScrollUp@1ms 2 ScrollDown 0 Ctrl+Alt+Shift+x+c Ctrl+Left Ctrl+\"1\" Alt+Tab Shift+Enter Ctrl+[ Ctrl+\\");
    assert_eq!(
        tape.commands()[0].value,
        Command::Key {
            key: Key::Enter,
            delay: Some(Duration::from_secs(30)),
            count: 3,
        }
    );
    assert_eq!(
        tape.commands()[1].value,
        Command::Scroll {
            direction: ScrollDirection::Up,
            delay: Some(Duration::from_millis(1)),
            count: 2,
        }
    );
    assert_eq!(
        tape.commands()[2].value,
        Command::Scroll {
            direction: ScrollDirection::Down,
            delay: None,
            count: 0,
        }
    );
    assert_eq!(
        tape.commands()[3].value,
        Command::ModifiedKeys {
            modifiers: vec![Modifier::Ctrl, Modifier::Alt, Modifier::Shift],
            keys: vec![ModifiedKey::Character('x'), ModifiedKey::Character('c')],
        }
    );
    for text in [
        "Enter 1.5",
        "Enter -1",
        "Enter 18446744073709551616",
        "Ctrl",
        "Ctrl+",
        "Ctrl+Alt",
        "Ctrl+x+Shift+y",
        "Ctrl+unknown",
        "Ctrl+1",
        "Alt+foo",
        "Shift+Ctrl+x",
        "Ctrl+Tab",
        "Alt+x+y",
        "Home",
        "End",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
}

#[test]
fn regex_delimiter_escapes_do_not_unescape_the_pattern() {
    let tape = parsed(r"Wait+Screen@2s /foo\/bar/ Wait /foo\\/ Set WaitPattern /x\\\/y/");
    assert_eq!(
        tape.commands()[0].value,
        Command::Wait {
            scope: WaitScope::Screen,
            timeout: Some(Duration::from_secs(2)),
            pattern: Some(r"foo\/bar".into()),
        }
    );
    assert!(
        matches!(&tape.commands()[1].value, Command::Wait { pattern: Some(pattern), .. } if pattern == r"foo\\")
    );
    for text in [
        "Wait+Other",
        "Wait@0",
        "Wait /[/",
        "Wait /missing",
        "Set WaitPattern /(/",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
}

#[test]
fn all_settings_have_typed_shape() {
    let tape = parsed(
        r##"
Set Shell bash
Set FontFamily "JetBrains Mono"
Set FontSize 22
Set Framerate 50
Set PlaybackSpeed 1
Set TypingSpeed 50ms
Set Rows 24
Set Columns 80
Set Padding 60
Set CursorBlink true
Set Height 600
Set Width 800
Set LetterSpacing -1
Set LineHeight 1.2
Set Theme {"background":"#000000"}
Set LoopOffset 20%
Set MarginFill "#123abc"
Set Margin 2
Set WindowBar "Rings"
Set WindowBarSize 2
Set BorderRadius 3
Set WaitTimeout 2s
Set WaitPattern /ready/
"##,
    );
    assert_eq!(tape.commands().len(), 23);
    let names = tape
        .commands()
        .iter()
        .map(|command| {
            let Command::Set(setting) = &command.value else {
                panic!("expected setting")
            };
            setting.name()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(), 23);
}

#[test]
fn invalid_values_never_fall_back_to_defaults() {
    for text in [
        "Set Unknown 1",
        "Set Rows 0",
        "Set Columns 65536",
        "Set Rows 2.5",
        "Set Framerate 0",
        "Set Framerate 256",
        "Set Framerate -1",
        "Set FontSize 0",
        "Set FontSize '-inf'",
        "Set FontSize '1e100'",
        "Set FontSize '1e-100'",
        "Set PlaybackSpeed 'NaN'",
        "Set PlaybackSpeed 0",
        "Set Padding -1",
        "Set Padding 4294967296",
        "Set CursorBlink 'true'",
        "Set CursorBlink yes",
        "Set LoopOffset 101",
        "Set WindowBar Unknown",
        "Set MarginFill '#123'",
        "Set WaitTimeout 0",
        "Set Theme {bad}",
        "Env '' value",
        "Env 'A=B' value",
        "Env A",
        "Output ''",
        "Output no-extension",
        "Source wrong.gif",
        "Screenshot wrong.gif",
        "Require ''",
        "Set FontFamily ''",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
    for text in [
        "Env 'A\0B' value",
        "Env A 'bad\0value'",
        "Output 'bad\0.gif'",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text:?}");
    }
}

#[test]
fn defaults_are_tape_defaults_not_session_defaults() {
    let result = plan("Output test.gif Sleep 1");
    let settings = result.settings();
    assert_eq!(settings.rows, 24);
    assert_eq!(settings.columns, 80);
    assert_eq!(settings.font_size, 22.0);
    assert_eq!(settings.font_family, "JetBrains Mono");
    assert_eq!(settings.padding, 60);
    assert_eq!(settings.framerate, 50);
    assert_eq!(settings.typing_speed, Duration::from_millis(50));
    assert_eq!(settings.playback_speed, 1.0);
    assert!(settings.cursor_blink);
    assert_eq!(
        settings.shell.program(),
        if cfg!(windows) { "powershell" } else { "bash" }
    );
    assert_eq!(result.warnings().len(), 2);
    assert_eq!(result.warnings()[0].kind, WarningKind::NativeGrid);
    assert_eq!(
        result.warnings()[1].kind,
        WarningKind::CursorBlinkPermission
    );
}

#[test]
fn compile_resolves_delays_and_hoists_shell_and_env() {
    let shell = if cfg!(windows) { "pwsh" } else { "fish" };
    let result = plan(&format!(
        "Output a.gif Set TypingSpeed 10ms Require echo Type 'one' Env A one Set Shell {shell} Set TypingSpeed 20ms Enter ScrollUp 2 Type@30ms 'two' Env A two Set Rows 90"
    ));
    assert_eq!(result.settings().shell.program(), shell);
    assert_eq!(result.settings().rows, 24);
    assert_eq!(result.settings().typing_speed, Duration::from_millis(10));
    assert_eq!(
        result
            .environment()
            .iter()
            .map(|item| item.value.1.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert!(
        matches!(result.actions()[0].value, Action::Type { delay, .. } if delay == Duration::from_millis(10))
    );
    assert!(
        matches!(result.actions()[1].value, Action::Key { delay, .. } if delay == Duration::from_millis(20))
    );
    assert!(
        matches!(result.actions()[2].value, Action::Scroll { delay, .. } if delay == Duration::from_millis(20))
    );
    assert!(
        matches!(result.actions()[3].value, Action::Type { delay, .. } if delay == Duration::from_millis(30))
    );
    assert!(result
        .warnings()
        .iter()
        .any(|w| w.kind == WarningKind::IgnoredLateSetting));
    let result = plan("Output a.gif Env A b Set Rows 99 Sleep 1");
    assert_eq!(
        result.settings().rows,
        24,
        "Env ends the upstream initial section"
    );
}

#[test]
fn explicit_settings_apply_without_mutating_the_tape() {
    let result = plan("Output a.gif Set Rows 35 Set Columns 100 Set FontFamily mono Set FontSize 14.5 Set Framerate 255 Set PlaybackSpeed 2 Set Padding 0 Set CursorBlink false Sleep 1");
    assert_eq!(result.settings().rows, 35);
    assert_eq!(result.settings().columns, 100);
    assert_eq!(result.settings().font_family, "mono");
    assert_eq!(result.settings().font_size, 14.5);
    assert_eq!(result.settings().framerate, 255);
    assert_eq!(result.settings().playback_speed, 2.0);
    assert_eq!(result.settings().padding, 0);
    assert!(!result.settings().cursor_blink);
    assert_eq!(result.warnings().len(), 1);
}

#[test]
fn shell_names_are_validated_without_executable_lookup() {
    let shells: &[&str] = if cfg!(windows) {
        &["powershell", "pwsh", "cmd"]
    } else {
        &["bash", "zsh", "fish"]
    };
    for shell in shells {
        plan(&format!("Output a.gif Set Shell {shell} Sleep 1"));
    }
    for shell in ["sh", "unknown", "/bin/bash", "bash -l", "BASH"] {
        let error = compile(&parsed(&format!(
            "Output a.gif Set Shell '{shell}' Sleep 1"
        )))
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(error.message.contains("Shell"));
    }
    let other = if cfg!(windows) { "bash" } else { "powershell" };
    assert!(compile(&parsed(&format!("Output a.gif Set Shell {other} Sleep 1"))).is_err());
}

#[test]
fn missing_multiple_late_and_non_gif_outputs_fail() {
    for text in [
        "Sleep 1",
        "Output a.mp4 Sleep 1",
        "Output frames/ Sleep 1",
        "Output a.gif Output b.gif Sleep 1",
        "Sleep 1 Output a.gif",
        "Env A b Output a.gif Sleep 1",
        "Output a.GIF Sleep 1",
        "Output a.gif Sleep 1 Require echo",
    ] {
        assert!(compile(&parsed(text)).is_err(), "{text}");
    }
}

#[test]
fn output_requires_a_lexical_filename() {
    let assert_directory_rejected = |path| {
        let error = compile(&parsed(&format!("Output '{path}' Sleep 1"))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(error.message.contains("file"));
        assert_eq!(error.location.column, 1);
    };
    for path in ["frames.gif/", "frames.gif/.", "frames.gif/./"] {
        assert_directory_rejected(path);
    }
    #[cfg(windows)]
    for path in [r"frames.gif\", r"frames.gif\.", r"frames.gif\.\"] {
        assert_directory_rejected(path);
    }
    for path in ["frames/demo.gif", "främés/演示.gif", r"frames\demo.gif"] {
        let result = plan(&format!("Output '{path}' Sleep 1"));
        assert_eq!(result.output().value, PathBuf::from(path));
    }
}

#[test]
fn visibility_is_validated_without_an_eof_resume() {
    for text in [
        "Output a.gif",
        "Output a.gif Hide Sleep 1",
        "Output a.gif Hide Type 'hidden' Hide",
        "Output a.gif Show Show",
        "Output a.gif Type '' Enter 0 Sleep 0",
    ] {
        assert!(compile(&parsed(text)).is_err(), "{text}");
    }
    let result = plan("Output a.gif Hide Hide Type 'hidden' Show Show Sleep 1 Hide Hide");
    assert!(!result.initially_visible());
    assert_eq!(result.actions().len(), 4);
    assert!(matches!(result.actions()[0].value, Action::Type { .. }));
    assert!(matches!(result.actions()[1].value, Action::Show));
    assert!(matches!(result.actions()[3].value, Action::Hide));
    let result = plan("Output a.gif Sleep 1 Hide");
    assert!(result.initially_visible());
    assert!(matches!(
        result.actions().last().unwrap().value,
        Action::Hide
    ));
    plan("Output a.gif Hide Sleep 1 Show");
}

#[test]
fn known_unsupported_commands_and_settings_fail_even_when_late_or_hidden() {
    for command in [
        "Wait /ready/",
        "Screenshot image.png",
        "Copy 'text'",
        "Paste",
        "Set Height 600",
        "Set Width 800",
        "Set LetterSpacing 1",
        "Set LineHeight 1",
        "Set Theme dark",
        "Set LoopOffset 20%",
        "Set MarginFill red",
        "Set Margin 1",
        "Set WindowBar Rings",
        "Set WindowBarSize 2",
        "Set BorderRadius 3",
        "Set WaitTimeout 1s",
        "Set WaitPattern /ready/",
    ] {
        for prefix in ["Output a.gif ", "Output a.gif Sleep 1 Hide "] {
            let error = compile(&parsed(&format!("{prefix}{command}"))).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Unsupported, "{command}");
            assert!(error.message.contains("not supported"), "{command}");
        }
    }
}

#[test]
fn malformed_trailing_commands_fail_before_compilation() {
    for text in [
        "Output a.gif Type 'valid' Sleep",
        "Output a.gif Sleep 1 Unknown",
        "Output a.gif Sleep 1 Set",
        "Output a.gif Sleep 1 Type",
        "Output a.gif Sleep 1 Enter@",
        "Output a.gif Sleep 1 Env A",
    ] {
        assert!(parse("test.tape", text).is_err(), "{text}");
    }
}

#[test]
fn source_uses_invocation_cwd_discards_outputs_and_retains_provenance() {
    let tape = load("source/root.tape", fixtures()).unwrap();
    assert_eq!(tape.commands().len(), 4);
    let result = compile(&tape).unwrap();
    assert_eq!(result.output().value, PathBuf::from("root.gif"));
    assert_eq!(result.settings().rows, 30);
    assert!(
        matches!(&result.actions()[0].value, Action::Type { text, .. } if text == "from invocation cwd")
    );
    let location = &result.actions()[0].location;
    assert_eq!(location.path, fixtures().join("imported.tape"));
    let parent = location.included_from.as_ref().unwrap();
    assert_eq!(parent.path, fixtures().join("source/root.tape"));
    assert_eq!(parent.line, 2);
    assert!(location.to_string().contains("included from"));
}

#[test]
fn source_errors_are_located_and_do_not_recurse() {
    for (file, message) in [
        ("nested.tape", "nested Source"),
        ("empty.tape", "contains no commands"),
        ("absent.tape", "cannot read Source"),
    ] {
        let root = parsed(&format!("Output a.gif Source {file} Sleep 1"));
        let error = resolve_sources(&root, fixtures()).unwrap_err();
        assert!(error.message.contains(message), "{error}");
        if file == "nested.tape" {
            assert_eq!(error.location.path, fixtures().join(file));
            assert!(error.location.included_from.is_some());
        } else {
            assert_eq!(error.location.path, PathBuf::from("test.tape"));
        }
    }
    let error =
        resolve_sources(&parsed("Output a.gif Source malformed.tape"), fixtures()).unwrap_err();
    assert_eq!(error.location.path, fixtures().join("malformed.tape"));
    assert!(error.location.included_from.is_some());
    assert!(compile(&parsed("Output a.gif Source absent.tape Sleep 1"))
        .unwrap_err()
        .message
        .contains("unresolved Source"));
}

#[test]
fn source_expansion_precedes_phase_analysis_and_keeps_diagnostics() {
    let tape =
        resolve_sources(&parsed("Output a.gif Source late-setting.tape"), fixtures()).unwrap();
    let result = compile(&tape).unwrap();
    assert_eq!(result.settings().rows, 24);
    let warning = result
        .warnings()
        .iter()
        .find(|w| w.kind == WarningKind::IgnoredLateSetting)
        .unwrap();
    assert_eq!(warning.location.path, fixtures().join("late-setting.tape"));
    assert_eq!(warning.location.line, 2);
    assert!(warning.location.included_from.is_some());
    let tape =
        resolve_sources(&parsed("Output a.gif Source unsupported.tape"), fixtures()).unwrap();
    let error = compile(&tape).unwrap_err();
    assert_eq!(error.location.path, fixtures().join("unsupported.tape"));
    assert_eq!(error.location.line, 2);
    assert!(error.location.included_from.is_some());
    let tape = resolve_sources(
        &parsed("Output a.gif Source imported.tape Require echo"),
        fixtures(),
    )
    .unwrap();
    assert!(compile(&tape).unwrap_err().message.contains("initial"));
}
