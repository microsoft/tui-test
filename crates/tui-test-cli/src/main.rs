mod agent_context;
mod ansi;
mod cli;
mod config;
#[cfg(windows)]
mod console_input;
mod daemon;
mod ipc;
mod monitor;
mod monitor_input;
mod protocol;
mod skill;
mod tape;
mod versions;

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{CommandFactory, FromArgMatches};

use cli::{
    Cli, ClickCmd, Command, DaemonCmd, ExpectCmd, FindCmd, GetArg, HighlightCmd, KeyCmd, MatchArg,
    MouseCmd, OpenArgs, RecordCmd, RunArgs, ScreenshotArgs, ScrollCmd, TextQueryArgs,
    TextSelectorArgs, TextStyleArgs, WaitCmd, WhitespaceArg,
};
use protocol::{GetField, MouseAction, Request, Response};
use tui_test::{
    CaptureBackground, ExecutionContext, FailureArtifactOptions, LocatorDirection, LocatorQuery,
    LocatorSelector, MatchOccurrence, MouseOptions, TextAnchor, TextScope, TextSelector, TextStyle,
    WhitespaceMode,
};

/// Agent skill router, installed as `SKILL.md`.
const SKILL_MD: &str = include_str!("../../../SKILL.md");
/// Version-matched references installed beside the skill router.
const SKILL_REFERENCES: &[(&str, &str)] = &[
    (
        "references/cli.md",
        include_str!("../../../references/cli.md"),
    ),
    (
        "references/python.md",
        include_str!("../../../references/python.md"),
    ),
    (
        "references/javascript.md",
        include_str!("../../../references/javascript.md"),
    ),
    (
        "references/rust.md",
        include_str!("../../../references/rust.md"),
    ),
    (
        "references/recipes.md",
        include_str!("../../../references/recipes.md"),
    ),
    (
        "references/tape.md",
        include_str!("../../../references/tape.md"),
    ),
];

fn main() {
    let args = command_line_args();
    let matches = match Cli::command().try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayVersion => {
            std::process::exit(print_versions(&args));
        }
        Err(error) => error.exit(),
    };
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if let Some(Command::Tape(args)) = &cli.command {
        std::process::exit(tape::execute(args, cli.json, &matches));
    }
    let session = config::session_name_from_env(cli.session.clone());
    let execution_context = match build_execution_context(&cli) {
        Ok(context) => context,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    let Some(command) = cli.command else {
        let _ = Cli::command().print_help();
        std::process::exit(0);
    };

    let code = match command {
        Command::InternalDaemon => {
            if let Err(e) = daemon::run(session, cli.verbose) {
                eprintln!("daemon error: {e}");
                std::process::exit(5);
            }
            0
        }
        Command::Usage => {
            print!("{}", usage_text());
            0
        }
        Command::AgentContext => {
            println!("{}", agent_context::render());
            0
        }
        Command::Skill { add: true } => skill::add(SKILL_MD, SKILL_REFERENCES),
        Command::Skill { add: false } => {
            print!("{}", skill::render(SKILL_MD, SKILL_REFERENCES));
            0
        }
        Command::GetRecording {
            session: target,
            config,
        } => get_recording(target.unwrap_or(session), config.as_deref()),
        Command::Sessions => list_sessions(cli.json),
        Command::Close { all } if all => close_all(cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Start,
        } => daemon_start(&session, cli.verbose, cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Status,
        } => daemon_status(&session, cli.json),
        Command::Daemon {
            cmd: DaemonCmd::Stop { all },
        } => daemon_stop(
            &session,
            all,
            config::session_was_specified(&cli.session),
            cli.json,
        ),
        Command::Monitor { interactive } => monitor::run_client(&session, interactive),
        command => run_remote(&session, command, cli.json, cli.verbose, execution_context),
    };
    std::process::exit(code);
}

fn print_versions(args: &[OsString]) -> i32 {
    // Parse output options without validating commands that will not execute.
    fn output_options(command: clap::Command) -> clap::Command {
        let has_subcommands = command.get_subcommands().next().is_some();
        let mut command = command
            .disable_help_flag(true)
            .disable_help_subcommand(true)
            .disable_version_flag(true)
            .arg(
                clap::Arg::new("help")
                    .short('h')
                    .long("help")
                    .action(clap::ArgAction::Count),
            )
            .mut_subcommands(output_options);
        if has_subcommands {
            command = command.subcommand(
                clap::Command::new("help")
                    .disable_help_flag(true)
                    .disable_help_subcommand(true)
                    .arg(
                        clap::Arg::new("help")
                            .short('h')
                            .long("help")
                            .action(clap::ArgAction::Count),
                    )
                    .arg(clap::Arg::new("command").num_args(0..)),
            );
        }
        command
    }
    let matches = output_options(Cli::command())
        .arg(
            clap::Arg::new("version")
                .short('V')
                .long("version")
                .action(clap::ArgAction::Count),
        )
        .ignore_errors(true)
        .get_matches_from(args);
    match versions::render(matches.get_flag("json")) {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(error) => {
            eprintln!("failed to report versions: {error:#}");
            5
        }
    }
}

fn command_line_args() -> Vec<OsString> {
    command_line_args_from(std::env::args_os())
}

fn command_line_args_from(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args = args.into_iter().collect::<Vec<_>>();
    let Some(separator) = args.iter().position(|arg| arg == "--") else {
        return args;
    };
    if has_command_before_separator(&args[1..separator]) {
        return args;
    }
    args.insert(separator, OsString::from("run"));
    args
}

fn has_command_before_separator(args: &[OsString]) -> bool {
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        let value = arg.to_string_lossy();
        if matches!(
            value.as_ref(),
            "--session"
                | "--failure-artifacts"
                | "--failure-artifact-mode"
                | "--diagnostic-context"
                | "--context-file"
                | "--operation-name"
        ) {
            skip_value = true;
        } else if value == "--json"
            || value == "--verbose"
            || value == "-v"
            || value.starts_with("--session=")
        {
        } else if !value.starts_with('-') {
            return true;
        }
    }
    false
}

