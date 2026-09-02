# CLI

Use the CLI for terminal work split across separate commands.

[Back to the skill](../SKILL.md)

## Start

| Command | Use |
| --- | --- |
| `open [options]` | Open a shell. |
| `run [options] PROGRAM [ARGS...]` | Run an app. |
| `sessions` | List sessions. |
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
| `screenshot [PATH] [--background COLOR \| --transparent]` | Read text or save SVG. |

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
| `record start PATH [--background COLOR \| --transparent]` | Start a recording. |
| `record stop` | Finish it. |
| `get-recording [SESSION]` | Read the automatic asciinema recording. |
| `monitor` | Watch a session live. |

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
