# @microsoft/tui-test

Control, inspect, and test terminal apps from JavaScript or TypeScript.

## Install

```sh
npm install @microsoft/tui-test@beta
```

Node 20+ is supported. The package is ESM only. Bun and Deno support is best effort. Deno 2 needs local `node_modules` and `--allow-ffi`.

## Quick start

```js
import { TuiTest } from "@microsoft/tui-test";

const terminal = TuiTest.ephemeral();

try {
  await terminal.run("my-app");
  await terminal.getByText("Ready").expect();
  await terminal.getByText("Continue").click();
  await terminal.getByText("Done").expect();
} finally {
  await terminal.closeQuiet();
}
```

## API

### `TuiTest`

```ts
new TuiTest(session?: string, options?: ClientOptions)
```

| Option | Type | Default |
| --- | --- | --- |
| `session` | `string` | `TUI_TEST_SESSION` or `"default"` |
| `backend` | `"alacritty" \| "ghostty" \| "rio" \| "xtermjs"` | `"alacritty"` |
| `timeouts` | `Timeouts` | built-in defaults |
| `profile` | `Profile` | built-in profile |
| `screenHistoryLimit` | `number` | `10` |
| `artifacts` | `{ dir, onFailure?, includeRecording? }` | off |
| `recording` | `{ mode?, directory? }` | `{ mode: "always" }` |

`artifacts.onFailure` is `"bundle"`, `"json"`, `"svg"`, `"text"`, or `"none"`. Bundle mode writes `failure.md` for agents, an offline `failure.html` assertion/frame viewer with clickable cell metadata, `failure.json`, `timeline.json`, `current.txt`, and `current.svg`. Error artifact references expose `report`, `report_html`, and `timeline` paths. Checkpoints and sampled frames are bounded; missing frames are shown explicitly. `includeRecording: true` also copies an immutable prefix of the automatic cast for continuous replay. Recording mode is `"disabled"`, `"on-failure"`, or `"always"`.

The shared HTML viewer has a standalone browser suite: from `bindings/js`, run `npx playwright install chromium` then `npm run test:report`. It generates an artifact with the Rust core and opens it offline in Chromium; no native Node addon build is needed. `TUI_TEST_BROWSER_CHANNEL=msedge` can select an installed Edge browser instead. Playwright is a development-only dependency and is not bundled into reports.

The viewer's typed components live in `crates/tui-test/src/diagnostics/viewer`. Run `npm run build:report` to regenerate its minified JavaScript/CSS, and `npm run check:report` to type-check and verify generated assets. These are development tools only; Cargo embeds committed minified files without invoking Node. `recent_operations[].expectation` retains the selected assertion's operands, including successful locators, up to 8 KiB per operation. Treat these operands as potentially sensitive.

Composed locators retain their full query tree in operation expectations. The sidebar renders query descriptions such as `getByText("Docs").getByLink("test:docs")`, `.and(...)`, `.or(...)`, and `.filter({ has: ..., hasNot: ... })` beside actions. Diagnostic stages identify operand paths and aggregate candidate-bounded filter evaluations; only the decisive stage contributes mismatch overlays. `location()` uses the native expression input with final single-match resolution in the core.

`failure.html` can be distributed alone: its Attachments pane embeds the images, Markdown, structured evidence and included recording for offline preview/download. The trace layout shows expected/observed values alongside the terminal and labels the session, emulator, effective timeout defaults and failing assertion timeout. The embedded manifest snapshot excludes the HTML's own hash; the disk manifest includes it.

#### Properties

| Property | Type |
| --- | --- |
| `session` | `string` |
| `keyboard` | `Keyboard` |
| `mouse` | `Mouse` |

#### Lifecycle