/// Build the request for a daemon-backed command, then send it.
fn run_remote(
    session: &str,
    command: Command,
    json: bool,
    verbose: bool,
    context: ClientExecutionContext,
) -> i32 {
    let request = match build_request(command) {
        Ok(request) => context.apply(request),
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    // Closing must remain available when the running daemon is from an older
    // client; every other command requires matching protocol behavior.
    let allow_incompatible = request.is_close();
    let socket = config::socket_name(session);
    if allow_incompatible && !ipc::is_running(&socket) {
        if json {
            println!("{}", serde_json::json!({ "ok": true }));
        }
        return 0;
    }
    let conn = match connect_to_daemon(session, verbose, allow_incompatible) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 4;
        }
    };
    let response = if allow_incompatible {
        ipc::exchange_with_timeout(conn, &request, DAEMON_STOP_TIMEOUT)
    } else {
        ipc::exchange(conn, &request)
    };
    match response {
        Ok(resp) => print_response(&resp, json),
        Err(_) if allow_incompatible && !ipc::is_running(&socket) => {
            if json {
                println!("{}", serde_json::json!({ "ok": true }));
            }
            0
        }
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

struct ClientExecutionContext {
    context: ExecutionContext,
    retention_override: Option<tui_test::DiagnosticRetentionOptions>,
}

impl ClientExecutionContext {
    fn apply(self, mut request: Request) -> Request {
        if let (Some(retention), Request::Open { diagnostics, .. }) =
            (self.retention_override, &mut request)
        {
            // Copy explicit retention into the owned spawn request so the
            // protocol's default fallback cannot replace an explicit default.
            *diagnostics = retention;
        }
        request.with_context(self.context)
    }
}

fn build_execution_context(cli: &Cli) -> anyhow::Result<ClientExecutionContext> {
    let mut retention_override = None;
    let mut context: ExecutionContext = match &cli.context_file {
        Some(path) => {
            let value: serde_json::Value = serde_json::from_slice(
                &std::fs::read(path)
                    .with_context(|| format!("read context file {}", path.display()))?,
            )
            .context("invalid ExecutionContext JSON")?;
            let has_retention = value.get("retention").is_some();
            let context: ExecutionContext =
                serde_json::from_value(value).context("invalid ExecutionContext JSON")?;
            retention_override = has_retention.then_some(context.retention);
            context
        }
        None => ExecutionContext::default(),
    };
    if let Some(name) = &cli.operation_name {
        context.operation_name = Some(name.clone());
    }
    context.artifact = match cli.failure_artifacts.as_ref() {
        Some(directory) => Some(FailureArtifactOptions {
            directory: directory.clone(),
            mode: cli.failure_artifact_mode.into(),
            include_recording: cli.failure_artifact_recording,
        }),
        None => context.artifact,
    };
    if let Some(artifact) = &mut context.artifact {
        artifact.validate().map_err(anyhow::Error::msg)?;
        if !artifact.directory.as_os_str().is_empty() {
            artifact.directory = std::path::absolute(&artifact.directory)?;
        }
    }
    if let Some(trace) = &mut context.trace {
        trace.directory = std::path::absolute(&trace.directory)?;
    }
    for pair in &cli.diagnostic_context {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--diagnostic-context must use KEY=VALUE"))?;
        if key.trim().is_empty() {
            anyhow::bail!("--diagnostic-context key must not be empty");
        }
        context
            .diagnostic_context
            .insert(key.to_string(), value.to_string());
    }
    Ok(ClientExecutionContext {
        context,
        retention_override,
    })
}

fn connect_to_daemon(
    session: &str,
    verbose: bool,
    allow_incompatible: bool,
) -> anyhow::Result<ipc::Stream> {
    const ATTEMPTS: u32 = 3;
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        let socket = config::socket_name(session);
        if !(allow_incompatible && ipc::is_running(&socket)) {
            let _ = ensure_daemon(session, verbose)
                .map_err(|e| anyhow::anyhow!("failed to start daemon: {e}"))?;
        }
        match ipc::connect(&config::socket_name(session)) {
            Ok(conn) => return Ok(conn),
            Err(e) => last = Some(e),
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let err = last.expect("at least one connect attempt");
    Err(anyhow::anyhow!("request failed: {err}"))
}

fn ready_flag(wait_ready: bool, no_wait_ready: bool) -> Option<bool> {
    match (wait_ready, no_wait_ready) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

fn build_request(command: Command) -> anyhow::Result<Request> {
    let req = match command {
        Command::Open(OpenArgs {
            shell,
            backend,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            no_wait_ready,
            restart,
            profile,
            timeouts,
            diagnostics,
        }) => {
            let settings = resolve_client_settings(&profile)?;
            Request::Open {
                shell: shell.map(Into::into),
                program: None,
                backend: backend.map(Into::into).unwrap_or_default(),
                profile: settings.profile,
                cols,
                rows,
                cwd: Some(resolve_client_path(cwd.unwrap_or_else(|| ".".into()))?),
                env: parse_env(&env)?,
                wait_ready: ready_flag(wait_ready, no_wait_ready),
                restart,
                timeouts: settings.timeouts.with_overrides(timeouts.into()),
                recording: Box::new(settings.recording),
                style: Box::new(settings.style.clone()),
                trace: settings.trace,
                diagnostics: tui_test::DiagnosticRetentionOptions {
                    screen_history_limit: diagnostics
                        .screen_history_limit
                        .unwrap_or(settings.diagnostics.screen_history_limit),
                },
            }
        }
        Command::Run(RunArgs {
            program,
            args,
            backend,
            cols,
            rows,
            cwd,
            env,
            wait_ready,
            no_wait_ready,
            restart,
            profile,
            timeouts,
            diagnostics,
        }) => {
            let mut prog = vec![program];
            prog.extend(args);
            let settings = resolve_client_settings(&profile)?;
            Request::Open {
                shell: None,
                program: Some(prog),
                backend: backend.map(Into::into).unwrap_or_default(),
                profile: settings.profile,
                cols,
                rows,
                cwd: Some(resolve_client_path(cwd.unwrap_or_else(|| ".".into()))?),
                env: parse_env(&env)?,
                wait_ready: ready_flag(wait_ready, no_wait_ready),
                restart,
                timeouts: settings.timeouts.with_overrides(timeouts.into()),
                recording: Box::new(settings.recording),
                style: Box::new(settings.style.clone()),
                trace: settings.trace,
                diagnostics: tui_test::DiagnosticRetentionOptions {
                    screen_history_limit: diagnostics
                        .screen_history_limit
                        .unwrap_or(settings.diagnostics.screen_history_limit),
                },
            }
        }
        Command::Restart { graceful_timeout } => Request::Restart {
            graceful_timeout_ms: graceful_timeout,
        },
        Command::Close { .. } => Request::Close,
        // Every `daemon` subcommand is handled in `main`: they decide for
        // themselves whether to start a daemon, and requests built here always
        // do.
        Command::Daemon { .. } => {
            anyhow::bail!("internal: `daemon` must be handled before build_request")
        }
        Command::State => Request::State,
        Command::Text { full } => Request::Text { full },
        Command::PackedScreen { full } => Request::PackedScreen { full },
        Command::Trace {
            cmd: cli::TraceCmd::Finish { outcome },
        } => Request::FinishTrace {
            failed: matches!(outcome, cli::TraceOutcomeArg::Failed),
        },
        Command::Screenshot(ScreenshotArgs {
            path,
            out,
            full,
            zoom,
            background,
            transparent,
        }) => {
            let path = out.or(path).map(resolve_client_path).transpose()?;
            if (zoom.is_some() || background.is_some() || transparent) && path.is_none() {
                anyhow::bail!(
                    "screenshot --zoom, --background, and --transparent require --out or a path"
                );
            }
            Request::Screenshot {
                full,
                path,
                zoom,
                background: capture_background(background, transparent)?,
            }
        }
        Command::Record {
            cmd:
                RecordCmd::Start {
                    path,
                    format,
                    fps,
                    speed,
                    idle_time_limit,
                    zoom,
                    background,
                    transparent,
                },
        } => Request::StartRecording {
            path: resolve_client_path(path)?,
            format: format.map(Into::into),
            fps,
            speed,
            idle_time_limit,
            zoom,
            background: capture_background(background, transparent)?,
        },
        Command::Record {
            cmd: RecordCmd::Stop,
        } => Request::StopRecording,
        Command::Record {
            cmd: RecordCmd::Pause,
        } => Request::PauseRecording,
        Command::Record {
            cmd: RecordCmd::Resume,
        } => Request::ResumeRecording,
        Command::Cells { x, y, w, h } => Request::Cells { x, y, w, h },
        Command::Get {
            field: GetArg::ProgramStatus,
            id: Some(id),
        } => Request::GetProgramStatus { id },
        Command::Get { id: Some(_), .. } => {
            return Err(anyhow::anyhow!("--id is only valid for get program-status"))
        }
        Command::Get { field, id: None } => Request::Get {
            field: map_field(field),
        },
        Command::Type { text } => Request::Write { data: text },
        Command::Submit { text } => Request::Submit { data: text },
        Command::Key { action } => map_key(action),
        Command::Press { keys } => Request::Key {
            action: tui_test::KeyAction::Press,
            keys,
        },
        Command::Mouse { action } => Request::Mouse {
            action: map_mouse(action),
        },
        Command::Resize { cols, rows } => Request::Resize { cols, rows },
        Command::Scroll { target } => Request::Scroll {
            target: match target {
                ScrollCmd::Up { lines } => tui_test::ScrollTarget::Lines(lines),
                ScrollCmd::Down { lines } => tui_test::ScrollTarget::Lines(-lines),
                ScrollCmd::Top => tui_test::ScrollTarget::Top,
                ScrollCmd::Bottom => tui_test::ScrollTarget::Bottom,
            },
        },
        Command::Write { data } => Request::Write { data },
        Command::Signal { name } => Request::Signal {
            name: name.as_str().to_string(),
        },
        Command::Kill => Request::Signal {
            name: "KILL".to_string(),
        },
        Command::Interrupt => Request::Interrupt,
        Command::Wait { what } => map_wait(what)?,
        Command::Find { what } => Request::FindLocator {
            query: map_find(what)?,
        },
        Command::Resolve { what } => Request::ResolveLocator {
            query: map_find(what)?,
        },
        Command::Click { what } => map_click(what)?,
        Command::Highlight { what } => map_highlight(what)?,
        Command::Expect { what } => map_expect(what)?,
        _ => anyhow::bail!("unsupported command"),
    };
    Ok(req)
}

fn capture_background(
    background: Option<String>,
    transparent: bool,
) -> anyhow::Result<Option<CaptureBackground>> {
    if transparent {
        return Ok(Some(CaptureBackground::Transparent));
    }
    background
        .map(|value| CaptureBackground::parse(&value).map_err(anyhow::Error::msg))
        .transpose()
}

fn resolve_client_path(path: String) -> anyhow::Result<String> {
    if path.trim().is_empty() {
        anyhow::bail!("path must not be empty");
    }
    let path = std::path::absolute(path)?;
    Ok(path.to_string_lossy().into_owned())
}

fn resolve_client_settings(
    profile: &cli::ProfileArgs,
) -> anyhow::Result<tui_test::profile::Settings> {
    let mut settings = profile.resolve()?;
    settings.trace.directory = std::path::absolute(&settings.trace.directory)?;
    if let Some(directory) = &mut settings.recording.directory {
        *directory = std::path::absolute(&*directory)?;
    }
    Ok(settings)
}

fn map_field(field: GetArg) -> GetField {
    match field {
        GetArg::Command => GetField::Command,
        GetArg::Output => GetField::Output,
        GetArg::ExitCode => GetField::ExitCode,
        GetArg::Cwd => GetField::Cwd,
        GetArg::Cursor => GetField::Cursor,
        GetArg::Viewport => GetField::Viewport,
        GetArg::Modes => GetField::Modes,
        GetArg::Colors => GetField::Colors,
        GetArg::Size => GetField::Size,
        GetArg::Title => GetField::Title,
        GetArg::Clipboard => GetField::Clipboard,
        GetArg::ProgressBar => GetField::ProgressBar,
        GetArg::ProgramStatus => GetField::ProgramStatuses,
        GetArg::Bells => GetField::BellCount,
        GetArg::BellEvents => GetField::BellEvents,
    }
}

fn map_key(action: KeyCmd) -> Request {
    let (action, keys) = match action {
        KeyCmd::Press { keys } => (tui_test::KeyAction::Press, keys),
        KeyCmd::Down { keys } => (tui_test::KeyAction::Down, keys),
        KeyCmd::Repeat { keys } => (tui_test::KeyAction::Repeat, keys),
        KeyCmd::Up { keys } => (tui_test::KeyAction::Up, keys),
    };
    Request::Key { action, keys }
}

fn map_mouse(action: MouseCmd) -> MouseAction {
    match action {
        MouseCmd::Click {
            x,
            y,
            on_text,
            options,
            clicks,
        } => MouseAction::Click {
            x,
            y,
            on_text,
            options: options.into(),
            clicks,
        },
        MouseCmd::Move { x, y } => MouseAction::Move { x, y },
        MouseCmd::Down { x, y, options } => MouseAction::Down {
            x,
            y,
            options: options.into(),
        },
        MouseCmd::Up { x, y, options } => MouseAction::Up {
            x,
            y,
            options: options.into(),
        },
        MouseCmd::Drag {
            x1,
            y1,
            x2,
            y2,
            options,
        } => MouseAction::Drag {
            x1,
            y1,
            x2,
            y2,
            options: options.into(),
        },
        MouseCmd::Scroll { direction, amount } => MouseAction::Scroll {
            direction: direction.as_str().to_string(),
            amount,
        },
    }
}

fn map_wait(what: WaitCmd) -> anyhow::Result<Request> {
    Ok(match what {
        WaitCmd::Text {
            query,
            not,
            timeout,
        } => Request::WaitLocator {
            query: map_query(query, MatchOccurrence::Any),
            not,
            timeout_ms: timeout,
        },
        WaitCmd::Query {
            query,
            not,
            timeout,
        } => Request::WaitLocator {
            query: read_query(query)?,
            not,
            timeout_ms: timeout,
        },
        WaitCmd::Title {
            text,
            regex,
            not,
            timeout,
        } => Request::WaitTitle {
            text,
            regex,
            timeout_ms: timeout,
            not,
        },
        WaitCmd::Clipboard {
            text,
            regex,
            timeout,
        } => Request::WaitClipboard {
            text,
            regex,
            timeout_ms: timeout,
        },
        WaitCmd::Idle { timeout } => Request::WaitIdle {
            timeout_ms: timeout,
        },
        WaitCmd::Command { timeout } => Request::WaitCommand {
            timeout_ms: timeout,
        },
        WaitCmd::Exit { timeout } => Request::WaitExit {
            timeout_ms: timeout,
        },
        WaitCmd::Ready { timeout } => Request::WaitReady {
            timeout_ms: timeout,
        },
        WaitCmd::ProgressBar(args) => Request::WaitProgressBar {
            expected: tui_test::api::ProgressBarMatch {
                state: args.state.map(Into::into),
                progress: args.progress,
            },
            timeout_ms: args.timeout,
        },
        WaitCmd::ProgramStatus(args) => Request::WaitProgramStatus {
            timeout_ms: args.timeout,
            expected: args.expected(),
        },
        WaitCmd::Bell { timeout } => Request::WaitBell {
            timeout_ms: timeout,
        },
    })
}

fn map_occurrence(
    mode: Option<MatchArg>,
    nth: Option<usize>,
    default: MatchOccurrence,
) -> MatchOccurrence {
    if let Some(index) = nth {
        return MatchOccurrence::Nth(index);
    }
    match mode {
        Some(MatchArg::Any) => MatchOccurrence::Any,
        Some(MatchArg::Unique) => MatchOccurrence::Unique,
        Some(MatchArg::First) => MatchOccurrence::First,
        Some(MatchArg::Last) => MatchOccurrence::Last,
        None => default,
    }
}

fn map_anchor(
    text: Option<String>,
    regex: bool,
    mode: Option<MatchArg>,
    nth: Option<usize>,
) -> Option<TextAnchor> {
    text.map(|text| TextAnchor {
        text,
        regex,
        occurrence: map_occurrence(mode, nth, MatchOccurrence::Unique),
    })
}

fn map_selector(text: String, args: TextSelectorArgs) -> TextSelector {
    TextSelector {
        text,
        regex: args.regex,
        full: args.full,
        whitespace: match args.whitespace {
            WhitespaceArg::Exact => WhitespaceMode::Exact,
            WhitespaceArg::Normalize => WhitespaceMode::Normalize,
        },
        scope: TextScope {
            after: map_anchor(
                args.after_text,
                args.after_regex,
                args.after_match,
                args.after_nth,
            ),
            before: map_anchor(
                args.before_text,
                args.before_regex,
                args.before_match,
                args.before_nth,
            ),
        },
    }
}

fn map_style(args: TextStyleArgs) -> TextStyle {
    TextStyle {
        foreground: args.fg,
        background: args.bg,
        bold: args.bold,
        dim: args.dim,
        italic: args.italic,
        underline_style: args.underline_style,
        underline_color: args.underline_color,
        inverse: args.inverse,
        hidden: args.hidden,
        strikethrough: args.strikethrough,
        blink: args.blink,
    }
}

fn map_query(args: TextQueryArgs, default: MatchOccurrence) -> LocatorQuery {
    let occurrence = map_occurrence(args.selector.match_mode, args.selector.nth, default);
    let query = LocatorQuery {
        selector: LocatorSelector::Text(map_selector(args.text, args.selector)),
        occurrence: if args.link.is_some() {
            MatchOccurrence::Any
        } else {
            occurrence.clone()
        },
        within: None,
        direction: LocatorDirection::Within,
        style: map_style(*args.style),
    };
    match args.link {
        Some(uri) => LocatorQuery {
            occurrence,
            within: Some(Box::new(query)),
            ..LocatorQuery::link(uri)
        },
        None => query,
    }
}

fn read_query(args: cli::LocatorQueryArgs) -> anyhow::Result<LocatorQuery> {
    let json = match (args.query, args.query_file) {
        (Some(json), None) => json,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read locator query file {}", path.display()))?,
        _ => anyhow::bail!("supply exactly one of --query or --query-file"),
    };
    serde_json::from_str(&json).context("invalid LocatorQuery JSON")
}

fn map_find(what: FindCmd) -> anyhow::Result<LocatorQuery> {
    Ok(match what {
        FindCmd::Text { query } => map_query(query, MatchOccurrence::Any),
        FindCmd::Query(query) => read_query(query)?,
    })
}

fn map_click(what: ClickCmd) -> anyhow::Result<Request> {
    Ok(match what {
        ClickCmd::Query {
            query,
            options,
            clicks,
            timeout,
        } => Request::ClickLocator {
            query: read_query(query)?,
            button: MouseOptions::from(options).sgr_code(),
            clicks,
            timeout_ms: timeout,
        },
        ClickCmd::Text {
            query,
            options,
            clicks,
            timeout,
        } => Request::ClickLocator {
            query: map_query(query, MatchOccurrence::Any),
            button: MouseOptions::from(options).sgr_code(),
            clicks,
            timeout_ms: timeout,
        },
    })
}

fn map_highlight(what: HighlightCmd) -> anyhow::Result<Request> {
    Ok(match what {
        HighlightCmd::Query { query, timeout } => Request::HighlightLocator {
            query: read_query(query)?,
            timeout_ms: timeout,
        },
        HighlightCmd::Text { query, timeout } => Request::HighlightLocator {
            query: map_query(query, MatchOccurrence::Any),
            timeout_ms: timeout,
        },
    })
}

fn map_expect(what: ExpectCmd) -> anyhow::Result<Request> {
    Ok(match what {
        ExpectCmd::Query {
            query,
            not,
            timeout,
        } => Request::ExpectLocator {
            query: read_query(query)?,
            not,
            timeout_ms: timeout,
        },
        ExpectCmd::Text {
            query,
            not,
            timeout,
        } => Request::ExpectLocator {
            query: map_query(query, MatchOccurrence::Any),
            not,
            timeout_ms: timeout,
        },
        ExpectCmd::Title {
            text,
            regex,
            not,
            timeout,
        } => Request::ExpectTitle {
            text,
            regex,
            not,
            timeout_ms: timeout,
        },
        ExpectCmd::ExitCode { code, timeout } => Request::ExpectExitCode {
            code,
            timeout_ms: timeout,
        },
        ExpectCmd::Output { text, regex } => Request::ExpectOutput { text, regex },
        ExpectCmd::Mode { name, off, timeout } => Request::ExpectMode {
            mode: name,
            enabled: !off,
            timeout_ms: timeout,
        },
        ExpectCmd::Colors(cli::ExpectColorsArgs {
            foreground,
            background,
            cursor,
            palette,
            timeout,
        }) => Request::ExpectColors {
            foreground,
            background,
            cursor,
            palette,
            timeout_ms: timeout,
        },
        ExpectCmd::Cursor(cli::ExpectCursorArgs {
            visible,
            hidden,
            shape,
            x,
            y,
            timeout,
        }) => Request::ExpectCursor {
            // `--visible` and `--hidden` are separate flags rather than one
            // optional boolean so that naming neither leaves visibility
            // unchecked, which is what a caller asserting only a shape wants.
            visible: match (visible, hidden) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
            shape,
            x,
            y,
            timeout_ms: timeout,
        },
        ExpectCmd::ProgressBar(args) => Request::ExpectProgressBar {
            expected: tui_test::api::ProgressBarMatch {
                state: args.state.map(Into::into),
                progress: args.progress,
            },
            timeout_ms: args.timeout,
        },
        ExpectCmd::ProgramStatus(args) => Request::ExpectProgramStatus {
            timeout_ms: args.timeout,
            expected: args.expected(),
        },
        ExpectCmd::Bell { count, timeout } => Request::ExpectBellCount {
            count,
            timeout_ms: timeout,
        },
        ExpectCmd::Snapshot {
            name,
            update,
            include_style,
            include_title,
            cwd,
        } => Request::Snapshot {
            name,
            update,
            include_style,
            include_title,
            cwd: Some(resolve_client_path(cwd.unwrap_or_else(|| ".".into()))?),
        },
    })
}

fn parse_env(pairs: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    pairs
        .iter()
        .map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| anyhow::anyhow!("invalid env (expected KEY=VALUE): {p}"))
        })
        .collect()
}

