# Rust reference

Use the Rust crate when terminal control belongs inside a Rust application or
test. The crate runs the terminal engine in-process; it does not require the
standalone CLI or daemon.

Return to the [interface selector](../SKILL.md) if a persistent terminal across
independent agent commands is the actual requirement.

## Install

The package name is `tui-test-rs`; the Rust library name is `tui_test`.

```sh
cargo add tui-test-rs@0.1.0-beta.2
```

Enable `recording-raster` for APNG, GIF, and MP4 export:

```sh
cargo add tui-test-rs@0.1.0-beta.2 --features recording-raster
```

Optional terminal backends are exposed through the `ghostty`, `rio`, and
`xtermjs` features. Alacritty is always available and is the default.

## Process-local model

- `Session` owns one named terminal.
- `SessionRegistry` manages process-local sessions.
- Rust sessions cannot be listed, attached to, driven, or monitored by the
  standalone CLI.
- Give parallel tests unique session names.

## Example

```rust
use tui_test::{OpenOptions, Operation, Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = Session::new(format!("rust-example-{}", std::process::id()));
    session.open(OpenOptions::default())?;

    session.execute(Operation::Submit {
        data: Some("echo hello".into()),
    })?;
    session.execute(Operation::WaitCommand {
        timeout_ms: Some(30_000),
    })?;
    session.execute(Operation::ExpectText {
        text: "hello".into(),
        regex: false,
        full: false,
        strict: false,
        not: false,
        fg: None,
        bg: None,
        timeout_ms: Some(5_000),
    })?;
    session.execute(Operation::ExpectExitCode {
        code: 0,
        timeout_ms: Some(5_000),
    })?;

    session.close()?;
    Ok(())
}
```

Use `OpenOptions` to configure shell or program startup, dimensions, cwd,
environment, terminal backend, profile, readiness, and timeout defaults.

## API shape

`Operation` represents the shared terminal command surface: input, keyboard
and mouse events, PTY control, inspection, captures, waits, assertions, and
cleanup. `Session::execute` returns an `OperationResult` appropriate to the
operation.

Prefer specific operations and typed results over parsing rendered text. Use
text and cell inspection when the rendered terminal itself is the behavior
under test.

The complete API documentation is available at
<https://docs.rs/tui-test-rs/latest/tui_test/>.
