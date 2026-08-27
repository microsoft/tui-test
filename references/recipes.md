# Task recipes

Choose the interface through the [main skill](../SKILL.md), then adapt these
patterns to the project language. The API names are `snake_case` in Python and
`camelCase` in JavaScript.

## Test a shell command

The reliable sequence is submit, wait for command completion, check visible or
captured output, then check the exit code.

### CLI

```sh
tui-test open
tui-test submit "my-command --flag"
tui-test wait command
tui-test expect output "expected text"
tui-test expect exit-code 0
tui-test close
```

### Python

```python
from tui_test.testing import terminal

async with terminal() as term:
    await term.submit("my-command --flag")
    await term.wait_command()
    await term.expect_output("expected text")
    await term.expect_exit_code(0)
```

### JavaScript

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({}, async (term) => {
  await term.submit("my-command --flag");
  await term.waitCommand();
  await term.expectOutput("expected text");
  await term.expectExitCode(0);
});
```

Use rendered `expectText` instead of `expectOutput` when terminal formatting or
screen placement is the behavior under test.

## Drive a full-screen TUI

Run the program directly, wait for the first screen, interact, wait for the
resulting state, and assert before exiting.

### Python

```python
from tui_test.testing import terminal

async with terminal(program=("vim", "file.txt")) as term:
    await term.wait_idle()
    await term.keyboard.press("i")
    await term.type("hello")
    await term.keyboard.press("Escape", ":", "w", "q", "Enter")
    await term.wait_exit()
```

### JavaScript

```js
import { withTerminal } from "@microsoft/tui-test/test";

await withTerminal({ program: ["vim", "file.txt"] }, async (term) => {
  await term.waitIdle();
  await term.keyboard.press("i");
  await term.type("hello");
  await term.keyboard.press("Escape", ":", "w", "q", "Enter");
  await term.waitExit();
});
```

If the TUI exposes a stable label or window title, prefer `waitText` or
`waitTitle` over `waitIdle`.

## Wait for a transient state

Wait for a loading marker to appear and then disappear:

```python
await term.wait_text("Loading")
await term.wait_text("Loading", not_=True)
await term.expect_text("Ready")
```

```js
await term.waitText("Loading");
await term.waitText("Loading", { not: true });
await term.expectText("Ready");
```

This is more deterministic than sleeping for an estimated duration.

## Handle command echo

Shells usually display the command and its output:

```text
$ echo hello
hello
```

Strict text assertions require exactly one match, so either assert on captured
output or explicitly allow duplicates:

```python
await term.expect_output("hello")
await term.expect_text("hello", strict=False)
```

```js
await term.expectOutput("hello");
await term.expectText("hello", { strict: false });
```

```sh
tui-test expect output "hello"
tui-test expect text "hello" --no-strict
```

Do not globally disable strictness; it catches ambiguous UI labels.

## Inspect colors and cells

Use cells for exact grid state and text assertions for a label with a known
color:

```sh
tui-test cells 0 0 20 1
tui-test expect text "ERROR" --fg "#ff0000"
```

```python
cells = await term.cells(0, 0, 20, 1)
await term.expect_text("ERROR", fg="#ff0000")
```

```js
const cells = await term.cells(0, 0, 20, 1);
await term.expectText("ERROR", { fg: "#ff0000" });
```

Set a deterministic profile palette when an application uses indexed ANSI
colors and exact RGB values matter.

## Capture a screenshot

Use a text screenshot for diagnostics and an SVG path for a visual artifact:

```python
text = await term.screenshot()
path = await term.screenshot("artifacts/failure.svg", full=True)
```

```js
const text = await term.screenshot();
const path = await term.screenshot("artifacts/failure.svg", { full: true });
```

```sh
tui-test screenshot artifacts/failure.svg --full
```

In automated library tests, configure failure artifacts up front so assertion
errors carry terminal state even when the helper closes the session.

## Record an interaction

Start recording before the behavior of interest and always stop it:

```python
await term.start_recording("artifacts/demo.png", zoom=0.5)
try:
    await term.submit("my-command")
    await term.wait_command()
finally:
    await term.stop_recording()
```

```js
await term.startRecording("artifacts/demo.png", { zoom: 0.5 });
try {
  await term.submit("my-command");
  await term.waitCommand();
} finally {
  await term.stopRecording();
}
```

Use `.cast` for a compact replayable trace, APNG/GIF for portable animation,
and MP4 when `ffmpeg` is available.

## Run tests in parallel

Never share a fixed session name across parallel workers.

Python:

```python
term = TuiTest.ephemeral("test-name")
```

JavaScript:

```js
const term = TuiTest.ephemeral("test-name");
```

The testing helpers create unique names automatically. Explicit names are only
appropriate when deliberate reuse within the same process is part of the test.

## Debug a flaky terminal test

1. Replace sleeps with the narrowest meaningful wait.
2. Assert exit status separately from text.
3. Make shell, size, cwd, environment, backend, palette, and timeouts explicit.
4. Inspect `state`, structured getters, and a screenshot at the failure point.
5. Check whether strict text matching found zero or multiple occurrences.
6. Confirm the test uses a unique session and always awaits cleanup.
7. On the CLI, enable `--verbose` on a fresh daemon and inspect its reported
   log path.
