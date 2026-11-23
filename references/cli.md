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
| `interrupt` | Cancel pending operations and terminate the child; retain the session. |
| `sessions` | List sessions. |
| `close [--all]` | Close sessions. |

Use `--session NAME` to select a session. `open` and `run` reuse it unless `--restart` is set.

`open` and `run` accept `--screen-history-limit COUNT` to control how many distinct recent screens are retained for failures.

Use `--recording-mode disabled|on-failure|always` with `open` or `run` to control
automatic asciicast capture without enabling a trace. The default is `disabled`.
`on-failure` keeps the cast only when an operation fails or `trace finish failed`
reports a failed outcome; `always` also keeps successful sessions.
`[recording].directory` still selects the destination. Active tracing takes
precedence over this flag, as does an explicit trace setting in `--context-file`.

`restart` replays the last successful spawn, preserving its original working directory, options, and latest terminal size. It sends Ctrl-C and waits up to 5000 ms before forcing replacement; `--graceful-timeout 0` skips the wait. It works after child exit, but not after `close` or daemon shutdown. The terminal and automatic recording start fresh.

`interrupt` calls the same cancellation mechanism as Rust `Session::interrupt`.
It bypasses the operation queue, including a pending startup.
It preserves the daemon and terminal state for inspection.
Use `restart` to start another child, or `close` to release the session.
This command is not Ctrl-C or SIGINT. Use `key press Ctrl+C` or `signal INT` for a terminal interrupt character.

## Tape to GIF

```sh
tui-test tape check demo.tape --json
tui-test tape run demo.tape
```

`check` loads all sources, compiles the complete tape, and checks execution requirements without a child or output writes.
`run` repeats those checks, executes the tape, exports one GIF, and closes its isolated local session.
Both commands report terminal columns and rows, actual pixel dimensions, and source-located warnings.
Relative tape, `Source`, and output paths use the invocation directory, not the tape directory.

Tape commands do not use a daemon, project configuration, or `TUI_TEST_SESSION`.
They reject `--session`, `--verbose`, `--failure-artifacts`, `--failure-artifact-mode`, `--failure-artifact-recording`, `--diagnostic-context`, `--context-file`, and `--operation-name`.
The core tape API does not accept a session execution context or produce the CLI failure bundles.
Use ordinary session commands when you need those diagnostics.
`--json` returns `{"ok":true,"data":{...}}` or `{"ok":false,"error":{...}}` on stdout, without diagnostic text on stderr.
Syntax, unsupported features, and preflight failures return exit code 2.
Execution failures use the core error exit code.

Warning: Run only trusted tapes. Tapes can execute shell commands; they are not sandboxed.
See [Tape](tape.md) for a runnable example, supported commands, and exact result fields.

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

`--link URL` matches that hyperlink on every cell, including spaces.
Use `--link ""` for unlinked cells. Other style filters ignore spaces.

`--link` is a separate constraint, not a style field.
Use a JSON query for style-only, link-only, relative, or composite locators.
The CLI does not use a shell expression language.

`click text` also accepts `--button left|middle|right`, `--alt`, `--ctrl`, `--shift`, `--clicks`, and `--timeout`.

## Nested locator queries

These commands accept the same native Rust `LocatorQuery` JSON:

```sh
tui-test find query --query '{"selector":{"kind":"style","selector":{"style":{"bold":true}}}}'
tui-test resolve query --query-file save.json
tui-test wait query --query-file save.json --timeout 5000
tui-test expect query --query-file save.json --not --timeout 5000
tui-test click query --query-file save.json --button right --ctrl --clicks 2
tui-test highlight query --query-file save.json --timeout 5000
```

Supply exactly one of `--query JSON` or `--query-file PATH`.
The client reads files before it contacts the daemon.
File paths use the client's working directory.
The JSON is a nested query, not the flat `LocatorExpression` node table from the native bindings.

Each query has these fields:

| Field | Values |
| --- | --- |
| `selector` | Required object with `kind` and `selector`, as shown below. |
| `occurrence` | `"any"` (default), `"unique"`, `"first"`, `"last"`, or `{"nth": N}`. |
| `within` | Optional parent query for a leaf selector. |
| `direction` | `"within"` (default), `"after"`, or `"before"`. |
| `style` | Optional whole-match `TextStyle` constraints for a leaf selector. |

