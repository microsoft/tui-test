use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::{Action, Key, Location, ModifiedKey, Modifier, Plan, ScrollDirection, Shell, Warning};
use crate::api::{ErrorKind, KeyAction, Operation, RecordingFormat, RunOptions, TuiTestError};
use crate::render::style::{Padding, Style};
use crate::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RunnerPhase {
    Preflight,
    Execution,
}

/// Source context and the core error category, suitable for future CLI exit codes.
#[derive(Debug, Clone, Serialize)]
pub struct RunnerError {
    pub location: Location,
    pub phase: RunnerPhase,
    pub kind: ErrorKind,
    pub message: String,
    /// Cleanup failures never replace the primary failure.
    pub cleanup_errors: Vec<String>,
}

impl RunnerError {
    fn preflight(location: &Location, message: impl Into<String>) -> Self {
        Self {
            location: location.clone(),
            phase: RunnerPhase::Preflight,
            kind: ErrorKind::Usage,
            message: message.into(),
            cleanup_errors: Vec::new(),
        }
    }

    fn execution(location: &Location, error: TuiTestError) -> Self {
        Self {
            location: location.clone(),
            phase: RunnerPhase::Execution,
            kind: error.kind,
            message: error.message,
            cleanup_errors: Vec::new(),
        }
    }
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)?;
        for error in &self.cleanup_errors {
            write!(f, "; cleanup: {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RunnerError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Geometry {
    pub columns: u16,
    pub rows: u16,
    /// Actual GIF dimensions at native 1x raster scale.
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub output: PathBuf,
    pub shell: PathBuf,
    pub geometry: Geometry,
    pub warnings: Vec<Warning>,
}

struct Prepared {
    summary: Summary,
    options: RunOptions,
}

/// Check the complete plan without a child, output writes, or process mutations.
///
/// The invocation directory and output parent must already exist.
pub fn check(plan: &Plan, invocation_cwd: impl AsRef<Path>) -> Result<Summary, RunnerError> {
    prepare(plan, invocation_cwd.as_ref()).map(|prepared| prepared.summary)
}

/// Execute a preflighted tape in a fresh, owned, in-process session.
///
/// This function does not read project configuration or use the global registry.
/// It closes the child on success and failure. A failed export retains the core
/// recovery cast; its path appears in the error.
pub fn run(plan: &Plan, invocation_cwd: impl AsRef<Path>) -> Result<Summary, RunnerError> {
    run_observed(plan, invocation_cwd.as_ref(), |_, _, _| {})
}

fn run_observed(
    plan: &Plan,
    invocation_cwd: &Path,
    mut observe: impl FnMut(&Session, &Action, bool),
) -> Result<Summary, RunnerError> {
    // Bracket logical tape actions, not recorder calls, so tests can measure
    // visible intervals independently of pause/resume behavior.
    let prepared = prepare(plan, invocation_cwd)?;
    let session = Session::new("tape");
    let mut started = false;
    let mut result = (|| {
        execute(
            &session,
            plan.shell_location(),
            Operation::Run(prepared.options),
        )?;
        execute(
            &session,
            plan.shell_location(),
            Operation::WaitLocator {
                query: crate::LocatorQuery::text("> "),
                not: false,
                timeout_ms: Some(5_000),
            },
        )?;
        if plan.initially_visible() {
            observe(&session, &Action::Show, true);
            start_recording(&session, plan, &prepared.summary)?;
            started = true;
            observe(&session, &Action::Show, false);
        }
        for action in plan.actions() {
            observe(&session, &action.value, true);
            match &action.value {
                Action::Hide => {
                    execute(&session, &action.location, Operation::PauseRecording)?;
                }
                Action::Show if !started => {
                    start_recording(&session, plan, &prepared.summary)?;
                    started = true;
                }
                Action::Show => {
                    execute(&session, &action.location, Operation::ResumeRecording)?;
                }
                value => perform(&session, &action.location, value)?,
            }
            observe(&session, &action.value, false);
        }
        Ok(())
    })();
    // Stop while paused at a hidden EOF. Resume would capture hidden content.
    if started {
        observe(&session, &Action::Hide, true);
        merge_cleanup(
            &mut result,
            execute(&session, &plan.output().location, Operation::StopRecording).map(|_| ()),
        );
        observe(&session, &Action::Hide, false);
    }
    merge_cleanup(
        &mut result,
        session
            .close()
            .map_err(|error| RunnerError::execution(plan.shell_location(), error)),
    );
    result.map(|()| prepared.summary)
}

fn merge_cleanup(result: &mut Result<(), RunnerError>, cleanup: Result<(), RunnerError>) {
    if let Err(error) = cleanup {
        match result {
            Ok(()) => *result = Err(error),
            Err(primary) => primary.cleanup_errors.push(error.to_string()),
        }
    }
}

fn execute(
    session: &Session,
    location: &Location,
    operation: Operation,
) -> Result<crate::OperationResult, RunnerError> {
    session
        .execute(operation)
        .map_err(|error| RunnerError::execution(location, error))
}

fn start_recording(session: &Session, plan: &Plan, summary: &Summary) -> Result<(), RunnerError> {
    execute(
        session,
        &plan.output().location,
        Operation::StartRecording {
            path: summary
                .output
                .to_str()
                .expect("preflight checked UTF-8")
                .into(),
            format: Some(RecordingFormat::Gif),
            fps: Some(plan.settings().framerate),
            speed: Some(plan.settings().playback_speed),
            idle_time_limit: None,
            zoom: Some(0.5),
            background: None,
        },
    )
    .map(|_| ())
}

fn perform(session: &Session, location: &Location, action: &Action) -> Result<(), RunnerError> {
    match action {
        Action::Type { text, delay } => {
            for character in text.chars() {
                execute(
                    session,
                    location,
                    Operation::Write {
                        data: character.to_string(),
                    },
                )?;
                std::thread::sleep(*delay);
            }
        }
        Action::Key { key, delay, count } => {
            for _ in 0..*count {
                execute(
                    session,
                    location,
                    key_operation(vec![key_name(*key).into()]),
                )?;
                std::thread::sleep(*delay);
            }
        }
        Action::ModifiedKeys { modifiers, keys } => {
            execute(
                session,
                location,
                key_operation(modified_tokens(modifiers, keys)),
            )?;
        }
        Action::Scroll {
            direction,
            delay,
            count,
        } => {
            for _ in 0..*count {
                execute(
                    session,
                    location,
                    Operation::Scroll {
                        target: scroll_target(*direction),
                    },
                )?;
                std::thread::sleep(*delay);
            }
        }
        Action::Sleep(duration) => std::thread::sleep(*duration),
        Action::Hide | Action::Show => unreachable!("visibility is managed by run"),
    }
    Ok(())
}

fn scroll_target(direction: ScrollDirection) -> crate::ScrollTarget {
    crate::ScrollTarget::Lines(match direction {
        ScrollDirection::Up => 1,
        ScrollDirection::Down => -1,
    })
}

fn key_operation(keys: Vec<String>) -> Operation {
    Operation::Key {
        keys,
        action: KeyAction::Press,
    }
}

fn key_name(key: Key) -> &'static str {
    match key {
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Enter => "Enter",
        Key::Escape => "Escape",
        Key::Tab => "Tab",
        Key::Space => "Space",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
    }
}

