# Shared native library

`tui-test-ffi` exposes the Rust engine through the typed C ABI used by Go
(`purego`), Python (`ctypes`), and JavaScript (Koffi).
The library and language package must come from the same source revision.
This ABI is an internal contract, not a stable public C SDK.

From the repository root:

```sh
cargo build --locked -p tui-test-ffi
```

Set `TUI_TEST_NATIVE_LIBRARY` to the built shared library when using a binding
from a source checkout.

See the [native library reference](https://tui-test.dev/docs/reference/native-library/)
for platform-specific builds, generated declarations, ownership, and asynchronous calls.
