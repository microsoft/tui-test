# @microsoft/shell-use

Node bindings for [`shell-use`](https://github.com/microsoft/shell-use); a terminal engine for driving and asserting on real shells and TUI programs.

## Install

```sh
npm install @microsoft/shell-use # Node 20+

bun add @microsoft/shell-use # Bun (best effort)

deno add npm:@microsoft/shell-use # Deno 2 (best effort)
```

The package is ESM only.

## Runtime Requirements

- Node: 20+
- Bun: treated as best effort
- Deno: 2, treated as best effort. Requires a local `node_modules` directory (`deno install` / `--node-modules-dir`) and `--allow-ffi` (in addition to `--allow-read --allow-write`)

## Quick start

```js
import { ShellUse } from "@microsoft/shell-use";

const su = new ShellUse();
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

All derive from `ShellUseError` and carry `kind` and `exitCode`. `waitX` and `expectX` reject with `ExpectationError` on failure. Assertion errors include the current visible terminal content.

## API

`new ShellUse(session?, { timeouts?, artifacts? })` mirrors the cli: `open` / `run`, `type` / `write`, `submit`, `press` / `keys`, `mouse.click|move|down|up|drag|scroll`, `resize`, `signal` / `kill`, `state`, `text`, `cells`, `getCommand` / `getOutput` / `getExitCode` / `getCwd` / `getCursor` / `getSize` / `getBellCount`, `screenshot`, `waitText` / `waitIdle` / `waitCommand` / `waitExit` / `waitReady` / `waitBell`, `expectText` / `expectExitCode` / `expectOutput` / `expectBellCount` / `expectSnapshot`, `close`, and `closeQuiet`.

Module-level helpers: `sessions()`, `closeAll()`, `getRecording()`, `uniqueSession()`.

`open` and `run` accept `{ cols, rows, cwd, env, waitReady, retries, timeouts }`. The timeout classes are `text`, `idle`, `command`, `exit`, and `ready`; `timeouts` sets session defaults, the constructor sets client-wide ones. Unknown class names throw.

`ShellUse.ephemeral(prefix?, opts?)` creates a client bound to a unique session
name (via `uniqueSession()`), useful for parallel test workers that should not
collide. All sessions are process-local. `artifacts: { dir, onFailure }`
attaches the terminal contents to an `ExpectationError`.

`@microsoft/shell-use/test` has helpers for terminal tests: `createTerminal`, `withTerminal`, `closeAllTracked`, `defaultShell`, and `terminalSnapshot`.

```js
import { withTerminal } from "@microsoft/shell-use/test";

await withTerminal({}, async (t) => {
  await t.submit("echo hi");
  await t.waitCommand();
  await t.expectText("hi");
});
```

Each terminal has a unique name, so parallel workers do not collide.
`setTerminalDefaults(...)` sets suite-wide options (`artifacts`, `timeouts`,
...).

## Cancellation and recordings

Cancelling a promise does not cancel the underlying Rust operation. Operations for single sessions wait for completion (ex: `close()`, `closeAll()`).

Closing a session removes it from `sessions()`, but keeps its recording. `getRecording()` can read that recording for the rest of the process. The 1024 most recently closed sessions have their recordings retained.

## Configuration

| Variable                       | Purpose                                                                     |
| ------------------------------ | --------------------------------------------------------------------------- |
| `SHELL_USE_SESSION`            | default session name                                                        |
| `SHELL_USE_TIMEOUT_<CLASS>_MS` | fallback timeout for one class (`TEXT`, `IDLE`, `COMMAND`, `EXIT`, `READY`) |
