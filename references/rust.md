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

Raster output uses installed fonts. Add a `recording-font-jetbrains-mono*` feature to bundle one.

### Recording timing

`Operation::StartRecording` samples the live terminal at the configured `fps`, which defaults to 30.
Raster recordings preserve elapsed time through `Operation::StopRecording`, including quiet periods.
There is no default idle cap or extra final hold.
Set `idle_time_limit` to cap unchanged visual intervals before `speed` scales their durations.
Asciicast remains a timestamped output stream, independent of raster sampling.

Run the [time-based recording example](../crates/tui-test/examples/time_based_recording.rs) from a source checkout:

```sh
cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example time_based_recording
```

The example records a six-second quiet screen and a second state for one second.
It writes `target/time-based-recording.gif`, reads the encoded duration, and checks it against elapsed time.
An optional first argument selects another output path.

Use `ExecutionContext` with `FailureArtifactOptions` to write failure artifacts. `TuiTestError` exposes the structured details and artifact reference.

Full API: [docs.rs](https://docs.rs/tui-test-rs/latest/tui_test/)