const DAEMON_STATE_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_LOCK_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonStart {
    AlreadyRunning,
    Started,
    Restarted,
}

struct DaemonLock {
    _file: std::fs::File,
}

impl DaemonLock {
    fn acquire(session: &str) -> anyhow::Result<Self> {
        Self::acquire_path(config::daemon_lock_file(session), DAEMON_LOCK_TIMEOUT)
    }

    fn acquire_path(path: std::path::PathBuf, timeout: Duration) -> anyhow::Result<Self> {
        // Keep the inode in place: unlinking a lock file lets contenders lock
        // different files at the same path. The OS releases ownership on exit.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        anyhow::bail!(
                            "timed out waiting for daemon lifecycle lock {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    anyhow::bail!(
                        "failed to acquire daemon lifecycle lock {}: {error}",
                        path.display()
                    );
                }
            }
        }
    }
}

/// Spawn or replace the daemon for this session when necessary.
fn ensure_daemon(session: &str, verbose: bool) -> anyhow::Result<DaemonStart> {
    config::ensure_home()?;
    let socket = config::socket_name(session);
    if let Some(identity) = running_daemon_identity(session, &socket)? {
        if identity.compatible() {
            report_existing_daemon(session, verbose);
            return Ok(DaemonStart::AlreadyRunning);
        }
    }

    let _lock = DaemonLock::acquire(session)?;

    match running_daemon_identity(session, &socket)? {
        Some(identity) if identity.compatible() => {
            report_existing_daemon(session, verbose);
            Ok(DaemonStart::AlreadyRunning)
        }
        Some(identity) => {
            restart_daemon(session, &socket, &identity.to_string(), verbose)?;
            Ok(DaemonStart::Restarted)
        }
        None => {
            start_daemon(session, &socket, verbose)?;
            Ok(DaemonStart::Started)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonIdentity {
    version: String,
    protocol_version: u32,
}

impl DaemonIdentity {
    fn compatible(&self) -> bool {
        self.version == env!("CARGO_PKG_VERSION")
            && self.protocol_version == protocol::PROTOCOL_VERSION
    }
}

impl std::fmt::Display for DaemonIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} (protocol {})",
            self.version, self.protocol_version
        )
    }
}

