# Building and maintaining the Go binding

The [Go README](README.md) covers installation and application usage. This guide covers native builds, validation, and release maintenance.

## Architecture

The public `tuitest` package at the module root owns clients, options, results, locators, and input helpers. Its `engine.go` adapter converts between public types and the internal native types.

The `internal/native` package owns the C ABI, purego calls, native memory lifetimes, library loading, and embedded platform libraries. It copies results into Go-owned values before releasing Rust allocations. Dependencies run from the public package into `internal/native`; the native package does not import the public package.

The Rust adapter source stays in `native/` and delegates to the existing `tui-test` session registry and operations. Platform libraries live under `internal/native/embedded` and are embedded into the application. On first use, the native package verifies or extracts its engine into a content-addressed user cache and loads it for the process lifetime.

Terminal behavior, session synchronization, assertions, and recording remain in the Rust engine. Go owns option ergonomics, error presentation, and test cleanup. Keep new terminal behavior in the engine so language bindings share it.

The native library and Go module must have matching versions. The C ABI is private to this binding. Native allocations must be released by their matching Rust functions, and native code must not retain Go pointers.

## Build from source

Install Go 1.26 or newer, Rust 1.90 or newer, Zig 0.16.0, and the native build tools required by your Rust target. Windows needs the MSVC build tools. These are contributor prerequisites; application developers using a published module only need Go.

From the repository root:

```sh
cargo build --locked -p tui-test-go
```

The native adapter enables the same terminal backends and recording features as the JavaScript and Python bindings. Zig is required by the Ghostty dependency. The build verifies the generated C header against the checked-in `internal/native/native.h`.

Copy the built library into its embedding directory before compiling or testing Go. For Linux amd64 with glibc:

```sh
cp target/debug/libtui_test_go.so bindings/go/internal/native/embedded/x86_64-unknown-linux-gnu/
cd bindings/go
CGO_ENABLED=0 go test ./...
```

For macOS Apple silicon, copy `target/debug/libtui_test_go.dylib` to `bindings/go/internal/native/embedded/aarch64-apple-darwin/`. Intel Macs use `x86_64-apple-darwin`. Linux arm64 uses `aarch64-unknown-linux-gnu`; musl builds use the corresponding `*-unknown-linux-musl` directory.

On Windows amd64, run from the repository root:

```powershell
Copy-Item target/debug/tui_test_go.dll bindings/go/internal/native/embedded/x86_64-pc-windows-msvc/
Set-Location bindings/go
$env:CGO_ENABLED = '0'
go test ./...
```

For a release build use `cargo build --release --locked -p tui-test-go` and copy from `target/release`. Rebuild the Go application after replacing an embedded library.

To consume unpublished changes from another Go module, build and copy the library as above, then add a local module replacement in the consuming project:

```sh
go mod edit -replace github.com/microsoft/tui-test/bindings/go=/absolute/path/to/tui-test/bindings/go
go get github.com/microsoft/tui-test/bindings/go
```

A source checkout contains placeholders for the other platforms. The release pipeline supplies all seven engine builds before publishing the module.

## Validate changes

After copying the native library into its embedding directory, run these commands from `bindings/go`:

```sh
gofmt -l .
go vet ./...
go test ./...
golangci-lint config verify
golangci-lint run ./...
```

`gofmt -l .` should produce no output. Also run `go test -race ./...` with cgo enabled and a C compiler available for Go's race detector; the binding itself does not require cgo. Use golangci-lint 2.12.2, as pinned in CI. The module's `.golangci.yml` loads `ruleguard/rules-team.go`; both were copied unchanged from RunnerFoundry. The ruleguard DSL dependency is declared in `go.mod`. Do not run the binding with the `ruleguard` build tag: the linter loads that rule file itself.

Install the linter using the [official binary installation instructions](https://golangci-lint.run/docs/welcome/install/local/). It is a development tool, not a runtime dependency.

From the repository root, also run the Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p tui-test-rs --all-targets --no-default-features -- -D warnings
cargo test --workspace -- --test-threads=1
```

The [CI workflow](../../.github/workflows/ci.yml) also runs the JavaScript and Python binding regression suites.

`TestCloseInterruptsPendingWait` is explicitly skipped for the accepted shared-runtime limitation in [issue #207](https://github.com/microsoft/tui-test/issues/207). Its regression body remains in place. Remove the skip and run the test when the upstream fix is incorporated. The `CloseAll` interruption test remains active.

### Updating the C header

When changing the native ABI, regenerate the header from the Rust definitions with cbindgen. From the repository root on Linux or macOS:

```sh
TUI_TEST_GO_UPDATE_HEADER=1 cargo build -p tui-test-go
cargo build --locked -p tui-test-go
```

On PowerShell:

```powershell
$env:TUI_TEST_GO_UPDATE_HEADER = '1'
cargo build -p tui-test-go
Remove-Item Env:TUI_TEST_GO_UPDATE_HEADER
cargo build --locked -p tui-test-go
```

Review the generated `internal/native/native.h` alongside the Rust and Go changes. The second build checks the header without rewriting it.
After changing ABI types, compare the Go layouts against the C compiler's sizes, alignments, and field offsets:

```sh
CGO_ENABLED=1 go test -tags=tuitest_abi_check -run '^TestNativeLayoutMatchesCompiler$' -count=1 ./internal/native
```

Run this command from `bindings/go` with a C compiler installed. On PowerShell, set `$env:CGO_ENABLED = '1'` before the `go test` command. This contributor-only check uses the header; normal builds and tests need no cgo.

## Release the binding

The [release workflow](../../.github/workflows/release.yml) builds libraries for Linux glibc and musl on amd64 and arm64, macOS on amd64 and arm64, and Windows on amd64. Each native archive includes its header, library, license, and build provenance, with a separate SHA-256 checksum.

The module assembly job verifies all seven archives and places their libraries under `internal/native/embedded/<target>`. It packages the Go module sources, libraries, and license together, then tests an external Go consumer with cgo disabled. Platform-specific embedding includes only the relevant libraries in an application; Linux includes both libc variants and tries them in order.

After successful release publication, the Go publication job creates a detached commit containing the bundled module and publishes `bindings/go/v<version>`. For example, repository release `0.1.0-beta.3` corresponds to Go tag `bindings/go/v0.1.0-beta.3`. This tag points to the assembled sources containing the binaries, not the original release commit. The original source commit is recorded in the bundled provenance. The publication job does not update the main branch and refuses to replace an existing tag containing different module contents.

GitHub release attachments alone cannot supply the binaries to `go get`; the nested Go tag must contain them. Use the workflow's publication job rather than manually tagging the unbundled source commit. A workflow-dispatch retry can publish the assembled module after a failed publication.

Update the Go/native version checks together when changing the workspace release version. The module and its embedded engine must come from the same source release.
