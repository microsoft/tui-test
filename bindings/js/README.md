# @microsoft/tui-test

Node bindings for [`tui-test`](https://github.com/microsoft/tui-test); a terminal engine for driving and asserting on real shells and TUI programs.

## Install

```sh
npm install @microsoft/tui-test@beta # Node 20+

bun add @microsoft/tui-test@beta # Bun (best effort)

deno add npm:@microsoft/tui-test@beta # Deno 2 (best effort)
```

The package is ESM only.

## Runtime Requirements

- Node: 20+
- Bun: treated as best effort
- Deno: 2, treated as best effort. Requires a local `node_modules` directory (`deno install` / `--node-modules-dir`) and `--allow-ffi` (in addition to `--allow-read --allow-write`)

## Quick start

```js
import { TuiTest } from "@microsoft/tui-test";

const su = new TuiTest();
await su.open();
await su.submit("echo hello");
await su.waitCommand();
await su.expectText("hello");
await su.expectExitCode(0);
await su.close();
```

## Errors

Every failure maps to one of the engine's error kinds:

| Class              | `exitCode` | Meaning                                  |
| ------------------ | ---------- | ---------------------------------------- |
| `ExpectationError` | 1          | an `expect`/`wait` condition was not met |
| `UsageError`       | 2          | invalid argument (e.g. a bad regex)      |
| `NoSessionError`   | 3          | no active session                        |
| `InternalError`    | 5          | internal engine error                    |

All derive from `TuiTestError` and carry `kind` and `exitCode`. `waitX` and `expectX` reject with `ExpectationError` on failure. Assertion errors include the current visible terminal content.

## API

`new TuiTest(session?, { backend?, profile?, timeouts?, artifacts? })` mirrors the cli: `open` / `run`, `type` / `write`, `submit`, `press` / `keys`, `mouse.click|move|down|up|drag|scroll`, `resize`, `signal` / `kill`, `state`, `text`, `cells`, `getCommand` / `getOutput` / `getExitCode` / `getCwd` / `getCursor` / `getSize` / `getTitle`, `screenshot`, `startRecording` / `stopRecording`, `waitText` / `waitTitle` / `waitIdle` / `waitCommand` / `waitExit` / `waitReady`, `expectText` / `expectTitle` / `expectExitCode` / `expectOutput` / `expectSnapshot`, `close`, and `closeQuiet`.

Module-level helpers: `sessions()`, `closeAll()`, `getRecording()`, `uniqueSession()`.

`open` and `run` accept
`{ backend, cols, rows, cwd, env, waitReady, retries, profile, timeouts }`.
The constructor also accepts `backend` and `profile` as defaults for later
opens and runs. Backend values are `"alacritty"` (default) and `"ghostty"`:

```js
const terminal = new TuiTest("work", { backend: "ghostty" });
await terminal.open();
await terminal.run("vim", ["file.txt"], { backend: "alacritty" });
```

The native package includes both emulators. Profiles are partial; omitted
fields use the built-in defaults:

```js
const terminal = new TuiTest("work", {
  profile: {
    scrollback: 500,
    colors: { red: "#ff0000", brightBlue: "#3366ff" },
  },
});
```

The timeout classes are `text`, `idle`, `command`, `exit`, and `ready`;
`timeouts` sets session defaults, and the constructor sets client-wide ones.
Unknown fields throw.

`TuiTest.ephemeral(prefix?, opts?)` creates a client bound to a unique session
name (via `uniqueSession()`), useful for parallel test workers that should not
collide. All sessions are process-local. `artifacts: { dir, onFailure }`
attaches the terminal contents to an `ExpectationError`.

`@microsoft/tui-test/test` has helpers for terminal tests: `createTerminal`, `withTerminal`, `closeAllTracked`, `defaultShell`, and `terminalSnapshot`.

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({}, async (t) => {
  await t.submit("echo hi");
  await t.waitCommand();
  await t.expectText("hi");
});
```

Each terminal has a unique name, so parallel workers do not collide.
`setTerminalDefaults(...)` sets suite-wide options (`profile`, `artifacts`,
`timeouts`, ...).

## Cancellation and recordings

Cancelling a promise does not cancel the underlying Rust operation. Operations for single sessions wait for completion (ex: `close()`, `closeAll()`).

Closing a session removes it from `sessions()`, but keeps its recording. `getRecording()` can read that recording for the rest of the process. The 1024 most recently closed sessions have their recordings retained.

```js
await su.startRecording("demo.png", { fps: 30, speed: 1 });
await su.submit("echo hello");
await su.waitCommand();
const path = await su.stopRecording();
```

`.png`/`.apng` selects lossless APNG, `.gif` selects GIF, and `.cast` selects
asciicast v2. The `format` option can override extension inference.

## Configuration

| Variable                       | Purpose                                                                     |
| ------------------------------ | --------------------------------------------------------------------------- |
| `TUI_TEST_SESSION`            | default session name                                                        |
| `TUI_TEST_TIMEOUT_<CLASS>_MS` | fallback timeout for one class (`TEXT`, `IDLE`, `COMMAND`, `EXIT`, `READY`) |
