use clap::{Args, Parser, Subcommand};

use tui_test::config::{DEFAULT_COLS, DEFAULT_ROWS};
use tui_test::shell::Shell;
use tui_test::{Backend, RecordingFormat, Timeouts};

#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum BackendArg {
    Alacritty,
    Ghostty,
    Rio,
    Xtermjs,
}

impl From<BackendArg> for Backend {
    fn from(backend: BackendArg) -> Self {
        match backend {
            BackendArg::Alacritty => Backend::Alacritty,
            BackendArg::Ghostty => Backend::Ghostty,
            BackendArg::Rio => Backend::Rio,
            BackendArg::Xtermjs => Backend::Xtermjs,
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum ShellArg {
    Bash,
    Powershell,
    Pwsh,
    Cmd,
    Fish,
    Zsh,
    Xonsh,
    Elvish,
    Nushell,
}

impl From<ShellArg> for Shell {
    fn from(shell: ShellArg) -> Self {
        match shell {
            ShellArg::Bash => Shell::Bash,
            ShellArg::Powershell => Shell::Powershell,
            ShellArg::Pwsh => Shell::Pwsh,
            ShellArg::Cmd => Shell::Cmd,
            ShellArg::Fish => Shell::Fish,
            ShellArg::Zsh => Shell::Zsh,
            ShellArg::Xonsh => Shell::Xonsh,
            ShellArg::Elvish => Shell::Elvish,
            ShellArg::Nushell => Shell::Nushell,
        }
    }
}

/// Which terminal profile a session runs with.
#[derive(Args, Clone, Default)]
pub struct ProfileArgs {
    /// Config file to read (default: ./tui-test.toml, then the platform config
    /// directory, then ~/.tui-test/tui-test.toml).
    #[arg(long, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,
    /// Named profile from the config file (default: `default`).
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,
}

impl ProfileArgs {
    /// Resolve to concrete settings. Done here, in the client, because the
    /// daemon is long-lived and shared and so has no working directory to
    /// resolve a project-local config against.
    pub fn resolve(&self) -> anyhow::Result<tui_test::profile::Settings> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        tui_test::profile::resolve_settings(self.config.as_deref(), self.profile.as_deref(), &cwd)
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum RecordingFormatArg {
    Apng,
    Gif,
    Mp4,
    Cast,
}

impl From<RecordingFormatArg> for RecordingFormat {
    fn from(format: RecordingFormatArg) -> Self {
        match format {
            RecordingFormatArg::Apng => RecordingFormat::Apng,
            RecordingFormatArg::Gif => RecordingFormat::Gif,
            RecordingFormatArg::Mp4 => RecordingFormat::Mp4,
            RecordingFormatArg::Cast => RecordingFormat::Cast,
        }
    }
}

/// Per-class default timeouts for a session, in milliseconds.
#[derive(Args, Clone, Copy, Default)]
pub struct TimeoutArgs {
    /// Default timeout for `expect text` / `wait text` (default 5000).
    #[arg(long = "timeout-text", value_name = "MS")]
    pub text: Option<u64>,
    /// Default timeout for `wait idle` (default 5000).
    #[arg(long = "timeout-idle", value_name = "MS")]
    pub idle: Option<u64>,
    /// Default timeout for `wait command` / `expect exit-code` (default 30000).
    #[arg(long = "timeout-command", value_name = "MS")]
    pub command: Option<u64>,
    /// Default timeout for `wait exit` (default 30000).
    #[arg(long = "timeout-exit", value_name = "MS")]
    pub exit: Option<u64>,
    /// Default timeout for `wait ready` (default 30000), and for the prompt
    /// wait `open` performs — which otherwise caps itself at 8000.
    #[arg(long = "timeout-ready", value_name = "MS")]
    pub ready: Option<u64>,
}

impl From<TimeoutArgs> for Timeouts {
    fn from(args: TimeoutArgs) -> Self {
        Timeouts {
            text: args.text,
            idle: args.idle,
            command: args.command,
            exit: args.exit,
            ready: args.ready,
        }
    }
}

#[derive(Parser)]
#[command(name = "tui-test", version, about = "Headless terminal cli + daemon")]
pub struct Cli {
    /// Target a named session (env: TUI_TEST_SESSION).
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Start the daemon with a verbose data log at ~/.tui-test/<session>.log.
    /// Records all PTY input/output. Only takes effect when the daemon starts.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Spawn a shell session (auto-starts the daemon).
    Open {
        /// Shell to launch (defaults to the platform shell).
        #[arg(long, value_enum)]
        shell: Option<ShellArg>,
        /// Terminal emulator to use (defaults to alacritty).
        #[arg(long, value_enum)]
        backend: Option<BackendArg>,
        /// Terminal width in columns.
        #[arg(long, default_value_t = DEFAULT_COLS)]
        cols: u16,
        /// Terminal height in rows.
        #[arg(long, default_value_t = DEFAULT_ROWS)]
        rows: u16,
        /// Working directory for the session.
        #[arg(long)]
        cwd: Option<String>,
        /// Environment overrides as KEY=VALUE (repeatable).
        #[arg(long = "env")]
        env: Vec<String>,
        /// Block until the shell reports a ready prompt (the default), or
        /// return as soon as it is spawned with --no-wait-ready.
        #[arg(long)]
        wait_ready: bool,
        /// Return as soon as the shell is spawned, without waiting for a prompt.
        #[arg(long, conflicts_with = "wait_ready")]
        no_wait_ready: bool,
        /// Replace a live session instead of reusing it.
        #[arg(long, visible_alias = "force")]
        restart: bool,
        #[command(flatten)]
        profile: ProfileArgs,
        #[command(flatten)]
        timeouts: TimeoutArgs,
    },
    /// Spawn a session running a program directly.
    Run {
        /// Program to run.
        program: String,
        /// Arguments passed to the program.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Terminal emulator to use (defaults to alacritty).
        #[arg(long, value_enum)]
        backend: Option<BackendArg>,
        /// Terminal width in columns.
        #[arg(long, default_value_t = DEFAULT_COLS)]
        cols: u16,
        /// Terminal height in rows.
        #[arg(long, default_value_t = DEFAULT_ROWS)]
        rows: u16,
        /// Working directory for the session.
        #[arg(long)]
        cwd: Option<String>,
        /// Environment overrides as KEY=VALUE (repeatable).
        #[arg(long = "env")]
        env: Vec<String>,
        /// Block until the program reports a ready prompt (off by default;
        /// only meaningful for programs with shell integration).
        #[arg(long)]
        wait_ready: bool,
        /// Return as soon as the program is spawned (the default).
        #[arg(long, conflicts_with = "wait_ready")]
        no_wait_ready: bool,
        /// Replace a live session instead of reusing it.
        #[arg(long, visible_alias = "force")]
        restart: bool,
        #[command(flatten)]
        profile: ProfileArgs,
        #[command(flatten)]
        timeouts: TimeoutArgs,
    },
    /// Close the current session (or all sessions).
    Close {
        /// Close every session, not just the current one.
        #[arg(long)]
        all: bool,
    },
    /// List active sessions.
    Sessions,
    /// Start, inspect, or stop a session's daemon.
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
    /// Print cwd, size, cursor, last command/exit, and a text snapshot.
    State,
    /// Print the terminal text.
    Text {
        /// Include scrollback, not just the visible viewport.
        #[arg(long)]
        full: bool,
    },
    /// Capture a screenshot: terminal text to stdout, or a full-color SVG image
    /// when an output path is given (crisp at any zoom).
    Screenshot {
        /// Write an SVG image to this path (alias for --out).
        path: Option<String>,
        /// Write an SVG image to this path.
        #[arg(short, long)]
        out: Option<String>,
        /// Include scrollback, not just the visible viewport.
        #[arg(long)]
        full: bool,
        /// Scale the SVG dimensions while keeping the same terminal cells.
        #[arg(long)]
        zoom: Option<f64>,
    },
    /// Start or stop an animated terminal recording.
    Record {
        #[command(subcommand)]
        cmd: RecordCmd,
    },
    /// Dump cell attributes for a region.
    Cells {
        /// Left column, 0-based.
        x: u16,
        /// Top row, 0-based.
        y: u16,
        /// Width in cells.
        #[arg(default_value_t = 1)]
        w: u16,
        /// Height in cells.
        #[arg(default_value_t = 1)]
        h: u16,
    },
    /// Get a structured field.
    Get {
        /// Field to print.
        #[arg(value_enum)]
        field: GetArg,
    },
    /// Type literal text.
    Type {
        /// Literal text to type.
        text: String,
    },
    /// Type text then submit with the shell return key.
    Submit {
        /// Text to type before the return key (optional).
        text: Option<String>,
    },
    /// Keyboard input.
    Key {
        #[command(subcommand)]
        action: KeyCmd,
    },
    /// Compatibility alias for `key press`.
    #[command(hide = true)]
    Press {
        /// Key names or combos to press in sequence.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Mouse control.
    Mouse {
        #[command(subcommand)]
        action: MouseCmd,
    },
    /// Resize the PTY and emulator.
    Resize {
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },
    /// Write raw bytes (no return key).
    Write {
        /// Raw bytes to write.
        data: String,
    },
    /// Send a signal to the session's child process.
    Signal {
        /// Signal to send.
        #[arg(value_enum)]
        name: SignalArg,
    },
    /// Kill the session's child process.
    Kill,
    /// Block until a condition holds: text on screen, screen idle, command
    /// done, or session exit. See `wait <subcommand> --help` for the
    /// differences (notably idle vs command).
    Wait {
        #[command(subcommand)]
        what: WaitCmd,
    },
    /// Assert a condition (exit 0 pass / 1 fail).
    Expect {
        #[command(subcommand)]
        what: ExpectCmd,
    },
    /// Print the session's recording (asciinema v2 cast) to stdout.
    ///
    /// Redirect to a `.cast` file for playback in the asciicast ecosystem.
    GetRecording {
        /// Session to read (defaults to --session / the default session).
        session: Option<String>,
    },
    /// Watch a session live in another terminal (full-color, framed).
    ///
    /// Takes over an alternate screen and streams the session as the agent
    /// drives it. Press `q`, `Esc`, or `Ctrl-C` to detach.
    Monitor,
    /// Print a compact command cheatsheet for agents.
    Usage,
    /// Print a machine-readable description of the full cli surface (JSON).
    ///
    /// Versioned via `schema_version`; lists every command, flag, type, enum,
    /// default, and the exit-code taxonomy. Generated from the cli definition.
    AgentContext,
    /// Print or install the long-form agent skill manifest (SKILL.md).
    Skill {
        /// Interactively install the skill by choosing its scope and agent directory.
        #[arg(long)]
        add: bool,
    },
    /// Internal: run the session daemon.
    #[command(name = "__daemon", hide = true)]
    InternalDaemon,
}

#[derive(Subcommand)]
pub enum RecordCmd {
    /// Start recording terminal output to APNG, GIF, MP4, or asciicast v2.
    Start {
        /// Output path. The extension selects APNG (.png/.apng), GIF, MP4, or cast.
        path: String,
        /// Override the format inferred from the output extension.
        #[arg(long, value_enum)]
        format: Option<RecordingFormatArg>,
        /// Maximum animation frame rate.
        #[arg(long)]
        fps: Option<u8>,
        /// Playback speed multiplier.
        #[arg(long)]
        speed: Option<f64>,
        /// Clamp idle gaps to this many seconds.
        #[arg(long)]
        idle_time_limit: Option<f64>,
        /// Scale image/video dimensions while keeping the same terminal cells.
        #[arg(long)]
        zoom: Option<f64>,
    },
    /// Stop the active recording and finish its output file.
    Stop,
}

/// Signals deliverable to a session's child process.
#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "upper")]
pub enum SignalArg {
    /// Interrupt the foreground program (Ctrl-C).
    Int,
    /// Terminate the child process.
    Term,
    /// Forcibly kill the child process.
    Kill,
    /// Quit the child process.
    Quit,
}

impl SignalArg {
    pub fn as_str(self) -> &'static str {
        match self {
            SignalArg::Int => "INT",
            SignalArg::Term => "TERM",
            SignalArg::Kill => "KILL",
            SignalArg::Quit => "QUIT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_action_commands_parse() {
        for action in ["press", "down", "repeat", "up"] {
            let cli = Cli::try_parse_from(["tui-test", "key", action, "Ctrl+a"])
                .expect("parse key action");
            let Some(Command::Key { action: parsed }) = cli.command else {
                panic!("expected key command for {action}");
            };
            let keys = match parsed {
                KeyCmd::Press { keys }
                | KeyCmd::Down { keys }
                | KeyCmd::Repeat { keys }
                | KeyCmd::Up { keys } => keys,
            };
            assert_eq!(keys, ["Ctrl+a"]);
        }

        let cli = Cli::try_parse_from(["tui-test", "press", "Ctrl+a"]).expect("parse press alias");
        match cli.command {
            Some(Command::Press { keys }) => assert_eq!(keys, ["Ctrl+a"]),
            _ => panic!("unexpected press alias command"),
        }
        assert!(Cli::try_parse_from(["tui-test", "keys", "Ctrl+a"]).is_err());
    }

    #[test]
    fn skill_accepts_the_add_flag() {
        let cli = Cli::try_parse_from(["tui-test", "skill", "--add"]).expect("parse skill");
        assert!(matches!(cli.command, Some(Command::Skill { add: true })));
    }

    #[test]
    fn wait_ready_parses_with_a_timeout() {
        let cli = Cli::try_parse_from(["tui-test", "wait", "ready", "--timeout", "1234"])
            .expect("parse wait ready");
        assert!(matches!(
            cli.command,
            Some(Command::Wait {
                what: WaitCmd::Ready {
                    timeout: Some(1234)
                }
            })
        ));
    }

    #[test]
    fn bell_commands_parse_with_counts_and_timeouts() {
        let cli = Cli::try_parse_from(["tui-test", "wait", "bell", "--timeout", "1234"])
            .expect("parse wait bell");
        assert!(matches!(
            cli.command,
            Some(Command::Wait {
                what: WaitCmd::Bell {
                    timeout: Some(1234)
                }
            })
        ));

        let cli = Cli::try_parse_from(["tui-test", "expect", "bell", "3", "--timeout", "4321"])
            .expect("parse expect bell");
        assert!(matches!(
            cli.command,
            Some(Command::Expect {
                what: ExpectCmd::Bell {
                    count: 3,
                    timeout: Some(4321)
                }
            })
        ));

        assert!(Cli::try_parse_from(["tui-test", "expect", "bell"]).is_err());
    }

    #[test]
    fn open_accepts_readiness_flags() {
        let cli = Cli::try_parse_from(["tui-test", "open", "--no-wait-ready"]).expect("parse open");
        assert!(matches!(
            cli.command,
            Some(Command::Open {
                wait_ready: false,
                no_wait_ready: true,
                ..
            })
        ));
    }

    #[test]
    fn open_and_run_accept_restart_and_force() {
        for args in [
            vec!["tui-test", "open", "--restart"],
            vec!["tui-test", "open", "--force"],
        ] {
            let cli = Cli::try_parse_from(args).expect("parse open restart");
            assert!(matches!(
                cli.command,
                Some(Command::Open { restart: true, .. })
            ));
        }

        for args in [
            vec!["tui-test", "run", "--restart", "vim"],
            vec!["tui-test", "run", "--force", "vim"],
        ] {
            let cli = Cli::try_parse_from(args).expect("parse run restart");
            assert!(matches!(
                cli.command,
                Some(Command::Run { restart: true, .. })
            ));
        }
    }

    #[test]
    fn open_shell_values_map_to_library_shells() {
        let cases = [
            ("bash", Shell::Bash),
            ("powershell", Shell::Powershell),
            ("pwsh", Shell::Pwsh),
            ("cmd", Shell::Cmd),
            ("fish", Shell::Fish),
            ("zsh", Shell::Zsh),
            ("xonsh", Shell::Xonsh),
            ("elvish", Shell::Elvish),
            ("nushell", Shell::Nushell),
        ];
        for (value, expected) in cases {
            let cli =
                Cli::try_parse_from(["tui-test", "open", "--shell", value]).expect("parse shell");
            let Some(Command::Open {
                shell: Some(shell), ..
            }) = cli.command
            else {
                panic!("expected Open with a shell");
            };
            assert_eq!(Shell::from(shell), expected);
        }
    }

    #[test]
    fn open_backend_values_map_to_terminal_backends() {
        for (name, expected) in [
            ("alacritty", Backend::Alacritty),
            ("ghostty", Backend::Ghostty),
            ("rio", Backend::Rio),
        ] {
            let cli = Cli::try_parse_from(["tui-test", "open", "--backend", name])
                .unwrap_or_else(|error| panic!("parse {name}: {error}"));
            let Some(Command::Open {
                backend: Some(backend),
                ..
            }) = cli.command
            else {
                panic!("expected Open with backend {name}");
            };
            assert_eq!(Backend::from(backend), expected);
        }
        assert!(Cli::try_parse_from(["tui-test", "open", "--backend", "libghostty"]).is_err());
    }

    #[test]
    fn run_accepts_readiness_flags() {
        let cli =
            Cli::try_parse_from(["tui-test", "run", "--wait-ready", "vim"]).expect("parse run");
        assert!(matches!(
            cli.command,
            Some(Command::Run {
                wait_ready: true,
                no_wait_ready: false,
                ..
            })
        ));
    }

    #[test]
    fn open_rejects_contradictory_readiness_flags() {
        assert!(
            Cli::try_parse_from(["tui-test", "open", "--wait-ready", "--no-wait-ready"]).is_err()
        );
    }

    #[test]
    fn run_rejects_contradictory_readiness_flags() {
        assert!(
            Cli::try_parse_from(["tui-test", "run", "--wait-ready", "--no-wait-ready", "vim"])
                .is_err()
        );
    }

    #[test]
    fn open_accepts_per_class_timeout_defaults() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "open",
            "--timeout-text",
            "30000",
            "--timeout-idle",
            "15000",
            "--timeout-ready",
            "20000",
        ])
        .expect("parse open with timeouts");
        let Some(Command::Open { timeouts, .. }) = cli.command else {
            panic!("expected Open");
        };
        let defaults: Timeouts = timeouts.into();
        assert_eq!(defaults.text, Some(30_000));
        assert_eq!(defaults.idle, Some(15_000));
        assert_eq!(defaults.ready, Some(20_000));
        assert_eq!(defaults.command, None, "unset classes stay unset");
        assert_eq!(defaults.exit, None);
    }

    #[test]
    fn recording_start_accepts_timeline_options() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "record",
            "start",
            "demo.mp4",
            "--format",
            "mp4",
            "--fps",
            "24",
            "--speed",
            "2",
            "--idle-time-limit",
            "3",
            "--zoom",
            "0.5",
        ])
        .expect("parse recording start");
        assert!(matches!(
            cli.command,
            Some(Command::Record {
                cmd: RecordCmd::Start {
                    format: Some(RecordingFormatArg::Mp4),
                    fps: Some(24),
                    speed: Some(2.0),
                    idle_time_limit: Some(3.0),
                    zoom: Some(0.5),
                    ..
                }
            })
        ));
    }

    #[test]
    fn screenshot_accepts_zoom() {
        let cli = Cli::try_parse_from([
            "tui-test",
            "screenshot",
            "--out",
            "screen.svg",
            "--zoom",
            "0.5",
        ])
        .expect("parse screenshot zoom");
        assert!(matches!(
            cli.command,
            Some(Command::Screenshot {
                zoom: Some(0.5),
                ..
            })
        ));
    }

    #[test]
    fn open_has_no_catch_all_timeout_flag() {
        assert!(Cli::try_parse_from(["tui-test", "open", "--timeout", "1000"]).is_err());
    }

    #[test]
    fn per_call_timeouts_are_unset_when_omitted() {
        let cli = Cli::try_parse_from(["tui-test", "wait", "idle"]).expect("parse wait idle");
        assert!(matches!(
            cli.command,
            Some(Command::Wait {
                what: WaitCmd::Idle { timeout: None }
            })
        ));

        let cli =
            Cli::try_parse_from(["tui-test", "expect", "text", "hi"]).expect("parse expect text");
        let Some(Command::Expect {
            what: ExpectCmd::Text { timeout, .. },
        }) = cli.command
        else {
            panic!("expected Expect text");
        };
        assert_eq!(timeout, None);
    }

    #[test]
    fn expect_exit_code_accepts_a_timeout() {
        let cli =
            Cli::try_parse_from(["tui-test", "expect", "exit-code", "0", "--timeout", "1234"])
                .expect("parse expect exit-code");
        assert!(matches!(
            cli.command,
            Some(Command::Expect {
                what: ExpectCmd::ExitCode {
                    code: 0,
                    timeout: Some(1234)
                }
            })
        ));
    }

    #[test]
    fn daemon_stop_accepts_all() {
        let cli = Cli::try_parse_from(["tui-test", "daemon", "stop", "--all"]).expect("parse stop");
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                cmd: DaemonCmd::Stop { all: true }
            })
        ));
    }

    #[test]
    fn daemon_start_is_its_own_subcommand() {
        let cli = Cli::try_parse_from(["tui-test", "daemon", "start"]).expect("parse start");
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                cmd: DaemonCmd::Start
            })
        ));
    }

    #[test]
    fn daemon_stop_defaults_to_no_target() {
        let cli = Cli::try_parse_from(["tui-test", "daemon", "stop"]).expect("parse stop");
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                cmd: DaemonCmd::Stop { all: false }
            })
        ));
        assert_eq!(cli.session, None, "no session means no target was named");
    }

    #[test]
    fn daemon_stop_records_an_explicit_session() {
        let cli = Cli::try_parse_from(["tui-test", "--session", "work", "daemon", "stop"])
            .expect("parse stop");
        assert_eq!(cli.session.as_deref(), Some("work"));
    }
}