| Method | Description |
| --- | --- |
| `TuiTest.ephemeral(prefix?, options?)` | Create a unique session. |
| `open(options?)` | Open a shell. |
| `run(program, args?, options?)` | Run a program. |
| `restart(options?)` | Restart the session. |
| `close()` | Close the session. |
| `closeQuiet()` | Close without throwing. |
| `[Symbol.asyncDispose]()` | Close from `await using`. |

`open()` options are `shell`, `backend`, `cols`, `rows`, `cwd`, `env`, `waitReady`, `restart`, `retries`, `profile`, `timeouts`, and `screenHistoryLimit`. `run()` accepts the same options except `shell`.

The default size is 80 by 30. Timeout defaults are 5 seconds for text and idle, and 30 seconds for command, exit, and ready.

#### Input

| Method | Description |
| --- | --- |
| `submit(text?)` | Type text and press Enter. |
| `type(text)` | Type text. |
| `write(data)` | Write raw bytes. |
| `press(...keys)` | Alias for `keyboard.press()`. |
| `resize(cols, rows)` | Resize the terminal. |
| `signal(name)` | Send `INT`, `TERM`, `KILL`, or `QUIT`. |
| `kill()` | Kill the child process. |

#### State

| Method | Returns |
| --- | --- |
| `state()` | `State` |
| `text({ full? })` | `string` |
| `cells(x, y, w?, h?)` | `Cell[]` |
| `getCommand()` | `string \| null` |
| `getOutput()` | `string \| null` |
| `getExitCode()` | `number \| null` |
| `getCwd()` | `string \| null` |
| `getCursor()` | `Cursor` |
| `getSize()` | `Size` |
| `getTitle()` | `string \| null` |
| `getClipboard()` | `string` |
| `getBellCount()` | `number` |
| `getBellEvents()` | `BellEvent[]` |

#### Waits and assertions

| Method | Description |
| --- | --- |
| `waitTitle(text, { regex?, not?, timeout? })` | Wait for a title. |
| `waitClipboard(pattern?, { timeout? })` | Wait for a clipboard change or match. |
| `waitIdle({ timeout? })` | Wait for the screen to stop changing. |
| `waitCommand({ timeout? })` | Wait for a submitted command. |
| `waitExit({ timeout? })` | Wait for the program to exit. |
| `waitReady({ timeout? })` | Wait for a shell prompt. |
| `waitBell({ timeout? })` | Wait for a bell. |
| `expectTitle(text, { regex?, not?, timeout? })` | Assert the title. |
| `expectExitCode(code, { timeout? })` | Assert the last exit code. |
| `expectOutput(text, { regex? })` | Assert command output. |
| `expectBellCount(count, { timeout? })` | Wait until the cumulative bell count reaches `count`. |
| `expectSnapshot(name, options?)` | Assert or update a snapshot. |

`waitClipboard()` waits for the next change. A string matches text. A `RegExp` keeps its JavaScript flags.

Snapshot options are `update`, `includeStyle`, and `includeTitle`.

#### Capture

| Method | Description |
| --- | --- |
| `screenshot(path?, { full?, zoom?, background?, transparent? })` | Return text or save SVG or PNG. |
| `startRecording(path, options?)` | Start APNG, GIF, MP4, or asciinema recording. |
| `stopRecording()` | Finish the recording and return its path. |

Recording options: `format`, `fps`, `speed`, `idleTimeLimit`, `zoom`, `background`, and `transparent`. MP4 requires `ffmpeg` and does not support transparency.

The extension selects the format: `.png` or `.apng`, `.gif`, `.mp4`, or `.cast`. `format` overrides it.

### `Locator`

Locators resolve against the latest terminal screen before every read or action.

```js
const save = terminal
  .getByText("Settings")
  .getByText("Save", { direction: "after" })
  .getByStyle({ foreground: "green" })
  .unique();

await save.click();
```

#### Create a locator

