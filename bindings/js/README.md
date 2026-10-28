# @microsoft/tui-test

Control, inspect, and test terminal apps from JavaScript or TypeScript.

## Install

```sh
npm install @microsoft/tui-test
```

Node 20+ is supported. The package is ESM only. Bun and Deno support is best effort. Deno 2 needs local `node_modules` and `--allow-ffi`.

Each platform package contains the `tui-test-ffi` shared library.
The JavaScript adapter uses Koffi to call the same typed C ABI as Go and Python.
It copies native results before it frees their Rust allocations.
The library stays loaded until process exit on all supported platforms.
Native operations can finish after JavaScript worker exit.
Native thread teardown releases unclaimed task handles and results without canceling operations or closing shared sessions.
Each binding copy uses its own anonymous Koffi layouts, so copies can share one Koffi module without type-name collisions.
Koffi supplies its own native addon; this package no longer builds a separate N-API adapter.
Keep optional dependencies enabled during installation; they contain the platform binaries for both packages.
The package needs no CLI.
Set `TUI_TEST_NATIVE_LIBRARY` to an absolute path to select a library from the same source revision.

For a local build, run `npm ci`, then `npm run build`.
Use `npm run build:native:debug` for a debug build.
These commands build `tui-test-ffi` and put the shared library in `native/`.
Run `cargo run --locked -p tui-test-bindgen` from the repository root to regenerate the native declarations.

For a distributable GNU Linux build, use:

```sh
npm run build:native -- --zig --target x86_64-unknown-linux-gnu
```

This requires Docker and builds the shared engine inside manylinux 2.28.
The container constrains both Cargo and Ghostty to the supported libc.
Use `aarch64-unknown-linux-gnu` for ARM64, with Docker emulation on an x86_64 host.
Musl builds with `--zig` still use the local Rust and Zig toolchains.

## Quick start

```js
import { TuiTest } from "@microsoft/tui-test";

const terminal = TuiTest.ephemeral();

try {
  await terminal.run(process.execPath, [
    "-e",
    "console.log('Ready'); setInterval(() => {}, 1000)",
  ]);
  await terminal.getByText("Ready").expect();
  console.log(await terminal.text());
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
| `backend` | `"alacritty" \| "ghostty" \| "rio" \| "xtermjs" \| "windows-terminal"` | `"alacritty"` |
| `timeouts` | `Timeouts` | built-in defaults |
| `profile` | `Profile` | built-in profile |
| `screenHistoryLimit` | `number` | `10` |
| `artifacts` | `{ dir, onFailure?, includeRecording? }` | off |
| `recording` | `{ directory? }` | default recording directory |
| `trace` | `{ mode?, directory? }` | `{ mode: "off", directory: ".tui-test/traces" }` |

`windows-terminal` requires a [custom Windows native library](../../crates/tui-test/native/windows-terminal/README.md)
built with that feature; it is not included in official native packages.

Set `trace.mode` to `"on"` for every session or `"on-failure"` for failures. Users can open `trace.html`; agents should read `trace.md`, `trace.json`, and `timeline.json`.

`artifacts.onFailure` selects `"none"`, `"text"`, `"html"`, or `"all"`. Use `includeRecording` to include the cast in failure artifacts.

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
| `scroll(lines)` | Move through native history; positive values show older output. |
| `scrollToTop()` / `scrollToBottom()` | Show the oldest output or the live screen. |
| `getViewport()` | Return `{ offset, history }`, in rows. |
| `signal(name)` | Send `INT`, `TERM`, `KILL`, or `QUIT`. |
| `kill()` | Kill the child process. |

#### Native history

`scroll(lines)` accepts integers from -2147483648 through 2147483647.
Positive values show older output. Negative values move toward the live screen.
Zero leaves the viewport unchanged. Movement stops at the available history boundaries.
`getViewport()` returns a typed `Viewport` with numeric `offset` and `history` fields.
Both fields count rows. An offset of zero means the live screen.
Counts outside JavaScript's safe integer range produce an error instead of lost precision.

Visible text, locators, and captures use this viewport.
`text({ full: true })` remains independent: it returns history plus the live screen.
New output preserves the historical view until retained history expires.
Keyboard input returns to the live screen.
Mouse input and historical locator clicks fail while the viewport shows history.
Call `scrollToBottom()` before mouse input. This API is separate from `mouse.scroll()`.

`getCursor()` and `State.cursor` retain the application cursor coordinates.
Captures project that cursor into the viewport and hide it outside the view.
Raster recordings capture viewport changes. Asciicast remains a PTY stream.

After a source build, run `node examples/scrollback.mjs` from this directory.
The [example](examples/scrollback.mjs) prints the live screen, history view, and unchanged full text.

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
    onFailure: "all",
    includeRecording: true,
  },
  trace: { mode: "on-failure", directory: "artifacts/traces" },
  recording: { directory: "artifacts/casts" },
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
