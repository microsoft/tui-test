use clap::{Parser, Subcommand};

use crate::config::{DEFAULT_COLS, DEFAULT_EXPECT_TIMEOUT_MS, DEFAULT_ROWS};
use crate::shell::Shell;

#[derive(Parser)]
#[command(name = "shell-use", version, about = "Headless terminal CLI + daemon")]
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
        shell: Option<Shell>,
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
    },
    /// Close the current session (or all sessions).
    Close {
        /// Close every session, not just the current one.
        #[arg(long)]
        all: bool,
    },
    /// List active sessions.
    Sessions,
    /// Inspect or stop the daemon.
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
    /// Print a machine-readable description of the full CLI surface (JSON).
    ///
    /// Versioned via `schema_version`; lists every command, flag, type, enum,
    /// default, and the exit-code taxonomy. Generated from the CLI definition.
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
}

#[derive(Subcommand)]
pub enum DaemonCmd {
    /// Show daemon status, socket, and log path.
    Status,
    /// Stop the daemon and close all sessions.
    Stop,
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
        #[arg(long, default_value_t = DEFAULT_EXPECT_TIMEOUT_MS)]
        timeout: u64,
    },
    /// Wait until the screen stops repainting (visual idle, NOT command done).
    ///
    /// A silent command (e.g. `sleep 100`) counts as idle right away. To wait
    /// for a command to finish, use `wait command`.
    Idle {
        /// Timeout in milliseconds.
        #[arg(long, default_value_t = DEFAULT_EXPECT_TIMEOUT_MS)]
        timeout: u64,
    },
    /// Wait until the foreground command finishes (via shell integration).
    ///
    /// Use this after `submit`. Without shell integration it falls back to
    /// "prompt returned and screen idle". Raise --timeout for long commands.
    Command {
        /// Timeout in milliseconds.
        #[arg(long, default_value_t = 30_000)]
        timeout: u64,
    },
    /// Wait until the session's program/shell itself exits.
    ///
    /// Use this for `run <program>` sessions or after sending `exit`.
    Exit {
        /// Timeout in milliseconds.
        #[arg(long, default_value_t = 30_000)]
        timeout: u64,
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
        #[arg(long, default_value_t = DEFAULT_EXPECT_TIMEOUT_MS)]
        timeout: u64,
    },
    /// Assert the last command's exit code.
    ExitCode {
        /// Expected exit code.
        code: i32,
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
    },
}
