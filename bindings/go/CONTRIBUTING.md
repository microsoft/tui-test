# Building and maintaining the Go binding

The [Go README](README.md) covers installation and application usage. This guide covers native builds and validation.

## Architecture

The public `tuitest` package at the module root owns clients, options, results, immutable locator queries, input helpers, lifecycle validation, and the typed error surface. Its `engine.go` adapter converts between public types and the internal native types.

The `internal/native` package owns the C ABI, purego calls, native memory lifetimes, and library loading. It copies results into Go-owned values before releasing Rust allocations. Dependencies run from the public package into `internal/native`; the native package does not import the public package.

The shared Rust adapter lives in `../../crates/tui-test-ffi/` and delegates to the existing session registry and operations.
Go, Python, and JavaScript use the same library through purego, ctypes, and Koffi.
Source builds select the library path through `TUI_TEST_NATIVE_LIBRARY`.
The library stays loaded for the process lifetime.

Release packaging adds platform-specific `go:embed` files and compressed native archives.
Linux embeds both libc variants for the selected architecture.
The loader checks for glibc's `gnu_get_libc_version` symbol in the running process.
It selects musl when that symbol is absent.
It never loads one engine as a fallback after the other fails.

The cache uses the library's SHA-256 digest as its directory name.
Extraction checks the archive and library checksum before an atomic hard-link publication.
Concurrent processes cannot replace a library that another process has already loaded.
A corrupt cache entry returns an error with its path.

Go uses pointer entrypoints for open, run, and screenshot options.
The screenshot pointer avoids a PureGo aggregate-argument failure on Linux AMD64.

Terminal behavior, session synchronization, locator evaluation, assertions, diagnostics, traces, artifacts, and recording remain in the Rust engine. Go owns option ergonomics, query construction, error presentation, and test cleanup. The `tuitesttest` package reports the test result through failure-aware close so the engine can retain on-failure traces. Keep new terminal behavior in the engine so language bindings share it.

The native library and Go module must come from the same source revision.
The C ABI is an internal contract shared by all three bindings, not a stable public C SDK.
Native allocations must be released by their matching Rust functions, and native code must not retain Go pointers.

