# JavaScript

Use `@microsoft/tui-test` from JavaScript or TypeScript.

[Back to the skill](../SKILL.md)

## Start

```sh
npm install @microsoft/tui-test@beta
```

```js
import { TuiTest } from "@microsoft/tui-test";

const terminal = TuiTest.ephemeral();
try {
  await terminal.run("my-app");
  await terminal.getByText("Ready").expect();
  await terminal.getByText("Continue").click();
} finally {
  await terminal.closeQuiet();
}
```

## Locators

```js
const locator = terminal
  .getByText("Settings")
  .getByText("Save", { direction: "after" })
  .unique();
```

| Method | Use |
| --- | --- |
| `getByText(text, options?)` | Match text or regex. |
| `getByStyle(style, options?)` | Match colors or attributes. |
| `any()` | Keep all matches. |
| `unique()` | Require one match. |
| `first()`, `last()`, `nth(index)` | Select a match. |
| `locations()`, `location()`, `count()`, `all()` | Read matches. |
| `wait({ state?, timeout? })` | Wait for visible or hidden. |
| `expect({ not?, timeout? })` | Assert. |
| `click(options?)` | Click. |
| `highlight({ timeout? })` | Highlight. |

Text options: `regex`, `full`, `whitespace`, and chained `direction`.

Click options: `button`, `alt`, `ctrl`, `shift`, `clicks`, and `timeout`.

## Session

| Method | Use |
| --- | --- |
| `open(options?)` | Open a shell. |
| `run(program, args?, options?)` | Run an app. |
| `submit(text?)` | Type and press Enter. |
| `type(text)`, `write(data)` | Send text or bytes. |
| `resize(cols, rows)` | Resize. |
| `signal(name)`, `kill()` | Stop the child. |
| `state()`, `text()`, `cells()` | Read terminal state. |
| `getCommand()`, `getOutput()`, `getExitCode()` | Read command state. |
| `getCwd()`, `getCursor()`, `getSize()`, `getTitle()` | Read terminal fields. |
| `getClipboard()` | Read the session clipboard. |
| `getBellCount()`, `getBellEvents()` | Read bells. |
| `waitCommand()`, `waitExit()`, `waitReady()`, `waitIdle()` | Wait for state. |
| `waitTitle()`, `waitClipboard()`, `waitBell()` | Wait for events. |
| `expectTitle()`, `expectOutput()`, `expectExitCode()` | Assert state. |
| `expectBellCount()`, `expectSnapshot()` | Assert bells or snapshots. |
| `screenshot()` | Read text or save SVG. |
| `startRecording()`, `stopRecording()` | Record. |
| `close()`, `closeQuiet()` | Close. |

Capture options: `background` and `transparent` (SVG, APNG, and GIF).

Constructor options: `backend`, `timeouts`, `profile`, `artifacts`, and `recording`.

Recording modes: `disabled`, `on-failure`, and `always`.

## Input helpers

```js
await terminal.keyboard.press("Ctrl+C");
await terminal.mouse.click(10, 5, { button: "right", ctrl: true });
```

Keyboard: `press`, `down`, `repeat`, and `up`.

Named keys include arrows, Home, End, PageUp, PageDown, Insert, Delete, Backspace, Tab, Enter, Space, Escape, and F1 through F12. Join modifiers with `+`.

Mouse: `click`, `move`, `down`, `up`, `drag`, and `scroll`.

## Test helpers

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({ program: ["my-app"] }, async (app) => {
  await app.getByText("Ready").expect();
});
```

Helpers: `createTerminal`, `withTerminal`, `closeAllTracked`, `setTerminalDefaults`, `resetTerminalDefaults`, and `terminalSnapshot`.

## Errors

`ExpectationError`, `UsageError`, `NoSessionError`, and `InternalError` extend `TuiTestError`.

Full API: [bindings/js/README.md](https://github.com/microsoft/tui-test/blob/main/bindings/js/README.md)
