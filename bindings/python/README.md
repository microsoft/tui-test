# tui-test for Python

Control, inspect, and test terminal apps from Python.

## Install

```sh
pip install --pre tui-test
```

Python 3.8+ is supported.

## Quick start

```python
from tui_test import TuiTest

async with TuiTest.ephemeral() as terminal:
    await terminal.run("my-app")
    await terminal.get_by_text("Ready").expect()
    await terminal.get_by_text("Continue").click()
    await terminal.get_by_text("Done").expect()
```

## API

### `TuiTest`

```python
TuiTest(session=None, *, backend=None, timeouts=None, profile=None, screen_history_limit=None, artifacts=None, recording=None)
```

| Option | Type | Default |
| --- | --- | --- |
| `session` | `str` | `TUI_TEST_SESSION` or `"default"` |
| `backend` | `"alacritty" \| "ghostty" \| "rio" \| "xtermjs"` | `"alacritty"` |
| `timeouts` | `Timeouts \| dict` | built-in defaults |
| `profile` | `Profile \| dict` | built-in profile |
| `screen_history_limit` | `int \| None` | core default |
| `artifacts` | `dict` | off |
| `recording` | `AutomaticRecording \| dict` | `{"mode": "always"}` |

`artifacts["on_failure"]` is `"bundle"`, `"json"`, `"svg"`, `"text"`, or `"none"`. Bundle mode writes `failure.json`, `report.md`, `current.txt`, and `current.svg`. `include_recording=True` also copies an immutable prefix of the automatic cast. Recording mode is `"disabled"`, `"on-failure"`, or `"always"`.

#### Properties

| Property | Type |
| --- | --- |
| `session` | `str` |
| `keyboard` | keyboard helper |
| `mouse` | mouse helper |

#### Lifecycle

| Method | Description |
| --- | --- |
| `TuiTest.ephemeral(prefix=None, **options)` | Create a unique session. |
| `await open(**options)` | Open a shell. |
| `await run(program, *args, **options)` | Run a program. |
| `await close()` | Close the session. |
| `await close_quiet()` | Close without raising. |
| `async with TuiTest()` | Close on exit. |

`open()` options are `shell`, `backend`, `cols`, `rows`, `cwd`, `env`, `wait_ready`, `restart`, `retries`, `profile`, and `timeouts`. `run()` accepts the same options except `shell`.

The default size is 80 by 30. Timeout defaults are 5 seconds for text and idle, and 30 seconds for command, exit, and ready.

#### Input

| Method | Description |
| --- | --- |
| `await submit(text=None)` | Type text and press Enter. |
| `await type(text)` | Type text. |
| `await write(data)` | Write raw bytes. |
| `await press(*keys)` | Alias for `keyboard.press()`. |
| `await resize(cols, rows)` | Resize the terminal. |
| `await signal(name)` | Send `INT`, `TERM`, `KILL`, or `QUIT`. |
| `await kill()` | Kill the child process. |

#### State

| Method | Returns |
| --- | --- |
| `await state()` | `State` |
| `await text(full=False)` | `str` |
| `await cells(x, y, w=1, h=1)` | `list[Cell]` |
| `await get_command()` | `str \| None` |
| `await get_output()` | `str \| None` |
| `await get_exit_code()` | `int \| None` |
| `await get_cwd()` | `str \| None` |
| `await get_cursor()` | `dict` |
| `await get_size()` | `dict` |
| `await get_title()` | `str \| None` |
| `await get_clipboard()` | `str` |
| `await get_bell_count()` | `int` |
| `await get_bell_events()` | `list[BellEvent]` |

#### Waits and assertions

| Method | Description |
| --- | --- |
| `await wait_title(text, regex=False, not_=False, timeout=None)` | Wait for a title. |
| `await wait_clipboard(text=None, timeout=None)` | Wait for a clipboard change or match. |
| `await wait_idle(timeout=None)` | Wait for the screen to stop changing. |
| `await wait_command(timeout=None)` | Wait for a submitted command. |
| `await wait_exit(timeout=None)` | Wait for the program to exit. |
| `await wait_ready(timeout=None)` | Wait for a shell prompt. |
| `await wait_bell(timeout=None)` | Wait for a bell. |
| `await expect_title(text, regex=False, not_=False, timeout=None)` | Assert the title. |
| `await expect_exit_code(code, timeout=None)` | Assert the last exit code. |
| `await expect_output(text, regex=False)` | Assert command output. |
| `await expect_bell_count(count, timeout=None)` | Wait until the cumulative bell count reaches `count`. |
| `await expect_snapshot(name, **options)` | Assert or update a snapshot. |

`wait_clipboard()` waits for the next change. A string matches text. A compiled `re.Pattern` matches a regular expression.

Snapshot options are `update`, `include_colors`, and `include_title`.

#### Capture

| Method | Description |
| --- | --- |
| `await screenshot(path=None, full=False, zoom=None)` | Return text or save SVG. |
| `await start_recording(path, **options)` | Start APNG, GIF, MP4, or asciinema recording. |
| `await stop_recording()` | Finish the recording and return its path. |

Recording options are `format`, `fps`, `speed`, `idle_time_limit`, and `zoom`. MP4 requires `ffmpeg`.

The extension selects the format: `.png` or `.apng`, `.gif`, `.mp4`, or `.cast`. `format` overrides it.

### `Locator`

Locators resolve against the latest terminal screen before every read or action.

```python
from tui_test import TextStyle

save = (
    terminal
    .get_by_text("Settings")
    .get_by_text("Save", direction="after")
    .get_by_style(TextStyle(foreground="green"))
    .unique()
)

await save.click()
```

#### Create a locator