fn running_daemon_identity(session: &str, socket: &str) -> anyhow::Result<Option<DaemonIdentity>> {
    match ipc::send_with_timeout(socket, &Request::Status, DAEMON_STATE_TIMEOUT) {
        Ok(status) => Ok(Some(DaemonIdentity {
            version: daemon_version(&status),
            protocol_version: daemon_protocol_version(&status),
        })),
        Err(error) if ipc::is_running(socket) => anyhow::bail!(
            "could not verify the daemon for session '{session}': {error}; run \
             `tui-test --session {session} close`, then retry"
        ),
        Err(_) => Ok(None),
    }
}

fn daemon_version(status: &Response) -> String {
    status
        .data
        .as_ref()
        .and_then(|data| data.get("version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn daemon_protocol_version(status: &Response) -> u32 {
    status
        .data
        .as_ref()
        .and_then(|data| data.get("protocol_version"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn report_existing_daemon(session: &str, verbose: bool) {
    if verbose {
        eprintln!(
            "note: daemon for session '{session}' is already running; verbose logging only \
             applies to a freshly started daemon. Run `tui-test --session {session} close` \
             first, then retry with --verbose."
        );
    }
}

fn restart_daemon(
    session: &str,
    socket: &str,
    running_version: &str,
    verbose: bool,
) -> anyhow::Result<()> {
    restart_daemon_with(
        session,
        running_version,
        verbose,
        || shutdown_daemon(socket),
        |expected| wait_for_daemon_state(socket, expected, DAEMON_STATE_TIMEOUT),
        || {
            let exe = std::env::current_exe()?;
            spawn_detached(&exe, session, verbose)
        },
    )
}

fn restart_daemon_with<Stop, Wait, Spawn>(
    session: &str,
    running_version: &str,
    verbose: bool,
    stop: Stop,
    mut wait: Wait,
    spawn: Spawn,
) -> anyhow::Result<()>
where
    Stop: FnOnce() -> anyhow::Result<()>,
    Wait: FnMut(bool) -> anyhow::Result<()>,
    Spawn: FnOnce() -> anyhow::Result<()>,
{
    stop().map_err(|error| {
        anyhow::anyhow!(
            "failed to stop daemon version {running_version} for session '{session}': {error}"
        )
    })?;
    wait(false)
        .map_err(|error| anyhow::anyhow!("daemon for session '{session}' did not stop: {error}"))?;

    if verbose {
        eprintln!(
            "restarting daemon for session '{session}' from version {running_version} to {}",
            env!("CARGO_PKG_VERSION")
        );
    }
    spawn()?;
    wait(true).map_err(|error| anyhow::anyhow!("daemon did not become ready: {error}"))
}

fn shutdown_daemon(socket: &str) -> anyhow::Result<()> {
    let response = ipc::send_with_timeout(socket, &Request::Shutdown, DAEMON_STOP_TIMEOUT)?;
    if response.ok {
        Ok(())
    } else {
        anyhow::bail!(
            "{}",
            response
                .message
                .as_deref()
                .unwrap_or("daemon refused to stop without an error message")
        )
    }
}

fn start_daemon(session: &str, socket: &str, verbose: bool) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    spawn_detached(&exe, session, verbose)?;
    wait_for_daemon_state(socket, true, DAEMON_STATE_TIMEOUT)
        .map_err(|error| anyhow::anyhow!("daemon did not become ready: {error}"))?;

    if verbose {
        eprintln!("daemon logging to {}", config::log_file(session).display());
    }
    Ok(())
}

fn wait_for_daemon_state(socket: &str, expected: bool, timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if ipc::is_running(socket) == expected {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if expected {
        anyhow::bail!("socket never started accepting connections")
    } else {
        anyhow::bail!("socket kept accepting connections")
    }
}

#[cfg(windows)]
fn spawn_detached(exe: &Path, session: &str, verbose: bool) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    disown_std_handles();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__daemon").arg("--session").arg(session);
    if verbose {
        cmd.arg("--verbose");
    }
    // CREATE_NEW_PROCESS_GROUP disables Ctrl+C and that ignore state is
    // inherited by ConPTY children. A detached daemon needs no console group.
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// Clear stdio inheritance so a detached Windows daemon cannot keep pipe EOF open.
#[cfg(windows)]
fn disown_std_handles() {
    use std::os::windows::io::AsRawHandle;

    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    extern "system" {
        fn SetHandleInformation(
            h_object: *mut std::ffi::c_void,
            dw_mask: u32,
            dw_flags: u32,
        ) -> i32;
    }

    let handles = [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ];
    for handle in handles {
        if !handle.is_null() {
            // Safety: the handle comes from the standard streams, which outlive
            // this call, and clearing the inherit flag never affects our own use.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

#[cfg(not(windows))]
fn spawn_detached(exe: &Path, session: &str, verbose: bool) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__daemon").arg("--session").arg(session);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// Stream a session's recording (asciinema v2 cast) to stdout.
fn get_recording(session: String, explicit_config: Option<&Path>) -> i32 {
    let socket = config::socket_name(&session);
    let live_path = if ipc::is_running(&socket) {
        let response = match ipc::connect(&socket) {
            Ok(connection) => match ipc::exchange(connection, &Request::FlushRecording) {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("failed to flush recording: {error}");
                    return 4;
                }
            },
            Err(error) => {
                eprintln!("failed to flush recording: {error}");
                return 4;
            }
        };
        if !response.ok {
            eprintln!(
                "{}",
                response
                    .message
                    .as_deref()
                    .unwrap_or("failed to flush recording")
            );
            return response.kind.map_or(5, tui_test::ErrorKind::exit_code);
        }
        let disabled = response
            .data
            .as_ref()
            .and_then(|data| data.get("disabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if disabled {
            eprintln!("automatic recording is disabled for session '{session}'");
            return 3;
        }
        response
            .data
            .as_ref()
            .and_then(|data| data.get("recording"))
            .and_then(serde_json::Value::as_str)
            .map(std::path::PathBuf::from)
    } else {
        None
    };
    let path = match live_path {
        Some(path) => path,
        None => match std::fs::read_to_string(config::recording_pointer_file(&session)) {
            Ok(path) if path.is_empty() => {
                eprintln!("no recording for session '{session}'");
                return 3;
            }
            Ok(path) => std::path::PathBuf::from(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let recording = match tui_test::profile::resolve_recording(explicit_config, &cwd) {
                    Ok(recording) => recording,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                };
                if recording.mode == tui_test::AutomaticRecordingMode::Disabled {
                    eprintln!("automatic recording is disabled for session '{session}'");
                    return 3;
                }
                if recording.directory.is_some() {
                    eprintln!("no recording for session '{session}'");
                    return 3;
                }
                config::recording_file(&session)
            }
            Err(error) => {
                eprintln!("failed to read recording metadata: {error}");
                return 5;
            }
        },
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(&bytes);
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no recording for session '{session}'");
            3
        }
        Err(e) => {
            eprintln!("failed to read recording: {e}");
            5
        }
    }
}

/// Every session in this home whose daemon is currently answering.
fn running_sessions() -> Vec<String> {
    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(config::home_dir()) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stripped) = name.strip_suffix(".pid") {
            if ipc::is_running(&config::socket_name(stripped)) {
                sessions.push(stripped.to_string());
            }
        }
    }
    sessions.sort();
    sessions
}

fn list_sessions(json: bool) -> i32 {
    let sessions = running_sessions();
    if json {
        println!("{}", serde_json::json!({ "sessions": sessions }));
    } else if sessions.is_empty() {
        println!("no active sessions");
    } else {
        for s in sessions {
            println!("{s}");
        }
    }
    0
}

fn close_all(json: bool) -> i32 {
    for name in running_sessions() {
        let _ = ipc::send_with_timeout(
            &config::socket_name(&name),
            &Request::Close,
            DAEMON_STOP_TIMEOUT,
        );
    }
    if json {
        println!("{}", serde_json::json!({ "ok": true }));
    } else {
        println!("closed all sessions");
    }
    0
}

/// Start a session's daemon, returning only after the socket accepts connections.
fn daemon_start(session: &str, verbose: bool, json: bool) -> i32 {
    let outcome = match ensure_daemon(session, verbose) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("failed to start daemon: {e}");
            return 4;
        }
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "session": session,
                "started": outcome != DaemonStart::AlreadyRunning,
                "restarted": outcome == DaemonStart::Restarted,
            })
        );
    } else {
        match outcome {
            DaemonStart::AlreadyRunning => {
                println!("daemon already running for session '{session}'");
            }
            DaemonStart::Started => println!("started daemon for session '{session}'"),
            DaemonStart::Restarted => println!("restarted daemon for session '{session}'"),
        }
    }
    0
}

