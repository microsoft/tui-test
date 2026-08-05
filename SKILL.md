---
name: shell-use
description: "Drive, inspect, assert on, record, and watch a real terminal from the command line with the shell-use cli. Use when running shells (bash, zsh, fish, PowerShell, pwsh, cmd, xonsh, elvish, nushell) or TUI programs (vim, less, top, etc.) in a headless PTY; sending keystrokes, key combos, or mouse input; resizing, writing raw bytes, or signaling the child; waiting for a command to finish or the screen to settle; asserting on terminal text, colors, exit codes, output, or snapshots; capturing text or full-color SVG screenshots; recording and replaying asciinema sessions; watching a live cli session while an agent drives it; or driving process-local sessions from Python or Node with the shell-use bindings."
---

# shell-use

`shell-use` controls a real terminal from the command line. It runs shells and
TUI programs in a headless PTY behind a background daemon: a stateless cli front
end talks to a daemon that owns the PTY and renders it into a full terminal
emulator. Each call connects, acts, and exits, and they all share one live
session. With it you can spawn a session, read the rendered screen, send keys
and mouse input, wait for a condition, assert on the result, and record the
session.

## Built for agents: self-documenting commands

Three commands let an agent look up the rest of the surface instead of guessing:

- `shell-use agent-context`: versioned JSON describing every command, flag,
  enum, default, and the exit-code taxonomy. It is generated from the cli, so it
  stays in sync. Read this first when you need exact argument shapes.
- `shell-use usage`: a one-screen command cheatsheet.
- `shell-use skill`: this guide.

## Core model

- **Sessions.** `--session <name>` (default `default`, env `SHELL_USE_SESSION`)
  selects a terminal. The first command auto-starts that session's daemon; the
  session persists across calls until `close`. Sessions are independent.
- **Stateless calls.** Each invocation connects to the daemon, acts, and exits.
  State (screen, cwd, last command) lives in the daemon, not the cli.
- **JSON.** Pass `--json` on any command for machine-readable output. Data goes
  to stdout, diagnostics to stderr. On failure the JSON carries a `"kind"`
  (`assertion` / `usage` / `no_session` / `internal`).
