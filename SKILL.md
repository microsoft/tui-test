---
name: tui-test
description: "Drive, inspect, assert on, and record real terminals with the tui-test CLI or its in-process Rust, Python, and JavaScript APIs. Use when an agent needs a headless shell or TUI; when code imports tui_test or @microsoft/tui-test; when Rust code embeds tui-test-rs; when sending keyboard or mouse input; when waiting for terminal state; when checking text, colors, output, exit codes, or snapshots; or when capturing screenshots and recordings."
---

# tui-test

`tui-test` runs real shells and full-screen terminal applications in a headless
PTY backed by a terminal emulator. It can drive the terminal, inspect rendered
state, wait for meaningful conditions, assert on results, and create artifacts.

## Choose the interface first

| Context | Use | Read next |
| --- | --- | --- |
| An agent or shell script controlling a terminal across separate commands | Standalone CLI | [CLI reference](references/cli.md) |
| Python application or test code | `tui_test.TuiTest` | [Python reference](references/python.md) |
| Node or TypeScript application or test code | `@microsoft/tui-test` | [JavaScript reference](references/javascript.md) |
| Rust application or test code | `tui-test-rs` | [Rust reference](references/rust.md) |
| A complete task such as testing a TUI, capturing an artifact, or debugging a flaky assertion | The interface already used by the project | [Recipes](references/recipes.md) |

Follow these rules:

- Use the in-process library when the project is already Python, JavaScript, or
  Rust. Do not shell out to the CLI from library code.
- Use the CLI when an agent needs a persistent terminal across independent tool
  calls or needs to hand the session to a person with `monitor`.
- CLI sessions live in a background daemon. Library sessions and registries are
  process-local. A library session cannot be listed, attached to, controlled,
  or monitored by the CLI.
- Read the reference for the chosen interface before writing commands or code.
  Method names and option shapes differ by language.

## Shared lifecycle

Every interface follows the same sequence:

1. **Create a session.** Use `open` for a shell or `run` for a program.
2. **Interact.** Submit shell commands, type text, press keys, use the mouse,
   resize the terminal, or signal the child.
3. **Synchronize.** Wait for terminal text, command completion, process exit,
   shell readiness, a title, a bell, or visual idleness.
4. **Inspect or assert.** Read state, text, cells, command output, exit status,
   cwd, cursor, title, or compare an expectation.
5. **Capture artifacts when useful.** Take a screenshot, save a recording, or
   attach terminal state to an assertion failure.
6. **Close the session.** Prefer context managers or test helpers in library
   code so cleanup also happens after failures.

Do not replace synchronization with fixed sleeps. Pick a wait that represents
the state the test actually needs.

## Quick starts

### CLI

```sh
tui-test open
tui-test submit "echo hello"
tui-test wait command
tui-test expect text "hello" --no-strict
tui-test expect exit-code 0
tui-test close
```

CLI calls are stateless clients of the same named daemon session. Use
`--session <name>` to isolate concurrent work.

### Python

```python
import asyncio
from tui_test import TuiTest

async def main():
    async with TuiTest.ephemeral("example") as terminal:
        await terminal.open()
        await terminal.submit("echo hello")
        await terminal.wait_command()
        await terminal.expect_text("hello", strict=False)
        await terminal.expect_exit_code(0)

asyncio.run(main())
```