/// Report on a session's daemon without starting one.
fn daemon_status(session: &str, json: bool) -> i32 {
    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        if json {
            println!(
                "{}",
                serde_json::json!({ "session": session, "running": false, "pid": null })
            );
        } else {
            eprintln!("no daemon running for session '{session}'");
        }
        return 3;
    }
    match ipc::send_with_timeout(&socket, &Request::Status, DAEMON_STATE_TIMEOUT) {
        Ok(resp) => print_response(&resp, json),
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

/// Stop targeted daemons without auto-starting anything first.
fn daemon_stop(session: &str, all: bool, targeted: bool, json: bool) -> i32 {
    if all {
        return stop_all_daemons(json);
    }
    if !targeted {
        eprintln!(
            "error: `daemon stop` needs a target; every session has its own daemon\n  \
             --session <NAME>  stop one session's daemon (env: TUI_TEST_SESSION)\n  \
             --all             stop every daemon\n\
             hint: `tui-test sessions` lists what is running"
        );
        return 2;
    }
    let socket = config::socket_name(session);
    if !ipc::is_running(&socket) {
        eprintln!("no daemon running for session '{session}'");
        return 3;
    }
    match shutdown_daemon(&socket)
        .and_then(|_| wait_for_daemon_state(&socket, false, DAEMON_STATE_TIMEOUT))
    {
        Ok(()) => {
            report_stopped(&[session.to_string()], json);
            0
        }
        Err(e) => {
            eprintln!("request failed: {e}");
            4
        }
    }
}

fn stop_all_daemons(json: bool) -> i32 {
    let mut stopped = Vec::new();
    for name in running_sessions() {
        let socket = config::socket_name(&name);
        if shutdown_daemon(&socket)
            .and_then(|_| wait_for_daemon_state(&socket, false, DAEMON_STATE_TIMEOUT))
            .is_ok()
        {
            stopped.push(name);
        }
    }
    report_stopped(&stopped, json);
    0
}

fn report_stopped(stopped: &[String], json: bool) {
    if json {
        println!("{}", serde_json::json!({ "ok": true, "stopped": stopped }));
    } else if stopped.is_empty() {
        println!("no daemons running");
    } else {
        for name in stopped {
            println!("stopped daemon for session '{name}'");
        }
    }
}

fn print_response(resp: &Response, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::to_string(resp).unwrap_or_default());
        return exit_code(resp);
    }
    if resp.ok {
        if let Some(data) = &resp.data {
            println!("{}", format_data(data));
        }
        0
    } else {
        if let Some(msg) = &resp.message {
            eprintln!("{msg}");
        }
        if let Some(artifact) = &resp.artifact {
            if let Some(path) = artifact.report_html.as_deref() {
                eprintln!("Failure report: {path}");
            }
            if let Some(path) = artifact.report.as_deref() {
                eprintln!("Agent report: {path}");
            }
            if let Some(path) = artifact.manifest.as_deref() {
                eprintln!("Failure artifact: {path}");
            } else {
                eprintln!("Failure artifact was not written ({:?})", artifact.status);
            }
            for error in &artifact.errors {
                eprintln!("Failure artifact error: {error}");
            }
        }
        exit_code(resp)
    }
}

/// Render a successful payload for a human, keeping text-only output bare.
fn format_data(data: &serde_json::Value) -> String {
    let text = data.get("text").and_then(|v| v.as_str());
    let Some(map) = data.as_object() else {
        return serde_json::to_string_pretty(data).unwrap_or_default();
    };
    match text {
        Some(text) if map.len() == 1 => text.to_string(),
        None => serde_json::to_string_pretty(data).unwrap_or_default(),
        Some(text) => {
            let mut out = String::new();
            for (key, value) in map {
                if key != "text" {
                    out.push_str(&format!("{key}: {}\n", compact(value)));
                }
            }
            out.push_str(text);
            out
        }
    }
}

/// One-line rendering of a field value, unquoted for plain strings.
fn compact(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn usage_text() -> &'static str {
    "tui-test: headless terminal cli + daemon\n\
\n\
SESSION   open [--shell S] [--cols N --rows N] [--cwd D] [--env K=V]\n\
                  [--config F] [--profile P] [--restart]\n\
          run [--config F] [--profile P] [--restart] <program> [args...]\n\
          [global options] -- <program> [args...]  (direct run shorthand)\n\
          restart [--graceful-timeout MS]\n\
          interrupt (cancel operations and terminate the child; retain session state)\n\
          sessions | close [--all] | daemon start|status | daemon stop --session N|--all\n\
INSPECT   state | text [--full] | screenshot [-o file.svg|file.png] [--full] [--zoom N]\n\
          [--background COLOR | --transparent]\n\
          find text \"T\" [selector/style options] | cells X Y [W H]\n\
          packed-screen [--full] --json | resolve text \"T\" [selector/style options]\n\
          get command|output|exit-code|cwd|cursor|viewport|modes|colors|size|title|clipboard|bells|bell-events|progress-bar\n\
QUERY     find|resolve|wait|expect|click|highlight query --query JSON|--query-file PATH\n\
          Native nested LocatorQuery JSON; no shell expression language\n\
HISTORY   scroll up|down [LINES] | scroll top|bottom\n\
INPUT     type \"text\" | submit [\"text\"]\n\
          key press|down|repeat|up <Key...>\n\
          click text \"T\" [selector/style options] [--button left|middle|right] [--alt --ctrl --shift]\n\
          mouse click X Y | mouse click --on-text \"OK\" | mouse move|down|up|drag|scroll\n\
PTY       resize COLS ROWS | write <data> | signal INT|TERM|KILL|QUIT | kill\n\
WAIT      wait title \"T\" [--regex --not --timeout MS]\n\
          wait progress-bar [--state inactive|normal|error|indeterminate|paused] [--progress 0..100] [--timeout MS]\n\
          wait text \"T\" [selector/style options] [--not --timeout MS]\n\
          wait clipboard [TEXT] [--regex] | wait idle | wait command | wait exit | wait ready | wait bell\n\
EXPECT    expect text \"T\" [selector/style options] [--not --timeout MS]\n\
          expect progress-bar [--state inactive|normal|error|indeterminate|paused] [--progress 0..100] [--timeout MS]\n\
          expect title \"T\" [--regex --not --timeout MS]\n\
          expect exit-code N | expect output \"T\" [--regex] | expect bell N\n\
          expect snapshot NAME [-u] [--include-style --include-title] [--cwd DIR]\n\
DEBUG     highlight text \"T\" [selector/style options] [--timeout MS]\n\
RECORD    record start OUT [--format apng|gif|mp4|cast] [--fps N] [--speed N] [--zoom N]\n\
          [--background COLOR | --transparent]\n\
          record pause|resume|stop | get-recording [session] > out.cast (when tracing is enabled)\n\
TRACE     trace finish passed|failed (keep the session open)\n\
TAPE      tape check FILE | tape run FILE (trusted scripts; isolated local session)\n\
WATCH     monitor [--interactive] (read-only detach: q/Esc/Ctrl-C; interactive detach: Ctrl+])\n\
AGENT     agent-context (JSON cli schema) | skill [--add] (workflow guide)\n\
VERSION   -V/--version (all versions)\n\
GLOBAL    --session NAME | --json | --verbose | --failure-artifacts DIR\n\
          [--failure-artifact-mode none|text|html|all]\n\
          [--failure-artifact-recording] [--diagnostic-context KEY=VALUE]\n\
          [--context-file PATH] [--operation-name NAME]\n\
EXIT      0 ok | 1 assertion/wait failed | 2 usage | 3 no session | 4 daemon/IPC | 5 internal\n\
"
}

