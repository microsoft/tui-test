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

`open` and `run` accept `--screen-history-limit COUNT` to control how many distinct recent screens are retained for failures.

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

Style options: `--fg`, `--bg`, `--bold`, `--dim`, `--italic`, `--underline-style`, `--underline-color`, `--inverse`, `--hidden`, `--strikethrough`, and `--blink`.

`--link URL` matches that hyperlink on every cell, including spaces.
Use `--link ""` for unlinked cells. Other style filters ignore spaces.

`--link` is a separate whole-match constraint, not a style field. Native
Rust/JavaScript/Python locators also support cell-set AND/OR and locator-based
containment filters; the CLI does not expose a shell expression syntax for
those operations. Locator protocol requests carry expression queries rather
than native binding stage arrays.

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

Interactive monitors mirror the session's keyboard, paste, and mouse modes.

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

The action list displays API-style query descriptions beside the operation, such as `getByText("Save")`, `getByStyle({ bold: true }).and(getByLink("test:docs"))`, and `.filter({ has: ..., hasNot: ... })`. These are reconstructed descriptions, not captured source code. Long expressions are ellipsized in the row but remain available in its tooltip, search, and Call evidence.

Locator diagnostics follow the expression tree. Each `stages[]` entry has an `expression_path` (`root`, `.within`, `.left`, `.right`, `.input`, `.has`, `.has_not`), `mode`, counts, and `evaluations`. Containment predicates are evaluated inside each candidate; repeated evaluations are aggregated by path. Counts on such entries are totals, not distinct whole-terminal candidates. Compound entries do not repeat their operand trees in `selector`; leaf selectors are retained when within the evidence budget.

Only the decisive failing stage supplies cell mismatch highlights. An empty union operand or an empty `has_not` search is not itself a failure. Explicit uniqueness errors propagate from the offending operand and cannot be hidden by OR or negation. `link_filter_removed_all`, `intersection_empty`, `union_empty`, and `filter_removed_all` distinguish whole-match link refinement, cell-set composition, and containment rejection. At most 128 stage entries with an 8 KiB evidence budget each are retained, with sample/trace truncation flags when limited.

The CLI daemon protocol is version 4. JS and Python native queries use one validated typed node-table/root expression input; the old linear-stage query format and style-link aliases are not accepted. `location()` requests final single-match resolution in the core, preserving explicit occurrence ordering on operands. The trace formatter handles text, style, exact-URI links, relative chains, AND/OR, filters, and occurrence placement without flattening the expression.

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