Selector objects:

| Kind | Value of the inner `selector` field |
| --- | --- |
| `"text"` | `{"text":"Save","regex":false,"full":false,"whitespace":"exact","scope":{}}` |
| `"style"` | `{"style":{"bold":true},"full":false}` |
| `"link"` | `{"uri":"test:save","full":false}` |
| `"and"` | `{"left":QUERY,"right":QUERY}` |
| `"or"` | `{"left":QUERY,"right":QUERY}` |
| `"filter"` | `{"input":QUERY,"has":QUERY,"has_not":QUERY}` |

For `filter`, omit `has` or `has_not` when not needed.
`and` intersects cells. `or` combines cells. `filter` tests containment.
Composite queries reject parent scope and nonempty whole-match style, just like Rust.
Apply these constraints to leaf operands before composition.
Style and link selectors select contiguous cell runs.
An empty link URI selects cells without a hyperlink.
Set `full: true` on leaf selectors to include scrollback.

Text `whitespace` accepts `"exact"` or `"normalize"`.
Text `scope` accepts optional `after` and `before` anchors.
Each anchor has `text`, optional `regex`, and optional `occurrence` (default: `"unique"`).
This preserves the same anchor behavior as the simple text flags.
Use `within` and `direction` for a parent locator instead of text anchors.

Style fields are `foreground`, `background`, `bold`, `dim`, `italic`, `underline_style`,
`underline_color`, `inverse`, `hidden`, `strikethrough`, and `blink`.
Boolean fields accept both `true` and `false`. Omitted fields impose no constraint.
Use a leaf query's `style` for whole-match constraints.
Use a `"style"` selector for per-cell style matches.

For example, `save.json` can select a linked, bold button after a heading:

```json
{
  "selector": {
    "kind": "and",
    "selector": {
      "left": {
        "selector": {"kind": "text", "selector": {"text": "Save"}},
        "within": {"selector": {"kind": "text", "selector": {"text": "Settings"}}},
        "direction": "after",
        "style": {"bold": true}
      },
      "right": {"selector": {"kind": "link", "selector": {"uri": "test:save"}}}
    }
  },
  "occurrence": "unique"
}
```

`find` returns current locations after occurrence selection. It can return zero or multiple matches.
`resolve` requires exactly one selected match, even with `"occurrence":"any"`.
Both commands reject ambiguous `"unique"` queries.
Both commands support the existing `text` flags as well as `query`.
Waits and actions apply occurrence selection and retry until their timeout.
`wait query` and `expect query` accept `--not` for absence.

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
| `mouse scroll up\|down [--amount N]` | Send mouse wheel input to the application. |
| `scroll up\|down [LINES]` | Move through native history; default: one row. |
| `scroll top\|bottom` | Show the oldest output or the live screen. |
| `resize COLS ROWS` | Resize. |
| `signal NAME` | Send a signal. |

Mouse button options are `--button left|middle|right`, `--alt`, `--ctrl`, and `--shift`.

Signals are `INT`, `TERM`, `KILL`, and `QUIT`.
`INT` sends the terminal interrupt character, like Ctrl-C.
The other choices kill the child through the platform process API.
They do not promise distinct POSIX signal delivery. `kill` is an alias for `signal KILL`.

## Native history

`get viewport --json` returns `{"value":{"offset":0,"history":N}}` within the response data.
The offset counts rows above the live screen. Zero means the live screen.
`history` counts the available history rows.
Scroll amounts must be integers from 0 through 2147483647.

`text`, locators, screenshots, and the monitor use the selected viewport.
`text --full` returns history plus the live screen, independent of that viewport.
New output preserves the selected historical view until retained history expires.
Live output follows automatically when history clears or a resize brings the viewport back to the live screen.
This also applies when resize changes the normal buffer while the alternate buffer is active.
Keyboard input returns to the live screen.
Mouse input and historical locator clicks fail while the viewport shows history.
Use `scroll bottom` before mouse input. Mouse behavior at the live screen does not change.
`get cursor` and `state` retain the application cursor coordinates.
Captures project that cursor into the viewport and hide it outside the view.
Raster recordings capture viewport changes. Asciicast records only the PTY stream.