/// Map a response to a stable process exit code (see the exit-code taxonomy).
fn exit_code(resp: &Response) -> i32 {
    if resp.ok {
        0
    } else {
        resp.kind.map(|k| k.exit_code()).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn query_commands_preserve_every_nested_locator_field() {
        let mut query = LocatorQuery::text(TextSelector {
            text: "Save".into(),
            regex: true,
            full: true,
            whitespace: WhitespaceMode::Normalize,
            scope: TextScope {
                after: Some(TextAnchor {
                    text: "Start".into(),
                    regex: false,
                    occurrence: MatchOccurrence::First,
                }),
                before: None,
            },
        })
        .and(LocatorQuery::style(TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        }))
        .or(LocatorQuery::link("test:save"))
        .filter(
            Some(LocatorQuery::text("Save")),
            Some(LocatorQuery::text("old")),
        );
        query.within = Some(Box::new(LocatorQuery::text("Settings")));
        query.direction = LocatorDirection::After;
        query.occurrence = MatchOccurrence::Nth(1);
        query.style = TextStyle {
            italic: Some(false),
            ..TextStyle::default()
        };
        let json = serde_json::to_string(&query).unwrap();
        for command in ["find", "resolve", "wait", "expect", "click", "highlight"] {
            let cli =
                Cli::try_parse_from(["tui-test", command, "query", "--query", &json]).unwrap();
            let request = build_request(cli.command.unwrap()).unwrap();
            let actual = match request {
                Request::FindLocator { query }
                | Request::ResolveLocator { query }
                | Request::WaitLocator { query, .. }
                | Request::ExpectLocator { query, .. }
                | Request::ClickLocator { query, .. }
                | Request::HighlightLocator { query, .. } => query,
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(actual, query, "{command}");
        }
    }

    #[test]
    fn query_arguments_require_one_source_and_report_invalid_json() {
        for command in ["find", "resolve", "wait", "expect", "click", "highlight"] {
            assert!(Cli::try_parse_from(["tui-test", command, "query"]).is_err());
            assert!(Cli::try_parse_from([
                "tui-test",
                command,
                "query",
                "--query",
                "{}",
                "--query-file",
                "query.json",
            ])
            .is_err());
            let cli = Cli::try_parse_from(["tui-test", command, "query", "--query", "{}"]).unwrap();
            assert!(build_request(cli.command.unwrap())
                .unwrap_err()
                .to_string()
                .contains("LocatorQuery"));
        }
    }

    #[test]
    fn query_file_uses_the_same_native_json_contract() {
        let query = read_query(cli::LocatorQueryArgs {
            query: None,
            query_file: Some(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/locator-query.json"),
            ),
        })
        .unwrap();
        assert_eq!(query.occurrence, MatchOccurrence::Unique);
        let LocatorSelector::And { left, .. } = query.selector else {
            panic!("expected composite query");
        };
        assert_eq!(left.direction, LocatorDirection::After);
        assert!(left.within.is_some());
        assert_eq!(left.style.bold, Some(true));
        assert!(read_query(cli::LocatorQueryArgs {
            query: None,
            query_file: Some(std::path::PathBuf::from("nonexistent-locator-query.json")),
        })
        .is_err());
    }

    #[test]
    fn wait_text_and_query_preserve_negative_wait_options() {
        for args in [
            vec![
                "tui-test",
                "wait",
                "text",
                "gone",
                "--not",
                "--timeout",
                "123",
                "--bold=false",
            ],
            vec![
                "tui-test",
                "wait",
                "query",
                "--query",
                r#"{"selector":{"kind":"text","selector":{"text":"gone"}},"style":{"bold":false}}"#,
                "--not",
                "--timeout",
                "123",
            ],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Request::WaitLocator {
                query,
                not,
                timeout_ms,
            } = build_request(cli.command.unwrap()).unwrap()
            else {
                panic!("expected wait locator");
            };
            assert!(not);
            assert_eq!(timeout_ms, Some(123));
            assert_eq!(query.style.bold, Some(false));
        }
    }

    #[test]
    fn new_session_commands_map_to_requests() {
        let cli = Cli::try_parse_from(["tui-test", "interrupt"]).unwrap();
        assert!(matches!(
            build_request(cli.command.unwrap()).unwrap(),
            Request::Interrupt
        ));
        let cli = Cli::try_parse_from(["tui-test", "packed-screen", "--full"]).unwrap();
        assert!(matches!(
            build_request(cli.command.unwrap()).unwrap(),
            Request::PackedScreen { full: true }
        ));
        for (outcome, expected) in [("passed", false), ("failed", true)] {
            let cli = Cli::try_parse_from(["tui-test", "trace", "finish", outcome]).unwrap();
            assert!(
                matches!(build_request(cli.command.unwrap()).unwrap(), Request::FinishTrace { failed } if failed == expected)
            );
        }
        assert!(Cli::try_parse_from(["tui-test", "trace", "finish"]).is_err());
        let cli = Cli::try_parse_from([
            "tui-test",
            "expect",
            "snapshot",
            "screen",
            "--cwd",
            "baselines",
        ])
        .unwrap();
        let Request::Snapshot { cwd, .. } = build_request(cli.command.unwrap()).unwrap() else {
            panic!("expected snapshot");
        };
        assert_eq!(
            std::path::PathBuf::from(cwd.unwrap()),
            std::path::absolute("baselines").unwrap()
        );
    }

    #[test]
    fn context_file_preserves_fields_and_explicit_flags_override_it() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/execution-context.json");
        let cli = Cli::try_parse_from([
            "tui-test",
            "--context-file",
            path.to_str().unwrap(),
            "--operation-name",
            "save",
            "--diagnostic-context",
            "step=override",
            "state",
        ])
        .unwrap();
        let context = build_execution_context(&cli).unwrap().context;
        assert_eq!(context.operation_name.as_deref(), Some("save"));
        assert_eq!(context.diagnostic_context["step"], "override");
        assert_eq!(context.diagnostic_context["test"], "settings");
        assert_eq!(context.retention.screen_history_limit, 3);
        assert_eq!(
            context.trace.as_ref().unwrap().mode,
            tui_test::TraceMode::OnFailure
        );
        assert_eq!(
            context.trace.unwrap().directory,
            std::path::absolute("artifacts/traces").unwrap()
        );
        let artifact = context.artifact.unwrap();
        assert_eq!(
            artifact.directory,
            std::path::absolute("artifacts/failures").unwrap()
        );
        assert_eq!(artifact.mode, tui_test::FailureArtifactMode::Text);
        assert!(artifact.include_recording);
        for flag in ["--context-file", "--operation-name"] {
            let args = command_line_args_from(
                ["tui-test", flag, "run", "--", "sh"]
                    .into_iter()
                    .map(OsString::from),
            );
            assert!(matches!(
                Cli::try_parse_from(args).unwrap().command,
                Some(Command::Run(_))
            ));
        }
    }

    #[test]
    fn context_file_retention_overrides_are_resolved_before_protocol_dispatch() {
        let directory = unique_test_path("context-retention");
        std::fs::create_dir_all(&directory).unwrap();
        let config_path = directory.join("config.toml");
        let context_path = directory.join("context.json");
        let config = "[diagnostics]\nscreen-history-limit = 3\n";
        std::fs::write(&config_path, config).unwrap();
        let default = tui_test::DiagnosticRetentionOptions::default().screen_history_limit;
        for (retention, flag, expected) in [
            (None, None, 3),
            (None, Some("5"), 5),
            (Some(default), None, default),
            (Some(default), Some("5"), default),
            (Some(0), None, 0),
            (Some(7), None, 7),
        ] {
            let value = match retention {
                Some(limit) => json!({"retention": {"screen_history_limit": limit}}),
                None => json!({"operation_name": "spawn"}),
            };
            std::fs::write(&context_path, serde_json::to_vec(&value).unwrap()).unwrap();
            for command in ["open", "run"] {
                let mut args = vec![
                    "tui-test",
                    "--context-file",
                    context_path.to_str().unwrap(),
                    command,
                    "--config",
                    config_path.to_str().unwrap(),
                ];
                if let Some(flag) = flag {
                    args.extend(["--screen-history-limit", flag]);
                }
                if command == "run" {
                    args.push("program");
                }
                let cli = Cli::try_parse_from(args).unwrap();
                let context = build_execution_context(&cli).unwrap();
                assert_eq!(
                    context
                        .retention_override
                        .map(|options| options.screen_history_limit),
                    retention
                );
                let request = context.apply(build_request(cli.command.unwrap()).unwrap());
                let decoded: Request =
                    serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
                let request = match decoded {
                    Request::WithContext { request, context } => {
                        if let Some(limit) = retention {
                            assert_eq!(context.retention.screen_history_limit, limit);
                        }
                        *request
                    }
                    request => request,
                };
                let Request::Open { diagnostics, .. } = request else {
                    panic!("expected spawn request");
                };
                assert_eq!(diagnostics.screen_history_limit, expected);
            }
            assert_eq!(std::fs::read_to_string(&config_path).unwrap(), config);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&std::fs::read(&context_path).unwrap())
                    .unwrap(),
                value
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn context_file_disabled_artifacts_preserve_empty_directories() {
        let directory = unique_test_path("disabled-artifact-context");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("context.json");
        for artifact in [
            json!({"mode": "none"}),
            json!({"mode": "none", "directory": ""}),
            json!({"mode": "none", "directory": "artifacts/disabled"}),
        ] {
            let value = json!({"artifact": artifact});
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            let cli = Cli::try_parse_from([
                "tui-test",
                "--context-file",
                path.to_str().unwrap(),
                "state",
            ])
            .unwrap();
            let context = build_execution_context(&cli).unwrap().context;
            let actual = context.artifact.unwrap();
            assert_eq!(actual.mode, tui_test::FailureArtifactMode::None);
            assert!(actual.validate().is_ok());
            if artifact["directory"].as_str().is_none_or(str::is_empty) {
                assert!(actual.directory.as_os_str().is_empty());
            } else {
                assert_eq!(
                    actual.directory,
                    std::path::absolute("artifacts/disabled").unwrap()
                );
            }
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap())
                    .unwrap(),
                value
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn context_file_active_artifacts_reject_empty_directories_with_rust_validation() {
        let directory = unique_test_path("active-artifact-context");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("context.json");
        for mode in ["all", "text", "html"] {
            for artifact in [
                json!({"mode": mode}),
                json!({"mode": mode, "directory": ""}),
            ] {
                std::fs::write(
                    &path,
                    serde_json::to_vec(&json!({"artifact": artifact})).unwrap(),
                )
                .unwrap();
                let cli = Cli::try_parse_from([
                    "tui-test",
                    "--context-file",
                    path.to_str().unwrap(),
                    "state",
                ])
                .unwrap();
                let error = build_execution_context(&cli)
                    .err()
                    .expect("invalid artifact directory");
                assert_eq!(
                    error.to_string(),
                    "failure artifact directory must not be empty"
                );
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn version_flags_remain_child_arguments_after_run_program() {
        for flag in ["--version", "-V"] {
            for arguments in [
                vec!["tui-test", "run", "python", flag],
                vec!["tui-test", "run", "--", "python", flag],
                vec!["tui-test", "run", "python", "--", flag],
                vec!["tui-test", "--", "python", flag],
            ] {
                let arguments = command_line_args_from(arguments.into_iter().map(OsString::from));
                let cli = Cli::try_parse_from(arguments).unwrap();
                let Some(Command::Run(RunArgs { program, args, .. })) = cli.command else {
                    panic!("expected run");
                };
                assert_eq!(program, "python");
                assert_eq!(args, [flag]);
            }
        }
    }

    fn unique_test_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tui-test-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn a_text_only_payload_prints_the_bare_screen() {
        assert_eq!(
            format_data(&json!({ "text": "hello\nworld" })),
            "hello\nworld"
        );
    }

    #[test]
    fn screenshot_zoom_without_output_is_rejected() {
        let cli = Cli::try_parse_from(["tui-test", "screenshot", "--zoom", "0.5"]).unwrap();
        let command = cli.command.unwrap();
        assert!(build_request(command)
            .unwrap_err()
            .to_string()
            .contains("require --out"));
    }

    #[test]
    fn recording_controls_map_to_protocol_requests() {
        for (command, kind) in [("pause", "pause_recording"), ("resume", "resume_recording")] {
            let cli = Cli::try_parse_from(["tui-test", "record", command]).unwrap();
            let request = build_request(cli.command.unwrap()).unwrap();
            let json = serde_json::to_value(&request).unwrap();
            assert_eq!(json, serde_json::json!({ "kind": kind }));
            let decoded: Request = serde_json::from_value(json).unwrap();
            assert!(matches!(
                (command, decoded),
                ("pause", Request::PauseRecording) | ("resume", Request::ResumeRecording)
            ));
        }
    }

    #[test]
    fn capture_background_flags_map_to_requests() {
        let screenshot = Cli::try_parse_from([
            "tui-test",
            "screenshot",
            "screen.svg",
            "--background",
            "#123456",
        ])
        .unwrap();
        assert!(matches!(
            build_request(screenshot.command.unwrap()).unwrap(),
            Request::Screenshot {
                background: Some(CaptureBackground::Color(color)),
                ..
            } if color.to_hex() == "#123456"
        ));

        let recording =
            Cli::try_parse_from(["tui-test", "record", "start", "demo.gif", "--transparent"])
                .unwrap();
        assert!(matches!(
            build_request(recording.command.unwrap()).unwrap(),
            Request::StartRecording {
                background: Some(CaptureBackground::Transparent),
                ..
            }
        ));
    }

    #[test]
    fn top_level_separator_expands_to_run() {
        let args = command_line_args_from(
            ["tui-test", "--session", "demo", "--", "vim", "--clean"]
                .into_iter()
                .map(OsString::from),
        );
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Run(RunArgs {
                program,
                args,
                ..
            })) if program == "vim" && args == ["--clean"]
        ));
    }

    #[test]
    fn top_level_separator_expands_to_run_with_diagnostic_flags() {
        for flags in [
            vec!["--failure-artifacts", "run"],
            vec!["--failure-artifact-mode", "text"],
            vec!["--diagnostic-context", "test=save"],
            vec![
                "--failure-artifacts",
                "artifacts",
                "--failure-artifact-mode",
                "text",
                "--failure-artifact-recording",
                "--diagnostic-context",
                "test=save",
                "--diagnostic-context",
                "step=launch",
            ],
            vec![
                "--failure-artifacts=artifacts",
                "--failure-artifact-mode=text",
                "--diagnostic-context=test=save",
            ],
        ] {
            for explicit_run in [false, true] {
                let mut args = vec!["tui-test", "--json", "--session", "demo"];
                args.extend(&flags);
                if explicit_run {
                    args.push("run");
                }
                args.extend(["--", "vim", "--clean"]);
                let rewritten = command_line_args_from(args.iter().map(OsString::from));
                let cli = Cli::try_parse_from(rewritten)
                    .unwrap_or_else(|error| panic!("{args:?}: {error}"));
                assert!(matches!(
                    cli.command,
                    Some(Command::Run(RunArgs { program, args, .. }))
                        if program == "vim" && args == ["--clean"]
                ));
            }
        }
    }

    #[test]
    fn separator_after_a_subcommand_is_not_rewritten() {
        let args = command_line_args_from(
            ["tui-test", "run", "--", "vim", "--clean"]
                .into_iter()
                .map(OsString::from),
        );
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Run(RunArgs {
                program,
                args,
                ..
            })) if program == "vim" && args == ["--clean"]
        ));
    }

    #[test]
    fn failure_artifact_flags_build_an_execution_context() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "--failure-artifacts",
            "artifacts",
            "--failure-artifact-mode",
            "text",
            "--failure-artifact-recording",
            "--diagnostic-context",
            "test=save",
            "expect",
            "text",
            "missing",
        ])
        .unwrap();
        let context = build_execution_context(&cli).unwrap().context;
        let artifact = context.artifact.unwrap();
        assert!(artifact.directory.is_absolute());
        assert_eq!(artifact.mode, tui_test::FailureArtifactMode::Text);
        assert!(artifact.include_recording);
        assert_eq!(
            context.diagnostic_context.get("test").map(String::as_str),
            Some("save")
        );
    }

    #[test]
    fn a_rich_payload_prints_its_fields_before_the_screen() {
        let rendered = format_data(&json!({
            "cwd": "/tmp",
            "cols": 80,
            "timeouts": { "text": 300 },
            "text": "screen",
        }));
        assert!(rendered.contains("cwd: /tmp"), "{rendered}");
        assert!(rendered.contains("cols: 80"), "{rendered}");
        assert!(rendered.contains("timeouts: {\"text\":300}"), "{rendered}");
        assert!(rendered.ends_with("screen"), "{rendered}");
    }

    #[test]
    fn a_payload_without_text_falls_back_to_json() {
        let rendered = format_data(&json!({ "pid": 42 }));
        assert!(rendered.contains("\"pid\""), "{rendered}");
        assert!(rendered.contains("42"), "{rendered}");
    }

    #[test]
    fn ready_flag_resolves_the_paired_switches() {
        assert_eq!(ready_flag(false, false), None);
        assert_eq!(ready_flag(true, false), Some(true));
        assert_eq!(ready_flag(false, true), Some(false));
    }

    #[test]
    fn empty_recording_paths_are_rejected_before_resolution() {
        let error = resolve_client_path("  ".to_string()).unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn spawn_cwd_and_artifact_paths_are_resolved_before_ipc() {
        for args in [
            vec!["tui-test", "open"],
            vec!["tui-test", "open", "--cwd", "relative"],
            vec!["tui-test", "run", "--cwd", "relative", "--", "program"],
        ] {
            let cli = Cli::try_parse_from(&args).unwrap();
            let Request::Open { cwd, trace, .. } = build_request(cli.command.unwrap()).unwrap()
            else {
                panic!("expected open request");
            };
            let expected = if args.contains(&"--cwd") {
                "relative"
            } else {
                "."
            };
            assert_eq!(
                std::path::PathBuf::from(cwd.unwrap()),
                std::path::absolute(expected).unwrap()
            );
            assert!(trace.directory.is_absolute());
        }
        for args in [
            vec!["tui-test", "screenshot", "relative.svg"],
            vec!["tui-test", "screenshot", "--out", "relative.svg"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Request::Screenshot { path, .. } = build_request(cli.command.unwrap()).unwrap()
            else {
                panic!("expected screenshot request");
            };
            assert_eq!(
                std::path::PathBuf::from(path.unwrap()),
                std::path::absolute("relative.svg").unwrap()
            );
        }
    }

    #[test]
    fn find_text_maps_selector_options_to_the_protocol() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "find",
            "text",
            "Save",
            "--after-text",
            "Settings",
            "--whitespace",
            "normalize",
            "--nth",
            "1",
        ])
        .unwrap();
        let Request::FindLocator { query } = build_request(cli.command.expect("command")).unwrap()
        else {
            panic!("expected find text request");
        };
        let LocatorSelector::Text(selector) = &query.selector else {
            panic!("expected text locator");
        };
        assert_eq!(selector.scope.after.as_ref().unwrap().text, "Settings");
        assert_eq!(selector.whitespace, WhitespaceMode::Normalize);
        assert_eq!(query.occurrence, MatchOccurrence::Nth(1));
    }

    #[test]
    fn click_text_maps_to_one_strict_action_request() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "click",
            "text",
            "Save",
            "--after-text",
            "Settings",
            "--fg",
            "2",
            "--button",
            "right",
            "--ctrl",
            "--shift",
        ])
        .unwrap();
        let Request::ClickLocator {
            query,
            button,
            clicks,
            ..
        } = build_request(cli.command.expect("command")).unwrap()
        else {
            panic!("expected click locator request");
        };
        let LocatorSelector::Text(selector) = &query.selector else {
            panic!("expected text locator");
        };
        assert_eq!(selector.scope.after.as_ref().unwrap().text, "Settings");
        assert_eq!(query.occurrence, MatchOccurrence::Any);
        assert_eq!(query.style.foreground.as_deref(), Some("2"));
        assert_eq!(button, 22);
        assert_eq!(clicks, 1);
    }

    #[test]
    fn wait_text_command_maps_to_a_locator_wait() {
        let cli = Cli::try_parse_from(["tui-test", "wait", "text", "Saving"]).unwrap();
        assert!(matches!(
            build_request(cli.command.unwrap()).unwrap(),
            Request::WaitLocator {
                not: false,
                timeout_ms: None,
                ..
            }
        ));
    }

    #[test]
    fn removed_locator_command_is_rejected() {
        assert!(Cli::try_parse_from(["tui-test", "locator", "Save"]).is_err());
    }

    #[test]
    fn expect_text_maps_style_options_to_the_protocol() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "expect",
            "text",
            "Warning",
            "--bold",
            "--underline-style",
            "curly",
        ])
        .unwrap();
        let Request::ExpectLocator { query, .. } =
            build_request(cli.command.expect("command")).unwrap()
        else {
            panic!("expected styled text request");
        };
        assert!(matches!(&query.selector, LocatorSelector::Text(_)));
        assert_eq!(query.occurrence, MatchOccurrence::Any);
        assert_eq!(query.style.bold, Some(true));
        assert_eq!(query.style.underline_style.as_deref(), Some("curly"));
    }

    #[test]
    fn daemon_with_same_package_and_pre_style_protocol_is_incompatible() {
        let status = Response::with(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": 5,
        }));
        let identity = DaemonIdentity {
            version: daemon_version(&status),
            protocol_version: daemon_protocol_version(&status),
        };
        assert!(
            !identity.compatible(),
            "ensure_daemon must replace a daemon that cannot apply Open style"
        );
        assert!(DaemonIdentity {
            protocol_version: protocol::PROTOCOL_VERSION,
            ..identity
        }
        .compatible());
    }

    #[test]
    fn daemon_identity_uses_package_version() {
        assert_eq!(
            daemon_version(&Response::with(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "protocol_version": protocol::PROTOCOL_VERSION,
            }))),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            daemon_protocol_version(&Response::with(json!({
                "protocol_version": protocol::PROTOCOL_VERSION,
            }))),
            protocol::PROTOCOL_VERSION
        );
        assert_eq!(daemon_version(&Response::with(json!({}))), "unknown");
        assert_eq!(daemon_protocol_version(&Response::with(json!({}))), 0);
        assert!(!DaemonIdentity {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: 0,
        }
        .compatible());
    }

    #[test]
    fn daemon_lifecycle_lock_serializes_and_recovers_unowned_files() {
        let root = unique_test_path("daemon-lock");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("work.pid.lock");
        let first = DaemonLock::acquire_path(path.clone(), Duration::from_secs(1)).unwrap();

        let blocked_path = path.clone();
        let blocked = std::thread::spawn(move || {
            let start = Instant::now();
            let lock = DaemonLock::acquire_path(blocked_path, Duration::from_secs(1)).unwrap();
            (start.elapsed(), lock)
        });
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        let (waited, second) = blocked.join().unwrap();
        assert!(waited >= Duration::from_millis(75));
        drop(second);

        std::fs::write(&path, b"stale").unwrap();
        let recovered = DaemonLock::acquire_path(path.clone(), Duration::from_secs(1)).unwrap();
        drop(recovered);
        assert!(
            path.exists(),
            "releasing ownership must not unlink the lock"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_live_lifecycle_lock_is_not_stolen_based_on_file_age() {
        let root = unique_test_path("live-daemon-lock");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("work.pid.lock");
        let first = DaemonLock::acquire_path(path.clone(), Duration::from_secs(1)).unwrap();
        first
            ._file
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        assert!(DaemonLock::acquire_path(path.clone(), Duration::from_millis(75)).is_err());
        drop(first);
        drop(DaemonLock::acquire_path(path, Duration::from_secs(1)).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dropping_an_old_lock_does_not_remove_a_replacement() {
        let root = unique_test_path("replaced-daemon-lock");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("work.pid.lock");
        let first = DaemonLock::acquire_path(path.clone(), Duration::from_secs(1)).unwrap();
        std::fs::rename(&path, root.join("old.lock")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        drop(first);
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_lock_is_released_when_the_owner_exits() {
        const CHILD_PATH: &str = "TUI_TEST_LOCK_TEST_CHILD";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let path = std::path::PathBuf::from(path);
            let _lock = DaemonLock::acquire_path(path.clone(), Duration::from_secs(1)).unwrap();
            std::fs::write(path.with_extension("ready"), b"ready").unwrap();
            loop {
                std::thread::park();
            }
        }
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let root = unique_test_path("crashed-daemon-lock");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("work.pid.lock");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::lifecycle_lock_is_released_when_the_owner_exits",
            ])
            .env(CHILD_PATH, &path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let child = ChildGuard(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.with_extension("ready").exists() {
            assert!(Instant::now() < deadline, "child never acquired the lock");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(DaemonLock::acquire_path(path.clone(), Duration::ZERO).is_err());
        drop(child);
        drop(DaemonLock::acquire_path(path, Duration::from_secs(1)).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn daemon_restart_waits_for_shutdown_before_spawning() {
        let events = RefCell::new(Vec::new());
        restart_daemon_with(
            "work",
            "0.0.0-old",
            false,
            || {
                events.borrow_mut().push("shutdown");
                Ok(())
            },
            |expected| {
                events
                    .borrow_mut()
                    .push(if expected { "started" } else { "stopped" });
                Ok(())
            },
            || {
                events.borrow_mut().push("spawn");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            events.into_inner(),
            ["shutdown", "stopped", "spawn", "started"]
        );
    }
}
