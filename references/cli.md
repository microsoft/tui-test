# CLI

Use the CLI for terminal work split across separate commands.

[Back to the skill](../SKILL.md)

## Start

| Command | Use |
| --- | --- |
| `open [options]` | Open a shell. |
| `run [options] PROGRAM [ARGS...]` | Run an app. |
| `sessions` | List sessions, including monitored tests. |
| `close [--all]` | Close sessions. |

Use `--session NAME` to select a session. `open` and `run` reuse it unless `--restart` is set.

## Locate text

```sh
tui-test find text "Save"
tui-test expect text "Save" --fg green
tui-test click text "Save" --after-text "Settings"
tui-test highlight text 'item \d+' --regex
```

Common options:

| Option | Use |
| --- | --- |
| `--regex` | Match a regular expression. |
| `--full` | Include scrollback. |
| `--whitespace exact\|normalize` | Match whitespace. |
| `--after-text TEXT` | Search after an anchor. |
| `--before-text TEXT` | Search before an anchor. |
| `--match any\|unique\|first\|last` | Select matches. |
| `--nth N` | Select a zero-based match. |

Style options: `--fg`, `--bg`, `--bold`, `--dim`, `--italic`, `--underline-style`, `--underline-color`, `--inverse`, `--hidden`, `--strikethrough`, and `--blink`.

`click text` also accepts `--button left|middle|right`, `--alt`, `--ctrl`, `--shift`, `--clicks`, and `--timeout`.

## Send input

| Command | Use |
| --- | --- |
| `submit [TEXT]` | Type and press Enter. |
| `type TEXT` | Type text. |
| `write DATA` | Write raw bytes. |
| `key press KEYS...` | Press keys. |
| `key down\|repeat\|up KEYS...` | Send key events. |
| `mouse click [X Y] [options]` | Click a cell or `--on-text`. |
| `mouse move X Y` | Move the pointer. |
| `mouse down\|up X Y [options]` | Press or release a button. |
| `mouse drag X1 Y1 X2 Y2 [options]` | Drag. |
| `mouse scroll up\|down [--amount N]` | Scroll. |
| `resize COLS ROWS` | Resize. |
| `signal NAME` | Send a signal. |

Mouse button options are `--button left|middle|right`, `--alt`, `--ctrl`, and `--shift`.

## Wait

| Command | Use |
| --- | --- |
| `wait command` | Wait for `submit`. |
| `wait exit` | Wait for `run`. |
| `wait ready` | Wait for a prompt. |
| `wait idle` | Wait for the screen to settle. |
| `wait title TEXT` | Wait for a title. |
| `wait clipboard [TEXT]` | Wait for a clipboard change or match. |
| `wait bell` | Wait for a bell. |

Most waits accept `--timeout MS`. `expect`, `click`, and `highlight` retry. `find` reads the current screen.

## Inspect

| Command | Use |
| --- | --- |
| `state` | Read session state and text. |
| `text [--full]` | Read terminal text. |
| `cells X Y [W H]` | Read cells and styles. |
| `get FIELD` | Read one field. |
| `screenshot [PATH]` | Read text or save SVG. |

Fields: `command`, `output`, `exit-code`, `cwd`, `cursor`, `size`, `title`, `clipboard`, `bells`, and `bell-events`.

## Assert

| Command | Use |
| --- | --- |
| `expect text TEXT` | Assert a locator. |
| `expect title TEXT` | Assert a title. |
| `expect output TEXT` | Assert command output. |
| `expect exit-code CODE` | Assert the last exit code. |
| `expect bell COUNT` | Wait until the cumulative bell count reaches `COUNT`. |
| `expect snapshot NAME [-u] [--include-colors] [--include-title]` | Assert a snapshot. |

## Capture

| Command | Use |
| --- | --- |
| `record start PATH` | Start a recording. |
| `record stop` | Finish it. |
| `get-recording [SESSION]` | Read the automatic asciinema recording. |
| `monitor` | Watch a session; q, Esc, or Ctrl+C detaches. |
| `monitor --interactive` | Forward keyboard, paste, and supported SGR mouse input; Ctrl+] detaches. |

Interactive monitors apply the target's keyboard and paste modes before reading
input. SGR mouse clicks, drags, and motion are enabled when requested by the
target, with coordinates translated past the monitor's border. Viewer modes are
restored on detach; read-only monitoring does not change input modes.

Both modes resize the app to fit inside the border. Interactive viewers take
priority. Detaching restores the previous viewer's size. A yellow border and
`! too small` warn that content is clipped.

## Inspect tests

Monitoring is off by default. Enable it in your
[JavaScript](javascript.md#inspect-failed-tests),
[Python](python.md#inspect-failed-tests), or [Rust](rust.md#inspect-failed-tests)
tests to pause them on failure. In another terminal, run:

```sh
tui-test sessions --waiting
tui-test --session login monitor --interactive
```

Use the session name unless several sessions share it. For duplicate names,
copy the UUID from `sessions` and run `tui-test monitor --interactive --id UUID`.
With no target, `monitor` opens a searchable picker in an interactive terminal.
Filter sessions with `--waiting`, `--failed`, or `--cwd current`;
`monitor --latest` selects the most recent match.

By default, the test waits up to 30 seconds for a monitor to attach. If none
attaches, the test resumes. Otherwise, it waits until all monitors detach.
Press Ctrl+] in interactive mode or q, Esc, or Ctrl+C in read-only mode.
Keep the test process running while you inspect it.

API options override these environment defaults:

| Variable | Default | Values |
| --- | --- | --- |
| `TUI_TEST_MONITORING` | disabled | `1` to enable. |
| `TUI_TEST_WAIT_AT_END` | `never` | `never`, `failure`, `always`. |
| `TUI_TEST_FIRST_ATTACH_TIMEOUT` | `30000` | Milliseconds; `infinite` waits indefinitely. |
| `TUI_TEST_LABEL` | unset | Display label. |

Use unlimited waits only for local debugging.

## Configure

```toml
[profiles.default]
scrollback = 10000

[recording]
mode = "on-failure"
directory = "./artifacts"
```

Recording modes: `disabled`, `on-failure`, and `always`.

## Agent commands

| Command | Use |
| --- | --- |
| `usage` | Short guide. |
| `agent-context` | Exact command schema as JSON. |
| `skill` | Complete agent guide. |
| `skill --add` | Install this skill. |
