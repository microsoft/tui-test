# Windows Terminal headless backend (draft)

The opt-in `windows-terminal` feature uses Windows Terminal's actual
`Microsoft::Terminal::Core::Terminal`. Alacritty remains the default. The feature
and CLI value are unavailable on non-Windows targets; enabling all Cargo features
on Linux or macOS does not download, build, or link Windows code.

## Source build

Requirements:

- Windows 10 1903 or later; x64 or ARM64 MSVC target.
- PowerShell 7, Git, NuGet CLI, Visual Studio 2022 17.14 with **MSVC v143
  14.44**, or Visual Studio 2026 with **v145** C++ tools (including ARM64 tools for cross-compilation), and Windows SDK
  **10.0.26100.0**.
- Network access to GitHub and NuGet for the first native build. No Windows
  Terminal installation, display, application package, GUI process, or Windows
  Terminal SDK binary is needed.
- CLI/FFI builds also retain the existing Ghostty dependency, which requires
  **Zig 0.16.0** exactly. The core `windows-terminal` feature alone does not need Zig.

From the repository root:

```powershell
.\scripts\build-windows-terminal.ps1
cargo build --locked -p tui-test-cli -p tui-test-ffi --features windows-terminal
cargo test --locked -p tui-test-rs --features windows-terminal terminal::windows_terminal -- --test-threads=1
.\target\debug\tui-test.exe open --backend windows-terminal
```

For ARM64, pass `-Architecture ARM64`, then use Cargo's
`--target aarch64-pc-windows-msvc`. The native library always uses release `/MT`,
including for Cargo debug builds, matching this workspace's `+crt-static`.
To consume a separately built library directory, set
`TUI_TEST_WINDOWS_TERMINAL_LIB_DIR` to the script's output:
`target\windows-terminal\x64` or `target\windows-terminal\arm64`. Cargo verifies
the upstream revision, architecture, ABI, CRT, and copies of the compiled bridge
inputs before linking. It fails with instructions if the build is absent or stale;
there is no automatic download, binary substitution, or emulator fallback.

The script uses the exact commit in `upstream-revision.txt`:
**f3560938b2924bb6dc1bcaac8738b438ead10523** (MIT). It does not patch upstream
sources. C++/WinRT and WIL versions are pinned in `packages.config`; vcpkg is
pinned to **927f62e4b8838bd7e441e9c45103a16ffd75007e**, also the upstream
manifest's package baseline. Our triplets select static libraries, static CRT,
and the selected Visual Studio toolset (v143 or v145) consistently for dependencies
and core, rather than upstream's unconditional v145 triplets.

The source checkout, restored packages, downloaded build tools, static archives,
scratch files, and MSBuild binary logs stay under `target`. Our
`headless-core.vcxproj` compiles the five unchanged upstream core source files and
its IDL, importing upstream build settings and native library projects. It omits
the application, GPU atlas/shader build, and MIDI audio implementation. No render
engine is attached to the dummy renderer and no painting thread or window is
started. Only required static libraries are merged into the bridge archive.

If NuGet.org is inaccessible, Microsoft's public mirror can be selected explicitly:

```powershell
.\scripts\build-windows-terminal.ps1 -NuGetSource https://pkgs.dev.azure.com/dnceng/public/_packaging/dotnet-public/nuget/v3/index.json
```

No separate shim DLL or VC redistributable is required at runtime. The executable
uses Windows system libraries, including WinRT and the OS ICU libraries.
DirectWrite headers/libraries are build dependencies of the upstream renderer
base; no GUI render engine is used. Retain the script's copied Windows Terminal license and notice when
redistributing custom builds, together with the licenses of its dependencies.

## Adapter

All native state is created, called, and destroyed on one worker thread.
`process` incrementally decodes PTY UTF-8 to UTF-16 and calls `Terminal::Write`.
Cells, graphemes, trailing-wide cells, SGR attributes, underline styles/colors,
OSC 8 links, cursor, scrollback, viewport, and palette are read from native
buffers/settings. PTYs and shell integration still belong to tui-test.

