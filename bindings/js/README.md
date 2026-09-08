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
| `artifacts` | `{ dir, onFailure? }` | off |
| `recording` | `{ mode?, directory? }` | `{ mode: "always" }` |
| `monitoring` | `MonitoringOptions` | disabled |

`artifacts.onFailure` is `"svg"`, `"text"`, or `"none"`. Recording mode is `"disabled"`, `"on-failure"`, or `"always"`.

Monitoring is opt-in. An enabled session remains owned by the Node process, but
is advertised through one lazy native bridge per process so another terminal can run
`tui-test --session NAME monitor --interactive`. `tui-test sessions` shows the
exact `--id OWNER/SESSION` target when names are duplicated. No daemon is required;
the test process keeps its original registry and PTY.

```js
const terminal = new TuiTest("login", {
  monitoring: {
    enabled: true,
    waitAtEnd: "failure",
    firstAttachTimeout: 30_000,
    label: "login rejects expired tokens",
    metadata: {
      testFile: "test/login.test.mjs",
      testName: "rejects expired tokens",
      framework: "node:test",
      worker: "1",
      tags: ["login", "smoke"],
    },
  },
});
```

`waitAtEnd` is `"never"`, `"failure"`, or `"always"`.
`firstAttachTimeout` defaults to 30,000 milliseconds; `null` waits indefinitely
for the first attachment. `holdWhileAttached` defaults to true and waits until
all long-lived monitors disconnect, not just the interactive owner. Snapshot
requests do not count as attachments. Only one monitor can own interactive input;
other monitors can remain read-only. Input, resize, and mouse operations remain
available during inspection.

Environment equivalents are `TUI_TEST_MONITORING`,
`TUI_TEST_WAIT_AT_END`, `TUI_TEST_FIRST_ATTACH_TIMEOUT`, and `TUI_TEST_LABEL`.
Use `TUI_TEST_FIRST_ATTACH_TIMEOUT=infinite` for an unlimited first-attachment wait.
Explicit options override the corresponding environment values, including
`enabled: false`. Without an explicit or environment label, supplied `testFile`
and `testName` metadata form a label; no runner-global state is inferred.

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
| `close()` | Await active inspection and close its original target. |
| `closeQuiet()` | Close without throwing. |
| `finish({ outcome, error? })` | Apply configured end-of-test inspection, then close. |
| `inspectFailure(error)` | Inspect, close, and rethrow the same error object. |
| `[Symbol.asyncDispose]()` | Await active inspection and close from `await using`; cleanup errors propagate. |

`open()` options are `shell`, `backend`, `cols`, `rows`, `cwd`, `env`, `waitReady`, `restart`, `retries`, `profile`, and `timeouts`. `run()` accepts the same options except `shell`.

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

Snapshot options are `update`, `includeColors`, and `includeTitle`.

#### Capture

| Method | Description |
| --- | --- |
| `screenshot(path?, { full?, zoom? })` | Return text or save SVG. |
| `startRecording(path, options?)` | Start APNG, GIF, MP4, or asciinema recording. |
| `stopRecording()` | Finish the recording and return its path. |

Recording options are `format`, `fps`, `speed`, `idleTimeLimit`, and `zoom`. MP4 requires `ffmpeg`.

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
| `locator.getByText(text, options?)` | `regex`, `full`, `whitespace`, `direction` |
| `locator.getByStyle(style, options?)` | `full`, `direction` |

`whitespace` is `"exact"` or `"normalize"`. `direction` is `"within"`, `"after"`, or `"before"`.

Style fields are `foreground`, `background`, `bold`, `dim`, `italic`, `underlineStyle`, `underlineColor`, `inverse`, `hidden`, `strikethrough`, and `blink`.

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

When `waitAtEnd: "failure"` is enabled, `withTerminal()` catches the callback
error, waits up to `firstAttachTimeout` for a monitor, holds while monitor
streams remain attached, closes, and rethrows the exact original error object.
No attachment before the finite timeout proceeds directly to cleanup.
Initialization failures also attempt inspection when a monitorable target exists.
Inspection and cleanup errors never replace the callback's original thrown value
(including `undefined`) or alter its stack. After a successful callback, inspection
or cleanup errors reject; simultaneous secondary errors form an `AggregateError`.

`finish()` is the explicit outcome boundary for runner integrations. `close()`,
`closeQuiet()`, and async disposal join an inspection already in progress, including
its initialization; they do not infer whether a test failed or start failure
inspection themselves. Concurrent finish/cleanup calls share one operation.
Cleanup cannot close a replacement session with the same name. Opening the
terminal again after completion starts a new lifecycle.

`defaultShell` is the platform default.

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({ program: ["my-app"] }, async (terminal) => {
  await terminal.getByText("Ready").expect();
});
```

#### Manual failing inspection fixture

Build the binding with `npm run build`, then run `npm run inspect:failure`.
This deliberately fails an assertion, prints the exact CLI attach command, and
keeps the actual test-owned PTY available for inspection. Run that command in
another terminal; type into the fixture, resize it, then disconnect all monitors.
The original assertion is rethrown and the fixture exits nonzero. Without a
monitor, it resumes after the default 30-second first-attachment timeout.
`TUI_TEST_FIRST_ATTACH_TIMEOUT` can adjust that timeout.

The fixture lives in `manual/inspection.mjs`, outside the default test suites.
It runs only through the explicit script (or the `--inspect-failure` flag).

When `TUI_TEST_BIN` points to a built CLI, the binding's options tests also run
a CLI interoperability regression: a second, unmonitored native PTY runs the CLI
viewer against the original JavaScript-owned target. The test checks discovery,
interactive input, resize, detach, and preservation of the original failure.
It skips when `TUI_TEST_BIN` is absent.

### Configuration

```js
const terminal = new TuiTest("test", {
  profile: {
    scrollback: 500,
    colors: { foreground: "#ffffff", background: "#000000" },
  },
  timeouts: { text: 10_000, command: 60_000 },
  artifacts: { dir: "artifacts", onFailure: "svg" },
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

All errors extend `TuiTestError` and include `kind` and `exitCode`. Expectation errors can include `terminal.text` and `terminal.screenshot`.

Sessions are local to the current process and cannot be controlled by the CLI. Cancelling a promise does not stop an active terminal operation.
