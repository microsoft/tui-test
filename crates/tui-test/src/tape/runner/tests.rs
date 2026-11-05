use super::*;
use crate::tape::{compile, parse};
use std::sync::atomic::{AtomicU64, Ordering};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tape-runner-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn plan(body: &str) -> Plan {
    compile(&parse("runner.tape", &format!(
        "Output result.gif\nSet Rows 3\nSet Columns 16\nSet Padding 0\nSet FontSize 12\nSet CursorBlink false\n{body}"
    )).unwrap()).unwrap()
}

#[test]
fn modified_plus_preserves_modifiers_and_every_key() {
    let tokens = modified_tokens(
        &[Modifier::Ctrl, Modifier::Alt],
        &[
            ModifiedKey::Character('+'),
            ModifiedKey::Character('é'),
            ModifiedKey::Named(Key::Enter),
        ],
    );
    assert_eq!(tokens, ["Ctrl+Alt+Shift+=", "Ctrl+Alt+é", "Ctrl+Alt+Enter"]);
    for token in tokens {
        crate::input::keys::token_to_presses(&token, KeyAction::Press).unwrap();
        crate::input::keys::token_to_seq(&token).unwrap();
    }
}

#[test]
fn scroll_sign_matches_core_viewport_contract() {
    assert_eq!(
        scroll_target(ScrollDirection::Up),
        crate::ScrollTarget::Lines(1)
    );
    assert_eq!(
        scroll_target(ScrollDirection::Down),
        crate::ScrollTarget::Lines(-1)
    );
}

#[test]
fn shell_arguments_are_fixed_and_controls_override_env() {
    for shell in [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::Powershell,
        Shell::Pwsh,
        Shell::Cmd,
    ] {
        let mut env = vec![
            ("PS1".into(), "uncontrolled".into()),
            ("PATH".into(), "keep".into()),
        ];
        let arguments = clean_shell(shell, &mut env);
        assert_eq!(env_value(&env, "PS1"), Some("> "));
        assert_eq!(env_value(&env, "PATH"), Some("keep"));
        assert!(!arguments.iter().any(|arg| arg.contains("uncontrolled")));
        match shell {
            Shell::Bash => assert_eq!(arguments, ["--noprofile", "--norc", "-i"]),
            Shell::Zsh => assert_eq!(arguments, ["-d", "-f", "-i"]),
            Shell::Fish => assert!(
                arguments.contains(&"--no-config".into())
                    && arguments.contains(&"--private".into())
            ),
            Shell::Powershell | Shell::Pwsh => assert!(
                arguments.contains(&"-NoProfile".into())
                    && arguments.last().unwrap().contains("SaveNothing")
            ),
            Shell::Cmd => assert_eq!(arguments, ["/D", "/Q", "/K", "prompt $G$S"]),
        }
    }
}

#[test]
fn environment_keys_follow_host_rules() {
    let mut env = Vec::new();
    set_env(&mut env, "PATH", "first");
    set_env(&mut env, "Path", "second");
    assert_eq!(
        env_value(&env, "PATH"),
        Some(if cfg!(windows) { "second" } else { "first" })
    );
    set_env(&mut env, "PATH", "last");
    assert_eq!(env_value(&env, "PATH"), Some("last"));
}

#[cfg(windows)]
#[test]
fn child_cwd_preserves_drive_and_unc_paths_without_verbatim_prefixes() {
    for (canonical, expected) in [
        (r"\\?\C:\work\tape", r"C:\work\tape"),
        (r"\\?\UNC\server\share\tape", r"\\server\share\tape"),
        (r"C:\work\tape", r"C:\work\tape"),
        (r"\\server\share\tape", r"\\server\share\tape"),
    ] {
        assert_eq!(child_cwd(Path::new(canonical)), expected);
    }
}

#[test]
fn cleanup_errors_do_not_mask_primary_context() {
    let plan = plan("Sleep .1");
    let primary =
        RunnerError::execution(&plan.output().location, TuiTestError::internal("primary"));
    let mut result = Err(primary);
    merge_cleanup(
        &mut result,
        Err(RunnerError::execution(
            plan.shell_location(),
            TuiTestError::internal("close"),
        )),
    );
    let error = result.unwrap_err();
    assert_eq!(error.message, "primary");
    assert_eq!(error.cleanup_errors.len(), 1);
    assert!(error.to_string().contains("close"));
}

