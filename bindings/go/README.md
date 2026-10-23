# tui-test for Go

Control, inspect, test, and record terminal apps from Go. The binding runs the Rust engine in your process and does not require the CLI.

## Install

Use Go 1.26 or newer and select a published Go module release:

```sh
go get github.com/microsoft/tui-test/bindings/go@v0.1.1
```

The repository tag is `bindings/go/v<version>`.
Released modules contain prebuilt native engines. Users need no Rust compiler,
Zig, C compiler, CLI, or separate installer.
Source checkouts do not contain these binaries; see [Build and use](../../crates/tui-test-ffi/README.md#build-and-use).

| OS | Go architectures | Native runtime |
| --- | --- | --- |
| Linux | `amd64`, `arm64` | glibc 2.28+ or musl 1.2+ |
| macOS | `amd64`, `arm64` | macOS system libraries |
| Windows | `amd64` | Windows system libraries |

Go downloads all platform archives with the module.
Each executable embeds compressed archives for its target OS and architecture.
Linux executables contain both libc variants and select the one that matches the running process.
Other platforms contain one variant.

On first use, the binding extracts the selected engine into the user cache:
`tui-test/native/<sha256>/`.
Later processes verify and reuse the same file.
The engine stays loaded for the process lifetime.
There are no runtime downloads.

The cache must permit file creation, hard links, and native library loading.
For a read-only or restricted environment, use an existing installation as described below.

### Linux and cross-compilation

PureGo avoids cgo, but it still uses the system dynamic loader and libc.
Embedding a library does not make the executable statically linked.
Native glibc builds select the GNU engine. Normal native Alpine builds select the musl engine.
Neither needs a custom Go build tag.
Cgo-enabled applications can require a newer libc than the bundled engine.

Cross-compile for glibc Linux with:

```sh
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build .
```

For musl, also select the destination interpreter:

```sh
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 \
  go build -ldflags '-I /lib/ld-musl-x86_64.so.1' .
```

For ARM64 musl, use `GOARCH=arm64` and `/lib/ld-musl-aarch64.so.1`.
The interpreter must exist on the destination machine.
A missing interpreter prevents startup before the binding can select an engine.

### Existing native installation

Set `TUI_TEST_NATIVE_LIBRARY` to override the bundled engine:

```sh
export TUI_TEST_NATIVE_LIBRARY=/absolute/path/to/libtui_test_ffi.so
```

Use `libtui_test_ffi.dylib` on macOS or `tui_test_ffi.dll` on Windows.
Use the native archive from the same release and the correct platform.
This mode needs no extraction cache.
An invalid override returns an error; it never falls back to the bundled engine.
The loader checks the ABI version, but that check does not prove an exact source-revision match.

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

The [smoke example](examples/smoke/main.go) runs its own child process and needs no external shell:

```sh
go run github.com/microsoft/tui-test/bindings/go/examples/smoke
```

Without an override, it prints `Go binding smoke passed (bundled engine)`.

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
| Native history | `Scroll(int32)`, `ScrollToTop`, `ScrollToBottom`, `GetViewport` |
| Screen | `State`, `Text`, `Cells` |
| Command state | `GetCommand`, `GetOutput`, `GetExitCode`, `GetCwd` |
| Terminal state | `GetCursor`, `GetSize`, `GetTitle`, `GetClipboard`, `GetBellCount`, `GetBellEvents` |
| Waits | `WaitTitle`, `WaitClipboard`, `WaitIdle`, `WaitCommand`, `WaitExit`, `WaitReady`, `WaitBell` |
| Assertions | `ExpectTitle`, `ExpectExitCode`, `ExpectOutput`, `ExpectBellCount`, `ExpectSnapshot` |
| Capture | `Screenshot`, `StartRecording`, `PauseRecording`, `ResumeRecording`, `StopRecording` |

Use `WaitCommand` after submitting a shell command, and `WaitExit` after running a program directly. `WaitIdle` only establishes that the screen stopped changing.

`State` returns the visible text and terminal dimensions together with command, process-exit, cursor, mode, mouse-mode, color, timeout, readiness, and bell state. `Cells` returns styles and OSC 8 link metadata through `Cell.Link` and `Cell.LinkID`.

### Native history

`Scroll(lines int32)` shows older output for positive values and newer output for negative values.
Zero leaves the viewport unchanged. Movement stops at the available history boundaries.
`GetViewport()` returns `Viewport{Offset, History}` with full-width `uint64` row counts.
An offset of zero means the live screen.

Visible text, locators, and captures use this viewport.
`Text(TextOptions{Full: true})` remains independent: it returns history plus the live screen.
New output preserves the historical view until retained history expires.
Keyboard input returns to the live screen.
Mouse input and historical locator clicks fail while the viewport shows history.
Call `ScrollToBottom()` before mouse input. This API is separate from `Mouse.Scroll`.

`GetCursor()` and `State.Cursor` retain the application cursor coordinates.
Captures project that cursor into the viewport and hide it outside the view.
Raster recordings capture viewport changes. Asciicast remains a PTY stream.

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

`PauseRecording` excludes subsequent time while the terminal continues. `ResumeRecording` captures the current screen, not intermediate paused output.
These controls apply to all manual formats. Automatic diagnostic recordings remain continuous.
`StopRecording` also works during a pause. Repeated pause or resume calls return usage errors.

### Rendering settings and native assertions

Set `Style` in `ClientOptions` or `SpawnOptions`. `RenderStyle` supports fonts, colors, padding, window controls, borders, shadows, and cursor animation.
A per-call style replaces the constructor style. Nil fields use Rust defaults. Use `Ptr(false)` and `Ptr(0)` for explicit values.
Restart preserves the resolved style. These settings affect screenshots and raster recordings, not terminal colors or locator assertions.

```go
style := &tuitest.RenderStyle{
    FontSize: tuitest.Ptr(24.0),
    Window: tuitest.WindowStyle{TitleBar: tuitest.Ptr(false)},
    Cursor: tuitest.RecordingCursorStyle{
        Blink: tuitest.Ptr(false),
        BlinkIntervalMS: tuitest.Ptr(uint64(250)),
    },
}
terminal, err := tuitest.Ephemeral("styled", tuitest.ClientOptions{Style: style})
```

`Blink` permits application-requested animation. Its default is true. `BlinkIntervalMS` sets each phase before playback speed scaling; its default is 500.
`Padding` accepts either `Uniform` or individual sides, not both. Screenshots always use the visible cursor phase.
Run `go run ./examples/rendering` for a complete example.

`GetModes` and `GetColors` read native state. `ExpectCursor`, `ExpectMode`, and `ExpectColors` poll in Rust and use the text timeout by default.
Use `CursorExpectation` and `ColorExpectation` for optional properties. Palette expectations use `map[uint8]string`.

Raster recording samples the live terminal at `FPS` (default: 30) and preserves elapsed time until stop.
There is no default idle cap or extra final hold.
Set `IdleTimeLimit` to cap unchanged visual intervals before `Speed` scales playback time.
Asciicast remains a timestamped output stream; FPS sampling does not change it.

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
