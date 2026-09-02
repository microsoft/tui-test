# Python

Use `tui_test` from Python code and tests.

[Back to the skill](../SKILL.md)

## Start

```sh
pip install --pre tui-test
```

```python
from tui_test import TuiTest

async with TuiTest.ephemeral() as terminal:
    await terminal.run("my-app")
    await terminal.get_by_text("Ready").expect()
    await terminal.get_by_text("Continue").click()
```

## Locators

```python
locator = (
    terminal
    .get_by_text("Settings")
    .get_by_text("Save", direction="after")
    .unique()
)
```

| Method | Use |
| --- | --- |
| `get_by_text(text, **options)` | Match text or regex. |
| `get_by_style(style, **options)` | Match colors or attributes. |
| `any()` | Keep all matches. |
| `unique()` | Require one match. |
| `first()`, `last()`, `nth(index)` | Select a match. |
| `locations()`, `location()`, `count()`, `all()` | Read matches. |
| `wait(state="visible", timeout=None)` | Wait for visible or hidden. |
| `expect(not_=False, timeout=None)` | Assert. |
| `click(**options)` | Click. |
| `highlight(timeout=None)` | Highlight. |

Text options: `regex`, `full`, `whitespace`, and chained `direction`.

Click options: `button`, `alt`, `ctrl`, `shift`, `clicks`, and `timeout`.

## Session

| Method | Use |
| --- | --- |
| `open(**options)` | Open a shell. |
| `run(program, *args, **options)` | Run an app. |
| `submit(text=None)` | Type and press Enter. |
| `type(text)`, `write(data)` | Send text or bytes. |
| `resize(cols, rows)` | Resize. |
| `signal(name)`, `kill()` | Stop the child. |
| `state()`, `text()`, `cells()` | Read terminal state. |
| `get_command()`, `get_output()`, `get_exit_code()` | Read command state. |
| `get_cwd()`, `get_cursor()`, `get_size()`, `get_title()` | Read terminal fields. |
| `get_clipboard()` | Read the session clipboard. |
| `get_bell_count()`, `get_bell_events()` | Read bells. |
| `wait_command()`, `wait_exit()`, `wait_ready()`, `wait_idle()` | Wait for state. |
| `wait_title()`, `wait_clipboard()`, `wait_bell()` | Wait for events. |
| `expect_title()`, `expect_output()`, `expect_exit_code()` | Assert state. |
| `expect_bell_count()`, `expect_snapshot()` | Assert bells or snapshots. |
| `screenshot()` | Read text or save SVG or PNG. |
| `start_recording()`, `stop_recording()` | Record. |
| `close()`, `close_quiet()` | Close. |

Constructor options: `backend`, `timeouts`, `profile`, `artifacts`, and `recording`.

Recording modes: `disabled`, `on-failure`, and `always`.

## Input helpers

```python
await terminal.keyboard.press("Ctrl+C")
await terminal.mouse.click(10, 5, button="right", ctrl=True)
```

Keyboard: `press`, `down`, `repeat`, and `up`.

Named keys include arrows, Home, End, PageUp, PageDown, Insert, Delete, Backspace, Tab, Enter, Space, Escape, and F1 through F12. Join modifiers with `+`.

Mouse: `click`, `move`, `down`, `up`, `drag`, and `scroll`.

## Test helpers

```python
from tui_test.testing import terminal

async with terminal(program=("my-app",)) as app:
    await app.get_by_text("Ready").expect()
```

Helpers: `create_terminal`, `terminal`, `close_all_tracked`, `set_terminal_defaults`, `reset_terminal_defaults`, and `terminal_snapshot`.

## Errors

`ExpectationError`, `UsageError`, `NoSessionError`, and `InternalError` extend `TuiTestError`.

Full API: [bindings/python/README.md](https://github.com/microsoft/tui-test/blob/main/bindings/python/README.md)
