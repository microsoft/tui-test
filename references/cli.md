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

The HTML embeds its scripts, styles, original frame images, cell metadata, Markdown and all available attachments as data. Attachments can be previewed and downloaded after the original directory is deleted; there are no sibling-file or network dependencies. File contents are checked against their recorded hashes before embedding. The embedded `failure.json` is a manifest snapshot taken before the HTML write, so it cannot include the HTML's own hash; the disk manifest adds that final entry. The viewer crops screenshot chrome for clarity, while SVG downloads preserve the original image. There are no before/after operation tabs: actions select their completion checkpoint, with individual retained frames available separately.

The viewer uses tui-test's original emulator grids and SVG renderer, rather than reconstructing cell state in a second emulator. Asciinema-player supports seeking and markers, but does not expose cell metadata in its public API. Add `--failure-artifact-recording` to copy an immutable `session.cast` prefix through the failure boundary for continuous replay in an asciicast player. Recording is not required for frame inspection.

Retention is bounded: at most 32 recent operations, 10 recent sampled screens by default (0-50 configurable, also bounded to 512 KiB with the current screen always retained), and 32 distinct assertion/wait checkpoints bounded to 8 MiB. Setting `--screen-history-limit 0` disables historical checkpoint retention too. Passing checkpoints capture the screen at operation return; failure uses the pinned evaluation. Sampled screens do not represent every PTY write. Missing frames, evicted checkpoints, and oversized grids are reported explicitly, never substituted or interpolated. Play advances retained frames every 400 ms, stopping at the next retained checkpoint, not at original recording speed.

The timeline is capped at 8 MiB, HTML at 128 MiB, Markdown at 1 MiB, and the whole bundle at 256 MiB. The HTML allowance includes base64-encoded evidence, including an optional recording of up to 64 MiB. Newest frames get the timeline budget first. Individual oversized SVGs or cell grids can be omitted while screen text and operation evidence remain; omissions are explicit. Attachment text previews are bounded to 512 KiB, with complete bytes available via Download. Other artifact modes (`json`, `svg`, `text`, and `none`) retain their existing behavior and do not generate the viewer.

Failure bundles can contain locator operands, terminal output, titles, screenshots, snapshot evidence, diagnostic context, and recordings. Review them before uploading.

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

## Agent commands

| Command | Use |
| --- | --- |
| `usage` | Short guide. |
| `agent-context` | Exact command schema as JSON. |
| `skill` | Complete agent guide. |
| `skill --add` | Install this skill. |
