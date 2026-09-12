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

`restart` replays the last successful spawn, preserving its original working directory, options, and latest terminal size. It sends Ctrl-C and waits up to 5000 ms before forcing replacement; `--graceful-timeout 0` skips the wait. It works after child exit, but not after `close` or daemon shutdown. The terminal and automatic recording start fresh.

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

`--link URL` matches that hyperlink on every cell, including spaces.
Use `--link ""` for unlinked cells. Other style filters ignore spaces.

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
| `state` | Read session state, modes, colors, and text. |
| `text [--full]` | Read terminal text. |
| `cells X Y [W H]` | Read cells and styles. |
| `get FIELD` | Read one field. |
| `screenshot [PATH] [--background COLOR \| --transparent]` | Read text or save SVG or PNG. |

Fields: `command`, `output`, `exit-code`, `cwd`, `cursor`, `modes`, `colors`,
`size`, `title`, `clipboard`, `bells`, and `bell-events`.

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

Interactive input follows the app's keyboard, paste, and mouse modes.
Detaching restores the viewer's modes; read-only monitoring leaves them unchanged.

## Configure

```toml
[profiles.default]
scrollback = 10000

[recording]
mode = "on-failure"
directory = "./artifacts"
```

Recording modes: `disabled`, `on-failure`, and `always`.

### Styling screenshots and recordings

`[recording.style]` sets how screenshots and recordings are drawn. Every key is
optional; the defaults are shown.

```toml
[recording.style]
font_size = 17           # cell width and height follow it
title_font_size = 13
canvas_background = "#6867aa"   # the area around the window
canvas_padding = 24             # the gap around the window, on every side
# content_padding: the gap inside it, per side below (top 8, sides 15, bottom 14)

[recording.style.font]
# A CSS font stack, passed straight into the SVG and read left to right when
# picking faces for a raster recording. The default is:
#   "'Cascadia Code','JetBrains Mono','Fira Code',Menlo,Consolas,'DejaVu Sans Mono',monospace"
family = "Berkeley Mono, monospace"
# bold, italic and bold_italic fall back to family when unset.
bold = "JetBrains Mono ExtraBold"
# Extra fonts to load, on top of the installed ones. A relative path is
# resolved against this config file, so a repository can carry its own font.
files = ["fonts/BerkeleyMono.ttf"]

[recording.style.window]
title_bar = true         # false draws the grid with no chrome at all
traffic_lights = true
background = "#d9d9e8"   # the title bar, not the canvas or the terminal
foreground = "#414145"
divider = "#000000"

[recording.style.border]
width = 0                # 0 draws no border
color = "#000000"
radius = 8               # the panel's corners, border or not

[recording.style.shadow]
enabled = true
color = "#080812"
offset = 5
spread = 7
```

There are two gaps, and each side of either can differ. A table sets the sides
individually, and a side it does not name keeps its own default, so widening
the bottom alone does not collapse the other three:

```toml
# Around the window.
[recording.style.canvas_padding]
bottom = 48

# Between the window and the grid inside it. The default is wider at the sides
# than above and below, because a character sits tight in its cell
# horizontally while the rows already carry their own leading.
[recording.style.content_padding]
top = 16
left = 24
```

Defaults: `canvas_padding` is 24 on every side; `content_padding` is 8 at the
top, 15 at the sides and 14 at the bottom. Either accepts `0`, which trims
that gap entirely — `canvas_padding = 0` leaves the window filling the image,
and `content_padding = 0` puts the first cell against the window edge. With
`title_bar = false`, one number insets the grid evenly on all four sides:

```toml
[recording.style]
content_padding = 15

[recording.style.window]
title_bar = false
```

The terminal's own colors are separate, under `[profiles.<name>.colors]`: a
recording has three backgrounds, and `canvas_background` is the outermost.

Naming a font family that no installed or loaded face provides is not an error.
It falls back, exactly as an unavailable system font does.

Use a monospace font. Cell width is derived from `font_size` by a fixed ratio
and glyphs are scaled horizontally to fit it, so a proportional face draws
distorted rather than overflowing.

