# shell-use (Python)

Python bindings for [`shell-use`](https://github.com/microsoft/shell-use); a terminal automation, inspection, assertion, and recording engine written in Rust.

## Install

```sh
pip install shell-use
```

Requires Python 3.8+. Wheels are published for common platforms via `maturin`

## Quick start

```python
import asyncio
from shell_use import ShellUse

async def main():
    async with ShellUse() as su:
        await su.open()
        await su.submit("echo hello")
        await su.wait_command()
        await su.expect_text("hello")
        await su.expect_exit_code(0)

asyncio.run(main())
```

Drive a full-screen TUI:

```python
async with ShellUse("vim-session") as su:
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

All derive from `ShellUseError`. `wait_*` and `expect_*` raise `ExpectationError` on failure. Assertion errors include the current visible terminal content.

## API

`ShellUse(session="default", *, timeouts=None, artifacts=None)` mirrors the cli: `open` / `run`, `type` / `write`, `submit`, `press` / `keys`, `mouse.click|move|down|up|drag|scroll`, `resize`, `signal` / `kill`, `state`, `text`, `cells`, `get_command` / `get_output` / `get_exit_code` / `get_cwd` / `get_cursor` / `get_size` / `get_bell_count`, `screenshot`, `wait_text` / `wait_idle` / `wait_command` / `wait_exit` / `wait_ready` / `wait_bell`, `expect_text` / `expect_exit_code` / `expect_output` / `expect_bell_count` / `expect_snapshot`, `close`, and `close_quiet`.

Module-level helpers: `sessions()`, `close_all()`, `get_recording()`, `unique_session()`.

`open()` and `run()` accept `wait_ready=`, `retries=`, and `timeouts=`. The timeout classes are `text`, `idle`, `command`, `exit`, and `ready`; `timeouts=` sets session defaults, the constructor takes the same `Timeouts` (or a dict) as a client-wide default. Unknown class names raise.

`ShellUse.ephemeral(prefix=None, **kwargs)` binds a client to a unique,
process-local session name. `artifacts={"dir": ..., "on_failure": ...}`
attaches the terminal contents to an `ExpectationError`.

`shell_use.testing` has helpers for terminal tests: `create_terminal`, `terminal` (an async context manager), `close_all_tracked`, `DEFAULT_SHELL`, and `terminal_snapshot`.

```python
from shell_use.testing import terminal

async def test_echo():
    async with terminal() as t:
        await t.submit("echo hi")
        await t.wait_command()
        await t.expect_text("hi")
```

Each terminal is uniquely named, so parallel workers don't collide. `set_terminal_defaults(...)` sets suite-wide options (`timeouts`, `artifacts`, ...).

## Cancellation and recordings

Cancelling a task does not cancel the underlying Rust operation. Operations for single sessoins wait for completion (ex: `close()`, `close_all()`).

Closing a session removes it from `sessions()`, but keeps its recording. `get_recording()` can read that recording for the rest of the process. The 1024 most recently closed sessions have their recordings retained.

## Configuration

| Variable                       | Purpose                                                                     |
| ------------------------------ | --------------------------------------------------------------------------- |
| `SHELL_USE_SESSION`            | default session name                                                        |
| `SHELL_USE_TIMEOUT_<CLASS>_MS` | fallback timeout for one class (`TEXT`, `IDLE`, `COMMAND`, `EXIT`, `READY`) |
