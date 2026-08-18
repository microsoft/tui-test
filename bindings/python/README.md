# tui-test (Python)

Python bindings for [`tui-test`](https://github.com/microsoft/tui-test); a terminal automation, inspection, assertion, and recording engine written in Rust.

## Install

```sh
pip install --pre tui-test
```

Requires Python 3.8+. Wheels are published for common platforms via `maturin`

## Quick start

```python
import asyncio
from tui_test import TuiTest

async def main():
    async with TuiTest() as su:
        await su.open()
        await su.submit("echo hello")
        await su.wait_command()
        await su.expect_text("hello")
        await su.expect_exit_code(0)

asyncio.run(main())
```

Drive a full-screen TUI:

```python
async with TuiTest("vim-session") as su:
    await su.run("vim", "file.txt")
    await su.wait_idle()
    await su.press("i")
    await su.type("some text")
    await su.press("Escape", ":", "w", "q", "Enter")
    await su.wait_exit()
```

## Errors

Every failure maps to one of the engine's error kinds:

| Exception          | Exit code | Meaning                                  |
| ------------------ | --------- | ---------------------------------------- |
| `ExpectationError` | 1         | an `expect`/`wait` condition was not met |
| `UsageError`       | 2         | invalid argument (e.g. a bad regex)      |
| `NoSessionError`   | 3         | no active session                        |
| `InternalError`    | 5         | internal engine error                    |

All derive from `TuiTestError`. `wait_*` and `expect_*` raise `ExpectationError` on failure. Assertion errors include the current visible terminal content.

## API

`TuiTest(session="default", *, backend=None, timeouts=None, profile=None, artifacts=None)` mirrors the cli: `open` / `run`, `type` / `write`, `submit`, `keyboard.press|down|repeat|up`, compatibility `press`, `mouse.click|move|down|up|drag|scroll`, `resize`, `signal` / `kill`, `state`, `text`, `cells`, `get_command` / `get_output` / `get_exit_code` / `get_cwd` / `get_cursor` / `get_size` / `get_title` / `get_bell_count` / `get_bell_events`, `screenshot`, `start_recording` / `stop_recording`, `wait_text` / `wait_title` / `wait_idle` / `wait_command` / `wait_exit` / `wait_ready` / `wait_bell`, `expect_text` / `expect_title` / `expect_exit_code` / `expect_output` / `expect_bell_count` / `expect_snapshot`, `close`, and `close_quiet`.

`keyboard.press()` sends down then up. Use `keyboard.down()`,
`keyboard.repeat()`, and `keyboard.up()` for explicit events. Top-level
`press()` remains a compatibility alias.

Module-level helpers: `sessions()`, `close_all()`, `get_recording()`, `unique_session()`.

`open()` and `run()` accept `backend=`, `wait_ready=`, `retries=`, `profile=`,
and `timeouts=`. The constructor also accepts `backend=` and `profile=` as
defaults for later opens and runs. Backend values are `"alacritty"` (default)
and `"ghostty"`:

```python
terminal = TuiTest(backend="ghostty")
await terminal.open()
await terminal.run("vim", "file.txt", backend="alacritty")
```

The native package includes both emulators. Profiles are partial; omitted
fields use the built-in defaults:

```python
from tui_test import Colors, Profile, TuiTest

terminal = TuiTest(
    profile=Profile(
        scrollback=500,
        colors=Colors(red="#ff0000"),
    )
)
```

Mappings with the same shape are also accepted. The timeout classes are
`text`, `idle`, `command`, `exit`, and `ready`; `timeouts=` sets session
defaults, and the constructor takes the same `Timeouts` (or a dict) as a
client-wide default. Unknown fields raise.

`TuiTest.ephemeral(prefix=None, **kwargs)` binds a client to a unique,
process-local session name. `artifacts={"dir": ..., "on_failure": ...}`
attaches the terminal contents to an `ExpectationError`.

`tui_test.testing` has helpers for terminal tests: `create_terminal`, `terminal` (an async context manager), `close_all_tracked`, `DEFAULT_SHELL`, and `terminal_snapshot`.

```python
from tui_test.testing import terminal

async def test_echo():
    async with terminal() as t:
        await t.submit("echo hi")
        await t.wait_command()
        await t.expect_text("hi")
```

Each terminal is uniquely named, so parallel workers don't collide. `set_terminal_defaults(...)` sets suite-wide options (`profile`, `timeouts`, `artifacts`, ...).

## Cancellation and recordings

Cancelling a task does not cancel the underlying Rust operation. Operations for single sessoins wait for completion (ex: `close()`, `close_all()`).

Closing a session removes it from `sessions()`, but keeps its recording. `get_recording()` can read that recording for the rest of the process. The 1024 most recently closed sessions have their recordings retained.

```python
await su.start_recording("demo.png", fps=30, speed=1.0, zoom=0.5)
await su.submit("echo hello")
await su.wait_command()
path = await su.stop_recording()
```

`.png`/`.apng` selects lossless APNG, `.gif` selects GIF, `.mp4` selects MP4,
and `.cast` selects asciicast v2. Pass `format=` to override extension
inference. `zoom=` scales SVG screenshots and image/video recordings without
changing terminal rows or columns. MP4 recording requires `ffmpeg` to be
available on `PATH`.

## Configuration

| Variable                       | Purpose                                                                     |
| ------------------------------ | --------------------------------------------------------------------------- |
| `TUI_TEST_SESSION`            | default session name                                                        |
| `TUI_TEST_TIMEOUT_<CLASS>_MS` | fallback timeout for one class (`TEXT`, `IDLE`, `COMMAND`, `EXIT`, `READY`) |
