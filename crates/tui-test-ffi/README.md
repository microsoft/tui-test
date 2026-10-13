# Shared native library

`tui-test-ffi` exposes the Rust engine through one typed C ABI.
Go uses `purego`, Python uses `ctypes`, and JavaScript uses Koffi to load this library.
There is no separate Rust N-API adapter or JavaScript-specific native build.
Koffi supplies its own native addon.

The library and language package must come from the same source revision.
This ABI is an internal contract, not a stable public C SDK.
The language adapters preserve their public convenience APIs.
ABI version 4 includes the shared pointer and asynchronous entrypoints.
The existing `tui_screenshot` signature remains unchanged; `tui_screenshot_ptr` adds the pointer form.

## Build and use

From the repository root:

```sh
env -u CARGO_TARGET_DIR cargo build --locked -p tui-test-ffi
```

The output is `target/debug/libtui_test_ffi.so` on Linux, `libtui_test_ffi.dylib` on macOS, or `tui_test_ffi.dll` on Windows.
All three bindings accept `TUI_TEST_NATIVE_LIBRARY` as an explicit library path.
Python wheels and JavaScript platform packages include the library.

For example, run the Go smoke example on macOS:

```sh
export TUI_TEST_NATIVE_LIBRARY="$PWD/target/debug/libtui_test_ffi.dylib"
cd bindings/go
CGO_ENABLED=0 go run ./examples/smoke
```

The example starts a child terminal, submits input, checks its output, and prints `Go binding smoke passed`.

Build and run the JavaScript example from the repository root:

```sh
cd bindings/js
npm ci
npm run build:native:debug
npm run build:ts
node --input-type=module -e '
  import { TuiTest } from "./dist/index.js";
  const terminal = TuiTest.ephemeral();
  try {
    await terminal.run(process.execPath, ["-e", "console.log(\"Ready 界🙂\")"]);
    await terminal.getByText("Ready 界🙂").expect();
    await terminal.waitExit();
    const state = await terminal.state();
    console.log(`Terminal size: ${state.cols}x${state.rows}`);
    console.log(await terminal.text());
  } finally {
    await terminal.closeQuiet();
  }
'
```

The build copies the library to `native/tui-test.<platform>.<extension>`.
The example prints `Terminal size: 80x30` and text that contains `Ready 界🙂`.
Set `TUI_TEST_NATIVE_LIBRARY` to the absolute library path to exercise the explicit override.
The build does not regenerate declarations.

## Generate boundaries

The owned Rust models define the shared fields. Do not edit generated boundary files.
The existing `tools/bindgen` tool reads these models with `syn`.
It generates C boundary structs and fieldwise conversions, then uses `cbindgen` for the C header.
The C function signatures in `src/lib.rs` define the entrypoints.
The same tool generates the Go, Python, and JavaScript boundary declarations.
JavaScript uses generated `native/abi.js`, `native/models.js`, and `native/index.d.ts`.
The handwritten loader and adapter consume these declarations.

| Authoritative source in `crates/tui-test/src/` | Coverage |
| --- | --- |
| `api.rs` | Options, styles, results, operations, and input actions |
| `locator_query.rs` | Locator nodes, expressions, and node kinds |
| `profile.rs` | Profile fields |
| `diagnostics.rs` | Artifact, trace, retention, and session options |
| `shell/mod.rs`, `terminal/backend.rs` | Shell and backend variants |
| `terminal/cell.rs` | Underline variants |

From the repository root:

```sh
env -u CARGO_TARGET_DIR cargo run --locked -p tui-test-bindgen
env -u CARGO_TARGET_DIR cargo run --locked -p tui-test-bindgen -- --check
```

`--check` checks generated files without changes.
Normal generation can replace missing or stale output.
The generator does not compile the FFI crate to discover its models.
Keep `rustfmt` and `gofmt` on `PATH`.
Release packages include generated files and do not need the generator.

Transport mappings handle strings, pointer/length pairs, optional values, property names, colors, maps, and numeric conversions.
They are not another list of shared model fields.
Pointer ownership, result cleanup, lifecycle behavior, and public convenience APIs remain handwritten.
The core retains separate input and effective timeout models.

## Ownership

Input buffers must remain valid until a call returns.
The engine copies inputs before it executes an operation.
A null string pointer means absent.
A non-null pointer with zero length means an explicit empty string.

Every result belongs to this library until `tui_result_free` releases it.
Copy strings, arrays, and packed screen bytes before that call.
Do not free individual fields.
The called function determines which result field contains successful output.
Check `error_kind` before reading a successful result.

The JavaScript adapter follows these same rules.
JavaScript values remain valid after the adapter frees the native result.
Keep the library loaded while any call or allocation remains active.
Increase `ABI_VERSION` when a change makes existing declarations incompatible.

## Asynchronous calls

Immediate getters use Koffi's asynchronous calls without a polling timer.
The adapter retains their input buffers until the callback completes.
It limits concurrent reads without limiting the number of callers.

Other JavaScript operations use generated typed `tui_*_start` entrypoints.
Each start call copies its inputs before it returns an owned `TuiTask`.
A native worker executes the owned operation.
JavaScript polls `tui_task_poll` until it receives the result, then frees the result and task.
The existing synchronous functions remain available to Go and Python.

This avoids Koffi worker exhaustion: pending terminal waits cannot prevent input or close calls.
The current implementation uses one native thread per active operation and a one-millisecond poll interval.
Freeing a pending task abandons its result; it does not cancel the operation.
Close the session to interrupt its pending waits.