`internal/native/abi.go` and `internal/native/functions.go` are generated from the Rust ABI declarations.
The generator first derives Rust boundary structs and conversions from the owned Rust models.
See [the shared library guide](../../crates/tui-test-ffi/README.md#generate-boundaries) for authoritative sources and transport mappings.
Do not edit these files directly. From the repository root, regenerate them with:

```sh
cargo run --locked -p tui-test-bindgen
```

The same command updates the C header, Python declarations, and JavaScript declarations.
Use `cargo run --locked -p tui-test-bindgen -- --check` to check for stale output.

### Check the Go ABI

The optional compiler check compares Go types against the shared C header.
It checks the ABI version, struct sizes, alignment, field sizes, and field offsets.
It also requires a check for each generated Go struct.
The check uses cgo and requires a source checkout with `crates/tui-test-ffi/include/tui_test.h`.
Ordinary builds do not use cgo or this header.

From `bindings/go`, run:

```sh
CGO_ENABLED=1 go test -tags=tuitest_abi_check -run '^TestNativeLayoutMatchesCompiler$' -count=1 ./internal/native
```

CI runs this check on Linux, macOS, and Windows.
It also checks Go formatting, runs `go vet`, and tests with the race detector.
The race tests require the native library override described below:

```sh
go vet ./...
CGO_ENABLED=1 go test -race ./...
```

## Build from source

Install Go 1.26 or newer, Rust 1.90 or newer, and Zig 0.16.0. Linux needs a native C/C++ compiler and linker, macOS needs Apple command-line build tools, and Windows amd64 needs the MSVC Build Tools and `x86_64-pc-windows-msvc` Rust toolchain. The [installation guide](README.md#install) lists the supported host OS and architecture combinations.

From the repository root:

```sh
env -u CARGO_TARGET_DIR cargo build --locked -p tui-test-ffi
```

On musl Linux, disable static CRT linkage so Rust emits a `cdylib`:

```sh
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" cargo build --locked -p tui-test-ffi
```

Keep this flag for release builds and explicit musl targets.
For a cross-build, install `cargo-zigbuild` and the Rust target, then run:

```sh
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" cargo zigbuild --locked -p tui-test-ffi --target x86_64-unknown-linux-musl
```

The cross-built library is `target/x86_64-unknown-linux-musl/debug/libtui_test_ffi.so`.
It must run in a musl process.
The Zig build links the C++ runtime into the library.
Other toolchains can add shared dependencies; inspect them before distribution.

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

## Build a bundled module

One script creates native archives, assembles Go modules, and verifies complete release modules.
The native archives are the common input for Go embedding and explicit native installations.
Each archive contains the library, `LICENSE`, and `manifest.json`.
The manifest records the version, source revision, target, and library checksum.
The module assembler rejects missing, mixed-revision, or corrupt release artifacts.

From the repository root, package a local macOS ARM64 build:

```sh
python3 .github/scripts/release/package_native.py native \
  --library target/debug/libtui_test_ffi.dylib \
  --target aarch64-apple-darwin \
  --output target/native-packages
python3 .github/scripts/release/package_native.py go \
  --artifacts target/native-packages \
  --target aarch64-apple-darwin \
  --output target/go-package
python3 .github/scripts/test-go-embedded.py target/go-package
```

The output directory must not exist before module assembly.
Use the corresponding target and library path for other platforms.
For a Linux package, supply both GNU and musl archives for the selected architecture.
Omit `--target` to require all seven release targets.
The generated files belong only in the bundled release commit, not the source branch.

The consumer check creates a separate Go module with a local replacement.
It compiles without Rust, Zig, or a C compiler on `PATH`.
It runs the smoke example twice without an override or external tools.
Both runs print `Go binding smoke passed (bundled engine)`.
The first run extracts the engine; the second reuses the cache.

To use the local bundle in another project, replace the module with
`/absolute/path/to/tui-test/target/go-package`, rather than the source directory.

### Linux compatibility

Build GNU release libraries in a manylinux 2.28 container:

```sh
node bindings/js/scripts/build-native.mjs --zig --target aarch64-unknown-linux-gnu
```

This command requires Docker and Node.js.
It runs cargo-zigbuild inside the container and shares the Linux tool setup with Python wheel builds.
Cargo's glibc target alone does not constrain Ghostty's separate native Zig build.
The container also constrains that build to the supported libc.
Container builds use `target/manylinux` to avoid artifacts from a newer host libc.
The output remains under `target/aarch64-unknown-linux-gnu/release`.
Build musl separately with `-C target-feature=-crt-static`.
The release pipeline checks native dependencies before packaging.
It does not treat an arbitrary Ubuntu runner as a compatible glibc baseline.

The compatibility study confirmed these distinctions:

| Consumer | Result |
| --- | --- |
| Native Alpine Go build with `CGO_ENABLED=0` | Go selects the musl interpreter automatically |
| macOS cross-build for Linux | Go defaults to the glibc interpreter |
| GNU engine built for glibc 2.28 | Runs on AlmaLinux 8 and current Ubuntu |
| Musl engine | Runs on stock Alpine without a separate C++ runtime |
| Musl engine in a glibc Go process | Fails to load; it is not a universal Linux library |

CI covers both embedded engines, native Alpine builds, and explicit musl cross-builds.
It runs packaged consumers on AlmaLinux 8, Rocky Linux 9, Ubuntu, and Alpine.
The smoke consumer needs no shell, so it isolates native engine compatibility.

### Publication

The root release tag identifies source code.
The `bindings/go/v<version>` tag identifies a release-only child commit with generated bundles.
Go requires those files in the tagged repository revision; release attachments alone do not suffice.
Publication must not modify an existing tag with different module contents.
The source revision in each native manifest must match the root release.
The workflow verifies the assembled module, then uses Git to create and push only the Go tag.
It uses the same automation identity as the Homebrew publication workflow.

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
