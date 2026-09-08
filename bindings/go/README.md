# tui-test for Go

Control, inspect, test, and record terminal apps from Go. The binding runs the Rust engine in your process and does not require the CLI.

## Install

Install the Go binding from source. Keep the Go module and native library on one exact repository revision by using a local module replacement.

Supported host builds are:

| OS | Go architecture | Rust native target | Build prerequisites |
| --- | --- | --- | --- |
| Linux | `amd64` | `x86_64-unknown-linux-gnu` or `x86_64-unknown-linux-musl` | Go 1.26+, Rust 1.90+, Zig 0.16.0, and a native C/C++ compiler and linker |
| Linux | `arm64` | `aarch64-unknown-linux-gnu` or `aarch64-unknown-linux-musl` | Go 1.26+, Rust 1.90+, Zig 0.16.0, and a native C/C++ compiler and linker |
| macOS | `amd64` | `x86_64-apple-darwin` | Go 1.26+, Rust 1.90+, Zig 0.16.0, and Apple command-line build tools |
| macOS | `arm64` | `aarch64-apple-darwin` | Go 1.26+, Rust 1.90+, Zig 0.16.0, and Apple command-line build tools |
| Windows | `amd64` | `x86_64-pc-windows-msvc` | Go 1.26+, Rust 1.90+, Zig 0.16.0, and MSVC Build Tools |

Use a host-native build. Cross-compilation also requires the destination Rust target and its linker or toolchain, and is not the recommended source-install path.

1. Clone the repository, select an exact commit or tag that contains the Go binding, and record the resolved commit:

   ```sh
   git clone https://github.com/microsoft/tui-test.git
   cd tui-test
   git checkout --detach <commit-or-tag>
   git rev-parse HEAD
   ```

