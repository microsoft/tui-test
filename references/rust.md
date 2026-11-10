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

## Session behavior across bindings

The bindings expose terminal operations, not the Rust renderer or emulator extension interfaces.
The table uses JavaScript names. Python uses snake case. Go uses exported PascalCase names unless noted.

| Rust operation | Binding entry point |
| --- | --- |
| `Open`, `Run`, `Restart`, `Close` | `open`, `run`, `restart`, `close` |
| `FinishTrace` | `finishTrace(failed)`; does not close the terminal |
| `State`, `Text`, `PackedScreen`, `Cells` | `state`, `text`, `packedScreen`, `cells` |
| `GetCommand`, `GetOutput`, `GetExitCode`, `GetCwd` | `getCommand`, `getOutput`, `getExitCode`, `getCwd` |
| `GetCursor`, `GetModes`, `GetColors`, `GetSize`, `GetViewport` | Corresponding `get` methods |
| `GetTitle`, `GetClipboard`, `GetBellCount`, `GetBellEvents` | Corresponding `get` methods |
| `Write`, `Submit`, `Key` | `write`, `submit`, and `keyboard.press/down/repeat/up` |
| `Mouse` | `mouse.click/move/down/up/drag/scroll` |
| `Resize`, `Scroll`, `Signal` | `resize`, `scroll/scrollToTop/scrollToBottom`, `signal` |
| `WaitTitle`, `WaitClipboard`, `WaitClipboardMatch` | `waitTitle`, `waitClipboard` with an optional text or regex pattern |
| `WaitIdle`, `WaitCommand`, `WaitExit`, `WaitReady`, `WaitBell` | Corresponding `wait` methods |
| `FindLocator`, `ResolveLocator` | `locator.locations`, `locator.location` |
| `WaitLocator` | `locator.wait` or `locator.expect`; both support negative checks |
| `ClickLocator`, `HighlightLocator` | `locator.click`, `locator.highlightMatches` |
| `ExpectTitle`, `ExpectExitCode`, `ExpectMode`, `ExpectColors`, `ExpectCursor` | Corresponding `expect` methods |
| `ExpectOutput`, `ExpectBellCount` | Corresponding `expect` methods |
| `Snapshot`, `Screenshot` | `expectSnapshot`, `screenshot` |
| `StartRecording`, `PauseRecording`, `ResumeRecording`, `StopRecording` | Corresponding recording methods |

`highlight` retains its existing unit result. `highlightMatches` also returns the resolved matches.
Packed screens retain UTF-8 bytes, columns, rows, trailing spaces, and blank lines.
Python returns `(memoryview, cols, rows)`; JavaScript and Go return typed objects.
All result payloads copy native memory before the adapters release the native result.
Screenshot results use strings: a requested file yields its path, and inline capture yields its content.
Snapshot options retain `cwd`, update mode, style inclusion, and title inclusion.

### Input fields

`OpenOptions` and `RunOptions` retain backend, profile, dimensions, cwd, environment, readiness, restart, timeouts, recording, and rendering style.
Open adds the shell; run adds the program and arguments.
Per-call recording directories override constructor defaults.
Runtime APIs also expose `AutomaticRecording.mode` for casts without traces.
Its values are disabled, on-failure, and always. Explicit trace options take precedence.
Serde omits this runtime field, so the CLI configuration still accepts only the recording directory.
`ExecutionContext` retains the operation name, artifact options, diagnostic map, retention limit, and trace options.
Bindings accept this context on the constructor and on each spawn.
Per-spawn context overrides do not mutate constructor defaults.
Rust sanitizes diagnostic maps before it writes them.

Text selectors retain text, regex, full-grid selection, whitespace mode, and scope anchors.
Each anchor retains text, regex, and occurrence selection.
The shared expression uses `scope` for text anchors and `match_style` for whole-match style.
The separate `style` field remains the style-selector payload.
Public `withStyle`, `with_style`, and `WithStyle` apply whole-match style to text, style, and link locators.
Style tests ignore blank cells when visible characters exist.
Intersections with a style locator still select cells; they do not become whole-match filters.
Public `relativeTo`, `relative_to`, and `RelativeTo` apply `within`, `after`, or `before` to those locators.
Rust rejects scope or whole-match style on composite nodes.
Apply these fields to the operands before composition.
Composite locators retain branch selection and containment.
All locator operands must belong to the same owner.

### Session ownership and exclusions

Bindings use the process-local named registry rather than independent Rust `Session` values.
Their session lists provide the equivalent of `Session::is_open`.
`OpenResult.recording` supplies the automatic recording path.
Recording helpers read current recordings and recordings retained after close.
The path belongs to the engine; it is not a permanent archive.
`interrupt` cancels pending operations and terminates the child without an operation queue wait.
It is not a key event. Call `close` afterward to release the registry entry.
Later waits report cancellation until restart or close.
Registry interruption returns `NoSession` when the name has no session.

Monitor frames, raw input, engine logs, renderer internals, and emulator extension interfaces remain Rust implementation APIs.
Host signals retain platform support limits. Language regex objects retain their documented host-language behavior.
Named sessions do not share state with the CLI process.

### Recording timing

`Operation::StartRecording` samples the live terminal at the configured `fps`, which defaults to 30.
Raster recordings preserve active elapsed time through `Operation::StopRecording`, including quiet periods.
Use `Operation::PauseRecording` and `Operation::ResumeRecording` to exclude time while the terminal continues.
These controls support APNG, GIF, and MP4 only. Asciicast pause and resume return usage errors without changing the recording.
Resume captures the live terminal and restarts the visible cursor phase.
Stop during a pause excludes that pause. Automatic diagnostic recordings and recovery casts remain continuous, including output during pauses.
Repeated pause or resume calls return usage errors.
Run `cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example pause_resume_recording` for a GIF example.
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

## Program status

`Operation::GetProgramStatuses` returns the OSC 7501 records in stable ID order.
`Operation::GetProgramStatus { id }` returns one record, or `None` when absent.
`ProgramStatus` contains `id`, `state`, `app`, `title`, `message`, `kind`, and `progress`.
The app value includes inheritance. Text values contain decoded UTF-8.
`State.program_statuses` and `TerminalDiagnostics.program_statuses` contain the same status records.

`Operation::WaitProgramStatus` and `Operation::ExpectProgramStatus` accept `expected: ProgramStatusMatch` and `timeout_ms`.
A default match selects the root record through its empty ID.
Supplied fields must match exactly. An absent record never matches.
States are `Idle`, `Working`, `Done`, `Blocked`, and `Error`.
Kinds are `Permission`, `Question`, and `Auth`. Progress ranges from 0 to 100.

Run the [program status example](../crates/tui-test/examples/program_status.rs):

```sh
env -u CARGO_TARGET_DIR cargo run -p tui-test-rs --example program_status
```
