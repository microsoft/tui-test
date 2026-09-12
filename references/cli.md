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

`open` and `run` accept `--screen-history-limit COUNT` to control how many distinct recent screens are retained for failures.

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
| `expect snapshot NAME [-u] [--include-style] [--include-title]` | Assert a snapshot. |

## Capture

| Command | Use |
| --- | --- |
| `record start PATH` | Start a recording. |
| `record stop` | Finish it. |
| `get-recording [SESSION]` | Read the automatic asciinema recording. |
| `monitor` | Watch a session live. |
| `monitor --interactive` | Forward keyboard, paste, and supported SGR mouse input; Ctrl+] detaches. |

Interactive monitors apply the target's keyboard and paste modes before reading
input. SGR mouse clicks, drags, and motion are enabled when requested by the
target, with coordinates translated past the monitor's border. Viewer modes are
restored on detach; read-only monitoring does not change input modes.

## Failure diagnostics

Assertion failures always include structured `details` in `--json` output. The details identify the operation, final reason, locator stages and candidate counts, style mismatches, process/runtime state, recent operations, and a bounded history of distinct screens.

Write an offline artifact bundle:

```sh
tui-test --json \
  --failure-artifacts ./artifacts/failures \
  --diagnostic-context test=settings-save \
  expect text "Save" --fg green --timeout 5000
```

Bundle mode writes:

| File | Consumer |
| --- | --- |
| `failure.md` | Agent-first diagnosis, expected/actual values, assertion checkpoint links, full retained screen text, style mismatches, runtime/context, and omissions. |
| `failure.html` | Standalone trace viewer with a duration timeline, frame filmstrip, filterable Actions / Metadata sidebar, and Error / Cell / Call / Attachments panes. It starts at the pinned failure with explicit expected/observed values. Click actions or frames to navigate, and click terminal cells (or use arrow keys / coordinate inputs) to inspect their metadata. |
| `failure.json` | Authoritative versioned manifest, committed last, including artifact hashes, sizes, omissions, sensitivity, and write errors. |
| `timeline.json` | Versioned frame data used by the viewer: each `frames[]` entry has screen metadata, `svg`, a `cells[]` dictionary, and `grid[row][column]` indices into that dictionary. Cell colors retain `default`, ANSI indices, or RGB strings alongside resolved colors; widths distinguish wide glyphs from continuation cells. |
| `current.txt`, `current.svg` | The pinned failure screen. |

Artifact references use `report` for `failure.md`, `report_html` for `failure.html`, and `timeline` for `timeline.json`. Structured terminal history keeps recent samples in `screen_history.screens` and pinned assertion screens in `screen_history.checkpoints`; reports merge these by screen sequence. `runtime.session_name`, `runtime.shell`, and `runtime.timeouts` capture session identity and all effective timeout defaults; `operation.timeout_ms` is the failing operation's timeout, including a per-call override.

Each retained action can carry `expectation`: a typed locator query plus its required outcome, a scalar subject/value, or an explicit unavailable reason. Passing assertions show their own selector, style, scope, occurrence and negation in the action list, banner and Call pane; they never borrow the failing assertion's expectation. Timeout values remain in Metadata, not the top header.

The HTML embeds its scripts, styles, original frame images, cell metadata, Markdown and all available attachments as data. Attachments can be previewed and downloaded after the original directory is deleted; there are no sibling-file or network dependencies. File contents are checked against their recorded hashes before embedding. The embedded `failure.json` is a manifest snapshot taken before the HTML write, so it cannot include the HTML's own hash; the disk manifest adds that final entry. The viewer crops screenshot chrome for clarity, while SVG downloads preserve the original image. There are no before/after operation tabs: actions select their completion checkpoint, with individual retained frames available separately.

The viewer uses tui-test's original emulator grids and SVG renderer, rather than reconstructing cell state in a second emulator. Asciinema-player supports seeking and markers, but does not expose cell metadata in its public API. Add `--failure-artifact-recording` to copy an immutable `session.cast` prefix through the failure boundary for continuous replay in an asciicast player. Recording is not required for frame inspection.

Retention is bounded: at most 32 recent operations (each expectation limited to 8 KiB), 10 recent sampled screens by default (0-50 configurable, also bounded to 512 KiB with the current screen always retained), and 32 distinct assertion/wait checkpoints bounded to 8 MiB. Oversized expectations are replaced with an explicit unavailable reason, not a partial or inferred assertion. Setting `--screen-history-limit 0` disables historical checkpoint retention too. Passing checkpoints capture the screen at operation return; failure uses the pinned evaluation. Sampled screens do not represent every PTY write. Missing frames, evicted checkpoints, and oversized grids are reported explicitly, never substituted or interpolated. Play advances retained frames every 400 ms, stopping at the next retained checkpoint, not at original recording speed.

The timeline is capped at 8 MiB, HTML at 128 MiB, Markdown at 1 MiB, and the whole bundle at 256 MiB. The HTML allowance includes base64-encoded evidence, including an optional recording of up to 64 MiB. Newest frames get the timeline budget first. Individual oversized SVGs or cell grids can be omitted while screen text and operation evidence remain; omissions are explicit. Attachment text previews are bounded to 512 KiB, with complete bytes available via Download. Other artifact modes (`json`, `svg`, `text`, and `none`) retain their existing behavior and do not generate the viewer.

Failure bundles can contain operands from both successful and failed assertions, terminal output, hyperlink targets, titles, screenshots, snapshot evidence, diagnostic context, and recordings. Raw typed input and environment values are not added to operation history. Review bundles before uploading.

### Viewer development

The viewer is split into typed components under `crates/tui-test/src/diagnostics/viewer`: actions/timeline, terminal rendering, cell inspection, call/expectation details, metadata and attachments. `index.ts` only coordinates selection and playback. The DOM implementation has no framework runtime.

From `bindings/js`, run `npm run build:report` after changing viewer TypeScript or `report.css`. This uses a pinned esbuild version to produce the committed `report.min.js` and `report.min.css` assets that Rust embeds. `npm run check:report` type-checks the components and fails if generated assets are stale; CI runs it before the browser suite. Cargo builds use the checked-in assets and do not require Node or npm.

## Configure

```toml
[profiles.default]
scrollback = 10000

[recording]
mode = "on-failure"
directory = "./artifacts"

[diagnostics]
screen-history-limit = 10
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