#[derive(Subcommand)]
pub enum DaemonCmd {
    /// Start this session's daemon (idempotent; blocks until it accepts connections).
    Start,
    /// Show daemon status, socket, and log path.
    /// Reports without starting one; exits 3 when nothing is running.
    Status,
    /// Stop a session's daemon.
    /// Each session has its own daemon, so this needs --session <NAME> or --all.
    Stop {
        /// Stop every daemon in this home.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum GetArg {
    /// Last command line.
    Command,
    /// Output of the last command.
    Output,
    /// Exit code of the last command.
    ExitCode,
    /// Current working directory.
    Cwd,
    /// Cursor row and column.
    Cursor,
    /// Terminal size.
    Size,
    /// Window title, as set with OSC 0/2.
    Title,
    /// Cumulative terminal bell count.
    Bells,
    /// Recorded terminal bell events (sequence + elapsed time).
    BellEvents,
}

#[derive(Subcommand)]
pub enum KeyCmd {
    /// Simulate key presses, reporting releases when the negotiated mode supports them.
    Press {
        /// Key names or combos to press in sequence.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Simulate explicit keydown events.
    Down {
        /// Key names or combos to send down events for.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Send repeat events, or press-equivalent input in legacy mode.
    Repeat {
        /// Key names or combos to send repeat events for.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Simulate explicit keyup events when the negotiated mode supports them.
    Up {
        /// Key names or combos to send up events for.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum MouseCmd {
    /// Click at a cell, or on the first cell matching --on-text.
    Click {
        /// Column to click, 0-based (omit when using --on-text).
        x: Option<u16>,
        /// Row to click, 0-based (omit when using --on-text).
        y: Option<u16>,
        /// Click the first cell containing this text.
        #[arg(long)]
        on_text: Option<String>,
        /// Button: 0 left, 1 middle, 2 right.
        #[arg(long, default_value_t = 0)]
        button: u8,
        /// Number of clicks.
        #[arg(long, default_value_t = 1)]
        clicks: u8,
    },
    /// Move the pointer to a cell.
    Move {
        /// Target column, 0-based.
        x: u16,
        /// Target row, 0-based.
        y: u16,
    },
    /// Press a button at a cell (no release).
    Down {
        /// Column, 0-based.
        x: u16,
        /// Row, 0-based.
        y: u16,
        /// Button: 0 left, 1 middle, 2 right.
        #[arg(long, default_value_t = 0)]
        button: u8,
    },
    /// Release a button at a cell.
    Up {
        /// Column, 0-based.
        x: u16,
        /// Row, 0-based.
        y: u16,
        /// Button: 0 left, 1 middle, 2 right.
        #[arg(long, default_value_t = 0)]
        button: u8,
    },
    /// Drag from one cell to another.
    Drag {
        /// Start column, 0-based.
        x1: u16,
        /// Start row, 0-based.
        y1: u16,
        /// End column, 0-based.
        x2: u16,
        /// End row, 0-based.
        y2: u16,
        /// Button: 0 left, 1 middle, 2 right.
        #[arg(long, default_value_t = 0)]
        button: u8,
    },
    /// Scroll the wheel up or down.
    Scroll {
        /// Scroll direction.
        #[arg(value_enum)]
        direction: ScrollDir,
        /// Number of wheel steps.
        #[arg(long, default_value_t = 3)]
        amount: u16,
    },
}

/// Scroll-wheel direction.
#[derive(Clone, Copy, clap::ValueEnum)]
#[clap(rename_all = "lower")]
pub enum ScrollDir {
    /// Scroll up.
    Up,
    /// Scroll down.
    Down,
}

impl ScrollDir {
    pub fn as_str(self) -> &'static str {
        match self {
            ScrollDir::Up => "up",
            ScrollDir::Down => "down",
        }
    }
}

#[derive(Subcommand)]
pub enum WaitCmd {
    /// Wait until text/regex appears on screen (the most precise wait).
    Text {
        /// Text or regex to wait for.
        text: String,
        /// Treat <text> as a regular expression.
        #[arg(long)]
        regex: bool,
        /// Search the full scrollback, not just the visible viewport.
        #[arg(long)]
        full: bool,
        /// Invert: wait until the text is NOT present.
        #[arg(long)]
        not: bool,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait until the window title (set with OSC 0/2) matches text/regex.
    ///
    /// Programs set the title to announce what they are doing, so this is how
    /// to wait for one that reports progress there rather than on screen.
    Title {
        /// Text or regex to wait for in the title.
        text: String,
        /// Treat <text> as a regular expression.
        #[arg(long)]
        regex: bool,
        /// Invert: wait until the title does NOT match.
        #[arg(long)]
        not: bool,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait until the screen stops repainting (visual idle, NOT command done).
    ///
    /// A silent command (e.g. `sleep 100`) counts as idle right away. To wait
    /// for a command to finish, use `wait command`.
    Idle {
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait until the foreground command finishes (via shell integration).
    ///
    /// Use this after `submit`. Without shell integration it falls back to
    /// "prompt returned and screen idle". Raise --timeout for long commands.
    Command {
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait until the session's program/shell itself exits.
    ///
    /// Use this for `run <program>` sessions or after sending `exit`.
    Exit {
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait until the shell reports a ready prompt (via shell integration).
    /// Use after `run`-ing something prompt-aware, or to re-synchronise before input.
    Ready {
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Wait for the next terminal bell event.
    Bell {
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum ExpectCmd {
    /// Assert text is visible, optionally with a required color.
    Text {
        /// Text or regex to match.
        text: String,
        /// Treat <text> as a regular expression.
        #[arg(long)]
        regex: bool,
        /// Search the full scrollback, not just the visible viewport.
        #[arg(long)]
        full: bool,
        /// Allow multiple matches instead of requiring exactly one.
        #[arg(long = "no-strict")]
        no_strict: bool,
        /// Invert: assert the text is NOT present.
        #[arg(long)]
        not: bool,
        /// Require this foreground color on the match: `default`, an ansi256
        /// index (0-255), hex (#rrggbb), or rgb (r,g,b).
        #[arg(long)]
        fg: Option<String>,
        /// Require this background color on the match: `default`, an ansi256
        /// index (0-255), hex (#rrggbb), or rgb (r,g,b).
        #[arg(long)]
        bg: Option<String>,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Assert the window title (set with OSC 0/2) matches text/regex.
    Title {
        /// Text or regex to match against the title.
        text: String,
        /// Treat <text> as a regular expression.
        #[arg(long)]
        regex: bool,
        /// Invert: assert the title does NOT match.
        #[arg(long)]
        not: bool,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Assert the last command's exit code.
    /// Waits for the foreground command first, so this is safe right after `submit`.
    ExitCode {
        /// Expected exit code.
        code: i32,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Assert the last command's output.
    Output {
        /// Text or regex to match.
        text: String,
        /// Treat <text> as a regular expression.
        #[arg(long)]
        regex: bool,
    },
    /// Wait until the cumulative terminal bell count reaches this value.
    Bell {
        /// Minimum cumulative bell count.
        count: u64,
        /// Timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout: Option<u64>,
    },
    /// Assert the screen matches a saved snapshot.
    Snapshot {
        /// Snapshot name.
        name: String,
        /// Write the current screen as the new snapshot.
        #[arg(short = 'u', long)]
        update: bool,
        /// Include cell colors in the snapshot.
        #[arg(long)]
        include_colors: bool,
        /// Include the window title in the snapshot's frame. Off by default:
        /// a shell prompt often sets the title to a hostname and path, which
        /// would tie the snapshot to one machine.
        #[arg(long)]
        include_title: bool,
    },
}