Use [`tui_test.testing`](references/python.md#testing-helpers) in test suites.

### JavaScript

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({}, async (terminal) => {
  await terminal.submit("echo hello");
  await terminal.waitCommand();
  await terminal.expectText("hello", { strict: false });
  await terminal.expectExitCode(0);
});
```

Use `TuiTest.ephemeral()` when the test helper is not appropriate. The package
is ESM only.

### Rust

The Rust API exposes `Session`, `SessionRegistry`, `Operation`, and
`OperationResult`. See the [Rust reference](references/rust.md) for a complete
example and feature flags.

## Pick the right wait

| Needed state | Wait | Why |
| --- | --- | --- |
| Known text appears or disappears | `wait text` / `wait_text` / `waitText` | Most precise for application-visible state |
| A submitted shell command finishes | `wait command` / `wait_command` / `waitCommand` | Uses shell integration and tracks command boundaries |
| A directly-run program exits | `wait exit` / `wait_exit` / `waitExit` | Waits for the child process, not just a quiet screen |
| A TUI finishes repainting before the next interaction | `wait idle` / `wait_idle` / `waitIdle` | Detects visual quiescence; it does not mean process completion |
| An opened shell is ready | `wait ready` / `wait_ready` / `waitReady` | Uses the semantic prompt integration |
| A program announces state through its window title | `wait title` / `wait_title` / `waitTitle` | Avoids scraping unrelated screen text |
| A program rings the terminal bell | `wait bell` / `wait_bell` / `waitBell` | Waits for an event rather than polling |

Prefer `wait text` when the expected text is known. A silent long-running
command can satisfy `wait idle` while it is still running.

## Write reliable terminal tests

- Give parallel workers unique session names. Use `TuiTest.ephemeral`,
  `unique_session` / `uniqueSession`, or the language test helpers.
- Use `open` for shell-command workflows. Use `run` when process exit is the
  lifecycle boundary, especially for full-screen applications.
- A shell normally echoes the submitted command before printing its output.
  `expect text "hello"` may therefore match twice. Keep strict matching for
  unique UI labels; use `strict=False`, `{ strict: false }`, or CLI
  `--no-strict` when duplicate matches are expected.
- Assert the exit code separately from visible output. Matching expected text
  does not prove the command succeeded.
- Use viewport text for what a person can currently see and full scrollback
  only when the assertion intentionally covers prior output.
- Prefer structured getters for cwd, cursor, size, title, command output, and
  exit status instead of parsing a full-screen text snapshot.
- Keep terminal size, shell, cwd, environment, palette, and timeouts explicit
  when they affect the expected result.
- Always close sessions. Library cancellation stops the caller from waiting but
  does not cancel the underlying Rust operation.

## Capability map

Python uses `snake_case`; JavaScript uses `camelCase`. The CLI groups some
operations under subcommands.

| Task | CLI | Python | JavaScript |
| --- | --- | --- | --- |
| Open shell | `open` | `open()` | `open()` |
| Run program | `run` | `run()` | `run()` |
| Submit command | `submit` | `submit()` | `submit()` |
| Type without Enter | `type` | `type()` | `type()` |
| Press keys | `key press` | `keyboard.press()` | `keyboard.press()` |
| Mouse input | `mouse ...` | `mouse.*()` | `mouse.*()` |
| Read rendered state | `state`, `text`, `cells`, `get ...` | `state()`, `text()`, `cells()`, `get_*()` | `state()`, `text()`, `cells()`, `get*()` |
| Wait | `wait ...` | `wait_*()` | `wait*()` |
| Assert | `expect ...` | `expect_*()` | `expect*()` |
| Screenshot | `screenshot` | `screenshot()` | `screenshot()` |
| Record | `record start/stop` | `start_recording()` / `stop_recording()` | `startRecording()` / `stopRecording()` |
| Close | `close` | `close()` | `close()` |

## Failure model

The same failure taxonomy is used across interfaces:

| Kind | CLI exit code | Python | JavaScript |
| --- | --- | --- | --- |
| Assertion or wait failed | 1 | `ExpectationError` | `ExpectationError` |
| Invalid usage or argument | 2 | `UsageError` | `UsageError` |
| No active session | 3 | `NoSessionError` | `NoSessionError` |
| Daemon or IPC failure | 4 | Not applicable to the in-process API | Not applicable to the in-process API |
| Internal engine failure | 5 | `InternalError` | `InternalError` |

Python and JavaScript errors derive from `TuiTestError`. Expectation errors can
carry captured terminal state when failure artifacts are enabled.

## References

- [CLI reference](references/cli.md)
- [Python reference](references/python.md)
- [JavaScript reference](references/javascript.md)
- [Rust reference](references/rust.md)
- [Task recipes](references/recipes.md)

For exact standalone CLI flags, run `tui-test agent-context`; its versioned JSON
is generated from the installed CLI and is the source of truth.
