use clap::{Args, Parser, Subcommand};

use shell_use::config::{DEFAULT_COLS, DEFAULT_ROWS};
use shell_use::shell::Shell;
use shell_use::Timeouts;

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
    /// Config file to read (default: ./shell-use.toml, then
    /// ~/.shell-use/shell-use.toml).
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
    pub fn resolve(&self) -> anyhow::Result<shell_use::profile::Profile> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        shell_use::profile::resolve(self.config.as_deref(), self.profile.as_deref(), &cwd)
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
#[command(name = "shell-use", version, about = "Headless terminal cli + daemon")]
pub struct Cli {
    /// Target a named session (env: SHELL_USE_SESSION).
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Start the daemon with a verbose data log at ~/.shell-use/<session>.log.
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
    /// Send named keys, e.g. `press Escape : w q Enter` or `press Ctrl+C`.
    Press {
        /// Key names to send in sequence.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Send a single key combo, e.g. `keys Control+a`.
    Keys {
        /// Key combo to send, e.g. Control+a.
        combo: String,
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
    /// Redirect to a `.cast` file, then `asciinema play` it or render a GIF
    /// with `agg`.
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
    fn skill_accepts_the_add_flag() {
        let cli = Cli::try_parse_from(["shell-use", "skill", "--add"]).expect("parse skill");
        assert!(matches!(cli.command, Some(Command::Skill { add: true })));
    }

    #[test]
    fn wait_ready_parses_with_a_timeout() {
        let cli = Cli::try_parse_from(["shell-use", "wait", "ready", "--timeout", "1234"])
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
    fn open_accepts_readiness_flags() {
        let cli =
            Cli::try_parse_from(["shell-use", "open", "--no-wait-ready"]).expect("parse open");
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
                Cli::try_parse_from(["shell-use", "open", "--shell", value]).expect("parse shell");
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
    fn run_accepts_readiness_flags() {
        let cli =
            Cli::try_parse_from(["shell-use", "run", "--wait-ready", "vim"]).expect("parse run");
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
            Cli::try_parse_from(["shell-use", "open", "--wait-ready", "--no-wait-ready"]).is_err()
        );
    }

    #[test]
    fn run_rejects_contradictory_readiness_flags() {
        assert!(Cli::try_parse_from([
            "shell-use",
            "run",
            "--wait-ready",
            "--no-wait-ready",
            "vim"
        ])
        .is_err());
    }

    #[test]
    fn open_accepts_per_class_timeout_defaults() {
        let cli = Cli::try_parse_from([
            "shell-use",
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
    fn open_has_no_catch_all_timeout_flag() {
        assert!(Cli::try_parse_from(["shell-use", "open", "--timeout", "1000"]).is_err());
    }

    #[test]
    fn per_call_timeouts_are_unset_when_omitted() {
        let cli = Cli::try_parse_from(["shell-use", "wait", "idle"]).expect("parse wait idle");
        assert!(matches!(
            cli.command,
            Some(Command::Wait {
                what: WaitCmd::Idle { timeout: None }
            })
        ));

        let cli =
            Cli::try_parse_from(["shell-use", "expect", "text", "hi"]).expect("parse expect text");
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
            Cli::try_parse_from(["shell-use", "expect", "exit-code", "0", "--timeout", "1234"])
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
        let cli =
            Cli::try_parse_from(["shell-use", "daemon", "stop", "--all"]).expect("parse stop");
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                cmd: DaemonCmd::Stop { all: true }
            })
        ));
    }

    #[test]
    fn daemon_start_is_its_own_subcommand() {
        let cli = Cli::try_parse_from(["shell-use", "daemon", "start"]).expect("parse start");
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                cmd: DaemonCmd::Start
            })
        ));
    }

    #[test]
    fn daemon_stop_defaults_to_no_target() {
        let cli = Cli::try_parse_from(["shell-use", "daemon", "stop"]).expect("parse stop");
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
        let cli = Cli::try_parse_from(["shell-use", "--session", "work", "daemon", "stop"])
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