#[cfg(unix)]
#[test]
fn input_fixture() {
    use std::io::{Read, Write};
    let Ok(mode) = std::env::var("TUI_TEST_TAPE_FIXTURE") else {
        return;
    };
    // This fixture owns stdin and switches its PTY to raw mode for byte assertions.
    unsafe {
        let mut termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(0, &mut termios), 0);
        libc::cfmakeraw(&mut termios);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &termios), 0);
    }
    let cwd = std::env::current_dir().unwrap();
    std::fs::write(cwd.join("child.pid"), std::process::id().to_string()).unwrap();
    let mut output = std::io::stdout().lock();
    if mode == "fail-export" {
        // Preflight succeeds; the child then makes the destination invalid.
        std::fs::create_dir(cwd.join("result.gif")).unwrap();
    }
    if mode == "input" {
        write!(output, "\x1b[?1h").unwrap();
    }
    if mode == "scroll" {
        for index in 0..30 {
            write!(output, "LINE{index:02}\r\n").unwrap();
        }
    }
    write!(output, "> ").unwrap();
    output.flush().unwrap();
    let mut bytes = Vec::new();
    for byte in std::io::stdin().lock().bytes() {
        let byte = byte.unwrap();
        if mode == "fail-export" {
            write!(output, "DESTINATION_CHANGED\r\n").unwrap();
        } else {
            bytes.push(byte);
            std::fs::write(cwd.join("received"), &bytes).unwrap();
            write!(output, "RX{byte:02x}\r\n").unwrap();
        }
        output.flush().unwrap();
    }
}