See the runnable [JavaScript example](../bindings/js/examples/scrollback.mjs)
or the [Rust example](../crates/tui-test/examples/scrollback.rs).

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
| `wait text TEXT [--not]` | Wait for visible or absent text, with the existing selector and style flags. |
| `wait query --query-file PATH [--not]` | Wait for a nested locator query or its absence. |

Most waits accept `--timeout MS`. `expect`, `click`, and `highlight` retry. `find` reads the current screen.

## Inspect

| Command | Use |
| --- | --- |
| `state` | Read session state, modes, colors, and text. |
| `text [--full]` | Read terminal text. |
| `packed-screen [--full] --json` | Read dimensions and untrimmed UTF-8 rows. |
| `cells X Y [W H]` | Read cells and styles. |
| `get FIELD` | Read one field. |
| `-V` / `--version [--json]` | Report tui-test, emulator, and portable-pty versions, plus ConPTY on Windows. |
| `screenshot [PATH] [--background COLOR \| --transparent]` | Read text or save SVG or PNG. |

`-V` and `--version` print the same complete report without a session or daemon.
Use them before a subcommand. `run PROGRAM --version` passes the flag to the program.
Rust package versions come from the build's
resolved dependencies; Ghostty reports its native build version, and xterm.js
reports the vendored headless and Unicode 11 addon versions. On Windows, the
report includes the bundled ConPTY package and file versions without extracting
the bundle or starting a console host.

Windows sessions use tui-test's bundled Microsoft ConPTY host instead of the
system host. The engine extracts and verifies it on first PTY use in a process.
See the [ConPTY asset README](../crates/tui-test/assets/conpty/README.md) for its
source, version, and update instructions.

Fields: `command`, `output`, `exit-code`, `cwd`, `cursor`, `modes`, `colors`,
`size`, `title`, `clipboard`, `bells`, and `bell-events`.

`packed-screen --json` returns `{"ok":true,"data":{"cols":80,"rows":24,"text":"..."}}`.
The text preserves trailing spaces and blank rows. Newline characters separate rows.
Encode `data.text` as UTF-8 to recover the packed bytes.
Byte offsets are not cell offsets for Unicode text.
`--full` includes history plus the live screen, independent of the selected viewport.

## Assert

| Command | Use |
| --- | --- |
| `expect text TEXT` | Assert a locator. |
| `expect query --query-file PATH` | Assert a nested locator. |
| `expect title TEXT` | Assert a title. |
| `expect output TEXT` | Assert command output. |
| `expect exit-code CODE` | Assert the last exit code. |
| `expect bell COUNT` | Wait until the cumulative bell count reaches `COUNT`. |
| `expect snapshot NAME [-u] [--include-style] [--include-title] [--cwd DIR]` | Assert a snapshot. |

Snapshot `--cwd` selects the baseline root. The default is the client's working directory.
The client resolves relative paths before it sends the request.

## Capture

| Command | Use |
| --- | --- |
| `record start PATH [--background COLOR \| --transparent]` | Start a recording. |
| `record stop` | Finish it. |
| `record pause` | Pause APNG, GIF, or MP4 capture while the terminal continues. |
| `record resume` | Resume raster capture from the live terminal without paused time. |
| `get-recording [SESSION]` | Read the automatic asciinema recording. |
| `monitor` | Watch a session live. |
| `monitor --interactive` | Forward keyboard, paste, and supported SGR mouse input; Ctrl+] detaches. |

Interactive monitors mirror the session's keyboard, paste, and mouse modes.

APNG, GIF, and MP4 recordings sample the session's live emulator at `--fps` (default: 30).
GIF export rejects timelines above 100,000 encoded frames, including repeated frames for long holds.
The check occurs before image encoding; a failed export retains its recovery cast.
Output, resize, and viewport changes appear in the next sample.
A state shorter than one sample interval can be missed.
Exports encode these samples without replay through another emulator.

Raster recordings preserve active elapsed time until `record stop`, including quiet periods.
Pause and resume support APNG, GIF, and MP4 only. Asciicast requests return usage errors without changing the recording.
The terminal continues during a pause. Resume captures its current screen, not the intermediate output.
Stop also works during a pause. Automatic diagnostic recordings and recovery casts remain continuous, including output during pauses.
Pause requires an active recording. Resume requires a paused recording.
There is no default idle cap or extra final hold.
`--speed` scales playback time; `--idle-time-limit` explicitly caps each unchanged visual interval before speed scaling.
Cursor blink phases share that interval's idle limit; they do not count as terminal activity.
An idle limit of zero removes those intervals.
Media formats round durations to their supported precision.
Recordings retain frames in memory until export and merge identical samples.

