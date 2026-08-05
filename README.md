# shell-use

`shell-use` is a rust powered cli for controlling, inspecting, testing, and recording shell sessions and terminal apps. It supports all standard terminal actions (send keys, mouse clicks) & user actions (screenshot, record sessions), & testing (matches screenshot, contains text). `shell-use` supports Windows, Linux, & macOS and it supports a wide range of shells (see [Supported shells](#supported-shells)).

## Install

### install script

macOS / Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/microsoft/shell-use/main/install/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/microsoft/shell-use/main/install/install.ps1 | iex
```

Use `SHELL_USE_VERSION` to select a specific version or `SHELL_USE_INSTALL_DIR` to select an install location.

### homebrew (macOS/linux)

```sh
brew tap microsoft/shell-use https://github.com/microsoft/shell-use
brew install shell-use
```

## winget (windows)

```sh
winget install Microsoft.ShellUse
```

### download from releases

Download the latest [release](https://github.com/microsoft/shell-use/releases).

## Quick start

Run a command and check the result:

```sh
shell-use open                  # start a shell session (auto-starts the daemon)
shell-use submit "echo hello"   # type the command, press Enter
shell-use wait command          # block until it finishes
shell-use expect text "hello"   # assert it showed up
shell-use expect exit-code 0    # assert it exited 0
shell-use close
```

Drive a full-screen TUI the same way:

```sh
shell-use run vim file.txt
shell-use wait idle             # let the screen settle
shell-use press i
shell-use type "some text"
shell-use press Escape : w q Enter
shell-use wait exit
```

## Built for agents

`shell-use` is an AI native cli. Point yours at the built-in docs and it can serve itself the rest:

- `shell-use agent-context` prints versioned JSON for every command, flag, enum, default, and exit code. It is generated from the cli, so it cannot drift from the real surface.
- `shell-use usage` prints a one-screen cheatsheet.
- `shell-use skill` prints the full workflow guide ([SKILL.md](SKILL.md)).

### Skill quick start

```sh
shell-use skill --add
```

Adds the `shell-use` skill to the location the user selects in the TUI.

Each command returns a stable exit code (see [Exit codes](#exit-codes)), so an agent can tell an assertion failure from a missing session without scraping text.

## Programmatic usage

`shell-use` python & node client libraries that drive shell-use with the same commands as the cli. The clients manage the sessions for you without a daemon.

### Python ([`shell-use`](bindings/python/README.md))

```sh
pip install shell-use
```

```python
import asyncio
from shell_use import ShellUse

async def main():
    async with ShellUse() as su:
        await su.open()
        await su.submit("echo hello")
        await su.wait_command()
        await su.expect_text("hello")
        await su.expect_exit_code(0)

asyncio.run(main())
```

### Node ([`@microsoft/shell-use`](bindings/js/README.md))

```sh
npm install @microsoft/shell-use # Node 20+

bun add @microsoft/shell-use # Bun (best effort)

deno add npm:@microsoft/shell-use # Deno 2 (best effort)
```

```js
import { ShellUse } from "@microsoft/shell-use";

const su = new ShellUse();
await su.open();
await su.submit("echo hello");
await su.waitCommand();
await su.expectText("hello");
await su.expectExitCode(0);
await su.close();
```

Node is the supported runtime. Bun and Deno compatibility is best effort; Deno requires a local `node_modules` directory and `--allow-ffi` to load the native addon.

## Command reference

Global flags: `--session <name>` (env `SHELL_USE_SESSION`, default `default`), `--json` for machine-readable output, and `--verbose`/`-v` to log PTY traffic (see [Debugging](#debugging)).

### Timeouts

Waits and assertions fall into five timeout classes:

| Class | Applies to | Default |
| --- | --- | --- |
| `text` | `expect text`, `wait text`, `wait bell`, `expect bell` | 5000 ms |
| `idle` | `wait idle` | 5000 ms |
| `command` | `wait command`, `expect exit-code` | 30000 ms |
| `exit` | `wait exit` | 30000 ms |
| `ready` | `wait ready`, and the prompt wait inside `open` | 30000 ms |

`open`'s prompt wait caps at 8000 ms unless you set a `ready` timeout.

Set a session default at `open`, override it per call:

```sh
shell-use open --timeout-text 30000 --timeout-idle 15000 --timeout-ready 20000
shell-use wait text "done" --timeout 60000   # just this call
```

Precedence: `--timeout`, then the session default from `open`/`run`, then
`SHELL_USE_TIMEOUT_<CLASS>_MS` (read when the daemon starts). `shell-use state`
prints a session's effective timeouts.

### Session & lifecycle

| Command                                                      | Description                                 |
| ------------------------------------------------------------ | ------------------------------------------- |
| `open [--shell S] [--cols N --rows N] [--cwd D] [--env K=V] [--timeout-<class> MS]` | Spawn a shell session.                      |
| `run <program> [args...]`                                    | Spawn a session running a program directly. |
| `sessions`                                                   | List active sessions.                       |
| `close [--all]`                                              | Close the current session (or all).         |
| `daemon start` / `daemon status` / `daemon stop --session N \| --all` | Start, inspect, or stop a session's daemon. |

Each session has its own daemon, so `daemon stop` needs `--session <name>` or
`--all`. `close` stops it too.

`open` waits for a prompt before returning, `run` does not. Override with
`--wait-ready` / `--no-wait-ready`. An explicit `--wait-ready` fails (exit 1) if
no prompt appears; `open`'s implicit wait reports `ready` in its payload either
way.

### Inspection

| Command                                             | Description                                                                                 |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `state`                                             | cwd, size, cursor, last command + exit code, bell count, effective timeouts, text snapshot. |
| `text [--full]`                                     | Plain text of the viewport (or scrollback).                                                 |
| `screenshot [-o file.svg] [--full]`                 | Terminal text to stdout, or a crisp full-color SVG image (svg-term-style window) to a file. |
| `cells X Y [W H]`                                   | Per-cell attributes (char, fg, bg, flags).                                                  |
| `get command\|output\|exit-code\|cwd\|cursor\|size\|bells` | Structured getters.                                                                    |

`state` prints `key: value` lines then the screen; `text` and `screenshot`
print the screen bare.

### Input

| Command                                                       | Description                                                  |
| ------------------------------------------------------------- | ------------------------------------------------------------ |
| `type "text"`                                                 | Type literal text.                                           |
| `submit ["text"]`                                             | Type then press the shell return key.                        |
| `press <Key...>`                                              | Named keys, e.g. `press Escape : w q Enter`, `press Ctrl+C`. |
| `keys "Control+a"`                                            | A single key combo.                                          |
| `mouse click X Y` / `mouse click --on-text "OK" [--clicks N]` | Click by coords or label.                                    |
| `mouse move\|down\|up\|drag\|scroll ...`                      | Full mouse control.                                          |

### PTY

| Command                           | Description                      |
| --------------------------------- | -------------------------------- |
| `resize COLS ROWS`                | Resize the PTY and emulator.     |
| `write <data>`                    | Write raw bytes (no return key). |
| `signal INT\|TERM\|KILL` / `kill` | Signal / kill the child.         |

### Wait

| Command                                             | Description                         |
| --------------------------------------------------- | ----------------------------------- |
| `wait text "T" [--regex --full --not --timeout MS]` | Until text is (not) visible.        |
| `wait idle`                                         | Until the screen stops changing.    |
| `wait command`                                      | Until the current command finishes. |
| `wait exit`                                         | Until the session exits.            |
| `wait ready`                                        | Until the shell reports a prompt.   |
| `wait bell`                                         | Until the next terminal bell event. |

### Expect (exit 0 = pass, 1 = fail)

| Command                                                                         | Description                                |
| ------------------------------------------------------------------------------- | ------------------------------------------ |
| `expect text "T" [--regex --full --no-strict --not --fg C --bg C --timeout MS]` | Visibility + optional color.               |
| `expect exit-code N [--timeout MS]`                                             | Last command's exit code.                  |
| `expect output "T" [--regex]`                                                   | Last command's captured output.            |
| `expect bell N [--timeout MS]`                                                   | Cumulative bell count reaches at least N. |
| `expect snapshot NAME [-u] [--include-colors]`                                  | Compare against `__snapshots__/NAME.snap`. |

Colors accept ANSI-256 (`9`), hex (`#ff0000`), or rgb (`255,0,0`).

### Screenshots

Screenshots render a snapshot of the session in the current terminal by default, but can render an SVG using the `-o` output flag. Nerd Font icons are embedded as vector paths, so SVGs remain self-contained without changing the font stack for regular text.

<p align="center">
  <img alt="full-color SVG screenshot of a TUI rendered by shell-use" src="static/screen.svg" width="400">
</p>

### Recording

Every session records automatically from the moment it opens, in the standard
[asciinema v2](https://docs.asciinema.org/manual/asciicast/v2/) cast format.

| Command                   | Description                                     |
| ------------------------- | ----------------------------------------------- |
| `get-recording [session]` | Print the session's recording (cast) to stdout. |

```sh
shell-use get-recording > demo.cast   # capture the current session's recording
asciinema play demo.cast              # replay it
agg demo.cast demo.gif                # render a GIF
```

### Live monitor

Watch a live session in a second terminal while an agent drives it. Both share
the same daemon. `monitor` takes over an alternate screen and streams the
session in full color at ~20fps; press `q`, `Esc`, or `Ctrl-C` to detach.

In-process Python and Node sessions cannot be monitored from another process.

https://github.com/user-attachments/assets/741c985f-7861-41c5-9ceb-0f82f705b43f

| Command   | Description                                                                       |
| --------- | --------------------------------------------------------------------------------- |
| `monitor` | Attach a live, full-color framed view of the session (`--session` selects which). |

```sh
shell-use --session work monitor   # watch the 'work' session live
```

It needs an interactive terminal (exit `2` otherwise) and an existing session
(exit `3` if none). The view reads only the shared screen state, so watching
never blocks the commands the agent is running, and resizing the window just
re-fits the frame.

### Agents

| Command         | Description                                                                                                                           |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `usage`         | Compact command cheatsheet.                                                                                                           |
| `agent-context` | Versioned JSON describing every command, flag, enum, default, and the exit-code taxonomy (generated from the cli, so it can't drift). |
| `skill`         | Long-form workflow guide ([SKILL.md](SKILL.md)).                                                                                      |

### Exit codes

Every command returns a stable exit code so an agent can branch on the failure class without parsing text:

| Code | Meaning                                               |
| ---- | ----------------------------------------------------- |
| `0`  | success                                               |
| `1`  | assertion or wait condition not met (`expect`/`wait`) |
| `2`  | usage / invalid argument                              |
| `3`  | no active session (run `open`/`run` first)            |
| `4`  | daemon or IPC error                                   |
| `5`  | internal error                                        |

With `--json`, failures also carry a `"kind"` field (`assertion`/`usage`/`no_session`/`internal`).

## Supported shells

- bash
- zsh
- fish
- PowerShell (`powershell` and `pwsh`)
- xonsh
- elvish
- nushell
- cmd

## Comparison

|                                      | shell-use                                        | [tui-use](https://github.com/onesuper/tui-use) | [terminal-use](https://github.com/flipbit03/terminal-use) |
| ------------------------------------ | ------------------------------------------------ | ---------------------------------------------- | --------------------------------------------------------- |
| Language                             | Rust                                             | TypeScript/Node                                | Rust                                                      |
| Emulator                             | alacritty                                        | xterm (headless)                               | alacritty                                                 |
| Shell command tracking               | ✅ command boundaries, exit codes, cwd           | ❌                                             | ❌                                                        |
| Testing / snapshots                  | ✅ `expect` text / output / exit-code / snapshot | ❌                                             | ❌                                                        |
| Color & per-cell attributes          | ✅ fg/bg, ANSI-256/hex/rgb, `cells`              | ❌ plain text (+ highlights)                   | via PNG                                                   |
| Image screenshots                    | ✅ SVG                                           | ❌                                             | ✅ PNG                                                    |
| Built-in recording                   | ✅ always-on asciinema cast + GIF                | ❌                                             | ❌                                                        |
| Live monitor view                    | ✅                                               | ❌                                             | ✅                                                        |
| Stable exit-code taxonomy for agents | ✅                                               | ❌                                             | ❌                                                        |
| Python & JavaScript bindings         | ✅                                               | ❌                                             | ❌                                                        |
| Runtime                              | native                                           | Node.js                                        | native                                                    |
| Platforms                            | Windows + Unix                                   | Windows + Unix                                 | Linux / macOS                                             |

## Debugging

By default the daemon writes no log. Start it with `--verbose` to record every byte read from and written to the PTY, plus lifecycle events, to `~/.shell-use/<session>.log`.

## Contributing

This project welcomes contributions and suggestions. Most contributions require you to agree to a
Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us
the rights to use your contribution. For details, visit https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA bot will automatically determine whether you need to provide
a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions
provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft
trademarks or logos is subject to and must follow
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship.
Any use of third-party trademarks or logos are subject to those third-party's policies.