#[cfg(not(feature = "recording-raster"))]
#[test]
fn check_and_run_require_raster_without_side_effects() {
    let directory = Directory::new();
    let plan = plan("Sleep .1");
    for error in [
        check(&plan, &directory.0).unwrap_err(),
        run(&plan, &directory.0).unwrap_err(),
    ] {
        assert_eq!(error.kind, ErrorKind::Usage);
        assert_eq!(error.phase, RunnerPhase::Preflight);
        assert!(error.message.contains("recording-raster"));
    }
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[cfg(feature = "recording-raster")]
mod raster {
    use super::*;
    use std::time::Instant;

    #[test]
    fn overlapping_runs_reserve_output_before_capture_starts() {
        let directory = Directory::new();
        let plan = plan("Sleep .02");
        let mut attempted = false;
        let summary = run_observed(&plan, &directory.0, |_, action, entering| {
            if !attempted && entering && matches!(action, Action::Show) {
                attempted = true;
                let error = run(&plan, &directory.0).unwrap_err();
                assert!(error.message.contains("owned"), "{error}");
            }
        })
        .unwrap();
        assert!(attempted);
        decode(&summary.output);
        assert!(!directory.0.join("result.gif.tui-test.lock").exists());
        run(&plan, &directory.0).unwrap();
    }

    #[test]
    fn preflight_rejects_unrepresentable_prompt_and_excessive_gif_expansion() {
        let directory = Directory::new();
        for (body, expected) in [
            (
                "Set Columns 1 Set Rows 1 Sleep .1",
                "at least three columns",
            ),
            (
                "Set Columns 2 Set Rows 1 Sleep .1",
                "at least three columns",
            ),
            (
                "Set PlaybackSpeed 0.0000000000000001 Sleep .1",
                "GIF frame limit",
            ),
        ] {
            let plan = plan(body);
            for error in [
                check(&plan, &directory.0).unwrap_err(),
                run(&plan, &directory.0).unwrap_err(),
            ] {
                assert_eq!(error.phase, RunnerPhase::Preflight);
                assert!(error.message.contains(expected), "{error}");
            }
            assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
        }
        run(&plan("Set Columns 3 Set Rows 1 Sleep .02"), &directory.0).unwrap();
    }

    #[test]
    fn actual_elapsed_export_limit_releases_owned_output() {
        let directory = Directory::new();
        let plan = plan("Set PlaybackSpeed .000000000001 Type@0 'X'");
        check(&plan, &directory.0).unwrap();
        let error = run_observed(&plan, &directory.0, |_, action, entering| {
            if entering && matches!(action, Action::Type { .. }) {
                // This models scheduler delay that the static plan cannot predict.
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .unwrap_err();
        assert_eq!(error.phase, RunnerPhase::Execution);
        assert!(error.message.contains("GIF frame limit"), "{error}");
        assert!(error.message.contains("captured cast retained"), "{error}");
        assert!(directory.0.join("result.gif.tui-test.cast").exists());
        assert!(!directory.0.join("result.gif.tui-test.lock").exists());
    }

    #[test]
    fn preflight_rejects_lock_only_output_without_modifying_it() {
        let directory = Directory::new();
        let plan = plan("Hide Sleep .02 Show Sleep .02");
        let lock = directory.0.join("result.gif.tui-test.lock");
        std::fs::create_dir(&lock).unwrap();
        std::fs::write(lock.join("owner"), "preserve").unwrap();
        for error in [
            check(&plan, &directory.0).unwrap_err(),
            run(&plan, &directory.0).unwrap_err(),
        ] {
            assert_eq!(error.phase, RunnerPhase::Preflight);
            assert_eq!(error.kind, ErrorKind::Usage);
            assert_eq!(error.location, plan.output().location);
            assert!(error.message.contains("already owned"), "{error}");
        }
        assert_eq!(
            std::fs::read_to_string(lock.join("owner")).unwrap(),
            "preserve"
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(lock).unwrap().count(), 1);
    }

    #[test]
    fn ownership_is_exclusive_and_does_not_remove_existing_files() {
        let directory = Directory::new();
        let output = directory.0.join("result.gif");
        let location = plan("Sleep .02").output().location.clone();
        let lock = reserve_output(&output, &location).unwrap();
        assert!(reserve_output(&output, &location).is_err());
        assert!(lock.is_dir());
        std::fs::remove_dir(lock).unwrap();
        let cast = directory.0.join("result.gif.tui-test.cast");
        std::fs::write(&cast, "recovery").unwrap();
        let lock = reserve_output(&output, &location).unwrap();
        assert!(check_output_files(&output, &location).is_err());
        std::fs::remove_dir(lock).unwrap();
        assert_eq!(std::fs::read_to_string(cast).unwrap(), "recovery");
    }

    fn decode(path: &Path) -> ((u16, u16), u64, Vec<Vec<u8>>) {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options
            .read_info(std::fs::File::open(path).unwrap())
            .unwrap();
        let dimensions = (decoder.width(), decoder.height());
        let mut centiseconds = 0;
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            centiseconds += u64::from(frame.delay);
            frames.push(frame.buffer.to_vec());
        }
        assert!(!frames.is_empty());
        (dimensions, centiseconds, frames)
    }

    fn wait(session: &Session, text: &str) {
        session
            .execute(Operation::WaitLocator {
                query: crate::LocatorQuery::text(text),
                not: false,
                timeout_ms: Some(5_000),
            })
            .unwrap();
    }

    fn launch(plan: &Plan, directory: &Path) -> Session {
        let prepared = prepare(plan, directory).unwrap();
        let session = Session::new("tape-runner-unit");
        session.run(prepared.options).unwrap();
        wait(&session, "> ");
        session
    }

    fn timed_run(plan: &Plan, directory: &Path) -> (Summary, (Duration, Duration)) {
        let mut visible: Option<(Instant, Instant)> = None;
        let mut before = Instant::now();
        let mut minimum = Duration::ZERO;
        let mut maximum = Duration::ZERO;
        let summary = run_observed(plan, directory, |session, action, entering| {
            if entering {
                before = Instant::now();
                return;
            }
            let after = Instant::now();
            match action {
                Action::Show if visible.is_none() => visible = Some((before, after)),
                Action::Hide => {
                    if let Some((start_before, start_after)) = visible.take() {
                        minimum += before.duration_since(start_after);
                        maximum += after.duration_since(start_before);
                    }
                }
                // Confirm hidden edits before the next visibility boundary.
                Action::Type { text, .. } if visible.is_none() => wait(session, text),
                Action::Key {
                    key: Key::Backspace,
                    ..
                } if visible.is_none() => {
                    session
                        .execute(Operation::WaitLocator {
                            query: crate::LocatorQuery::text("Z"),
                            not: true,
                            timeout_ms: Some(5_000),
                        })
                        .unwrap();
                }
                _ => {}
            }
        })
        .unwrap();
        assert!(visible.is_none());
        let speed = plan.settings().playback_speed;
        (summary, (minimum.div_f64(speed), maximum.div_f64(speed)))
    }

    fn assert_timing(duration: u64, bounds: (Duration, Duration), requested: Duration) {
        let recorded = Duration::from_millis(duration * 10);
        let quantum = Duration::from_millis(10);
        let minimum = bounds.0.max(requested);
        assert!(
            recorded + quantum >= minimum && recorded <= bounds.1 + quantum,
            "GIF duration {recorded:?}, measured visible interval {minimum:?}..={:?}",
            bounds.1
        );
    }

    #[cfg(unix)]
    fn fixture_plan(directory: &Path, mode: &str, actions: &str) -> Plan {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(directory.join("bin")).unwrap();
        let binary = std::env::current_exe().unwrap();
        let quoted = binary.to_str().unwrap().replace('\'', "'\\''");
        let script = directory.join("bin/bash");
        std::fs::write(&script, format!(
            "#!/bin/sh\nexec '{quoted}' --exact tape::runner::tests::input_fixture --nocapture --test-threads=1\n"
        )).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        plan(&format!(
            "Env PATH 'bin'\nEnv TUI_TEST_TAPE_FIXTURE '{mode}'\n{actions}"
        ))
    }

    #[test]
    fn check_is_read_only_and_returns_actual_geometry() {
        let directory = Directory::new();
        let plan = plan("Sleep .1");
        let original = plan.clone();
        let before_env = std::env::vars_os().collect::<Vec<_>>();
        let before_cwd = std::env::current_dir().unwrap();
        let summary = check(&plan, &directory.0).unwrap();
        assert_eq!(summary.output, directory.0.join("result.gif"));
        assert_eq!(summary.geometry.columns, 16);
        assert_eq!(summary.geometry.rows, 3);
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
        assert_eq!(plan, original);
        assert_eq!(std::env::vars_os().collect::<Vec<_>>(), before_env);
        assert_eq!(std::env::current_dir().unwrap(), before_cwd);
        assert!(serde_json::to_string(&summary)
            .unwrap()
            .contains("geometry"));
    }

    #[test]
    fn child_environment_and_invocation_directory_reach_a_real_command() {
        let directory = Directory::new();
        let environment = std::env::vars_os().collect::<Vec<_>>();
        let cwd = std::env::current_dir().unwrap();
        std::fs::write(
            directory.0.join("child.cjs"),
            "require('node:fs').writeFileSync('child.json', JSON.stringify({value: process.env.TAPE_RUNNER_VALUE, session: process.env.TUI_TEST_SESSION, cwd: process.cwd()})); console.log('CHILD_DONE');",
        )
        .unwrap();
        let plan =
            plan("Require node\nEnv TAPE_RUNNER_VALUE child\nType@0 'node child.cjs'\nEnter@0");
        let session = launch(&plan, &directory.0);
        // Tape EOF closes the shell without a command-completion wait. Test the
        // child environment through the same executor, then await its result.
        for action in plan.actions() {
            perform(&session, &action.location, &action.value).unwrap();
        }
        let completed = session.execute(Operation::WaitLocator {
            query: crate::LocatorQuery::text("CHILD_DONE"),
            not: false,
            timeout_ms: Some(5_000),
        });
        let failure_screen = completed.as_ref().err().map(|_| {
            session.execute(Operation::Screenshot {
                full: true,
                path: None,
                zoom: None,
                background: None,
            })
        });
        session.close().unwrap();
        assert!(completed.is_ok(), "{completed:?}\n{failure_screen:?}");
        let child: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.0.join("child.json")).unwrap())
                .unwrap();
        assert_eq!(child["value"], "child");
        assert_eq!(
            child["session"].as_str(),
            std::env::var("TUI_TEST_SESSION").ok().as_deref()
        );
        assert_eq!(
            Path::new(child["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            directory.0
        );
        assert_eq!(std::env::vars_os().collect::<Vec<_>>(), environment);
        assert_eq!(std::env::current_dir().unwrap(), cwd);
    }

    #[test]
    fn invalid_options_and_missing_parent_preserve_existing_output() {
        let directory = Directory::new();
        #[cfg(unix)]
        let environment = {
            fixture_plan(&directory.0, "input", "Sleep .1");
            "Env PATH 'bin'\nEnv TUI_TEST_TAPE_FIXTURE 'input'\n"
        };
        #[cfg(not(unix))]
        let environment = "";
        let target = directory.0.join("result.gif");
        std::fs::write(&target, b"existing artifact").unwrap();
        for (setting, message) in [
            ("Set Padding 4294967295", "padding"),
            ("Set FontSize 2000", "font_size"),
            ("Set FontSize 0.00001", "font_size"),
            ("Set FontFamily ' '", "font.family"),
            (
                "Set FontFamily 'tui-test-font-that-does-not-exist'",
                "FontFamily",
            ),
            (
                "Set PlaybackSpeed 0.0000000000000000000000001",
                "PlaybackSpeed",
            ),
            ("Set Columns 32767", "GIF"),
        ] {
            let plan = plan(&format!(
                "{setting}\n{environment}Type 'touch marker'\nEnter\nSleep .1"
            ));
            let error = run(&plan, &directory.0).unwrap_err();
            assert_eq!(error.phase, RunnerPhase::Preflight);
            assert!(error.message.contains(message), "{error}");
            assert_eq!(error.location.path, PathBuf::from("runner.tape"));
            if message != "GIF" {
                assert_eq!(error.location.line, 7);
            }
            assert_eq!(std::fs::read(&target).unwrap(), b"existing artifact");
            assert!(!directory.0.join("marker").exists());
            assert!(!directory.0.join("child.pid").exists());
        }
        let missing =
            compile(&parse("missing.tape", "Output absent/result.gif Sleep .1").unwrap()).unwrap();
        assert!(check(&missing, &directory.0)
            .unwrap_err()
            .message
            .contains("must already exist"));
        assert!(!directory.0.join("absent").exists());
        std::fs::write(directory.0.join("result.gif.tui-test.cast"), b"recovery").unwrap();
        assert!(check(&plan("Sleep .1"), &directory.0)
            .unwrap_err()
            .message
            .contains("work file"));
        assert_eq!(
            std::fs::read(directory.0.join("result.gif.tui-test.cast")).unwrap(),
            b"recovery"
        );
    }

    #[test]
    fn late_invalid_timeline_fails_before_any_child_or_output() {
        let directory = Directory::new();
        let actions = "Type 'touch marker'\nEnter\nSleep .1\nSet TypingSpeed 9223372036s\nEnter 18446744073709551615";
        #[cfg(unix)]
        let plan = fixture_plan(&directory.0, "input", actions);
        #[cfg(not(unix))]
        let plan = plan(actions);
        let error = run(&plan, &directory.0).unwrap_err();
        assert_eq!(error.phase, RunnerPhase::Preflight);
        assert_eq!(error.location.line, if cfg!(unix) { 13 } else { 11 });
        assert!(error.message.contains("timeline"));
        assert!(!directory.0.join("child.pid").exists());
        assert!(!directory.0.join("marker").exists());
        assert!(!directory.0.join("result.gif").exists());
        assert!(!directory.0.join("result.gif.tui-test.cast").exists());
    }

    #[test]
    fn preflight_setting_errors_keep_source_import_context() {
        let directory = Directory::new();
        std::fs::write(directory.0.join("settings.tape"), "Set FontSize 2000").unwrap();
        let tape = parse(
            "parent.tape",
            "Output result.gif\nSource settings.tape\nSleep .1",
        )
        .unwrap();
        let plan = compile(&crate::tape::resolve_sources(&tape, &directory.0).unwrap()).unwrap();
        let error = check(&plan, &directory.0).unwrap_err();
        assert_eq!(error.location.path, directory.0.join("settings.tape"));
        assert_eq!(error.location.line, 1);
        assert_eq!(error.location.included_from.as_ref().unwrap().line, 2);
        assert!(error.to_string().contains("included from parent.tape:2:1"));
        assert!(!directory.0.join("result.gif").exists());
    }

    #[test]
    fn quiet_sleep_and_speed_decode_to_expected_gif_duration() {
        let directory = Directory::new();
        for speed in [1.0, 2.0] {
            let plan = plan(&format!("Set PlaybackSpeed {speed}\nSleep .4\nHide"));
            let (summary, bounds) = timed_run(&plan, &directory.0);
            let (dimensions, duration, frames) = decode(&summary.output);
            assert_eq!(
                dimensions,
                (
                    summary.geometry.width as u16,
                    summary.geometry.height as u16
                )
            );
            assert_timing(duration, bounds, Duration::from_secs_f64(0.4 / speed));
            assert!(frames.iter().all(|frame| frame == &frames[0]));
            assert!(!directory.0.join("result.gif.tui-test.cast").exists());
        }
    }

    #[test]
    fn initial_and_final_hidden_work_never_enter_gif_frames_or_time() {
        let directory = Directory::new();
        let (baseline, _) = timed_run(
            &plan("Hide\nType@40ms 'X'\nShow\nSleep .2\nHide"),
            &directory.0,
        );
        let (_, _, baseline_frames) = decode(&baseline.output);
        let (hidden, bounds) = timed_run(&plan("Hide\nHide\nType@40ms 'X'\nSleep .3\nShow\nShow\nSleep .2\nHide\nHide\nType@40ms 'ZZZ'\nSleep .3"), &directory.0);
        let (_, duration, frames) = decode(&hidden.output);
        assert_timing(duration, bounds, Duration::from_millis(200));
        assert!(frames.iter().all(|frame| frame == &baseline_frames[0]));
    }

    #[test]
    fn pause_and_resume_remove_hidden_intervals_and_hidden_edits() {
        let directory = Directory::new();
        let baseline = run(&plan("Sleep .4"), &directory.0).unwrap();
        let (_, _, baseline_frames) = decode(&baseline.output);
        let (paused, bounds) = timed_run(
            &plan("Sleep .2\nHide\nType@40ms 'Z'\nBackspace@40ms\nSleep .3\nShow\nSleep .2\nHide"),
            &directory.0,
        );
        let (_, duration, frames) = decode(&paused.output);
        assert_timing(duration, bounds, Duration::from_millis(400));
        assert!(frames.iter().all(|frame| frame == &baseline_frames[0]));
    }

    #[test]
    fn type_uses_unicode_scalars_and_delays_after_the_last_step() {
        let directory = Directory::new();
        let plan = plan("Sleep .1");
        let session = launch(&plan, &directory.0);
        let started = Instant::now();
        perform(
            &session,
            &plan.output().location,
            &Action::Type {
                text: "é界".into(),
                delay: Duration::from_millis(60),
            },
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(120));
        wait(&session, "é界");
        let started = Instant::now();
        perform(
            &session,
            &plan.output().location,
            &Action::Key {
                key: Key::Backspace,
                delay: Duration::from_millis(50),
                count: 2,
            },
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(100));
        session
            .execute(Operation::WaitLocator {
                query: crate::LocatorQuery::text("é界"),
                not: true,
                timeout_ms: Some(5_000),
            })
            .unwrap();
        session.close().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn input_bytes_cover_unicode_repetition_modifier_chains_and_plus() {
        let directory = Directory::new();
        let plan = fixture_plan(&directory.0, "input", "Sleep .1");
        let session = launch(&plan, &directory.0);
        let location = &plan.output().location;
        session
            .execute(Operation::ExpectMode {
                mode: "application_cursor_keys".into(),
                enabled: true,
                timeout_ms: Some(5_000),
            })
            .unwrap();
        perform(
            &session,
            location,
            &Action::Type {
                text: "é界".into(),
                delay: Duration::ZERO,
            },
        )
        .unwrap();
        perform(
            &session,
            location,
            &Action::Key {
                key: Key::Up,
                count: 1,
                delay: Duration::ZERO,
            },
        )
        .unwrap();
        perform(
            &session,
            location,
            &Action::Key {
                key: Key::Enter,
                count: 2,
                delay: Duration::ZERO,
            },
        )
        .unwrap();
        perform(
            &session,
            location,
            &Action::ModifiedKeys {
                modifiers: vec![Modifier::Ctrl],
                keys: vec![
                    ModifiedKey::Character('a'),
                    ModifiedKey::Character('b'),
                    ModifiedKey::Character('+'),
                    ModifiedKey::Character('é'),
                ],
            },
        )
        .unwrap();
        session
            .execute(Operation::Write { data: "!".into() })
            .unwrap();
        wait(&session, "RX21");
        let mut expected = "é界\x1bOA\r\r".as_bytes().to_vec();
        expected.extend_from_slice(b"\x01\x02\x1b[61;6u\x1b[233;5u!");
        assert_eq!(
            std::fs::read(directory.0.join("received")).unwrap(),
            expected
        );
        session.close().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn viewport_scroll_records_history_without_application_input() {
        let directory = Directory::new();
        let plan = fixture_plan(&directory.0, "scroll", "Sleep .1");
        let prepared = prepare(&plan, &directory.0).unwrap();
        let session = launch(&plan, &directory.0);
        wait(&session, "LINE29");
        start_recording(&session, &plan, &prepared.summary).unwrap();
        let location = &plan.output().location;
        let started = Instant::now();
        perform(
            &session,
            location,
            &Action::Scroll {
                direction: ScrollDirection::Up,
                count: 2,
                delay: Duration::from_millis(70),
            },
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(140));
        wait(&session, "LINE26");
        let crate::OperationResult::Viewport(viewport) =
            session.execute(Operation::GetViewport).unwrap()
        else {
            panic!()
        };
        assert_eq!(viewport.offset, 2);
        perform(
            &session,
            location,
            &Action::Scroll {
                direction: ScrollDirection::Down,
                count: 2,
                delay: Duration::from_millis(70),
            },
        )
        .unwrap();
        wait(&session, "LINE29");
        let crate::OperationResult::Viewport(viewport) =
            session.execute(Operation::GetViewport).unwrap()
        else {
            panic!()
        };
        assert_eq!(viewport.offset, 0);
        session.execute(Operation::StopRecording).unwrap();
        session.close().unwrap();
        assert!(!directory.0.join("received").exists());
        let (_, duration, frames) = decode(&prepared.summary.output);
        assert!(duration >= 28);
        assert!(
            frames.iter().any(|frame| frame != &frames[0]),
            "viewport did not change GIF pixels"
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_failure_reports_recovery_cast_and_closes_owned_child() {
        let directory = Directory::new();
        let plan = fixture_plan(&directory.0, "fail-export", "Sleep .1");
        let error = run(&plan, &directory.0).unwrap_err();
        assert_eq!(error.phase, RunnerPhase::Execution);
        assert_eq!(error.kind, ErrorKind::Internal);
        assert!(error.message.contains("captured cast retained"), "{error}");
        assert!(
            error.message.contains("result.gif.tui-test.cast"),
            "{error}"
        );
        assert!(directory.0.join("result.gif.tui-test.cast").is_file());
        let pid: i32 = std::fs::read_to_string(directory.0.join("child.pid"))
            .unwrap()
            .parse()
            .unwrap();
        // The recorded process ID belongs only to this fixture child.
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "owned child survived run failure"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[cfg(unix)]
    #[test]
    fn effective_path_relative_cwd_and_require_use_last_env_without_mutation() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = Directory::new();
        std::fs::create_dir(directory.0.join("bin")).unwrap();
        let shell = crate::shell::which("bash").unwrap();
        symlink(shell, directory.0.join("bin/bash")).unwrap();
        let executable = directory.0.join("bin/required-tool");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plan = plan("Require required-tool\nRequire './bin/required-tool'\nEnv PATH 'missing'\nEnv PATH 'bin'\nSleep .1");
        let before = std::env::var_os("PATH");
        let summary = check(&plan, &directory.0).unwrap();
        assert_eq!(summary.shell, directory.0.join("bin/bash"));
        assert_eq!(std::env::var_os("PATH"), before);
        run(&plan, &directory.0).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check(&plan, &directory.0)
            .unwrap_err()
            .message
            .contains("Require"));
        let builtin = super::plan("Require cd\nEnv PATH 'bin'\nSleep .1");
        assert!(check(&builtin, &directory.0)
            .unwrap_err()
            .message
            .contains("Require"));
    }

    #[cfg(unix)]
    #[test]
    fn native_shells_have_controlled_prompts_and_do_not_load_personal_rc() {
        let directory = Directory::new();
        for shell in ["bash", "zsh", "fish"] {
            if crate::shell::which(shell).is_none() {
                eprintln!("{shell} is unavailable; native shell test requires its installation");
                continue;
            }
            let home = directory.0.join(shell);
            std::fs::create_dir_all(home.join(".config/fish")).unwrap();
            for name in [
                ".bashrc",
                ".bash_profile",
                ".zshrc",
                ".zshenv",
                ".config/fish/config.fish",
            ] {
                std::fs::write(home.join(name), "echo UNWANTED_RC\n").unwrap();
            }
            let plan = plan(&format!(
                "Set Shell {shell}\nEnv HOME '{}'\nEnv ZDOTDIR '{}'\nEnv XDG_CONFIG_HOME '{}/.config'\nEnv PS1 'BAD_PROMPT'\nSleep .1",
                home.display(), home.display(), home.display()
            ));
            let session = launch(&plan, &directory.0);
            let crate::OperationResult::Text(text) =
                session.execute(Operation::Text { full: true }).unwrap()
            else {
                panic!()
            };
            assert!(text.contains('>'), "{shell}: {text:?}");
            assert!(
                !text.contains("UNWANTED_RC") && !text.contains("BAD_PROMPT"),
                "{shell}: {text:?}"
            );
            session.close().unwrap();
            assert!(!home.join(".bash_history").exists());
            assert!(!home.join(".zsh_history").exists());
            assert!(!home.join(".local/share/fish/fish_history").exists());
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_shells_start_with_controlled_prompts() {
        let directory = Directory::new();
        for shell in ["powershell", "pwsh", "cmd"] {
            if crate::shell::which(shell).is_none() {
                eprintln!("{shell} is unavailable; install it to exercise this native shell");
                continue;
            }
            let plan = plan(&format!(
                "Set Shell {shell}\nSet Columns 80\nSet Rows 6\nSleep .2"
            ));
            let session = launch(&plan, &directory.0);
            if shell != "cmd" {
                let crate::OperationResult::Text(text) =
                    session.execute(Operation::Text { full: true }).unwrap()
                else {
                    panic!()
                };
                assert_eq!(text.trim(), ">", "{shell} startup output: {text:?}");
                perform(&session, plan.shell_location(), &Action::Type {
                    text: "if (Get-Command Get-PSReadLineOption -ErrorAction SilentlyContinue) { (Get-PSReadLineOption).HistorySaveStyle } else { 'Save' + 'Nothing' }".into(),
                    delay: Duration::ZERO,
                }).unwrap();
                perform(
                    &session,
                    plan.shell_location(),
                    &Action::Key {
                        key: Key::Enter,
                        delay: Duration::ZERO,
                        count: 1,
                    },
                )
                .unwrap();
                wait(&session, "SaveNothing");
            }
            session.close().unwrap();
            run(&plan, &directory.0).unwrap();
            decode(&directory.0.join("result.gif"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn effective_pathext_accepts_existing_extensions_and_case_insensitive_env() {
        let directory = Directory::new();
        let cmd = crate::shell::which("cmd").unwrap();
        std::fs::copy(cmd, directory.0.join("cmd.exe")).unwrap();
        std::fs::write(directory.0.join("required.EXE"), b"lookup only").unwrap();
        let plan = plan("Set Shell cmd\nRequire required.EXE\nEnv PATH missing\nEnv Path '.'\nEnv PATHEXT '.EXE'\nSleep .1");
        assert_eq!(
            check(&plan, &directory.0).unwrap().shell,
            directory.0.join("./cmd.exe")
        );
        run(&plan, &directory.0).unwrap();
    }
}