`files` are read for recordings, which rasterize the glyphs themselves. An SVG
screenshot can only name a font, so it renders with `family` as installed on
whatever opens it.

### Per-profile recording

A profile may carry its own `[recording]`, inheriting every key it does not
name:

```toml
[recording]
mode = "on-failure"
directory = "./artifacts"

[recording.style]
canvas_background = "#101014"
canvas_padding = 32

[profiles.docs.recording]
mode = "always"

[profiles.docs.recording.style]
font_size = 24
```

The `docs` profile records always, still writes to `./artifacts`, and draws at
24px over the `#101014` canvas with 32px of padding. Inheritance is key by key
at every depth, so naming one style key keeps the rest of the file's look.

A fuller example, where each profile names a different mix:

```toml
# What every profile gets unless it says otherwise.
[recording]
mode = "on-failure"
directory = "./artifacts"

[recording.style]
font_size = 17
canvas_background = "#101014"
canvas_padding = 30

[recording.style.window]
background = "#1a1a22"
foreground = "#d8d8e8"
divider = "#2a2a36"

# Docs: always record, somewhere else, in bigger type.
[profiles.docs.recording]
mode = "always"
directory = "./docs/media"

[profiles.docs.recording.style]
font_size = 24

# CI: keep the file's mode and directory, strip the chrome so artifacts stay small.
[profiles.ci.recording.style]
canvas_padding = 8

[profiles.ci.recording.style.window]
title_bar = false

[profiles.ci.recording.style.shadow]
enabled = false

# Demo: a light look for slides.
[profiles.demo.recording]
mode = "always"

[profiles.demo.recording.style]
canvas_background = "#f6f6f8"
font_size = 20

[profiles.demo.recording.style.window]
background = "#e8e8ef"
foreground = "#33333a"

[profiles.demo.recording.style.border]
width = 2
color = "#8a8aa0"
```

| Profile | mode | directory | what it keeps from the file |
| --- | --- | --- | --- |
| `docs` | `always` | `./docs/media` | the canvas, its padding, the title bar colors |
| `ci` | `on-failure` | `./artifacts` | the font size and the title bar colors it did not name |
| `demo` | `always` | `./artifacts` | the padding and the divider it did not name |

`--profile` selects one, and `default` is what it selects when the flag is
absent. Naming a profile the file does not define is an error, except for
`default` itself.

## Terminal modes

`get modes` and `state.modes` report boolean modes; all keys are present.
`state.mouse_mode` is `none`, `click`, `drag`, or `motion`.
Use `expect mode NAME [--off]` to check a mode.

```sh
tui-test get cursor --json          # x, y, visible, shape, color
tui-test expect cursor --hidden
tui-test expect cursor --visible --shape bar
tui-test expect cursor --x 4 --y 0
tui-test expect mode alternate_screen
tui-test expect mode bracketed_paste --off
```

`expect cursor` checks only the properties you name. Cursor visibility and
wrapping are on by default. For portable alternate-screen tests, use
`CSI ?1049 h` to enter and `CSI ?1049 l` to exit.

## Terminal colors

`get colors` and `state.colors` report `foreground`, `background`, `cursor`,
and `palette` entries that differ from the profile. Unchanged defaults use
the session profile.

```sh
tui-test get colors --json                       # foreground, background, cursor, palette
tui-test expect colors --background '#1d1f21'
tui-test expect colors --foreground 7 --cursor '#ff0000'
tui-test expect colors --palette '1=#00ff00' --palette '200=#123456'
```

Colors accept hex RGB, `r,g,b`, or an ANSI index from the session's palette.
`default` is not accepted. `expect colors` checks the named slots together;
repeat `--palette` to check multiple entries.

## Agent commands

| Command | Use |
| --- | --- |
| `usage` | Short guide. |
| `agent-context` | Exact command schema as JSON. |
| `skill` | Complete agent guide. |
| `skill --add` | Install this skill. |
