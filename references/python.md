# Python reference

Use the Python binding when application or test code already runs in Python.
The binding executes the terminal engine in-process and does not install or
require the standalone CLI.

Return to the [interface selector](../SKILL.md) before continuing if a
persistent cross-command agent session is the actual requirement.

## Install and import

```sh
pip install --pre tui-test
```

Python 3.8 or later is required.

```python
from tui_test import TuiTest
```

All terminal operations are asynchronous.

## Process-local model

- Sessions, the registry, and retained recordings belong to the current Python
  process.
- The CLI cannot list, attach to, drive, or monitor these sessions.
- `sessions()`, `close_all()`, and `get_recording()` only see this process.
- Give parallel tests unique names with `TuiTest.ephemeral()`,
  `unique_session()`, or the testing helpers.

## Direct lifecycle

Use an async context manager so the terminal closes on success or failure:

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

Use `open()` for a shell. Use `run(program, *args)` for a program whose process
lifecycle is the test boundary:

```python
async with TuiTest.ephemeral("editor") as terminal:
    await terminal.run("vim", "file.txt")
    await terminal.wait_idle()
    await terminal.keyboard.press("i")
    await terminal.type("some text")
    await terminal.keyboard.press("Escape", ":", "w", "q", "Enter")
    await terminal.wait_exit()
```

Top-level `press()` remains a compatibility alias for `keyboard.press()`.

## Testing helpers

Prefer `tui_test.testing` in pytest or unittest suites. It creates a unique
session, opens a shell or program, tracks it, and closes it in `finally`.

```python
from tui_test.testing import terminal

async def test_echo():
    async with terminal() as term:
        await term.submit("echo hello")
        await term.wait_command()
        await term.expect_text("hello", strict=False)
        await term.expect_exit_code(0)
```

Available helpers:

| Helper | Purpose |
| --- | --- |
| `terminal(**options)` | Async context manager that creates and cleans up a terminal |
| `create_terminal(**options)` | Create, open, and track a terminal |
| `close_all_tracked()` | Close terminals created by the helper layer |
| `set_terminal_defaults(**options)` | Set suite-wide dimensions, shell, profile, artifacts, timeouts, and related options |
| `reset_terminal_defaults()` | Clear suite-wide defaults |
| `DEFAULT_SHELL` | Platform default shell name |
| `terminal_snapshot(text)` | Normalize trailing whitespace for framework snapshots |

Pass `program=("vim", "file.txt")` to run a program instead of opening a
shell. Other useful options include `backend`, `shell`, `cols`, `rows`, `cwd`,
`env`, `session`, `prefix`, `retries`, `wait_ready`, `timeouts`, `profile`, and
`artifacts`.

## Constructor and spawn options

```python
terminal = TuiTest(
    session="work",
    backend="ghostty",
    timeouts={"text": 15_000, "command": 60_000},
    profile={"scrollback": 500},
    artifacts={"dir": "test-artifacts"},
)

await terminal.open(
    shell="bash",
    cols=120,
    rows=40,
    cwd="project",
    env={"CI": "1"},
)
```

The constructor sets client defaults. `open()` and `run()` can override the
backend, dimensions, cwd, environment, readiness behavior, retry count,
profile, and timeout defaults for one spawn. Unknown option fields raise.

`run()` takes argv as positional arguments:

```python
await terminal.run("python", "-m", "my_app")
```

Do not pass the argv tail as one list.

## API map

| Task | Methods |
| --- | --- |
| Lifecycle | `open`, `run`, `close`, `close_quiet` |
| Text input | `submit`, `type`, `write` |
| Keyboard | `keyboard.press`, `keyboard.down`, `keyboard.repeat`, `keyboard.up` |
| Mouse | `mouse.click`, `mouse.move`, `mouse.down`, `mouse.up`, `mouse.drag`, `mouse.scroll` |
| PTY control | `resize`, `signal`, `kill` |
| Rendered state | `state`, `text`, `cells` |
| Structured getters | `get_command`, `get_output`, `get_exit_code`, `get_cwd`, `get_cursor`, `get_size`, `get_title`, `get_bell_count`, `get_bell_events` |
| Captures | `screenshot`, `start_recording`, `stop_recording` |
| Waits | `wait_text`, `wait_title`, `wait_idle`, `wait_command`, `wait_exit`, `wait_ready`, `wait_bell` |
| Assertions | `expect_text`, `expect_title`, `expect_exit_code`, `expect_output`, `expect_bell_count`, `expect_snapshot` |

Module-level helpers are `sessions()`, `close_all()`, `get_recording()`, and
`unique_session()`.

## Waiting and assertions

Use the condition that represents progress:

```python
await terminal.wait_text("Ready")
await terminal.wait_text("Loading", not_=True)
await terminal.wait_command()
await terminal.wait_exit()
await terminal.wait_idle()
```

`wait_idle()` only means the screen stopped repainting briefly. It does not
mean a silent command or program finished.

Text assertions are strict by default:

```python
await terminal.expect_text("Save")
await terminal.expect_text("hello", strict=False)
await terminal.expect_text("ERROR", fg="#ff0000")
await terminal.expect_output(r"^done$", regex=True)
await terminal.expect_exit_code(0)
```

Use `strict=False` only when multiple matches are expected, such as a shell
echoing a submitted command before printing the same text.

## Profiles and backends

The native Python package includes `alacritty`, `ghostty`, `rio`, and
`xtermjs`. Alacritty is the default.

Profiles are partial. Use mappings or the typed `Profile` and `Colors` classes:

```python
from tui_test import Colors, Profile, TuiTest

terminal = TuiTest(
    profile=Profile(
        scrollback=500,
        colors=Colors(red="#ff0000"),
    )
)
```

The in-process binding accepts profile objects directly; it does not discover
or load CLI `tui-test.toml` files.

Timeout classes are `text`, `idle`, `command`, `exit`, and `ready`. Set them on
the client, per spawn, or per operation where supported.

## Screenshots, recordings, and failure artifacts

```python
svg = await terminal.screenshot()
path = await terminal.screenshot("terminal.svg", full=True, zoom=0.5)

await terminal.start_recording("demo.png", fps=30, speed=1.0, zoom=0.5)
await terminal.submit("echo hello")
await terminal.wait_command()
path = await terminal.stop_recording()
```

`.png`/`.apng` selects APNG, `.gif` selects GIF, `.mp4` selects MP4, and
`.cast` selects asciinema v2. MP4 export requires `ffmpeg`.

Configure `artifacts` on `TuiTest` or the testing helpers to attach terminal
state to an `ExpectationError`. This is preferable to catching an assertion
and trying to inspect a session after cleanup.

Closing removes a session from `sessions()` but retains its automatic
recording. `get_recording(session)` can retrieve one of the 1024 most recently
closed recordings during the rest of the process.

## Errors and cancellation

| Exception | Meaning |
| --- | --- |
| `ExpectationError` | A wait or assertion condition was not met |
| `UsageError` | An argument or option was invalid |
| `NoSessionError` | The session was not active |
| `InternalError` | The engine failed internally |

All derive from `TuiTestError`. They expose stable error kinds and exit-code
equivalents; expectation errors can carry a terminal artifact.

Cancelling an awaiting Python task does not cancel the underlying Rust
operation. Operations for one session are serialized. Use context managers and
allow cleanup to finish.

The package README is available at
<https://github.com/microsoft/tui-test/blob/main/bindings/python/README.md>.