| Method | Options |
| --- | --- |
| `terminal.getByText(text, options?)` | `regex`, `full`, `whitespace` |
| `terminal.getByStyle(style, options?)` | `full` |
| `terminal.getByLink(uri, options?)` | `full` |
| `locator.getByText(text, options?)` | `regex`, `full`, `whitespace`, `direction` |
| `locator.getByStyle(style, options?)` | `full`, `direction` |
| `locator.getByLink(uri, options?)` | `full`, `direction` |

`whitespace` is `"exact"` or `"normalize"`. `direction` is `"within"`, `"after"`, or `"before"`.

Style fields are `foreground`, `background`, `bold`, `dim`, `italic`, `underlineStyle`, `underlineColor`, `inverse`, `hidden`, `strikethrough`, and `blink`.

`getByLink(uri)` matches an exact OSC 8 target, not visible URL text;
`getByLink("")` requires no link. Root style/link selectors find runs within
each row. Chained calls with the default `within` direction check whole
matches. Styles skip blanks if visible text exists; links check every cell.

#### Compose locators

```js
const link = terminal.getByLink("https://example.com");
const bold = terminal.getByStyle({ bold: true });
const boldLinkCells = bold.and(link);
const either = link.or(terminal.getByText("Help"));
const sections = terminal.getByText("Docs and Help");
const containsLink = sections.filter({ has: link });
const withoutOldText = sections.filter({ hasNot: terminal.getByText("old") });
const entirelyLinked = sections.getByLink("https://example.com");
```

`.and()` keeps shared cells; `.or()` combines cells without duplicates.
Adjacent cells merge within each physical row, even across original matches.
Gaps and row breaks split runs. Counts and clicks use these runs, not
Playwright element identities. Text keeps exact whitespace.

`filter` accepts only locators. `has` requires a match inside each candidate;
`hasNot` requires none. Both conditions apply when supplied, and the inner
match may cover the whole candidate. For partially linked `"Docs"`,
`filter({ has: link })` keeps the whole word, `getByLink(uri)` rejects it,
and `.and(link)` returns its linked cells.

Use locators from one `TuiTest`. Composition leaves them unchanged and reads
one fresh snapshot when used. Selection order matters: `a.first().and(b)`
differs from `a.and(b).first()`. Any `full` branch includes scrollback for the
whole query. Errors, including `unique()` failures, propagate.

#### Select matches

| Method | Description |
| --- | --- |
| `any()` | Keep all matches. |
| `unique()` | Require one match. |
| `first()` | Select the first match. |
| `last()` | Select the last match. |
| `nth(index)` | Select a zero-based match. |

#### Read and act

| Method | Description |
| --- | --- |
| `locations()` | Return all selected locations. |
| `location()` | Return one location. |
| `count()` | Return the current count. |
| `all()` | Return one locator per current match. |
| `wait({ state?, timeout? })` | Wait for `"visible"` or `"hidden"`. |
| `expect({ not?, timeout? })` | Assert the locator. |
| `click(options?)` | Click the middle cell. |
| `highlight({ timeout? })` | Highlight matches. |

`click()` accepts `button`, `alt`, `ctrl`, `shift`, `clicks`, and `timeout`. `button` is `"left"`, `"middle"`, or `"right"`.

`location()` and `click()` require one match. `all()` does not wait.

### `Keyboard`

| Method | Description |
| --- | --- |
| `press(...keys)` | Press keys. |
| `down(...keys)` | Send keydown events. |
| `repeat(...keys)` | Send repeat events. |
| `up(...keys)` | Send keyup events. |

```js
await terminal.keyboard.press("Ctrl+C");
await terminal.keyboard.press("Escape", ":", "w", "q", "Enter");
```

Named keys include `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Insert`, `Delete`, `Backspace`, `Tab`, `Enter`, `Space`, `Escape`, and `F1` through `F12`. Join modifiers such as `Ctrl`, `Alt`, `Shift`, `Super`, `Meta`, or `Hyper` with `+`.

### `Mouse`

Coordinates are zero-based terminal cells.

