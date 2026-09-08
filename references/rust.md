# Rust

Use `tui-test-rs` from Rust code and tests.

[Back to the skill](../SKILL.md)

```sh
cargo add tui-test-rs@0.1.0-beta.3
```

```rust
use tui_test::{OpenOptions, Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terminal = Session::new("example");
    terminal.open(OpenOptions::default())?;
    terminal.get_by_text("Ready").expect()?;
    terminal.get_by_text("Continue").click()?;
    terminal.close()?;
    Ok(())
}
```

Core types:

| Type | Use |
| --- | --- |
| `Session` | Own one terminal. |
| `SessionRegistry` | Manage named sessions. |
| `Locator` | Find, wait, assert, click, and highlight. |
| `MouseButton`, `MouseOptions`, `LocatorClickOptions` | Configure buttons and modifiers. |
| `Operation` | Run a terminal operation. |
| `OpenOptions`, `RunOptions` | Start a shell or app. |
| `Profile`, `Timeouts` | Set colors, scrollback, and timeouts. |

Locator methods: `get_by_text`, `get_by_style`, `any`, `unique`, `first`, `last`, `nth`, `locations`, `location`, `count`, `all`, `wait`, `wait_hidden`, `expect`, `click`, and `highlight`. Option variants are `wait_with_timeout`, `expect_with`, `click_with`, and `highlight_with_timeout`.

## Process-local monitoring

`tui_test::monitoring` exposes the existing terminal to the CLI without creating
a daemon or replacing its PTY. Use `Monitor::for_session` with a `Session`, or
`Monitor::for_handle` with a registry `SessionHandle`.

```rust
use tui_test::{OpenOptions, Session};
use tui_test::monitoring::{Monitor, Options, Outcome, WaitPolicy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terminal = Session::new("login");
    terminal.open(OpenOptions::default())?;
    let mut monitor = Monitor::for_session(&terminal, Options {
        enabled: true,
        wait_at_end: WaitPolicy::Failure,
        ..Options::default()
    })?;
    match terminal.get_by_text("Ready").expect() {
        Ok(()) => monitor.finish(Outcome::Passed)?,
        Err(error) => return Err(monitor.finish_failure(error).into()),
    }
    Ok(())
}
```

| API | Purpose |
| --- | --- |
| `Options` | Opt-in monitoring, end-of-test wait policy, first-attachment timeout, client hold policy, and metadata. |
| `Options::from_env()` | Read the shared `TUI_TEST_MONITORING`, `TUI_TEST_WAIT_AT_END`, `TUI_TEST_FIRST_ATTACH_TIMEOUT`, and label defaults. |
| `WaitPolicy` | Choose never, failure-only, or always inspection. |
| `Outcome` | Report whether the test passed or failed. |
| `Monitor::id()` | Obtain the exact process/session attach target. |
| `Monitor::inspect(outcome)` | Run the configured inspection window without transferring ownership. |
| `Monitor::finish(outcome)` | Inspect as configured and close the captured terminal. |
| `Monitor::finish_failure(error)` | Retain the original error value while performing failure inspection and cleanup. |
| `Monitor::cancel_inspection()` | Cancel an outstanding inspection wait. |

Defaults are disabled, no end-of-test wait, a 30-second first-attachment window,
and holding cleanup while clients are attached. Waiting indefinitely requires
explicit configuration. Monitor state is tied to the actual PTY generation, so
an old inspection cannot close a replacement session with the same name.

For startup/readiness diagnostics, `open_for_monitoring` and
`run_for_monitoring` return the original startup result alongside an optional
captured target. An actually spawned child is retained on readiness failure so
it can be inspected; a program that could not be spawned has no target. Ordinary
`open` and `run` retain their existing cleanup behavior.

The CLI discovers enabled sessions through `tui-test sessions --waiting` and
attaches with `tui-test monitor --interactive --id OWNER/SESSION`. Only one
interactive viewer owns input; multiple read-only viewers are supported.
Each viewer has one bidirectional connection that stays attached through
resizes. Both modes resize the child, with interactive viewers taking priority
and the previous viewer's size restored on detach. Completion waits use native
notifications, and the terminal cannot survive the owning Rust process.

Add `recording-raster` for APNG, GIF, and MP4. Add `ghostty`, `rio`, or `xtermjs` for another backend.

Raster output uses installed fonts. Add a `recording-font-jetbrains-mono*` feature to bundle one.

Full API: [docs.rs](https://docs.rs/tui-test-rs/latest/tui_test/)
