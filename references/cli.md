# CLI reference

Use the standalone CLI when an agent or shell script needs to control one
persistent terminal through multiple independent commands. Return to the
[interface selector](../SKILL.md) if the work belongs inside Python,
JavaScript, or Rust code.

## Self-documentation

Read the installed command surface instead of guessing flags:

| Command | Purpose |
| --- | --- |
| `tui-test agent-context` | Versioned JSON for every command, flag, enum, default, and exit code |
| `tui-test usage` | One-screen command cheatsheet |
| `tui-test skill` | Complete self-contained guide: router followed by every bundled reference |

Use `--json` for machine-readable command results. Diagnostics go to stderr.

## Session model

- `--session <name>` selects a terminal. The default is `default`, or the value
  of `TUI_TEST_SESSION`.
- The first operation starts a per-session daemon. The daemon owns the PTY,
  terminal emulator, rendered state, and recording.
- Every CLI invocation connects, performs one operation, and exits. Later calls
  reconnect to the same session until `close`.
- Sessions are independent. Use unique names for concurrent work.
- `open` and `run` reuse a live child. Pass `--restart` when replacement is
  intentional.
- New sessions default to 80 columns by 30 rows.

## Command map

### Lifecycle

| Command | Purpose |
| --- | --- |
| `open [options]` | Open a supported shell |
| `run [options] <program> [args...]` | Run a program directly |
| `sessions` | List active CLI daemon sessions |
| `close` | Close the selected session |
| `close --all` | Close every CLI session |
| `daemon status` | Show daemon pid and optional log path |
| `daemon stop --session N` | Stop one daemon |
| `daemon stop --all` | Stop every daemon |

`open` supports `--shell`, `--backend`, dimensions, cwd, environment, profile,
timeout defaults, and restart behavior. `run` supports the same terminal
options except shell selection. Use `agent-context` for exact flags.

### Input and PTY control

| Command | Purpose |
| --- | --- |
| `submit ["text"]` | Type text and press Enter |
| `type "text"` | Type literal text without Enter |
| `key press <keys...>` | Simulate key presses |
| `key down`, `key repeat`, `key up` | Send explicit key event types |
| `mouse click`, `move`, `down`, `up`, `drag`, `scroll` | Send mouse input |
| `resize COLS ROWS` | Resize the PTY and emulator |
| `write <data>` | Write raw bytes to the PTY |
| `signal INT|TERM|KILL|QUIT` | Signal the child |
| `kill` | Kill the child |

Examples:

```sh
tui-test submit "echo hello"
tui-test key press Ctrl+C
tui-test key press Escape : w q Enter
tui-test mouse click --on-text "OK"
tui-test mouse scroll down --amount 5
tui-test resize 120 40
```

`key press` follows the Kitty keyboard protocol negotiated by the child.
Top-level `press` remains a compatibility alias.

### Inspection

| Command | Result |
| --- | --- |
| `state` | Cwd, size, cursor, title, command, exit code, bell count, timeouts, and viewport |
| `text` | Rendered viewport text |
| `text --full` | Full scrollback |
| `cells X Y [W H]` | Characters, colors, and flags for a cell region |
| `get command|output|exit-code|cwd|cursor|size|title|bells|bell-events` | One structured field |
| `screenshot` | Terminal text |
| `screenshot PATH` | Full-color SVG |

Use getters when a structured field answers the question. Use `cells` for
precise color and style inspection.

### Waiting

| Command | Waits for |
| --- | --- |
| `wait text "T"` | Text appears |
| `wait text "T" --not` | Text disappears |
| `wait title "T"` | Window title match |
| `wait command` | Current shell command completes |
| `wait exit` | Child process exits |
| `wait ready` | Shell prompt is ready |
| `wait idle` | Screen stops repainting briefly |
| `wait bell` | Next bell event |

Text and title waits support regular expressions and per-call timeouts. Prefer
the most specific wait. `wait idle` is visual quiescence, not process
completion.

### Assertions

| Command | Checks |
| --- | --- |
| `expect text "T"` | Visible text, optionally regex, color, full scrollback, or absence |
| `expect title "T"` | Window title |
| `expect output "T"` | Captured output of the last shell command |
| `expect exit-code N` | Last shell command's exit code |
| `expect bell N` | Cumulative bell count |
| `expect snapshot NAME` | Saved screen snapshot |