fn modified_tokens(modifiers: &[Modifier], keys: &[ModifiedKey]) -> Vec<String> {
    keys.iter()
        .map(|key| {
            let mut parts: Vec<String> = modifiers
                .iter()
                .map(|modifier| match modifier {
                    Modifier::Ctrl => "Ctrl".into(),
                    Modifier::Alt => "Alt".into(),
                    Modifier::Shift => "Shift".into(),
                })
                .collect();
            let key = match key {
                ModifiedKey::Named(key) => key_name(*key).into(),
                // A literal '+' would produce an empty key after token splitting.
                ModifiedKey::Character('+') => {
                    if !modifiers.contains(&Modifier::Shift) {
                        parts.push("Shift".into());
                    }
                    "=".into()
                }
                ModifiedKey::Character(character) => character.to_string(),
            };
            parts.push(key);
            parts.join("+")
        })
        .collect()
}

fn env_value<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(key, _)| same_env_key(key, name))
        .map(|(_, value)| value.as_str())
}

fn same_env_key(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn set_env(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    env.retain(|(existing, _)| !same_env_key(existing, key));
    env.push((key.into(), value.into()));
}

fn child_environment(plan: &Plan) -> Result<Vec<(String, String)>, RunnerError> {
    let mut env = std::env::vars_os()
        .map(|(key, value)| {
            key.into_string()
                .and_then(|key| value.into_string().map(|value| (key, value)))
                .map_err(|_| {
                    RunnerError::preflight(
                        plan.shell_location(),
                        "the core child environment requires UTF-8 keys and values",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for directive in plan.environment() {
        set_env(&mut env, &directive.value.0, &directive.value.1);
    }
    Ok(env)
}

fn clean_shell(shell: Shell, env: &mut Vec<(String, String)>) -> Vec<String> {
    // These controls override Env deliberately. PATH and other application
    // variables retain the tape's values.
    for (key, value) in [
        ("PS1", "> "),
        ("PS2", ""),
        ("PROMPT", "> "),
        ("RPROMPT", ""),
        ("PROMPT_COMMAND", ""),
        ("HISTFILE", ""),
        ("HISTSIZE", "0"),
        ("SAVEHIST", "0"),
        ("BASH_ENV", ""),
        ("ENV", ""),
        ("INPUTRC", if cfg!(unix) { "/dev/null" } else { "NUL" }),
    ] {
        set_env(env, key, value);
    }
    let args: &[&str] = match shell {
        Shell::Bash => &["--noprofile", "--norc", "-i"],
        Shell::Zsh => &["-d", "-f", "-i"],
        Shell::Fish => &[
            "--no-config", "--private", "-i", "-C",
            "function fish_prompt; printf '> '; end; function fish_right_prompt; end; set -g fish_greeting",
        ],
        Shell::Powershell | Shell::Pwsh => &[
            "-NoLogo", "-NoProfile", "-NoExit", "-Command",
            "function global:prompt { '> ' }; if (Get-Command Set-PSReadLineOption -ErrorAction SilentlyContinue) { Set-PSReadLineOption -HistorySaveStyle SaveNothing }",
        ],
        Shell::Cmd => &["/D", "/Q", "/K", "prompt $G$S"],
    };
    args.iter().map(|value| (*value).into()).collect()
}

fn child_cwd(cwd: &Path) -> String {
    let cwd = cwd.to_str().expect("preflight checked UTF-8");
    // Keep canonical paths for file operations, but use ordinary DOS/UNC paths
    // for child processes. PowerShell and Node do not accept verbatim cwd paths.
    #[cfg(windows)]
    {
        if let Some(path) = cwd.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{path}");
        }
        if let Some(path) = cwd.strip_prefix(r"\\?\") {
            return path.into();
        }
    }
    cwd.into()
}

fn prepare(plan: &Plan, invocation_cwd: &Path) -> Result<Prepared, RunnerError> {
    let output_location = &plan.output().location;
    let invalid = |message| RunnerError::preflight(output_location, message);
    if !cfg!(feature = "recording-raster") {
        return Err(invalid(
            "tape GIF execution requires the tui-test 'recording-raster' feature".into(),
        ));
    }
    let cwd = invocation_cwd.canonicalize().map_err(|error| {
        invalid(format!(
            "cannot resolve invocation directory {}: {error}",
            invocation_cwd.display()
        ))
    })?;
    if !cwd.is_dir() {
        return Err(invalid(format!(
            "invocation directory {} is not a directory",
            cwd.display()
        )));
    }
    let output = cwd.join(&plan.output().value);
    let parent = output.parent().expect("absolute output has a parent");
    let metadata = parent.metadata().map_err(|error| {
        invalid(format!(
            "output parent {} must already exist: {error}",
            parent.display()
        ))
    })?;
    if !metadata.is_dir() || cfg!(unix) && metadata.permissions().readonly() {
        return Err(invalid(format!(
            "output parent {} must be a writable directory",
            parent.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let parent_name = std::ffi::CString::new(parent.as_os_str().as_bytes())
            .expect("validated tape paths have no NUL");
        // Check directory access without creating a probe file.
        if unsafe { libc::access(parent_name.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            return Err(invalid(format!(
                "output parent {} is not writable: {}",
                parent.display(),
                std::io::Error::last_os_error()
            )));
        }
    }
    if let Some(metadata) = path_metadata(&output, output_location)? {
        if !metadata.is_file() || metadata.permissions().readonly() {
            return Err(invalid(format!(
                "output {} must be a writable regular file, not a symlink or directory",
                output.display()
            )));
        }
    }
    for suffix in [".tui-test.cast", ".tui-test.tmp"] {
        let mut path = output.as_os_str().to_os_string();
        path.push(suffix);
        if path_metadata(Path::new(&path), output_location)?.is_some() {
            return Err(invalid(format!(
                "refusing to overwrite recording work file {}",
                Path::new(&path).display()
            )));
        }
    }
    if output.to_str().is_none() || cwd.to_str().is_none() {
        return Err(invalid(
            "invocation directory and output must use UTF-8 paths".into(),
        ));
    }
    let mut env = child_environment(plan)?;
    let find = |name| {
        crate::shell::which_in(
            name,
            env_value(&env, "PATH").map(std::ffi::OsStr::new),
            env_value(&env, "PATHEXT").map(std::ffi::OsStr::new),
            &cwd,
        )
    };
    for requirement in plan.requirements() {
        if find(&requirement.value).is_none() {
            return Err(RunnerError::preflight(&requirement.location, format!(
                "Require {:?} is not an executable under the effective child PATH/PATHEXT and invocation directory",
                requirement.value
            )));
        }
    }
    let shell = find(plan.settings().shell.program()).ok_or_else(|| {
        RunnerError::preflight(plan.shell_location(), format!(
            "Shell {} is not an executable under the effective child PATH/PATHEXT and invocation directory",
            plan.settings().shell.program()
        ))
    })?;
    let program = shell
        .to_str()
        .ok_or_else(|| {
            RunnerError::preflight(
                plan.shell_location(),
                "Shell executable must use a UTF-8 path",
            )
        })?
        .to_string();
    let args = clean_shell(plan.settings().shell, &mut env);
    let settings = plan.settings();
    let style = tape_style(plan);
    style.validate().map_err(|error| {
        let name = if error.contains("padding") {
            "Padding"
        } else if error.contains("font.family") {
            "FontFamily"
        } else {
            "FontSize"
        };
        RunnerError::preflight(plan.setting_location(name), error)
    })?;
    let (width, height) = raster_dimensions(plan, &style)?;
    Duration::try_from_secs_f64(1.0 / settings.playback_speed).map_err(|_| {
        RunnerError::preflight(
            plan.setting_location("PlaybackSpeed"),
            "PlaybackSpeed is too small",
        )
    })?;
    let mut duration = 0.0;
    let mut visible = plan.initially_visible();
    for action in plan.actions() {
        let tokens = match &action.value {
            Action::Key { key, .. } => vec![key_name(*key).into()],
            Action::ModifiedKeys { modifiers, keys } => modified_tokens(modifiers, keys),
            _ => Vec::new(),
        };
        for token in tokens {
            crate::input::keys::token_to_presses(&token, KeyAction::Press)
                .and_then(|_| crate::input::keys::token_to_seq(&token))
                .map_err(|error| {
                    RunnerError::preflight(
                        &action.location,
                        format!("unsupported key {token:?}: {error}"),
                    )
                })?;
        }
        match action.value {
            Action::Hide => visible = false,
            Action::Show => visible = true,
            _ => {}
        }
        duration += if visible {
            match &action.value {
                Action::Type { text, delay } => delay.as_secs_f64() * text.chars().count() as f64,
                Action::Key { delay, count, .. } | Action::Scroll { delay, count, .. } => {
                    delay.as_secs_f64() * *count as f64
                }
                Action::Sleep(delay) => delay.as_secs_f64(),
                _ => 0.0,
            }
        } else {
            0.0
        };
        Duration::try_from_secs_f64(duration / settings.playback_speed).map_err(|_| {
            RunnerError::preflight(
                &action.location,
                "action durations exceed the recording timeline range at this PlaybackSpeed",
            )
        })?;
    }
    Ok(Prepared {
        summary: Summary {
            output,
            shell,
            geometry: Geometry {
                columns: settings.columns,
                rows: settings.rows,
                width,
                height,
            },
            warnings: plan.warnings().to_vec(),
        },
        options: RunOptions {
            program,
            args,
            env,
            cwd: Some(child_cwd(&cwd)),
            cols: settings.columns,
            rows: settings.rows,
            backend: Default::default(),
            profile: Default::default(),
            wait_ready: Some(false),
            restart: false,
            timeouts: Default::default(),
            recording: Default::default(),
            style,
        },
    })
}

fn path_metadata(
    path: &Path,
    location: &Location,
) -> Result<Option<std::fs::Metadata>, RunnerError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RunnerError::preflight(
            location,
            format!("cannot inspect output path {}: {error}", path.display()),
        )),
    }
}

fn tape_style(plan: &Plan) -> Style {
    let mut style = Style {
        font_size: plan.settings().font_size,
        canvas_padding: Padding::Uniform(0),
        content_padding: Padding::Uniform(plan.settings().padding),
        ..Default::default()
    };
    style.font.family = plan.settings().font_family.clone();
    style.window.title_bar = false;
    style.window.traffic_lights = false;
    style.border.width = 0.0;
    style.border.radius = 0.0;
    style.shadow.enabled = false;
    style.cursor.blink = plan.settings().cursor_blink;
    style
}

#[cfg(feature = "recording-raster")]
fn raster_dimensions(plan: &Plan, style: &Style) -> Result<(u32, u32), RunnerError> {
    crate::render::font::validate_requested_family(&style.font).map_err(|error| {
        RunnerError::preflight(plan.setting_location("FontFamily"), error.to_string())
    })?;
    let renderer = crate::render::raster::GridRenderer::with_zoom(
        plan.settings().columns,
        usize::from(plan.settings().rows),
        1.0,
        style.clone(),
    )
    .map_err(|error| {
        RunnerError::preflight(
            &plan.output().location,
            format!("invalid GIF geometry: {error}"),
        )
    })?;
    let dimensions = renderer.dimensions();
    if dimensions.0 > u32::from(u16::MAX) || dimensions.1 > u32::from(u16::MAX) {
        return Err(RunnerError::preflight(
            &plan.output().location,
            "GIF width and height must not exceed 65535 pixels",
        ));
    }
    Ok(dimensions)
}

#[cfg(not(feature = "recording-raster"))]
fn raster_dimensions(plan: &Plan, _style: &Style) -> Result<(u32, u32), RunnerError> {
    Err(RunnerError::preflight(
        &plan.output().location,
        "tape GIF execution requires the tui-test 'recording-raster' feature",
    ))
}

#[cfg(test)]
mod tests;