Some required modes are not exposed as direct getters. The bridge calls
`OutputStateMachineEngine::Dispatch().RequestMode` and
`QueryKittyKeyboardProtocol` directly, capturing only those synchronous responses
in a private buffer. It never writes introspection escape sequences to the
parser and never adds introspection responses to PTY input; a partial incoming
escape remains intact.

The C boundary uses fixed-width fields and length-delimited UTF-16, checks
arguments, catches C++ exceptions, and frees every allocation in the library
that created it. Native write/resize failures are sticky `Emulator::fault`s.
Dimensions outside `1..=32767`, or rows-plus-scrollback greater than
32767, are errors rather than silently clamped settings.

Native scroll-position callbacks normalize a latent offset after history clears
and preserve a user-selected primary viewport across an alternate-screen visit.
They do not parse output sequences or construct a replacement history buffer.

Bell callbacks feed `BellTracker`. All host-action callbacks are local/no-op:
no OS clipboard access, audio, notifications, window actions, or shell launches
are performed. Upstream OSC 52 writes go only to a per-instance string. At this
pin, upstream discards clipboard queries and ignores the OSC 52 destination;
the adapter reports selection access as unavailable rather than inventing a
separate selection. These upstream limitations remain visible in conformance
tests; the implementation does not parse another emulator's output to fill gaps.

This is a draft integration. The full existing emulator conformance macro and
focused native ABI/state tests are enabled, not ignored. The explicit Windows CI
steps build the real source and run those tests; passing parser-only or
feature-disabled checks is not evidence that this backend works.

### Remaining acceptance blockers

The x64 source build and a real Windows PTY/input/resize/shell-integration test
work, but the complete native backend suite is **not green**: **77 passed,
14 failed, 0 ignored**. The following
differences remain exposed, not skipped or filled in with another parser:

| Area | Behavior at the pinned upstream revision |
| --- | --- |
| Color queries | `AdaptDispatch::_ReturnOscResponse` always terminates with ST, including replies to BEL-terminated queries. |
| Underline colors | `SGR 58:...` works and maps correctly; `58;...` is not implemented by upstream's non-subparameter SGR handler. |
| Clipboard | OSC 52 writes are local, but native queries are discarded and the destination is ignored; clipboard and selection cannot be separated. |
| Title stack | Native `WindowManipulation` does not implement `CSI 22 t` / `CSI 23 t`. |
| Height growth | `Terminal::UserResize` deliberately keeps existing scrollback rows in history rather than consuming them into a taller viewport. This also applies to a deferred main-buffer resize after alternate-screen use. |

Only the existing, independently verified `NoLegacyAlternateScreen` divergence
is declared: upstream ignores `?47` and `?1047`, while `?1049` works. No new
conformance escape hatches have been added. Resolving the blockers requires a
reviewed contract decision or upstream capability changes; this adapter does not
publish patches to Windows Terminal. Keep the integration a draft until the
native acceptance tests pass.

Local x64 validation also passed workspace all-feature Clippy (`-D warnings`),
default and `windows-terminal` CLI/FFI builds, the CLI backend-selection test,
13 bindgen tests, generated-boundary checks, formatting, and the targeted
JavaScript/Python/Go backend-validation tests. The feature-enabled Python
binding successfully drove a real Windows PTY through the built FFI DLL.
ARM64 and non-Windows execution have not been tested locally.

## Language bindings

JavaScript, Python, and Go accept the `windows-terminal` spelling, but official
native packages intentionally do not contain this feature. Build `tui-test-ffi`
with `--features windows-terminal` and point `TUI_TEST_NATIVE_LIBRARY` at the
resulting `target\debug\tui_test_ffi.dll` (or its release equivalent). A default
or non-Windows native library rejects the backend; the binding does not fall back
to another emulator.