`expect text` is strict by default: exactly one match must exist. Use
`--no-strict` when command echo and output intentionally duplicate text.

```sh
tui-test expect text "ERROR" --fg "#ff0000"
tui-test expect text "done" --not
tui-test expect output "^hello$" --regex
tui-test expect snapshot main-view -u
```

Snapshots live in `__snapshots__` relative to the command's working directory.
Use `--include-colors` or `--include-title` only when those values are part of
the expected behavior.

## Core workflows

### Run a shell command

```sh
tui-test --session example open
tui-test --session example submit "echo hello"
tui-test --session example wait command
tui-test --session example expect text "hello" --no-strict
tui-test --session example expect exit-code 0
tui-test --session example close
```

### Drive a full-screen program

```sh
tui-test --session editor run vim file.txt
tui-test --session editor wait idle
tui-test --session editor key press i
tui-test --session editor type "some text"
tui-test --session editor key press Escape : w q Enter
tui-test --session editor wait exit
```

### Inspect colors

```sh
tui-test cells 0 0 20 1
tui-test expect text "ERROR" --fg "#ff0000"
tui-test expect text "OK" --fg 2 --bg 0
tui-test expect text "plain" --fg default
```

Colors accept ANSI-256 indices, `#rrggbb`, `r,g,b`, or `default`.

## Screenshots and recordings

```sh
tui-test screenshot terminal.svg
tui-test record start demo.png --zoom 0.5
tui-test submit "echo hello"
tui-test wait command
tui-test record stop
tui-test get-recording > session.cast
```

`.png`/`.apng` selects APNG, `.gif` selects GIF, `.mp4` selects MP4, and
`.cast` selects asciinema v2. MP4 export requires `ffmpeg`.

Every CLI session also records an asciinema v2 cast automatically. The
recording remains available by session name after the session closes.

## Live monitor

```sh
tui-test --session work monitor
```

`monitor` lets a person watch the session while an agent continues to drive it.
It requires an interactive terminal and an existing standalone CLI session.
Press `q`, `Esc`, or `Ctrl-C` to detach. In-process library sessions cannot be
monitored this way.

## Backends and configuration

The CLI supports `alacritty` (default), `ghostty`, and `rio`:

```sh
tui-test open --backend ghostty
tui-test run --backend rio -- vim file.txt
```

Profiles in `tui-test.toml` can define timeout defaults, scrollback, and the
terminal palette. Discovery checks the nearest project file, then platform
configuration locations. `--config` selects an explicit file and `--profile`
selects a named profile.

```toml
[profiles.ci]
scrollback = 500

[profiles.ci.timeouts]
text = 15000
ready = 60000

[profiles.ci.colors]
red = "#ff0000"
```

Named profiles do not inherit from `[profiles.default]`; omitted fields use
built-in defaults.

## Shell integration

`open --shell` supports `bash`, `zsh`, `fish`, `powershell`, `pwsh`, `cmd`,
`xonsh`, `elvish`, and `nushell`.

Semantic prompt markers and cwd reporting power `wait command`, exit-code
tracking, command output, and cwd inspection. Integration varies by shell:
PowerShell command/output capture is best effort because it lacks a native
pre-exec hook, while exit code and cwd still track. `cmd` is prompt-only.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Assertion or wait condition not met |
| 2 | Invalid usage |
| 3 | No active session |
| 4 | Daemon or IPC error |
| 5 | Internal error |

Branch on the exit code or JSON `kind`; do not parse diagnostic prose.

## Debugging

Enable PTY traffic logging only on a fresh daemon:

```sh
tui-test --session work close
tui-test --session work --verbose open
tui-test --session work daemon status
```

If a session appears frozen after `git log` or `git diff`, a pager may own the
terminal. Inspect `state`, press `q`, or prevent paging with
`git --no-pager`.

On Windows ConPTY, command/output capture can occasionally be less reliable
than rendered-grid assertions. Fresh ConPTY sessions may also have a
platform-provided title before the child sets one.
