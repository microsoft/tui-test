use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use super::{
    Command, Error, ErrorKind, Key, Located, Location, ModifiedKey, Modifier, ScrollDirection,
    Setting, Tape,
};

/// Controlled shells only. This is a name, never a command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Pwsh,
    Cmd,
}

impl Shell {
    pub fn program(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
            Self::Pwsh => "pwsh",
            Self::Cmd => "cmd",
        }
    }

    fn for_host(name: &str, location: &Location) -> Result<Self, Error> {
        match name {
            "bash" if cfg!(unix) => Ok(Self::Bash),
            "zsh" if cfg!(unix) => Ok(Self::Zsh),
            "fish" if cfg!(unix) => Ok(Self::Fish),
            "powershell" if cfg!(windows) => Ok(Self::Powershell),
            "pwsh" if cfg!(windows) => Ok(Self::Pwsh),
            "cmd" if cfg!(windows) => Ok(Self::Cmd),
            _ => Err(Error::new(
                location,
                ErrorKind::Unsupported,
                if cfg!(windows) {
                    "Shell must be powershell, pwsh, or cmd on Windows; paths and arguments are not supported"
                } else {
                    "Shell must be bash, zsh, or fish on Unix; paths and arguments are not supported"
                },
            )),
        }
    }
}

/// Effective initial values. These do not inherit ordinary session defaults.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub shell: Shell,
    pub rows: u16,
    pub columns: u16,
    pub font_family: String,
    pub font_size: f32,
    pub framerate: u8,
    pub typing_speed: Duration,
    pub playback_speed: f64,
    pub padding: u32,
    /// Permission to animate. A terminal application can still select a steady cursor.
    pub cursor_blink: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shell: if cfg!(windows) {
                Shell::Powershell
            } else {
                Shell::Bash
            },
            rows: 24,
            columns: 80,
            font_family: "JetBrains Mono".into(),
            font_size: 22.0,
            framerate: 50,
            typing_speed: Duration::from_millis(50),
            playback_speed: 1.0,
            padding: 60,
            cursor_blink: true,
        }
    }
}

/// Runtime work only. Delays remain per step, including the final step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Action {
    Type {
        text: String,
        delay: Duration,
    },
    Key {
        key: Key,
        delay: Duration,
        count: u64,
    },
    ModifiedKeys {
        modifiers: Vec<Modifier>,
        keys: Vec<ModifiedKey>,
    },
    Scroll {
        direction: ScrollDirection,
        delay: Duration,
        count: u64,
    },
    Sleep(Duration),
    Hide,
    Show,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WarningKind {
    NativeGrid,
    CursorBlinkPermission,
    IgnoredLateSetting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    pub location: Location,
    pub kind: WarningKind,
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// An owned executable plan. Only `compile` can construct it.
///
/// The plan contains one GIF target, supported actions, validated values, and
/// at least one visible action or a transition from hidden to visible.
/// A runner must not resume a paused recording at EOF.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    output: Located<std::path::PathBuf>,
    settings: Settings,
    #[serde(skip)]
    setting_locations: std::collections::BTreeMap<&'static str, Location>,
    shell_location: Location,
    environment: Vec<Located<(String, String)>>,
    requirements: Vec<Located<String>>,
    actions: Vec<Located<Action>>,
    initially_visible: bool,
    warnings: Vec<Warning>,
}

impl Plan {
    pub(super) fn setting_location(&self, name: &str) -> &Location {
        self.setting_locations
            .get(name)
            .unwrap_or(&self.output.location)
    }