2. From that checkout's repository root, build the native library:

   ```sh
   cargo build --locked -p tui-test-go
   ```

   The debug library is `target/debug/libtui_test_go.so` on Linux, `target/debug/libtui_test_go.dylib` on macOS, or `target/debug/tui_test_go.dll` on Windows. See [building and maintaining the Go binding](CONTRIBUTING.md#build-from-source) for release builds and platform-specific test commands.

3. In the Go application module, point the module path at the same checkout, then add it:

   ```sh
   go mod edit -replace github.com/microsoft/tui-test/bindings/go=/absolute/path/to/tui-test/bindings/go
   go get github.com/microsoft/tui-test/bindings/go
   ```

   The replacement makes the selected checkout—not a separately resolved remote version—the source of the Go module. When changing revisions, check out the new revision, rebuild the library, and keep the replacement pointed at that checkout.

4. Set `TUI_TEST_GO_NATIVE_LIBRARY` to the absolute path of the library before running the application. On Linux or macOS:

   ```sh
   export TUI_TEST_GO_NATIVE_LIBRARY=/absolute/path/to/tui-test/target/debug/libtui_test_go.so
   go run .
   ```

   Use the `.dylib` path shown above on macOS. On Windows PowerShell:

   ```powershell
   $env:TUI_TEST_GO_NATIVE_LIBRARY = (Resolve-Path C:\path\to\tui-test\target\debug\tui_test_go.dll)
   go run .
   ```

The binding loads the configured engine on first use and keeps it loaded for the process lifetime. It does not download anything at runtime. Running the quick start below succeeds when its output contains `hello`. An empty `TUI_TEST_GO_NATIVE_LIBRARY`, an unloadable library, or a library from an incompatible revision returns an initialization error before the terminal opens.

## Quick start

```go
package main

import (
    "fmt"
    "log"

    "github.com/microsoft/tui-test/bindings/go"
)

func main() {
    terminal, err := tuitest.Ephemeral("example", tuitest.ClientOptions{})
    if err != nil {
        log.Fatal(err)
    }
    defer terminal.CloseQuiet()

    if _, err := terminal.Open(tuitest.OpenOptions{}); err != nil {
        log.Print(err)
        return
    }
    if err := terminal.Submit(tuitest.Ptr("echo hello")); err != nil {
        log.Print(err)
        return
    }
    if err := terminal.WaitCommand(tuitest.WaitOptions{}); err != nil {
        log.Print(err)
        return
    }
    output, err := terminal.GetOutput()
    if err != nil {
        log.Print(err)
        return
    }
    if output != nil {
        fmt.Print(*output)
    }
}
```

Use `Run(program, args, SpawnOptions{})` to launch an application directly. Use `Open(OpenOptions{})` when you need a shell and its command tracking.

## Sessions and options

`New(session, ClientOptions{})` creates a client for a named session. An empty name uses `TUI_TEST_SESSION`, falling back to `"default"`. `Ephemeral(prefix, options)` creates a unique name. Construction does not start a terminal; call `Open` or `Run`.

Clients with the same name share the same in-process session. `Close` closes that session for every client. An existing client can open it again. `Restart` restarts the current launch; its optional `GracefulTimeout` defaults to 5 seconds. `Sessions` lists live sessions, and `CloseAll` closes them. When the effective automatic-recording mode is enabled, `OpenResult.Recording` contains its path and `Recording(session)` reads the current asciicast. After closure, `Recording` succeeds only if the recording was retained: always-mode recordings are retained, while on-failure recordings are retained only for failed sessions. When recording is disabled, `OpenResult.Recording` is empty and `Recording` returns an error.

`ClientOptions` sets the backend, profile, timeouts, screen-history limit, automatic recording, failure artifacts, and traces. `SpawnOptions` controls dimensions, working directory, environment, readiness, restart, retries, and per-launch overrides. `OpenOptions` embeds `SpawnOptions` and adds `Shell`.

The default backend is Alacritty. Supported backends are Alacritty, Ghostty, Rio, and xterm.js. The default size is 80 columns by 30 rows.

Automatic recording is disabled when recording, trace, and recording-inclusive artifact options are all omitted. `ClientOptions.Recording` can select `RecordingAlways`, `RecordingOnFailure`, or `RecordingDisabled`. A supplied `ClientOptions.Trace` overrides that selection: `TraceOn` records always, `TraceOnFailure` records on failure, and `TraceOff` disables recording. When trace options are omitted, `Artifacts.IncludeRecording` promotes a disabled recording to on-failure recording. These options can persist terminal output, so select them deliberately.

Optional pointer fields distinguish omission from an explicit value. For example:

```go
options := tuitest.SpawnOptions{
    Cols:      tuitest.Ptr(uint16(100)),
    Rows:      tuitest.Ptr(uint16(40)),
    WaitReady: tuitest.Ptr(false),
}
```

Timeouts use `*time.Duration`. A nil timeout uses the client setting, then the engine default: 5 seconds for text and idle, or 30 seconds for command, exit, and ready. Explicit zero remains zero. Negative durations are rejected; positive fractions of a millisecond round upward.

Methods block until their operation completes. Use goroutines for concurrent work. Operations on each session are serialized. This API does not accept contexts or promise per-call cancellation.

`SpawnOptions.Retries` applies only to retryable launch failures. Usage errors are returned immediately, and each retryable attempt is closed before the next attempt starts.

## API reference

Use `go doc github.com/microsoft/tui-test/bindings/go` for exported type fields and signatures. Methods return ordinary Go errors; getters preserve unavailable values with pointers where applicable.

| Capability | Methods |
| --- | --- |
| Construction and discovery | `New`, `Ephemeral`, `Ptr`, `Sessions`, `CloseAll`, `Recording` |
| Lifecycle | `Open`, `Run`, `Restart`, `Close`, `CloseQuiet`, `Session` |
| Input | `Type`, `Write`, `Submit`, `Press`, `Resize`, `Signal`, `Kill` |
| Keyboard | `Keyboard.Press`, `Down`, `Repeat`, `Up` |
| Mouse | `Mouse.Click`, `Move`, `Down`, `Up`, `Drag`, `Scroll` |
| Screen | `State`, `Text`, `Cells` |
| Command state | `GetCommand`, `GetOutput`, `GetExitCode`, `GetCwd` |
| Terminal state | `GetCursor`, `GetSize`, `GetTitle`, `GetClipboard`, `GetBellCount`, `GetBellEvents` |
| Waits | `WaitTitle`, `WaitClipboard`, `WaitIdle`, `WaitCommand`, `WaitExit`, `WaitReady`, `WaitBell` |
| Assertions | `ExpectTitle`, `ExpectExitCode`, `ExpectOutput`, `ExpectBellCount`, `ExpectSnapshot` |
| Capture | `Screenshot`, `StartRecording`, `StopRecording` |

Use `WaitCommand` after submitting a shell command, and `WaitExit` after running a program directly. `WaitIdle` only establishes that the screen stopped changing.

`State` returns the visible text and terminal dimensions together with command, process-exit, cursor, mode, mouse-mode, color, timeout, readiness, and bell state. `Cells` returns styles and OSC 8 link metadata through `Cell.Link` and `Cell.LinkID`.

### Locators

`GetByText`, `GetByStyle`, and `GetByLink` create lazy locators. Chaining returns a new locator and leaves the original unchanged. For example:

```go
ready := terminal.GetByText("Ready", tuitest.TextSelectorOptions{})
err := ready.Last().Expect(tuitest.LocatorExpectOptions{})
```

Select matches with `Any`, `Unique`, `First`, `Last`, or `Nth`. Inspect them with `Locations`, `Location`, `Count`, or `All`. Use `Wait`, `Expect`, `Click`, and `Highlight` for actions. `Location` and `Click` require a unique match unless you select one explicitly.

| Locator task | Methods |
| --- | --- |
| Create or refine | `GetByText`, `GetByStyle`, `GetByLink` |
| Compose | `And`, `Or`, `Filter` |
| Select | `Any`, `Unique`, `First`, `Last`, `Nth` |
| Inspect | `Locations`, `Location`, `Count`, `All` |
| Act or assert | `Wait`, `Expect`, `Click`, `Highlight` |

Nested text, style, and link selectors support `Within`, `After`, and `Before` directions. Text selectors also support regular expressions, scrollback, and exact or normalized whitespace. Style pointer fields preserve explicit false values, such as `Bold: tuitest.Ptr(false)`.

`GetByLink(uri, options)` matches an exact OSC 8 target rather than visible URL text; an empty URI requires cells with no link. Root style and link selectors find runs within each row. Chained selectors with the default `Within` direction test whole matches. Link selectors test every cell in a match.

Compose locators with `And`, `Or`, and `Filter`:

```go
label := terminal.GetByText("Documentation", tuitest.TextSelectorOptions{})
link := terminal.GetByLink("https://example.com/docs", tuitest.LinkSelectorOptions{})

linkedLabel, err := label.And(link)
if err != nil {
    log.Fatal(err)
}
container, err := label.Filter(tuitest.LocatorFilterOptions{Has: link})
if err != nil {
    log.Fatal(err)
}
```

`And` and `Or` form contiguous per-row cell runs. `Filter` preserves each input match: `Has` requires its locator inside the candidate, while `HasNot` requires no such match. Locators in a composition must belong to the same client. Composition leaves its operands unchanged and reads one fresh terminal snapshot when resolved. Apply `First`, `Last`, `Nth`, or `Unique` at the stage where selection should occur; selection before composition can produce a different result from selection afterward. If any branch sets `Full`, the whole query includes scrollback.

### Errors and artifacts

Use `errors.As` with `*tuitest.Error` to inspect `Kind`, `Message`, `Operation`, and optional diagnostics. Error kinds are `AssertionError`, `UsageError`, `NoSessionError`, and `InternalError`. Structured native failures expose `Details`; exported failure artifacts expose their paths and write status through `Artifact`.

Configure `ClientOptions.Artifacts` with a directory and `ArtifactAll`, `ArtifactHTML`, `ArtifactText`, or `ArtifactNone`. `IncludeRecording` copies the automatic cast into the failure artifact. Failure to capture an optional artifact does not replace the assertion error.

Configure `ClientOptions.Trace` with `TraceOn` to retain every session trace or `TraceOnFailure` to retain failed traces. Pass `CloseOptions{Failed: tuitest.Ptr(true)}` when closing a session whose failure happened outside a tui-test assertion so `TraceOnFailure` can retain it. The `tuitesttest` helper supplies this failure state during test cleanup.

Trace and recording-inclusive artifact options also determine the effective automatic-recording mode as described in [sessions and options](#sessions-and-options).

Trace and failure-artifact directories can contain terminal output, titles, locator operands, and recordings. Review them before uploading. Users can open `trace.html`; automated consumers should read `trace.md`, `trace.json`, and `timeline.json`.

### Recording and snapshots

`Screenshot` returns terminal text when its path is empty, or writes SVG or PNG when a path is supplied. `ScreenshotOptions` controls scrollback, zoom, background color, and transparency. Background color and transparency are mutually exclusive, and canvas options require an output path.

`StartRecording` supports asciinema, APNG, GIF, and MP4. File extensions select the format unless `RecordingOptions.Format` overrides it. Recording options control FPS, speed, idle-time limit, zoom, background color, and transparency. Background color and transparency are mutually exclusive. MP4 requires `ffmpeg` on the executable search path and does not support transparency. `StopRecording` finishes the recording and returns its path.

`ExpectSnapshot` returns `SnapshotPassed`, `SnapshotWritten`, or `SnapshotUpdated`. Set `SnapshotOptions.Update` only when you intend to update the baseline. Snapshot options also control style and title inclusion and the snapshot working directory.

## Test cleanup

The `tuitesttest` package creates unique sessions and registers cleanup before starting the terminal. It closes the session when the test ends, including after a fatal test failure.

```go
func TestTerminal(t *testing.T) {
    terminal := tuitesttest.New(t, tuitesttest.Options{})
    if err := terminal.Submit(tuitest.Ptr("echo hello")); err != nil {
        t.Fatal(err)
    }
    if err := terminal.WaitCommand(tuitest.WaitOptions{}); err != nil {
        t.Fatal(err)
    }
}
```

Import `testing`, `github.com/microsoft/tui-test/bindings/go`, and `github.com/microsoft/tui-test/bindings/go/tuitesttest` in your test file. `tuitesttest.New` accepts client, spawn, shell, program, argument, and session-prefix settings through `tuitesttest.Options`. `ClientOptions.Artifacts` exports evidence when a tui-test assertion fails. `ClientOptions.Trace` with `TraceOnFailure` retains a trace when test cleanup reports that the test failed. `tuitesttest.TerminalSnapshot` removes trailing whitespace and blank lines for text comparisons. Each call has its own options, so parallel tests do not share mutable defaults.

## Contributing

See [building and testing the Go binding](CONTRIBUTING.md).