Cast files retain timestamped terminal output and resize events.
FPS sampling does not change this stream or add viewport movements.
The external player selects its own emulator.
The cast does not explicitly represent quiet time after its last event.

This Unix example starts recording inside an alternate screen. The exported
animation preserves the primary screen when the program returns to it.
The pause keeps the alternate screen visible for one second.

```sh
session="live-recording-$$"
tui-test --session "$session" run --backend xtermjs --cols 24 --rows 4 sh -c \
  'stty -echo; printf "\033[?25lPRIMARY\033[?47h\033[H\033[2JALTERNATE"; read -r reply; printf "\033[?1049l"; read -r reply'
tui-test --session "$session" expect text ALTERNATE
tui-test --session "$session" record start live-recording.apng
sleep 1
tui-test --session "$session" key press Enter
tui-test --session "$session" expect text PRIMARY
tui-test --session "$session" record stop
tui-test --session "$session" close
```

## Failure diagnostics

Configure `[trace]` before `open` or `run`. Use `mode = "on"` to retain every session or `"on-failure"` to retain failures. Users can open `trace.html` or replay `session.cast`; agents should read `trace.md`, `trace.json`, and `timeline.json`.

Use `trace finish passed` or `trace finish failed` to supply an explicit test outcome.
This command finishes the trace without closing the session.
Use `close` separately for cleanup.
An explicit passed outcome overrides an earlier recorded failure.
An explicit failed outcome retains an `on-failure` trace for failures outside terminal assertions.

To write artifacts for one assertion:

```sh
tui-test --json \
  --failure-artifacts ./artifacts/failures \
  --diagnostic-context test=settings-save \
  expect text "Save" --fg green --timeout 5000
```

Artifact modes:

| Mode | Files |
| --- | --- |
| `none` | No per-failure files. |
| `text` | `failure.json`, `current.txt`, and `failure.md`. |
| `html` | A standalone `failure.html` with its diagnostic data and evidence embedded. |
| `all` | The text and HTML outputs plus `current.svg` and `timeline.json`. |

`--failure-artifact-recording` adds `session.cast`. Review artifacts before sharing because they can contain terminal output and other user data.

### Execution context

`--operation-name NAME` sets the operation name in traces and failure diagnostics.
`--context-file PATH` reads the native Rust `ExecutionContext` JSON:

```json
{
  "operation_name": "save-settings",
  "artifact": {
    "directory": "artifacts/failures",
    "mode": "text",
    "include_recording": true
  },
  "diagnostic_context": {"test": "settings"},
  "retention": {"screen_history_limit": 3},
  "trace": {"mode": "on-failure", "directory": "artifacts/traces"}
}
```

Every context field is optional. Paths use the client's working directory, not the JSON file's directory.
Explicit `--operation-name` and `--diagnostic-context` flags override corresponding file values.
`--failure-artifacts` replaces the file's artifact options with the artifact flags.
Context trace settings override the profile's trace settings.
Explicit context retention overrides `--screen-history-limit` and profile retention, including zero and the Rust default value.
If the context omits `retention`, `--screen-history-limit` takes precedence over profile retention.
Profile retention takes precedence over the Rust default.
An explicit `"retention": {}` selects the Rust default.
Context applies to terminal operations, not daemon control, local commands, `interrupt`, or `close`.

### Runnable CLI parity example

Run this Unix example from the repository root with a newly built `tui-test` on `PATH`.
It uses state-based waits and closes its named session on exit.
The example preserves its query, context, snapshot, packed screen, and trace under `artifacts/`.