| Method | Options |
| --- | --- |
| `terminal.get_by_text(text, **options)` | `regex`, `full`, `whitespace` |
| `terminal.get_by_style(style, **options)` | `full` |
| `locator.get_by_text(text, **options)` | `regex`, `full`, `whitespace`, `direction` |
| `locator.get_by_style(style, **options)` | `full`, `direction` |

`whitespace` is `"exact"` or `"normalize"`. `direction` is `"within"`, `"after"`, or `"before"`.

`TextStyle` fields are `foreground`, `background`, `bold`, `dim`, `italic`, `underline_style`, `underline_color`, `inverse`, `hidden`, `strikethrough`, and `blink`.

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
| `await locations()` | Return all selected locations. |
| `await location()` | Return one location. |
| `await count()` | Return the current count. |
| `await all()` | Return one locator per current match. |
| `await wait(state="visible", timeout=None)` | Wait for `"visible"` or `"hidden"`. |
| `await expect(not_=False, timeout=None)` | Assert the locator. |
| `await click(**options)` | Click the middle cell. |
| `await highlight(timeout=None)` | Highlight matches. |

`click()` accepts `button`, `alt`, `ctrl`, `shift`, `clicks`, and `timeout`. `button` is `"left"`, `"middle"`, or `"right"`.

`location()` and `click()` require one match. `all()` does not wait.

### Keyboard

| Method | Description |
| --- | --- |
| `await keyboard.press(*keys)` | Press keys. |
| `await keyboard.down(*keys)` | Send keydown events. |
| `await keyboard.repeat(*keys)` | Send repeat events. |
| `await keyboard.up(*keys)` | Send keyup events. |

```python
await terminal.keyboard.press("Ctrl+C")
await terminal.keyboard.press("Escape", ":", "w", "q", "Enter")
```

Named keys include `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Insert`, `Delete`, `Backspace`, `Tab`, `Enter`, `Space`, `Escape`, and `F1` through `F12`. Join modifiers such as `Ctrl`, `Alt`, `Shift`, `Super`, `Meta`, or `Hyper` with `+`.

### Mouse

Coordinates are zero-based terminal cells.

| Method | Description |
| --- | --- |
| `await mouse.click(x=None, y=None, **options)` | Click a cell or `on_text`. |
| `await mouse.move(x, y)` | Move the pointer. |
| `await mouse.down(x, y, **options)` | Press a button. |
| `await mouse.up(x, y, **options)` | Release a button. |
| `await mouse.drag(x1, y1, x2, y2, **options)` | Drag between cells. |
| `await mouse.scroll("up" \| "down", amount=3)` | Scroll. |

Button options are `button`, `alt`, `ctrl`, and `shift`. Click also accepts `on_text` and `clicks`.

```python
await terminal.mouse.click(10, 5, button="right", ctrl=True)
```

### Module functions

| Function | Description |
| --- | --- |
| `await sessions()` | List sessions in this process. |
| `await close_all()` | Close all sessions in this process. |
| `await get_recording(session=None)` | Return an automatic asciinema recording. |
| `unique_session(prefix=None)` | Create a unique session name. |

### Test helpers

Import from `tui_test.testing`.

| Function | Description |
| --- | --- |
| `await create_terminal(**options)` | Create, open, and track a terminal. |
| `async with terminal(**options)` | Open and close a terminal. |
| `await close_all_tracked()` | Close tracked terminals. |
| `set_terminal_defaults(**options)` | Set suite defaults. |
| `reset_terminal_defaults()` | Reset suite defaults. |
| `track_terminal(terminal)` | Track a terminal. |
| `untrack_terminal(terminal)` | Stop tracking a terminal. |
| `tracked_count()` | Count tracked terminals. |
| `terminal_snapshot(text)` | Normalize text for snapshots. |

`TerminalOptions` adds `shell`, `program`, `session`, and `prefix` to the client and spawn options.

`DEFAULT_SHELL` is the platform default.

```python
from tui_test.testing import terminal

async with terminal(program=("my-app",)) as app:
    await app.get_by_text("Ready").expect()
```

### Configuration

```python
from tui_test import AutomaticRecording, Colors, Profile, Timeouts

terminal = TuiTest(
    profile=Profile(
        scrollback=500,
        colors=Colors(foreground="#ffffff", background="#000000"),
    ),
    timeouts=Timeouts(text=10_000, command=60_000),
    artifacts={"dir": "artifacts", "on_failure": "svg"},
    recording=AutomaticRecording(mode="on-failure", directory="artifacts"),
)
```

### Types

| Type | Description |
| --- | --- |
| `State` | Session state and visible text. |
| `Cell` | One terminal cell and its style. |
| `TextMatch` | Matched text, positions, and spans. |
| `TextStyle` | Locator style fields. |
| `Profile` | Scrollback and colors. |
| `Timeouts` | Text, idle, command, exit, and ready timeouts. |
| `AutomaticRecording` | Automatic recording mode and directory. |
| `Colors` | Terminal palette. |
| `MouseButton` | `"left"`, `"middle"`, or `"right"`. |
| `TextPosition`, `TextSpan` | Match coordinates. |
| `FailureDetails` | Structured operation, locator, process, runtime, and screen evidence. |
| `FailureArtifactRef` | Paths and write status for a failure artifact. |

`__version__` contains the package version.

### Errors

| Error | Exit code |
| --- | --- |
| `ExpectationError` | `1` |
| `UsageError` | `2` |
| `NoSessionError` | `3` |
| `InternalError` | `5` |

All errors extend `TuiTestError`. Structured native failures expose `details` and `artifact`; expectation errors continue to populate compatibility `terminal.text` and `terminal.screenshot` fields. Failure artifacts can contain terminal output, titles, locator operands, and recordings, so review them before uploading.

Sessions are local to the current process and cannot be controlled by the CLI. Cancelling a task does not stop an active terminal operation.
