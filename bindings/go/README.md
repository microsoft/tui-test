# tui-test for Go

Control, inspect, test, and record terminal apps from Go. The binding runs the Rust engine in your process and does not require the CLI.

## Install

With Go 1.26 or newer, add the binding to your project:

```sh
go get github.com/microsoft/tui-test/bindings/go
```

Published Go modules include the Rust engine. You do not need the CLI, Rust, a C compiler, or native library configuration. The binding supports Windows amd64, macOS amd64 and arm64, and Linux amd64 and arm64 with glibc or musl.

The engine is embedded in your application and loaded automatically on first use. Loading requires a writable user cache directory where the operating system permits loading native libraries. The binding stores verified engine files under `tui-test/native` in the directory returned by Go's `os.UserCacheDir`. It does not download anything at runtime.

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

Clients with the same name share the same in-process session. `Close` closes that session for every client. An existing client can open it again. `Sessions` lists live sessions, `CloseAll` closes them, and `Recording(session)` retrieves the automatic recording's asciicast content after closure. `OpenResult.Recording` contains its path.

`ClientOptions` sets the backend, profile, timeouts, automatic recording, and assertion artifacts. `SpawnOptions` controls dimensions, working directory, environment, readiness, restart, retries, and per-launch overrides. `OpenOptions` embeds `SpawnOptions` and adds `Shell`.

The default backend is Alacritty. The native release also includes Ghostty, Rio, and xterm.js. The default size is 80 columns by 30 rows. Automatic recording defaults to `RecordingAlways`; use `RecordingDisabled` or `RecordingOnFailure` to change it.

Optional pointer fields distinguish omission from an explicit value. For example:

```go
options := tuitest.SpawnOptions{
    Cols:      tuitest.Ptr(uint16(100)),
    Rows:      tuitest.Ptr(uint16(40)),
    WaitReady: tuitest.Ptr(false),
}
```

Timeouts use `*time.Duration`. A nil timeout uses the client setting, then the engine default: 5 seconds for text and idle, or 30 seconds for command, exit, and ready. Explicit zero remains zero. Negative durations are rejected; positive fractions of a millisecond round upward.

Methods block until their operation completes. Use goroutines for concurrent work. The Rust runtime serializes operations on each session. This API does not accept contexts or promise per-call cancellation.

## API reference

Use `go doc github.com/microsoft/tui-test/bindings/go` for the complete exported types and signatures. Methods return ordinary Go errors; getters preserve unavailable values with pointers where applicable.

| Capability | Methods |
| --- | --- |
| Lifecycle | `Open`, `Run`, `Close`, `CloseQuiet`, `Session` |
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

### Locators

`GetByText` and `GetByStyle` create lazy locators. Chaining returns a new locator and leaves the original unchanged. For example:

```go
ready := terminal.GetByText("Ready", tuitest.TextSelectorOptions{})
err := ready.Last().Expect(tuitest.LocatorExpectOptions{})
```

Select matches with `Any`, `Unique`, `First`, `Last`, or `Nth`. Inspect them with `Locations`, `Location`, `Count`, or `All`. Use `Wait`, `Expect`, `Click`, and `Highlight` for actions. `Location` and `Click` require a unique match unless you select one explicitly.

Nested text/style selectors support `Within`, `After`, and `Before` directions. Text selectors also support regular expressions, scrollback, and exact or normalized whitespace. Style pointer fields preserve explicit false values, such as `Bold: tuitest.Ptr(false)`.

### Errors and artifacts

Use `errors.As` with `*tuitest.Error` to inspect `Kind`, `Message`, `Operation`, and optional terminal diagnostics. Error kinds are `AssertionError`, `UsageError`, `NoSessionError`, and `InternalError`.

Configure `ClientOptions.Artifacts` to attach text or SVG artifacts to assertion failures. Failure to capture an optional artifact does not replace the assertion error.

### Recording and snapshots

`StartRecording` supports asciinema, APNG, GIF, and MP4. File extensions select the format unless `RecordingOptions.Format` overrides it. `StopRecording` finishes the recording and returns its path. MP4 requires `ffmpeg` on the executable search path.

`ExpectSnapshot` returns `SnapshotPassed`, `SnapshotWritten`, or `SnapshotUpdated`. Set `SnapshotOptions.Update` only when you intend to update the baseline. Snapshot options also control color and title inclusion and the snapshot working directory.

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

Import `testing`, `github.com/microsoft/tui-test/bindings/go`, and `github.com/microsoft/tui-test/bindings/go/tuitesttest` in your test file. Pass client, spawn, shell, or program options through `tuitesttest.Options`. Configure `Client.Artifacts` to capture a failed test's terminal state. Each call has its own options, so parallel tests do not share mutable defaults.

## Contributing

See [building, testing, and releasing the Go binding](CONTRIBUTING.md).