```sh
(
set -eu
session="cli-parity-$$"
out="artifacts/$session"
mkdir -p "$out"
tt() { tui-test --session "$session" "$@"; }
trap 'tt close >/dev/null 2>&1 || :' 0
trap 'exit 1' 1 2 15
cp crates/tui-test-cli/tests/fixtures/locator-query.json "$out/query.json"
cat > "$out/context.json" <<EOF
{"trace":{"mode":"on","directory":"$out/traces"},"diagnostic_context":{"test":"cli-parity"}}
EOF
tt --context-file "$out/context.json" run --backend alacritty --cols 40 --rows 6 -- \
  sh -c 'stty -echo; printf "Settings\r\n\033[1m\033]8;;test:save\007Save\033]8;;\007\033[0m\r\n"; read answer; printf "\033[2J\033[HDone\r\n"; read answer'
tt wait text Settings --timeout 5000
tt --operation-name save-ready wait query --query-file "$out/query.json" --timeout 5000
tt resolve query --query-file "$out/query.json" --json
tt find query --query '{"selector":{"kind":"style","selector":{"style":{"bold":true}}}}' --json
tt highlight query --query-file "$out/query.json"
tt click query --query-file "$out/query.json"
tt packed-screen --json > "$out/screen.json"
tt expect snapshot save --cwd "$out" --update
tt expect snapshot save --cwd "$out"
tt key press Enter
tt wait text Done --timeout 5000
tt wait query --query-file "$out/query.json" --not --timeout 5000
tt expect query --query-file "$out/query.json" --not --timeout 5000
tt trace finish passed
tt interrupt
tt text
printf 'Artifacts: %s\n' "$out"
)
```

## Configure

```toml
[profiles.default]
scrollback = 10000

[recording]
directory = "./casts"

[trace]
mode = "on-failure"
directory = "./traces"

[diagnostics]
screen-history-limit = 10
```

Trace modes: `off`, `on-failure`, and `on`. The default is `off`.
`recording.directory` is optional and independent of `trace.directory`.
The old `recording.mode` field is rejected. Relative directories in a config
file resolve beside that file.

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

[recording.style.cursor]
blink = true
blink_interval_ms = 500
```

`cursor.blink` allows animation when the application's effective cursor style requests blinking.
Set it to `false` for a steady cursor.
`cursor.blink_interval_ms` must be a positive integer.
It sets each visible or hidden phase before playback speed scaling; 500 means a one-second cycle.
Low recording FPS can miss short phases.

Animation starts visible and resets when a sample detects a change in cursor position, style, or visibility.
Hidden and off-screen cursors stay hidden.
Screenshots always use the visible phase of an otherwise-visible cursor.
These settings do not change cursor inspection, waits, or asciicast output.
Profiles inherit and override these fields like other style settings.

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
Unavailable font files also use this fallback.
Raster output loads each canonical file path once.
Custom font files share a total 64 MiB data limit; exceeding this limit returns an error.

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
directory = "./artifacts"

[recording.style]
canvas_background = "#101014"
canvas_padding = 32

[profiles.docs.recording.style]
font_size = 24
```

The `docs` profile still writes to `./artifacts` and draws at 24px over the
`#101014` canvas with 32px of padding. Inheritance is key by key at every
depth, so naming one style key keeps the rest of the file's look. Whether a
recording is kept is not a `[recording]` key: use `--recording-mode` for cast-only
capture, or `[trace] mode` to retain casts alongside trace artifacts.

This also applies when the file uses uniform padding and a profile overrides one side:

```toml
[recording.style]
canvas_padding = 30
content_padding = 20

[profiles.docs.recording.style.canvas_padding]
bottom = 48

[profiles.docs.recording.style.content_padding]
bottom = 48
```

The `docs` profile keeps 30px of canvas padding and 20px of content padding on the other three sides.

A fuller example, where each profile names a different mix:

```toml
# What every profile gets unless it says otherwise.
[recording]
directory = "./artifacts"

[recording.style]
font_size = 17
canvas_background = "#101014"
canvas_padding = 30

[recording.style.window]
background = "#1a1a22"
foreground = "#d8d8e8"
divider = "#2a2a36"

# Docs: record somewhere else, in bigger type.
[profiles.docs.recording]
directory = "./docs/media"

[profiles.docs.recording.style]
font_size = 24

# CI: keep the file's directory, strip the chrome so artifacts stay small.
[profiles.ci.recording.style]
canvas_padding = 8

[profiles.ci.recording.style.window]
title_bar = false

[profiles.ci.recording.style.shadow]
enabled = false

# Demo: a light look for slides.
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

| Profile | directory | what it keeps from the file |
| --- | --- | --- |
| `docs` | `./docs/media` | the canvas, its padding, the title bar colors |
| `ci` | `./artifacts` | the font size and the title bar colors it did not name |
| `demo` | `./artifacts` | the padding and the divider it did not name |

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
