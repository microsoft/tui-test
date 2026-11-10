# C SDK

The SDK provides the shared tui-test engine through its generated C ABI.
C and C++ applications use the engine in their own process. They do not need
the CLI daemon, Node.js, Python, Go, or Rust at runtime.

## Download

Select `tui-test-c-sdk-<version>-<target>.tar.gz` from the GitHub release.
Check the adjacent `.sha256` file before extraction.
The optional `tui-test-with-c-sdk-<version>-<target>.tar.gz` archive also
contains the CLI in `bin/`. Existing CLI and language packages do not change.

| Target | Runtime |
| --- | --- |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | glibc 2.28 or later |
| `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | musl Linux |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` | macOS on the matching CPU |
| `x86_64-pc-windows-msvc` | Windows x64 with the MSVC runtime |

These are **shared libraries**, not static libraries. Match the OS, CPU, and
C runtime of the application. A musl library does not work in a glibc process.
The archive manifest records the target, source revision, package version,
ABI version, and SHA-256 checksum of each file.

The Windows archive includes `bin/tui_test_ffi.dll` and its import library,
`lib/tui_test_ffi.lib`. The import library does not contain the engine.
The engine includes bundled ConPTY assets and extracts them to a user cache.
The `licenses/` directory contains the notices for bundled assets.

## Build and run the examples

Extract the archive into `sdk`, then run:

```sh
cmake -S sdk/examples -B build -DCMAKE_PREFIX_PATH="$PWD/sdk"
cmake --build build --config Release
ctest --test-dir build -C Release --verbose
```

In PowerShell, use `-DCMAKE_PREFIX_PATH="$PWD/sdk"` with the same commands.
CMake selects the installed C/C++ toolchain. Use an x64 MSVC toolchain for
the Windows SDK.

`basic.c` launches a copy of itself as a child. It waits for the child's text,
reads the terminal, and closes the session. It does not depend on a shell.
The expected output for ABI 7 is:

```text
ABI: 7
Captured: Hello from C SDK
Closed session.
```

`async.c` starts a wait task and polls it on the same thread.
It also prints `Wait completed asynchronously.`. `cpp.cpp` uses the same
example from C++ and checks the header's C linkage.

## Use CMake

```cmake
find_package(tui-test CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE tui-test::ffi)
```

Set `CMAKE_PREFIX_PATH` to the extracted SDK directory. The imported target
provides the include directory and library path. The SDK can move before
configuration; its CMake files contain no build-machine paths.
For a version constraint, request the exact stable package version.
For prereleases, select the archive explicitly and omit the CMake version constraint.

On Windows, copy the DLL beside your executable. The examples do this:

```cmake
add_custom_command(TARGET my_app POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different
    $<TARGET_FILE:tui-test::ffi> $<TARGET_FILE_DIR:my_app>)
```

On Linux and macOS, CMake supplies a build-tree runtime search path.
For application distribution, install the library with your application and
set an appropriate install RPATH. Use `$ORIGIN` on Linux or `@loader_path`
on macOS, relative to your application's layout. The macOS library uses
the install name `@rpath/libtui_test_ffi.dylib`.

Without CMake, a Linux example is:

```sh
cc -std=c11 -I sdk/include sdk/examples/basic.c \
  -L sdk/lib -ltui_test_ffi -o basic
LD_LIBRARY_PATH="$PWD/sdk/lib" ./basic
```

On macOS, use `-Wl,-rpath,"$PWD/sdk/lib"` at link time.
From an MSVC developer prompt on Windows:

```bat
cl /I sdk\include sdk\examples\basic.c sdk\lib\tui_test_ffi.lib /Fe:basic.exe
copy sdk\bin\tui_test_ffi.dll .
basic.exe
```

## ABI and ownership

- The `session` argument contains JSON, not a plain session name:
  `{"name":"example"}`. It can also carry the optional context fields below.
  Encode strings with a JSON encoder when they contain user input.
- Include `tui_test.h`. Do not copy its declarations into your application.
- Compare `tui_abi_version()` with `ABI_VERSION` before other API calls.
  Use the header and library from the same SDK. Do not mix ABI versions.
- Zero-initialize option structures. An optional field uses `present` to
  distinguish an explicit value from a default.
- `TuiString` contains UTF-8 bytes and a byte length. It is not necessarily
  NUL-terminated. A NULL data pointer denotes absence; a non-NULL empty
  buffer denotes an empty string.
- Keep input buffers readable for the call. Start functions copy their inputs
  before they return, so those buffers need not live until task completion.
- Check `TuiResult.error_kind`. Zero means success; nonzero values describe
  assertion, usage, missing-session, or internal errors.
- Output pointers belong to the result. Copy any data you need before
  `tui_result_free()`. Free each result exactly once, including error results.
- Start, poll, and free each `TuiTask` on the same thread.
  A NULL poll result means pending. A completed poll transfers result ownership
  once. Free both the completed result and the task.
- Freeing a pending task discards its eventual result; it does not cancel the
  operation. Use `tui_interrupt()` to cancel operations and terminate the child.
- Call `tui_close()` to release the session and child, including on error paths.
  Session names share a process-wide registry; use distinct names for independent
  sessions. The examples each run in a separate process.

The header documents each function and its safety requirements.
The SDK does not promise ABI compatibility across different ABI versions.
No static `.a` library or static MSVC library is included.

## Session context and locators

The session JSON requires a string `name`. These optional fields use the same
contract as the language adapters:

| Field | Value |
| --- | --- |
| `operation_name` | String for diagnostic reports |
| `diagnostic_context` | Object with string keys and values |
| `artifact_directory` | Directory string |
| `artifact_mode` | `none`, `text`, `html`, or `all` |
| `artifact_include_recording` | Boolean |
| `trace_mode` | `off`, `on-failure`, or `on` |
| `trace_directory` | Directory string |
| `screen_history_limit` | Integer from 0 through 50 |

`TuiQuery.json` contains a locator expression with `nodes` and a `root` index.
For example:

```json
{"nodes":[{"kind":"text","text":"Hello from C SDK","occurrence":"any"}],"root":0}
```

The examples use this expression with `tui_wait_locator()` and its asynchronous
form. The C boundary uses the Rust locator model; it does not parse a separate
selector language.
