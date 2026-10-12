# Shared native library

`tui-test-ffi` exposes the Rust engine through a typed C ABI.
Go uses `purego`, and Python uses `ctypes`, to load the default shared library.
The optional `napi` feature adds the JavaScript adapter to this crate.
That adapter calls the C ABI functions directly and produces one self-contained `.node` addon.
The default build has no N-API dependencies.

The library and language package must come from the same source revision.
This ABI is an internal contract, not a stable public C SDK.

## Build and use

From the repository root:

```sh
cargo build --locked -p tui-test-ffi
```

The library is `target/debug/libtui_test_ffi.so` on Linux, `libtui_test_ffi.dylib` on macOS, or `tui_test_ffi.dll` on Windows.
Set `TUI_TEST_NATIVE_LIBRARY` to its absolute path to select it explicitly in Go or Python.
Python wheels also include the library. JavaScript ignores this variable and needs no companion library.

For example, run the Go smoke example on macOS:

```sh
export TUI_TEST_NATIVE_LIBRARY="$PWD/target/debug/libtui_test_ffi.dylib"
cd bindings/go
CGO_ENABLED=0 go run ./examples/smoke
```

The example starts a child terminal, submits input, checks its output, and prints `Go binding smoke passed`.

Build the JavaScript addon and declarations from the same crate:

```sh
cd bindings/js
npm run build:native:debug
npm run build:ts
node --input-type=module -e '
  import { TuiTest } from "./dist/index.js";
  const terminal = TuiTest.ephemeral();
  try {
    await terminal.run(process.execPath, ["-e", "console.log(\"Ready\")"]);
    await terminal.getByText("Ready").expect();
    console.log(await terminal.text());
  } finally {
    await terminal.closeQuiet();
  }
'
```

The build enables `--features napi` and writes the addon, loader, and declarations to `bindings/js/native/`.
The example prints terminal text that contains `Ready`.

## Generate declarations

The Rust declarations in `src/types.rs` and `src/lib.rs` are the source of truth.
The generator parses Rust syntax and rejects unsupported field types. It does not parse source with regular expressions.

```sh
cargo run --locked -p tui-test-bindgen
cargo run --locked -p tui-test-bindgen -- --check
```

The generator uses `cbindgen` for `include/tui_test.h`.
It also generates the Go structs and function table, and Python `ctypes` declarations.
The JavaScript adapter uses the canonical Rust ABI types without a generated function table.
Run the generator with Go and Rust formatters on `PATH`.

The language adapters still own asynchronous execution, error presentation, input storage, and result conversion.
They do not implement terminal behavior.

## Ownership

Input buffers must remain valid until a call returns. The engine copies inputs before it executes an operation.
A null string pointer means absent. A non-null pointer with zero length means an explicit empty string.

Every result belongs to this library until `tui_result_free` releases it.
Copy strings, arrays, and packed screen bytes before that call. Do not free individual fields.
The function called determines which result field contains the successful output.
Check `error_kind` before reading a successful result.

Keep the library loaded while any call or allocation remains active.
Increase `ABI_VERSION` when a change makes existing declarations incompatible.
