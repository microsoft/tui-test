# Rust

Use `tui-test-rs` from Rust code and tests.

[Back to the skill](../SKILL.md)

```sh
cargo add tui-test-rs@0.1.1
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
| `ExecutionContext`, `FailureArtifactOptions` | Attach diagnostic context and write failure artifacts. |
| `FailureDetails`, `FailureArtifactRef` | Inspect structured failures without parsing messages. |

Locator methods: `get_by_text`, `get_by_style`, `any`, `unique`, `first`, `last`, `nth`, `locations`, `location`, `count`, `all`, `wait`, `wait_hidden`, `expect`, `click`, and `highlight`. Option variants are `wait_with_timeout`, `expect_with`, `click_with`, and `highlight_with_timeout`.

Add `recording-raster` for APNG, GIF, and MP4. Add `ghostty`, `rio`, or `xtermjs` for another backend.
Source builds can additionally enable `wezterm` for WezTerm's headless VT core;
see the [backend build requirements](../README.md#shells-and-backends).

Raster output uses installed fonts. Add a `recording-font-jetbrains-mono*` feature to bundle one.

### Recording timing

`Operation::StartRecording` samples the live terminal at the configured `fps`, which defaults to 30.
Raster recordings preserve elapsed time through `Operation::StopRecording`, including quiet periods.
There is no default idle cap or extra final hold.
Set `idle_time_limit` to cap unchanged visual intervals before `speed` scales their durations.
Cursor blink phases share the source screen's idle limit.
Asciicast remains a timestamped output stream, independent of raster sampling.

Run the [time-based recording example](../crates/tui-test/examples/time_based_recording.rs) from a source checkout:

```sh
cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example time_based_recording
```

The example records a six-second quiet screen and a second state for one second.
It writes `target/time-based-recording.gif`, reads the encoded duration, and checks it against elapsed time.
An optional first argument selects another output path.

### Cursor animation

Set `OpenOptions.style.cursor` or `RunOptions.style.cursor` before you start the session:

```rust
let mut options = tui_test::OpenOptions::default();
options.style.cursor.blink = true;
options.style.cursor.blink_interval_ms = 250;
```

Blinking requires both this permission and an application cursor style that requests it.
The interval sets each visible or hidden phase; 250 ms gives a half-second cycle.
Set `blink = false` for a steady cursor.
Hidden and off-screen cursors stay hidden.
Screenshots remain deterministic, and asciicast output stays unchanged.

Run the [cursor recording example](../crates/tui-test/examples/cursor_blink_recording.rs):

```sh
cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example cursor_blink_recording
```

It writes slow, fast, and steady GIFs under `target/cursor-blink/`, plus matching PNG screenshots.
It checks that quiet-screen pixels animate only when allowed and that repeated screenshots match.
An optional first argument selects another output directory.

Use `ExecutionContext` with `FailureArtifactOptions` to write failure artifacts. `TuiTestError` exposes the structured details and artifact reference.

Full API: [docs.rs](https://docs.rs/tui-test-rs/latest/tui_test/)
