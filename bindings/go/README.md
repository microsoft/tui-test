# tui-test for Go

Control, inspect, test, and record terminal apps from Go 1.26 or newer. Published modules bundle the prebuilt native engine and do not require Rust, Zig, cgo, or the `tui-test` CLI.

```sh
go get github.com/microsoft/tui-test/bindings/go@v0.1.1
go run github.com/microsoft/tui-test/bindings/go/examples/smoke
```

Published modules support Linux, macOS, and Windows on `amd64` and `arm64`. Linux requires glibc 2.28+ or musl 1.2+ and a writable user cache on first run (or set `TUI_TEST_NATIVE_LIBRARY` to an explicit shared library path).

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

See the [Go API reference](https://tui-test.dev/docs/api/go/) for client options, locators, cross-compilation, recordings, and `tuitesttest` helpers, or start with [writing and running tests](https://tui-test.dev/docs/getting-started/testing/). For local source checkouts, see [`crates/tui-test-ffi/README.md`](https://github.com/microsoft/tui-test/blob/main/crates/tui-test-ffi/README.md#build-and-use).
