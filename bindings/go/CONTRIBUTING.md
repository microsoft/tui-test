# Building and maintaining the Go binding

The [Go README](README.md) covers installation and application usage. This guide covers native builds and validation.

## Architecture

The public `tuitest` package at the module root owns clients, options, results, locators, and input helpers. Its `engine.go` adapter converts between public types and the internal native types.

The `internal/native` package owns the C ABI, purego calls, native memory lifetimes, and library loading. It copies results into Go-owned values before releasing Rust allocations. Dependencies run from the public package into `internal/native`; the native package does not import the public package.

The Rust adapter source stays in `native/` and delegates to the existing `tui-test` session registry and operations. Source builds load that adapter from the path in `TUI_TEST_GO_NATIVE_LIBRARY` and keep it loaded for the process lifetime.

Terminal behavior, session synchronization, assertions, and recording remain in the Rust engine. Go owns option ergonomics, error presentation, and test cleanup. Keep new terminal behavior in the engine so language bindings share it.

The native library and Go module must come from the same source revision. The C ABI is private to this binding. Native allocations must be released by their matching Rust functions, and native code must not retain Go pointers.

## Build from source

Install Go 1.26 or newer, Rust 1.90 or newer, Zig 0.16.0, and the native build tools required by your Rust target. Windows needs the MSVC build tools.

From the repository root:

```sh
cargo build --locked -p tui-test-go
```

The native adapter enables the same terminal backends and recording features as the JavaScript and Python bindings. Zig is required by the Ghostty dependency.

Set `TUI_TEST_GO_NATIVE_LIBRARY` to the built library before running a Go application or test. For Linux:

```sh
cd bindings/go
TUI_TEST_GO_NATIVE_LIBRARY=../../target/debug/libtui_test_go.so CGO_ENABLED=0 go test ./...
```

On macOS, run from `bindings/go`:

```sh
TUI_TEST_GO_NATIVE_LIBRARY=../../target/debug/libtui_test_go.dylib CGO_ENABLED=0 go test ./...
```

On Windows amd64, run from the repository root:

```powershell
Set-Location bindings/go
$env:TUI_TEST_GO_NATIVE_LIBRARY = (Resolve-Path ../../target/debug/tui_test_go.dll)
$env:CGO_ENABLED = '0'
go test ./...
```

For a release build, use `cargo build --release --locked -p tui-test-go` and point the environment variable at the library under `target/release`.

To consume unpublished changes from another Go module, build the library as above, then add a local module replacement in the consuming project:

```sh
go mod edit -replace github.com/microsoft/tui-test/bindings/go=/absolute/path/to/tui-test/bindings/go
go get github.com/microsoft/tui-test/bindings/go
```

## Validate changes

After setting `TUI_TEST_GO_NATIVE_LIBRARY`, run these commands from `bindings/go`:

```sh
gofmt -l .
go vet ./...
go test ./...
```

`gofmt -l .` should produce no output. Also run `go test -race ./...` with cgo enabled and a C compiler available for Go's race detector; the binding itself does not require cgo.

From the repository root, also run the Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p tui-test-rs --all-targets --no-default-features -- -D warnings
cargo test --workspace -- --test-threads=1
```

`TestCloseInterruptsPendingWait` is explicitly skipped for the accepted shared-runtime limitation in [issue #207](https://github.com/microsoft/tui-test/issues/207). Its regression body remains in place. Remove the skip and run the test when the upstream fix is incorporated. The `CloseAll` interruption test remains active.