    /// The path is relative to the invocation directory, not the tape file.
    pub fn output(&self) -> &Located<std::path::PathBuf> {
        &self.output
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The last Shell directive, or the root location for the default shell.
    pub fn shell_location(&self) -> &Location {
        &self.shell_location
    }

    /// All Env directives in source order. The last duplicate key wins.
    /// A runner must apply them to the child, never the process environment.
    pub fn environment(&self) -> &[Located<(String, String)>] {
        &self.environment
    }

    pub fn requirements(&self) -> &[Located<String>] {
        &self.requirements
    }

    pub fn actions(&self) -> &[Located<Action>] {
        &self.actions
    }

    /// If false, do not start frame capture until the first Show action.
    pub fn initially_visible(&self) -> bool {
        self.initially_visible
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// Validate the whole tape before a runner starts a child or writes output.
///
/// Resolve Source first. This function performs no I/O or executable lookup.
/// Runtime TypingSpeed directives become resolved delays on subsequent actions.
pub fn compile(tape: &Tape) -> Result<Plan, Error> {
    let mut settings = Settings::default();
    let mut setting_locations = std::collections::BTreeMap::new();
    let mut shell_location = tape.origin.clone();
    let mut environment = Vec::new();
    let mut requirements = Vec::new();
    let mut actions = Vec::new();
    let mut output = None;
    let mut warnings = Vec::new();
    let mut initial_section = true;
    let mut typing_speed = settings.typing_speed;
    let mut grid_location = tape.origin.clone();
    let mut blink_location = tape.origin.clone();
    for command in &tape.commands {
        let location = &command.location;
        let unsupported = |message| Error::new(location, ErrorKind::Unsupported, message);
        let action = match &command.value {
            Command::Output(path) => {
                if !initial_section {
                    return Err(unsupported("late Output is not supported; put Output in the initial Set/Output/Require section"));
                }
                if output.is_some() {
                    return Err(unsupported(
                        "exactly one Output is supported; multiple targets are not supported",
                    ));
                }
                // Path components normalize trailing separators and ".", so check
                // the literal final component before accepting a file extension.
                let filename = path
                    .to_str()
                    .and_then(|value| value.rsplit(std::path::is_separator).next());
                if matches!(filename, None | Some("" | "." | "..")) {
                    return Err(unsupported(
                        "Output requires a .gif filename; frame directories are not supported",
                    ));
                }
                if Path::new(path).extension().and_then(|value| value.to_str()) != Some("gif") {
                    return Err(unsupported("only an explicit .gif Output is supported"));
                }
                output = Some(Located {
                    location: location.clone(),
                    value: path.clone(),
                });
                continue;
            }
            Command::Set(setting) => {
                match setting {
                    Setting::Shell(name) => {
                        settings.shell = Shell::for_host(name, location)?;
                        shell_location = location.clone();
                    }
                    Setting::Rows(_)
                    | Setting::Columns(_)
                    | Setting::FontFamily(_)
                    | Setting::FontSize(_)
                    | Setting::Framerate(_)
                    | Setting::PlaybackSpeed(_)
                    | Setting::TypingSpeed(_)
                    | Setting::Padding(_)
                    | Setting::CursorBlink(_) => {
                        if !initial_section && !matches!(setting, Setting::TypingSpeed(_)) {
                            warnings.push(Warning {
                                location: location.clone(),
                                kind: WarningKind::IgnoredLateSetting,
                                message: format!("Set {} is ignored after the initial section; move it above all actions and Env directives", setting.name()),
                            });
                            continue;
                        }
                        if initial_section {
                            setting_locations.insert(setting.name(), location.clone());
                        }
                        match setting {
                            Setting::Rows(value) => {
                                settings.rows = *value;
                                grid_location = location.clone();
                            }
                            Setting::Columns(value) => {
                                settings.columns = *value;
                                grid_location = location.clone();
                            }
                            Setting::FontFamily(value) => settings.font_family = value.clone(),
                            Setting::FontSize(value) => settings.font_size = *value,
                            Setting::Framerate(value) => settings.framerate = *value,
                            Setting::PlaybackSpeed(value) => settings.playback_speed = *value,
                            Setting::TypingSpeed(value) => {
                                typing_speed = *value;
                                if initial_section {
                                    settings.typing_speed = *value;
                                }
                            }
                            Setting::Padding(value) => settings.padding = *value,
                            Setting::CursorBlink(value) => {
                                settings.cursor_blink = *value;
                                blink_location = location.clone();
                            }
                            _ => unreachable!("supported setting"),
                        }
                    }
                    _ => {
                        return Err(unsupported(&format!(
                            "Set {} is recognized but not supported",
                            setting.name()
                        )))
                    }
                }
                continue;
            }
            Command::Require(name) => {
                if !initial_section {
                    return Err(unsupported(
                        "Require is supported only in the initial Set/Output/Require section",
                    ));
                }
                requirements.push(Located {
                    location: location.clone(),
                    value: name.clone(),
                });
                continue;
            }
            Command::Env { key, value } => {
                environment.push(Located {
                    location: location.clone(),
                    value: (key.clone(), value.clone()),
                });
                // VHS hoists Env but still ends its initial settings scan here.
                initial_section = false;
                continue;
            }
            Command::Type { text, delay } => Action::Type {
                text: text.clone(),
                delay: delay.unwrap_or(typing_speed),
            },
            Command::Key { key, delay, count } => Action::Key {
                key: *key,
                delay: delay.unwrap_or(typing_speed),
                count: *count,
            },
            Command::ModifiedKeys { modifiers, keys } => Action::ModifiedKeys {
                modifiers: modifiers.clone(),
                keys: keys.clone(),
            },
            Command::Scroll {
                direction,
                delay,
                count,
            } => Action::Scroll {
                direction: *direction,
                delay: delay.unwrap_or(typing_speed),
                count: *count,
            },
            Command::Sleep(value) => Action::Sleep(*value),
            Command::Hide => Action::Hide,
            Command::Show => Action::Show,
            Command::Source(_) => {
                return Err(Error::new(
                    location,
                    ErrorKind::Source,
                    "unresolved Source; call resolve_sources or load before compile",
                ))
            }
            Command::Wait { .. } => {
                return Err(unsupported("Wait is recognized but not supported"))
            }
            Command::Screenshot(_) => {
                return Err(unsupported("Screenshot is recognized but not supported"))
            }
            Command::Copy(_) => return Err(unsupported("Copy is recognized but not supported")),
            Command::Paste => return Err(unsupported("Paste is recognized but not supported")),
        };
        initial_section = false;
        actions.push(Located {
            location: location.clone(),
            value: action,
        });
    }
    let output = output.ok_or_else(|| {
        Error::new(
            &tape.origin,
            ErrorKind::InvalidValue,
            "an explicit .gif Output is required",
        )
    })?;
    let initially_visible = !actions
        .first()
        .is_some_and(|action| matches!(action.value, Action::Hide));
    let mut visible = initially_visible;
    let mut has_visible_work = false;
    // Drop redundant visibility changes, including the initial Hide. The flag
    // above prevents a runner from capturing a frame before hidden setup.
    actions.retain(|action| {
        match &action.value {
            Action::Hide => {
                if !visible {
                    return false;
                }
                visible = false;
            }
            Action::Show => {
                if visible {
                    return false;
                }
                visible = true;
                has_visible_work = true;
            }
            Action::Type { text, .. } => has_visible_work |= visible && !text.is_empty(),
            Action::Key { count, .. } | Action::Scroll { count, .. } => {
                has_visible_work |= visible && *count > 0
            }
            Action::Sleep(duration) => has_visible_work |= visible && !duration.is_zero(),
            Action::ModifiedKeys { .. } => has_visible_work |= visible,
        }
        true
    });
    if !has_visible_work {
        return Err(Error::new(
            &tape.origin,
            ErrorKind::InvalidValue,
            "tape has no visible work; add a visible action or Show after hidden setup",
        ));
    }
    warnings.insert(0, Warning {
        location: grid_location,
        kind: WarningKind::NativeGrid,
        message: "native terminal rows and columns replace VHS default pixel geometry; output is not pixel-identical to VHS".into(),
    });
    if settings.cursor_blink {
        warnings.push(Warning {
            location: blink_location,
            kind: WarningKind::CursorBlinkPermission,
            message: "CursorBlink true permits animation but does not force it; the application can select a steady cursor (core default: blink enabled, 500ms)".into(),
        });
    }
    Ok(Plan {
        output,
        settings,
        setting_locations,
        shell_location,
        environment,
        requirements,
        actions,
        initially_visible,
        warnings,
    })
}
