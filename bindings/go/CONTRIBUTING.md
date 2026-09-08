# Building and maintaining the Go binding

The [Go README](README.md) covers installation and application usage. This guide covers native builds, validation, and release maintenance.

## Architecture

The public `tuitest` package at the module root owns clients, options, results, immutable locator queries, input helpers, lifecycle validation, and the typed error surface. Its `engine.go` adapter converts between public types and the internal native types.

The `internal/native` package owns the C ABI, purego calls, native memory lifetimes, library loading, and embedded platform libraries. It copies results into Go-owned values before releasing Rust allocations. Dependencies run from the public package into `internal/native`; the native package does not import the public package.

The Rust adapter source stays in `native/` and delegates to the existing `tui-test` session registry and operations. Published platform libraries live under `internal/native/embedded` and are embedded into the application. On first use, the native package verifies or extracts its engine into a content-addressed user cache and loads it for the process lifetime. A non-empty `TUI_TEST_GO_NATIVE_LIBRARY` is the sole load candidate and does not fall back to embedded libraries if it fails.

Terminal behavior, session synchronization, locator evaluation, assertions, diagnostics, traces, artifacts, and recording remain in the Rust engine. Go owns option ergonomics, query construction, error presentation, and test cleanup. The `tuitesttest` package reports the test result through failure-aware close so the engine can retain on-failure traces. Keep new terminal behavior in the engine so language bindings share it.

The native library and Go module must have matching versions. The C ABI is private to this binding. Native allocations must be released by their matching Rust functions, and native code must not retain Go pointers.

## Build from source

Install Go 1.26 or newer, Rust 1.90 or newer, and the native build and link tools required by your Rust target. Linux GNU builds use the native C/C++ toolchain, macOS builds use the Apple command-line tools, and Windows amd64 builds use the MSVC toolchain. Linux musl release builds additionally use Zig 0.16.0 and `cargo-zigbuild`. These are contributor prerequisites; application developers using a published module only need Go.

From the repository root:

```sh
cargo build --locked -p tui-test-go
```

The native adapter enables the same terminal backends and recording features as the JavaScript and Python bindings. The build verifies the generated C header against the checked-in `internal/native/native.h`.

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

A source checkout contains placeholders for packaged platforms. The release pipeline supplies all seven engine builds before publishing the module, so source consumers must use `TUI_TEST_GO_NATIVE_LIBRARY`.

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

The [CI workflow](../../.github/workflows/ci.yml) first smoke-tests `TUI_TEST_GO_NATIVE_LIBRARY`, then embeds the host library and runs the Go checks and smoke example through the bundled path. It also runs the JavaScript and Python binding regression suites.

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

The private ABI version is separate from the workspace release version. The exported `tui_abi_version()` function in `native/src/lib.rs` currently returns `3`. Go registers that symbol as `nativeFunctionTable.AbiVersion`; `loadNativeFunctions` in `internal/native/loader.go` requires the returned value to equal `3`, and `checkNativeVersion` in `internal/native/session.go` repeats that requirement before a session operation. When the private C ABI changes incompatibly, update all three values together. The CI override and bundled smoke runs both initialize the native engine, so either run fails with an incompatible-version error if the Rust value and Go requirement differ.

After changing ABI types, compare the Go layouts against the C compiler's sizes, alignments, and field offsets:

```sh
CGO_ENABLED=1 go test -tags=tuitest_abi_check -run '^TestNativeLayoutMatchesCompiler$' -count=1 ./internal/native
```

Run this command from `bindings/go` with a C compiler installed. On PowerShell, set `$env:CGO_ENABLED = '1'` before the `go test` command. This contributor-only check uses the header; normal builds and tests need no cgo.

## Release the binding

The normal release starts when a maintainer pushes a repository tag matching `<major>.<minor>.<patch>` or `<major>.<minor>.<patch>-beta.<number>`. For example:

```sh
git tag 0.1.0
git push origin 0.1.0
```

The workflow checks out that tag, verifies its format and project versions, builds the release, and creates the GitHub release before publishing the bundled Go module tag.

The [release workflow](../../.github/workflows/release.yml) builds libraries for Linux glibc and musl on amd64 and arm64, macOS on amd64 and arm64, and Windows on amd64. GNU targets use native Rust and linker runners, macOS uses the Apple toolchain, Windows uses MSVC, and musl targets use Zig 0.16.0 with `cargo-zigbuild`. Each native archive includes its header, library, license, version, target, and source commit, with a separate SHA-256 checksum. Each target is tested from an external Go consumer with cgo disabled and without Rust on its executable search path.

The module assembly job verifies every checksum and the recorded version, target, source commit, library presence, and size before placing all seven libraries under `internal/native/embedded/<target>`. It adds the license, records the source release, checks the assembled module size, and tests an independent consumer with cgo disabled. Platform-specific embedding includes only the relevant libraries in an application; Linux includes both libc variants and tries them in order.

After successful release publication, the Go publication job creates a commit containing the bundled module and publishes `bindings/go/v<version>`. For example, repository release `0.1.0` corresponds to Go tag `bindings/go/v0.1.0`. This tag points to the assembled sources containing the binaries, not the original release commit. The original source commit is recorded in the bundled provenance. The publication job does not update the main branch and refuses to replace an existing tag containing different module contents.

GitHub release attachments alone cannot supply the binaries to `go get`; the nested Go tag must contain them. Use the workflow's publication job rather than manually tagging the unbundled source commit.

If publication fails after the repository tag exists, open the `Release` workflow in GitHub Actions, choose **Run workflow**, and set its required `tag` input to that existing tag, such as `0.1.0`. A manual run rebuilds from the named tag and can retry the package and Go-module publication jobs; it does not recreate the push-only GitHub release. Go publication is successful when the `publish-go-module` job succeeds and the remote `bindings/go/v<version>` tag contains the assembled module. If that nested tag already has identical contents, the job reports that it already contains the bundled module and succeeds; different contents cause the job to fail without replacing the tag.

The module and its embedded engine must come from the same source release.
