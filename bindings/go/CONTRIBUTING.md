# Building and maintaining the Go binding

The [Go README](README.md) covers installation and application usage. This guide covers native builds and validation.

## Architecture

The public `tuitest` package at the module root owns clients, options, results, immutable locator queries, input helpers, lifecycle validation, and the typed error surface. Its `engine.go` adapter converts between public types and the internal native types.

The `internal/native` package owns the C ABI, purego calls, native memory lifetimes, and library loading. It copies results into Go-owned values before releasing Rust allocations. Dependencies run from the public package into `internal/native`; the native package does not import the public package.

The shared Rust adapter lives in `../../crates/tui-test-ffi/` and delegates to the existing session registry and operations.
Go and Python use the same library. JavaScript uses the crate's optional N-API build.
Source builds select the Go/Python library path through `TUI_TEST_NATIVE_LIBRARY`.
The library stays loaded for the process lifetime.

Go uses pointer entrypoints for open, run, and screenshot options.
The screenshot pointer avoids a PureGo aggregate-argument failure on Linux AMD64.

Terminal behavior, session synchronization, locator evaluation, assertions, diagnostics, traces, artifacts, and recording remain in the Rust engine. Go owns option ergonomics, query construction, error presentation, and test cleanup. The `tuitesttest` package reports the test result through failure-aware close so the engine can retain on-failure traces. Keep new terminal behavior in the engine so language bindings share it.

The native library and Go module must come from the same source revision.
The C ABI is an internal contract shared by Go and Python, not a stable public C SDK.
Native allocations must be released by their matching Rust functions, and native code must not retain Go pointers.

`internal/native/abi.go` and `internal/native/functions.go` are generated from the Rust ABI declarations.
The generator first derives Rust boundary structs and conversions from the owned Rust models.
See [the shared library guide](../../crates/tui-test-ffi/README.md#generate-boundaries) for authoritative sources and transport mappings.
Do not edit these files directly. From the repository root, regenerate them with:

```sh
cargo run --locked -p tui-test-bindgen
```

The same command updates the C header and Python declarations.
Use `cargo run --locked -p tui-test-bindgen -- --check` to check for stale output.

## Build from source

Install Go 1.26 or newer, Rust 1.90 or newer, and Zig 0.16.0. Linux needs a native C/C++ compiler and linker, macOS needs Apple command-line build tools, and Windows amd64 needs the MSVC Build Tools and `x86_64-pc-windows-msvc` Rust toolchain. The [installation guide](README.md#install) lists the supported host OS and architecture combinations.

From the repository root:

```sh
cargo build --locked -p tui-test-ffi
```

The shared library enables all terminal backends and bundled recording font styles. Zig is required by the Ghostty dependency.

Set `TUI_TEST_NATIVE_LIBRARY` to the built library before running a Go application or test. For Linux:

```sh
cd bindings/go
TUI_TEST_NATIVE_LIBRARY=../../target/debug/libtui_test_ffi.so CGO_ENABLED=0 go test ./...
```

On macOS, run from `bindings/go`:

```sh
TUI_TEST_NATIVE_LIBRARY=../../target/debug/libtui_test_ffi.dylib CGO_ENABLED=0 go test ./...
```

On Windows amd64, run from the repository root:

```powershell
Set-Location bindings/go
$env:TUI_TEST_NATIVE_LIBRARY = (Resolve-Path ../../target/debug/tui_test_ffi.dll)
$env:CGO_ENABLED = '0'
go test ./...
```

For a release build, use `cargo build --release --locked -p tui-test-ffi` and point the environment variable at the library under `target/release`.

To consume unpublished changes from another Go module, build the library as above, then add a local module replacement in the consuming project:

```sh
go mod edit -replace github.com/microsoft/tui-test/bindings/go=/absolute/path/to/tui-test/bindings/go
go get github.com/microsoft/tui-test/bindings/go
```

## Validate changes

After setting `TUI_TEST_NATIVE_LIBRARY`, run these commands from `bindings/go`:

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
