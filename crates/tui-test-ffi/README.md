# Shared native library

`tui-test-ffi` exposes the Rust engine through a typed C ABI.
Go uses `purego`, and Python uses `ctypes`, to load the default shared library.
The optional `napi` feature adds the JavaScript adapter to this crate.
That adapter constructs Rust `Operation` values and converts owned `OperationResult` values directly.
Both adapters share safe Rust execution, context validation, and close logic.
The JavaScript build produces one self-contained `.node` addon without C ABI calls or result pointers.
The default build has no N-API dependencies.

The library and language package must come from the same source revision.
This ABI is an internal contract, not a stable public C SDK.

## Build and use

From the repository root:

```sh
env -u CARGO_TARGET_DIR cargo build --locked -p tui-test-ffi
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

The build enables `--features napi` and writes the addon, loader, and declarations to `bindings/js/native/`.
The example prints the terminal size and text that contains `Ready 界🙂`.

## Generate boundaries

The owned Rust models define the shared fields. Do not edit generated boundary files.
The existing `tools/bindgen` tool reads these models with `syn`.
It generates boundary structs, fieldwise conversions, and all N-API exports before it runs `cbindgen`.
It rejects field types without a transport mapping.

| Authoritative source in `crates/tui-test/src/` | Coverage |
| --- | --- |
| `api.rs` | Options, styles, results, and the operation, result, key action, and mouse action enums |
| `locator_query.rs` | Locator nodes, expressions, and node kinds |
| `profile.rs` | Profile fields flattened into open/run options |
| `diagnostics.rs` | Artifact, trace, and retention options, including the shared session input |
| `shell/mod.rs`, `terminal/backend.rs` | Shell and backend enum variants |
| `terminal/cell.rs` | Underline variants for the JavaScript string enum |

The result models include `OpenResult`, `Cursor`, `Size`, `BellEvent`, `State`, `TerminalColors`, and `Cell`.
They also include `TextPosition`, `TextSpan`, `TextMatch`, `EffectiveTimeouts`, and `PackedScreen`.
The operation variants supply title, clipboard, snapshot, screenshot, and recording option fields.
The clipboard options select the regex and timeout fields from the title operation.

```sh
env -u CARGO_TARGET_DIR cargo run --locked -p tui-test-bindgen
env -u CARGO_TARGET_DIR cargo run --locked -p tui-test-bindgen -- --check
```

The generator writes these Rust files under `crates/tui-test-ffi/src/`:

- `types.rs` and `napi/types.rs`
- `input_generated.rs` and `output_generated.rs`
- `napi/convert_generated.rs`
- `napi/bindings_generated.rs`
- `session_generated.rs`

It then generates `include/tui_test.h`, the Go ABI structs and function table, and Python `ctypes` declarations.
The C function signatures in `src/lib.rs` remain authoritative for the C entrypoints.
The JavaScript adapter still calls owned Rust operations directly. It does not call the C ABI.

`--check` checks all these outputs without changing files.
Normal generation can replace missing or stale generated Rust files.
The generator does not compile the FFI crate to discover its models.
Keep `rustfmt` and `gofmt` on `PATH`.

Run `npm run build:native:debug` from `bindings/js` after generation.
The N-API CLI updates `native/index.d.ts` and the loader from the generated Rust exports.
CI separately checks these N-API outputs after the native build.
Release packages include generated Rust files. They do not need the generator during a package build.

### Transport mappings and handwritten code

`tools/bindgen/src/boundaries.rs` contains transport mappings, not another list of shared fields.
It maps C strings, pointer/length pairs, option tags, and fixed ABI field order.
It maps JavaScript numbers, nullable properties, property names, color unions, maps, and owned buffers.
It also generates strict locator input keys from the source fields.
The existing C ABI v3 layouts and JavaScript signatures remain unchanged.

`tools/bindgen/src/napi.rs` generates the `NativeSession` class, every method, and all global functions.
Its `METHODS` table maps JavaScript names to owned variants, result selectors, and trace names.
It derives parameter types, numeric conversions, and operation fields from `Operation` and `MouseAction`.
It derives result types and selection code from `OperationResult`.
Explicit mappings retain argument defaults, grouped options, fixed key actions, and the locator selection flag.
`MouseClickOptions` comes from `MouseAction::Click`; the generator also declares the `Color` transport alias.

Lifecycle signatures come from the actual functions in `napi/mod.rs`, not a second declaration list.
Generated wrappers call those functions for constructor state, context overrides, registry access, and close semantics.
The clipboard helper chooses a wait operation and validates its regex.
All regular operation methods construct owned Rust operations directly in generated code.
No N-API method calls the C ABI.

These parts remain handwritten:

- `src/type_helpers.rs`: C strings, option tags, map entries, color tags, and the result envelope.
- `src/napi/type_helpers.rs`: the strict-input macro and transport imports.
- `input.rs`, `output.rs`, and `napi/convert.rs`: pointer ownership, numeric validation, profile validation, and semantic adapters.
- `lib.rs` and `session.rs`: C entrypoints and shared execution.
- `napi/mod.rs`: worker execution, error envelopes, panic containment, lifecycle state, and clipboard overload semantics.
- The public Go, Python, and TypeScript convenience APIs.

The generator rejects N-API exports or derive imports outside `napi/types.rs` and `napi/bindings_generated.rs`.
New operation, action, or result variants require an explicit export decision.
This coverage check prevents new core variants from silently missing a boundary mapping.

The generator copies C transport helpers into `types.rs`; only `type_helpers.rs` is handwritten.
The result envelope is a transport container, not a mirror of the Rust result enum.
Mouse codes, snapshot codes, capture backgrounds, and locator occurrence rules retain explicit transport logic.
The core still has separate input and effective timeout models.
Generation does not merge different owned API concepts or remove handwritten language convenience APIs.

### Source propagation example

Run this example from the repository root:

```sh
env -u CARGO_TARGET_DIR cargo test --locked -p tui-test-bindgen \
  boundaries::tests::owned_field_propagates_to_both_declarations_and_conversions -- --exact
env -u CARGO_TARGET_DIR cargo test --locked -p tui-test-bindgen boundaries::napi::tests
env -u CARGO_TARGET_DIR cargo run --locked -p tui-test-bindgen -- --check
```

The test adds a cursor field to the source model in memory.
It verifies that both boundary declarations and both conversions contain the new field.
Other generator tests cover input numbers, strict locator keys, enum variants, missing outputs, and stale outputs.
The N-API tests change operation fields and result payloads in memory.
They verify generated signatures, numeric validation, dispatch, and mouse option declarations.
They also check lifecycle signature propagation, export coverage, trace names, defaults, and rejection of handwritten exports.
Then run the JavaScript terminal example above or the Go smoke example.
These examples exercise the generated result conversions with a real terminal.

## C ABI ownership

Input buffers must remain valid until a call returns. The engine copies inputs before it executes an operation.
A null string pointer means absent. A non-null pointer with zero length means an explicit empty string.

Every result belongs to this library until `tui_result_free` releases it.
Copy strings, arrays, and packed screen bytes before that call. Do not free individual fields.
The function called determines which result field contains the successful output.
Check `error_kind` before reading a successful result.

Keep the library loaded while any call or allocation remains active.
Increase `ABI_VERSION` when a change makes existing declarations incompatible.

## JavaScript ownership

The N-API adapter moves owned strings, arrays, and screen bytes from Rust results into JavaScript values.
It does not allocate or free C ABI results.
Blocking workers execute operations with a typed session context; they do not serialize that context for each call.
The adapter retains JavaScript numeric validation and the structured error envelope.