| Method | Description |
| --- | --- |
| `click(x?, y?, options?)` | Click a cell or `onText`. |
| `move(x, y)` | Move the pointer. |
| `down(x, y, options?)` | Press a button. |
| `up(x, y, options?)` | Release a button. |
| `drag(x1, y1, x2, y2, options?)` | Drag between cells. |
| `scroll("up" \| "down", { amount? })` | Scroll. |

Button options are `button`, `alt`, `ctrl`, and `shift`. Click also accepts `onText` and `clicks`.

```js
await terminal.mouse.click(10, 5, { button: "right", ctrl: true });
```

### Module functions

| Function | Description |
| --- | --- |
| `sessions()` | List sessions in this process. |
| `closeAll()` | Close all sessions in this process. |
| `getRecording(session?)` | Return an automatic asciinema recording. |
| `uniqueSession(prefix?)` | Create a unique session name. |

### Test helpers

Import from `@microsoft/tui-test/test`.

| Function | Description |
| --- | --- |
| `createTerminal(options?)` | Create, open, and track a terminal. |
| `withTerminal(options, callback)` | Run a callback and close the terminal. |
| `closeAllTracked()` | Close tracked terminals. |
| `setTerminalDefaults(options)` | Set suite defaults. |
| `resetTerminalDefaults()` | Reset suite defaults. |
| `trackTerminal(terminal)` | Track a terminal. |
| `untrackTerminal(terminal)` | Stop tracking a terminal. |
| `trackedCount()` | Count tracked terminals. |
| `terminalSnapshot(text)` | Normalize text for snapshots. |

`CreateTerminalOptions` adds `shell`, `program`, `session`, and `prefix` to the client and spawn options.

`defaultShell` is the platform default.

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({ program: ["my-app"] }, async (terminal) => {
  await terminal.getByText("Ready").expect();
});
```

### Configuration

```js
const terminal = new TuiTest("test", {
  profile: {
    scrollback: 500,
    colors: { foreground: "#ffffff", background: "#000000" },
  },
  timeouts: { text: 10_000, command: 60_000 },
  screenHistoryLimit: 10,
  artifacts: {
    dir: "artifacts/failures",
    onFailure: "bundle",
    includeRecording: true,
  },
  recording: { mode: "on-failure", directory: "artifacts" },
});
```

### Types

| Type | Description |
| --- | --- |
| `State` | Session state and visible text. |
| `Cell` | One terminal cell and its style. |
| `TextMatch` | Matched text, positions, and spans. |
| `TextStyleExpectation` | Locator style fields. |
| `Profile`, `Colors` | Scrollback and colors. |
| `Timeouts` | Text, idle, command, exit, and ready timeouts. |
| `AutomaticRecording` | Automatic recording mode and directory. |
| `FailureDetails` | Structured operation, locator, process, runtime, and screen evidence. |
| `FailureArtifactRef` | Paths and write status for a failure artifact. |
| `MouseButton` | `"left"`, `"middle"`, or `"right"`. |
| `MouseClickOptions` | Button, modifiers, target text, and click count. |
| `MouseButtonOptions` | Button and modifiers. |
| `LocatorClickOptions` | Button, modifiers, click count, and timeout. |
| `OpenResult` | Session start result. |

`VERSION` contains the package version.

### Errors

| Error | `exitCode` |
| --- | --- |
| `ExpectationError` | `1` |
| `UsageError` | `2` |
| `NoSessionError` | `3` |
| `InternalError` | `5` |

All errors extend `TuiTestError` and include `kind` and `exitCode`. Structured native failures expose `details` and `artifact`; expectation errors continue to populate compatibility `terminal.text` and `terminal.screenshot` fields. Failure artifacts can contain terminal output, titles, locator operands, and recordings, so review them before uploading.

Sessions are local to the current process and cannot be controlled by the CLI. Cancelling a promise does not stop an active terminal operation.