- **Verbose.** `--verbose` / `-v` starts the daemon with a full PTY traffic log
  (see [Debugging](#debugging)). Only takes effect when the daemon starts.
- **Defaults.** New sessions are `80x30`. Timeouts come in five classes: `text`
  and `idle` default to 5s; `command`, `exit`, and `ready` to 30s. Set a session
  default with `open --timeout-<class> <ms>`, or override one call with
  `--timeout`. `state` reports the effective values.

## Exit codes

Every command returns a stable exit code so you can branch on the failure class
without parsing text:

| Code | Meaning                                                 |
| ---- | ------------------------------------------------------- |
| `0`  | success                                                 |
| `1`  | assertion or wait condition not met (`expect` / `wait`) |
| `2`  | usage / invalid argument                                |
| `3`  | no active session (run `open` / `run` first)            |
| `4`  | daemon or IPC error                                     |
| `5`  | internal error                                          |

## Command reference

### Session & lifecycle

| Command                                                                  | Description                                                            |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| `open [--shell S] [--cols N] [--rows N] [--cwd D] [--env K=V]...`        | Spawn a shell session (auto-starts the daemon). `--env` is repeatable. |
| `run <program> [args...] [--cols N] [--rows N] [--cwd D] [--env K=V]...` | Spawn a session running a program directly (no shell).                 |
| `sessions`                                                               | List active sessions.                                                  |
| `close [--all]`                                                          | Close the current session (or every session with `--all`).             |
| `daemon start`                                                           | Start this session's daemon. Most commands start one on demand.        |
| `daemon status`                                                          | Inspect a session's daemon (pid, log path). Exit 3 if none is running. |
| `daemon stop --session N \| --all`                                       | Stop one session's daemon, or every daemon. Needs a target.            |

### Inspection

| Command                                             | Description                                                                                                         |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `state`                                             | cwd, size, cursor, last command + exit code, bell count, timeouts, and a text snapshot.                             |
| `text [--full]`                                     | Rendered viewport text, or full scrollback with `--full`.                                                           |
| `screenshot [PATH] [-o FILE] [--full]`              | Terminal text to stdout, or a full-color SVG image (crisp at any zoom, svg-term-style window) when a path is given. |
| `cells X Y [W H]`                                   | Per-cell attributes (char, fg, bg, flags) for a region.                                                             |
| `get command\|output\|exit-code\|cwd\|cursor\|size\|bells` | One structured field.                                                                                       |

### Input

| Command                                                                    | Description                                                                  |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `type "text"`                                                              | Type literal text (no return key).                                           |
| `submit ["text"]`                                                          | Type text then press the shell's return key. Omit text to just submit.       |
| `press <Key...>`                                                           | Named keys, e.g. `press Escape : w q Enter`, `press Ctrl+C`.                 |
| `keys "Ctrl+a"`                                                            | A single key combo.                                                          |
| `mouse click X Y` / `mouse click --on-text "OK" [--button N] [--clicks N]` | Click by coordinates or by visible label.                                    |
| `mouse move\|down\|up\|drag\|scroll ...`                                   | Full mouse control (`--button` default 0=left, `scroll --amount` default 3). |

### PTY control

| Command                        | Description                                          |
| ------------------------------ | ---------------------------------------------------- |
| `resize COLS ROWS`             | Resize the PTY and emulator.                         |
| `write <data>`                 | Write raw bytes to the PTY (no return key appended). |
| `signal INT\|TERM\|KILL\|QUIT` | Send a signal to the session's child process.        |
| `kill`                         | Kill the session's child process.                    |

### Wait (block until a condition holds)

| Command                                             | Description                                                                                |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `wait text "T" [--regex --full --not --timeout MS]` | Until text/regex is (with `--not`, is not) visible. Most precise wait.                     |
| `wait idle [--timeout MS]`                          | Until the screen stops repainting (~250ms quiet).                                          |
| `wait command [--timeout MS]`                       | Until the current foreground command finishes (needs shell integration).                   |
| `wait exit [--timeout MS]`                          | Until the session's program/shell itself exits.                                            |
| `wait ready [--timeout MS]`                         | Until the shell reports a ready prompt (needs shell integration). `open` waits by default. |
| `wait bell [--timeout MS]`                          | Until the next terminal bell event.                                                  |

### Expect (exit 0 = pass, 1 = fail)

| Command                                                                         | Description                                                                   |
| ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `expect text "T" [--regex --full --no-strict --not --fg C --bg C --timeout MS]` | Visibility plus optional color. `--no-strict` relaxes a strict single-match.  |
| `expect exit-code N [--timeout MS]`                                             | The last command's exit code. Waits for the command to finish first.          |
| `expect output "T" [--regex]`                                                   | The last command's captured output.                                           |
| `expect bell N [--timeout MS]`                                                   | The cumulative bell count reaches at least N.                          |
| `expect snapshot NAME [-u] [--include-colors]`                                  | Compare the screen against `__snapshots__/NAME.snap`; `-u` writes/updates it. |

Colors accept ansi-256 (`9`), hex (`#ff0000`), or rgb (`255,0,0`).

### Recording, monitor & self-docs

| Command                             | Description                                                                  |
| ----------------------------------- | ---------------------------------------------------------------------------- |
| `get-recording [session]`           | Print a session's asciinema v2 cast to stdout (works even after it stopped). |
| `monitor`                           | Watch the session live, full-color, in another terminal.                     |
| `usage` / `agent-context` / `skill` | Self-documentation (see top of guide).                                       |

## Workflow: run a command and check the result

```sh
shell-use open                       # start a shell session
shell-use submit "echo hello"        # type text + Enter
shell-use wait command               # block until the command finishes
shell-use expect text "hello"        # assert it appeared (exit 1 if not)
shell-use expect exit-code 0         # assert the command succeeded
shell-use close
```

`submit` types text then presses Enter; `type` types without Enter; `press`
sends named keys (`press Escape : w q Enter`, `press Ctrl+C`); `keys` sends one
combo (`keys "Ctrl+a"`).

## Workflow: drive a TUI program

```sh
shell-use run vim file.txt
shell-use wait idle                  # let the screen finish rendering
shell-use press i                    # enter insert mode
shell-use type "some text"
shell-use press Escape : w q Enter   # save and quit
shell-use wait exit
```

## Workflow: mouse interaction

```sh
shell-use mouse click --on-text "OK"     # click a label, no coordinates needed
shell-use mouse click 10 5 --clicks 2    # double-click at column 10, row 5
shell-use mouse scroll down --amount 5   # scroll the wheel
shell-use mouse drag 2 2 20 2            # drag from (2,2) to (20,2)
```

## Workflow: assert colors

```sh
shell-use cells 0 0 10 1                       # inspect char/fg/bg/flags
shell-use expect text "ERROR" --fg "#ff0000"   # text present AND red
shell-use expect text "OK" --fg 2 --bg 0       # ansi-256 fg/bg
shell-use expect text "plain" --fg default     # asserts the cell set no color
```

## Workflow: snapshot testing

```sh
shell-use expect snapshot main-view -u                    # create/update the snapshot
shell-use expect snapshot main-view                       # later: assert it still matches
shell-use expect snapshot main-view --include-colors      # also compare per-cell colors
```

Snapshots live in `__snapshots__/<NAME>.snap` next to where you run the command.

## Waiting: pick the right one

- `wait text "T"`: waits until text/regex is visible. The most precise wait; use
  it whenever you know what output to look for. `--not` waits for it to disappear.
- `wait command`: waits until the current command finishes, via the shell's OSC
  integration markers. Use it after `submit`. Without shell integration it falls
  back to "screen idle". Bump `--timeout` for long commands (default 30s).
- `wait idle`: waits until the screen stops repainting. This tracks visual
  quiescence, not completion: a silent command like `sleep 100` counts as idle
  almost immediately. Use it to let a TUI finish drawing.
- `wait exit`: waits until the program/session itself exits. Use for
  `run <program>` sessions or after sending `exit`.
- `wait ready`: waits until the shell reports a ready prompt. `open` does this
  for you.

## Recording

Every session records automatically from the moment it opens, in asciinema v2
cast format, stored in your XDG cache by session name. The path is reported in
the `open` / `run` response. Recordings persist after the session ends; stale
ones are swept when a daemon next starts (recordings of still-running sessions
are kept).

```sh
shell-use get-recording > demo.cast    # current session's recording to stdout
shell-use get-recording work > w.cast  # a specific session by name (even if stopped)
```

Play it with `asciinema play demo.cast`, or render a GIF with
`agg demo.cast demo.gif`.

## Live monitor

Watch a session live in a second terminal while an agent drives it; both share
the same daemon. `monitor` takes over an alternate screen and streams the
session in full color at ~20fps. Press `q`, `Esc`, or `Ctrl-C` to detach.

```sh
shell-use --session work monitor   # watch the 'work' session live
```

It needs an interactive terminal (exit `2` otherwise) and an existing session
(exit `3` if none). It only reads shared screen state, so watching never blocks
the commands the agent runs; resizing the window re-fits the frame.

This works only with standalone CLI sessions.

## Programmatic use (Python and JavaScript)

The Python and JavaScript packages bind the Rust terminal engine directly and
run sessions in-process. Session names, registries, and recordings are
process-local. A native session cannot be listed, attached to, controlled, or
monitored from another process, including by the standalone CLI.

Language packages do not install or require the `shell-use` CLI. Only the
standalone CLI uses the daemon and JSON-over-local-socket protocol described
elsewhere in this guide.

Node is the supported JavaScript runtime. Bun and Deno compatibility is best
effort and does not gate releases. Deno requires a local `node_modules`
directory and `--allow-ffi` in addition to read/write permissions.

```sh
pip install shell-use              # Python 3.8+, imported as `shell_use`
npm install @microsoft/shell-use   # Node 20+ (ESM only)
bun add @microsoft/shell-use       # Bun (best effort)
deno add npm:@microsoft/shell-use  # Deno 2 (best effort)
```

Python:

```python
import asyncio
from shell_use import ShellUse

async def main():
    async with ShellUse() as su:                     # closes the session on exit
        await su.open()
        await su.submit("echo hello")
        await su.wait_command()
        await su.expect_text("hello", strict=False)  # command echo + output both match
        await su.expect_exit_code(0)

asyncio.run(main())
```

Node (the same API may work on Bun and Deno):

```js
import { ShellUse } from "@microsoft/shell-use";

const su = new ShellUse();
await su.open();
await su.submit("echo hello");
await su.waitCommand();
await su.expectText("hello", { strict: false });
await su.expectExitCode(0);
await su.close();
```

Methods mirror the cli commands: `open` / `run`, `submit` / `type` / `write`,
`press` / `keys`, `mouse.click|move|down|up|drag|scroll`, `resize`, `signal` /
`kill`, `state`, `text`, `cells`, the dedicated `get_command` / `get_output` /
`get_exit_code` / `get_cwd` / `get_cursor` / `get_size` / `get_bell_count`
methods,
`screenshot`, `wait_text` / `wait_idle` / `wait_command` / `wait_exit` /
`wait_ready` / `wait_bell`, `expect_text` / `expect_exit_code` /
`expect_output` / `expect_bell_count` / `expect_snapshot`, and `close`.
Python module-level helpers are `sessions`,
`close_all`, and `get_recording`; JavaScript exports `sessions`, `closeAll`,
and `getRecording`. The JavaScript client otherwise uses the same names in
camelCase (`waitCommand`, `expectText`, `getExitCode`, etc.).

The constructors accept a session name plus timeout and artifact options:
`ShellUse(session="default", *, timeouts=None, artifacts=None)` in Python and
`new ShellUse(session?, { timeouts?, artifacts? })` in JavaScript. `run` takes
the program then its arguments (`await su.run("vim", "file.txt")` in Python,
`await su.run("vim", ["file.txt"])` in JavaScript).

Failures raise typed errors instead of returning exit codes, one class per row of
the applicable [exit-code table](#exit-codes): `ExpectationError` (1),
`UsageError` (2), `NoSessionError` (3), and `InternalError` (5), all subclasses
of `ShellUseError`.

## Supported shells & integration

`open --shell S` accepts: `bash`, `zsh`, `fish`, `powershell`, `pwsh`, `cmd`,
`xonsh`, `elvish`, `nushell`. Omit `--shell` to use the platform default.

`shell-use` injects shell integration (standard OSC 133 semantic-prompt markers,
plus OSC 7 for cwd) so it can track command boundaries, exit codes, cwd, and
command/output text across shells. This is what powers `wait command`,
`expect exit-code`, `get cwd`, and `get command|output`.

Integration coverage varies by shell: `powershell` has no native pre-exec hook
so command/output text is best-effort (exit code and cwd still track); `cmd` is
prompt-only.

## Debugging

By default the daemon writes no log. Start it with `--verbose` to record every
byte read from and written to the PTY, plus lifecycle events, to
`~/.shell-use/<session>.log`. Logging is fixed when the daemon starts, so enable
it on a fresh daemon (close any existing one first):

```sh
shell-use --session work close            # stop any existing daemon
shell-use --session work --verbose open   # start one with logging on
shell-use --session work submit "ls"
cat ~/.shell-use/work.log
```

`shell-use daemon status` reports the active log path.

**Stuck session?** If the screen is frozen and input seems ignored (e.g. after
`git log` / `git diff`), a full-screen pager such as `less` is likely holding the
terminal, and `Ctrl+C` won't quit it. Confirm with `shell-use state`
(`"ready": false` and a stale last command). Quit the pager with
`shell-use press q`, or avoid it with `git --no-pager <cmd>` or `GIT_PAGER=cat`.

**Platform note.** On Windows ConPTY, `get output` and `get command` text can on some rare occasions be
unreliable due to screen repainting; grid-based checks (`expect text`,
`expect exit-code`) are unaffected.
