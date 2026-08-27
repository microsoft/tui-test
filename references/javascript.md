# JavaScript reference

Use the JavaScript binding when application or test code already runs in Node
or TypeScript. The package executes the terminal engine in-process and does not
install or require the standalone CLI.

Return to the [interface selector](../SKILL.md) before continuing if a
persistent cross-command agent session is the actual requirement.

## Install and import

```sh
npm install @microsoft/tui-test@beta
```

Node 20 or later is the supported runtime. The package is ESM only.

```js
import { TuiTest } from "@microsoft/tui-test";
```

Bun and Deno support is best effort. Deno 2 needs a local `node_modules`
directory and `--allow-ffi` in addition to read/write permissions.

## Process-local model

- Sessions, the registry, and retained recordings belong to the current
  JavaScript process.
- The CLI cannot list, attach to, drive, or monitor these sessions.
- `sessions()`, `closeAll()`, and `getRecording()` only see this process.
- Give parallel tests unique names with `TuiTest.ephemeral()`,
  `uniqueSession()`, or the test helpers.

## Preferred test lifecycle

Use `@microsoft/tui-test/test` in test suites. `withTerminal` creates a unique
session, opens it, and closes it in `finally`.

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({}, async (terminal) => {
  await terminal.submit("echo hello");
  await terminal.waitCommand();
  await terminal.expectText("hello", { strict: false });
  await terminal.expectExitCode(0);
});
```

Available helpers:

| Helper | Purpose |
| --- | --- |
| `withTerminal(options, callback)` | Create and clean up a terminal around a callback |
| `createTerminal(options)` | Create, open, and track a terminal |
| `closeAllTracked()` | Close terminals created by the helper layer |
| `setTerminalDefaults(options)` | Set suite-wide dimensions, shell, profile, artifacts, timeouts, and related options |
| `resetTerminalDefaults()` | Clear suite-wide defaults |
| `defaultShell` | Platform default shell name |
| `terminalSnapshot(text)` | Normalize trailing whitespace for framework snapshots |

Pass `program: ["vim", "file.txt"]` to run a program instead of opening a
shell. Other useful options include `backend`, `shell`, `cols`, `rows`, `cwd`,
`env`, `session`, `prefix`, `retries`, `waitReady`, `timeouts`, `profile`, and
`artifacts`.

## Direct lifecycle

When the test helper is not appropriate, use an ephemeral client and close it
in `finally`:

```js
import { TuiTest } from "@microsoft/tui-test";

const terminal = TuiTest.ephemeral("example");
try {
  await terminal.open();
  await terminal.submit("echo hello");
  await terminal.waitCommand();
  await terminal.expectText("hello", { strict: false });
  await terminal.expectExitCode(0);
} finally {
  await terminal.closeQuiet();
}
```

Use `open()` for a shell. Use `run(program, args)` for a program whose process
lifecycle is the test boundary:

```js
const terminal = TuiTest.ephemeral("editor");
try {
  await terminal.run("vim", ["file.txt"]);
  await terminal.waitIdle();
  await terminal.keyboard.press("i");
  await terminal.type("some text");
  await terminal.keyboard.press("Escape", ":", "w", "q", "Enter");
  await terminal.waitExit();
} finally {
  await terminal.closeQuiet();
}
```

Top-level `press()` remains a compatibility alias for `keyboard.press()`.

## Constructor and spawn options

```js
const terminal = new TuiTest("work", {
  backend: "ghostty",
  timeouts: { text: 15_000, command: 60_000 },
  profile: { scrollback: 500 },
  artifacts: { dir: "test-artifacts" },
});

