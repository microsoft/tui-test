# CLI

Use the CLI for terminal work split across separate commands.

[Back to the skill](../SKILL.md)

## Start

| Command | Use |
| --- | --- |
| `open [options]` | Open a shell. |
| `run [options] PROGRAM [ARGS...]` | Run an app. |
| `[global options] -- PROGRAM [ARGS...]` | Alias for `run`. |
| `restart [--graceful-timeout MS]` | Restart the session. |
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

Style options: `--fg`, `--bg`, `--bold`, `--dim`, `--italic`, `--underline-style`, `--underline-color`, `--inverse`, `--hidden`, `--strikethrough`, `--blink`, and `--link`.

`--link` matches a cell's OSC 8 target: `--link https://example.com` requires that link, and `--link ""` requires a cell that links nowhere. It applies to every cell of the match, blanks included, because a space inside a link is part of the link. The appearance options skip blanks, which cannot show them, so `A B` linked throughout with only the letters bold matches `--bold --link ...` just as it matches either alone.

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
| `state` | Read session state, terminal modes, and text. |
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
| `expect snapshot NAME [-u] [--include-style] [--include-title]` | Assert a snapshot. |

## Capture

| Command | Use |
| --- | --- |
| `record start PATH [--background COLOR \| --transparent]` | Start a recording. |
| `record stop` | Finish it. |
| `get-recording [SESSION]` | Read the automatic asciinema recording. |
| `monitor` | Watch a session live. |
| `monitor --interactive` | Forward keyboard, paste, and supported SGR mouse input; Ctrl+] detaches. |

Interactive monitors apply the target's keyboard and paste modes before reading
input. SGR mouse clicks, drags, and motion are enabled when requested by the
target, with coordinates translated past the monitor's border. Viewer modes are
restored on detach; read-only monitoring does not change input modes.

## Configure

```toml
[profiles.default]
scrollback = 10000

[recording]
mode = "on-failure"
directory = "./artifacts"
```

Recording modes: `disabled`, `on-failure`, and `always`.

## Terminal modes

`state` reports the modes the child has turned on, under `modes`:

| Key | Sequence | Meaning |
| --- | --- | --- |
| `application_cursor_keys` | `CSI ?1 h` | cursor keys send `SS3` |
| `cursor_visible` | `CSI ?25 h` | the cursor is drawn (on by default) |
| `application_keypad` | `ESC =` | keypad sends application sequences |
| `origin` | `CSI ?6 h` | cursor confined to the scroll region |
| `wraparound` | `CSI ?7 h` | text wraps at the right margin (on by default) |
| `insert` | `CSI 4 h` | printed text shifts the line right |
| `focus_events` | `CSI ?1004 h` | focus changes are reported to the child |
| `bracketed_paste` | `CSI ?2004 h` | pastes are bracketed |
| `alternate_screen` | `CSI ?1049 h` | the alternate screen is showing |

The `Sequence` column names one way to reach each mode, not every one.
`alternate_screen` reports whether the alternate screen is showing, however it
was reached: `CSI ?1049 h` is what a full-screen program sends and every
backend honors it, while the older `CSI ?47 h` and `CSI ?1047 h` are honored
by the ghostty and xterm.js backends and ignored by alacritty and rio. Prefer
`?1049` in a test that has to behave the same everywhere. `CSI ?1049 l` leaves
the alternate screen whichever sequence entered it.

Mouse tracking is reported separately, as `mouse_mode`: `none`, `click`,
`drag`, or `motion`. It is not in the table because it is not a set of
independent switches — `CSI ?1002 h` replaces `CSI ?1000 h` rather than
joining it, so booleans would claim two are on when only the last is honored.
It reports the tracking level regardless of how the child asked for the
reports to be encoded, so `CSI ?1000 h` alone is `click` whether or not
`CSI ?1006 h` followed it.

Read them with `get modes`, and assert one with `expect mode <NAME> [--off]`.
The cursor has its own command, since position and shape have nowhere else to
live:

```sh
tui-test get cursor --json          # x, y, visible, shape, color
tui-test expect cursor --hidden
tui-test expect cursor --visible --shape bar
tui-test expect cursor --x 4 --y 0
tui-test expect mode alternate_screen
tui-test expect mode bracketed_paste --off
```

`expect cursor` checks only the properties you name, so asserting a shape
leaves visibility and position alone.

Every key is always present, so `false` means off rather than unknown. The set
is deliberately closed: a mode is listed only when all four backends report it
identically, which the conformance suite checks.

## Terminal colors

`state` reports the colors the terminal is painting with, under `colors`:

| Key | Sequence | Meaning |
| --- | --- | --- |
| `foreground` | `OSC 10` | the default foreground |
| `background` | `OSC 11` | the default background |
| `cursor` | `OSC 12` | the cursor color |
| `palette` | `OSC 4` | palette entries a program overrode, keyed by index |

The three defaults are always reported, resolved through the profile so a slot
nothing has touched still has an answer. `palette` lists only the entries that
differ from the profile, so it names what a program changed rather than all
256 slots, and `OSC 104` empties it again.

Read them with `get colors`, and assert them with `expect colors`:

```sh
tui-test get colors --json                       # foreground, background, cursor, palette
tui-test expect colors --background '#1d1f21'
tui-test expect colors --foreground 7 --cursor '#ff0000'
tui-test expect colors --palette '1=#00ff00' --palette '200=#123456'
```

Every color takes the same spellings `--fg` does — a hex value (`#rrggbb`),
an RGB triple (`r,g,b`), or an ANSI index — except `default`, which has
nothing to refer to here since these slots *are* the defaults. An index is
resolved against the session's own palette, so `--background 0` means the
black this profile paints.

`expect colors` checks only the slots you name, and `--palette` is repeatable.
All of them are matched together, so a program that recolors several at once
is asserted as one state rather than a race between polls.

## Agent commands

| Command | Use |
| --- | --- |
| `usage` | Short guide. |
| `agent-context` | Exact command schema as JSON. |
| `skill` | Complete agent guide. |
| `skill --add` | Install this skill. |