await terminal.open({
  shell: "bash",
  cols: 120,
  rows: 40,
  cwd: "project",
  env: { CI: "1" },
});
```

The constructor sets client defaults. `open()` and `run()` can override the
backend, dimensions, cwd, environment, readiness behavior, retry count,
profile, and timeout defaults for one spawn. Unknown option fields throw.

`run()` takes argv as an array, then spawn options:

```js
await terminal.run("node", ["app.js", "--color"], { cols: 120 });
```

## API map

| Task | Methods |
| --- | --- |
| Lifecycle | `open`, `run`, `close`, `closeQuiet` |
| Text input | `submit`, `type`, `write` |
| Keyboard | `keyboard.press`, `keyboard.down`, `keyboard.repeat`, `keyboard.up` |
| Mouse | `mouse.click`, `mouse.move`, `mouse.down`, `mouse.up`, `mouse.drag`, `mouse.scroll` |
| PTY control | `resize`, `signal`, `kill` |
| Rendered state | `state`, `text`, `cells` |
| Structured getters | `getCommand`, `getOutput`, `getExitCode`, `getCwd`, `getCursor`, `getSize`, `getTitle`, `getBellCount`, `getBellEvents` |
| Captures | `screenshot`, `startRecording`, `stopRecording` |
| Waits | `waitText`, `waitTitle`, `waitIdle`, `waitCommand`, `waitExit`, `waitReady`, `waitBell` |
| Assertions | `expectText`, `expectTitle`, `expectExitCode`, `expectOutput`, `expectBellCount`, `expectSnapshot` |

Module-level helpers are `sessions()`, `closeAll()`, `getRecording()`, and
`uniqueSession()`.

## Waiting and assertions

Use the condition that represents progress:

```js
await terminal.waitText("Ready");
await terminal.waitText("Loading", { not: true });
await terminal.waitCommand();
await terminal.waitExit();
await terminal.waitIdle();
```

`waitIdle()` only means the screen stopped repainting briefly. It does not mean
a silent command or program finished.

Text assertions are strict by default:

```js
await terminal.expectText("Save");
await terminal.expectText("hello", { strict: false });
await terminal.expectText("ERROR", { fg: "#ff0000" });
await terminal.expectOutput("^done$", { regex: true });
await terminal.expectExitCode(0);
```

Use `{ strict: false }` only when multiple matches are expected, such as a
shell echoing a submitted command before printing the same text.

## Profiles and backends

The native JavaScript package includes `alacritty`, `ghostty`, `rio`, and
`xtermjs`. Alacritty is the default.

Profiles are partial:

```js
const terminal = new TuiTest("work", {
  profile: {
    scrollback: 500,
    colors: { red: "#ff0000", brightBlue: "#3366ff" },
  },
});
```

The in-process binding accepts profile objects directly; it does not discover
or load CLI `tui-test.toml` files.

Timeout classes are `text`, `idle`, `command`, `exit`, and `ready`. Set them on
the client, per spawn, or per operation where supported.

## Screenshots, recordings, and failure artifacts

```js
const svg = await terminal.screenshot();
const path = await terminal.screenshot("terminal.svg", {
  full: true,
  zoom: 0.5,
});

await terminal.startRecording("demo.png", {
  fps: 30,
  speed: 1,
  zoom: 0.5,
});
await terminal.submit("echo hello");
await terminal.waitCommand();
const recording = await terminal.stopRecording();
```

`.png`/`.apng` selects APNG, `.gif` selects GIF, `.mp4` selects MP4, and
`.cast` selects asciinema v2. MP4 export requires `ffmpeg`.

Configure `artifacts` on `TuiTest` or the test helpers to attach terminal state
to an `ExpectationError`. This is preferable to catching an assertion and
trying to inspect a session after cleanup.

Closing removes a session from `sessions()` but retains its automatic
recording. `getRecording(session)` can retrieve one of the 1024 most recently
closed recordings during the rest of the process.

## Errors and cancellation

| Class | Meaning |
| --- | --- |
| `ExpectationError` | A wait or assertion condition was not met |
| `UsageError` | An argument or option was invalid |
| `NoSessionError` | The session was not active |
| `InternalError` | The engine failed internally |

All derive from `TuiTestError` and expose `kind` and `exitCode`. Expectation
errors can carry a terminal artifact.

Abandoning or externally cancelling a promise does not cancel the underlying
Rust operation. Operations for one session are serialized. Use
`withTerminal()` or await cleanup in `finally`.

The package README is available at
<https://github.com/microsoft/tui-test/blob/main/bindings/js/README.md>.
