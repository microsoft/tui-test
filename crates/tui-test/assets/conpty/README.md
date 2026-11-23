# Bundled ConPTY

These are the Microsoft-signed binaries from
[`Microsoft.Windows.Console.ConPTY` 1.25.260303002-preview](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY/1.25.260303002-preview).
This is the 1.25.260303002 build used by
[node-pty](https://github.com/microsoft/node-pty/tree/c11e72b7986df6062f938f43fc238591c47ed18b/third_party/conpty/1.25.260303002).
The Windows file version is `1.25.2603.3002`. The package supports Windows 10
build 17763 and later and uses the MIT license in [LICENSE](LICENSE).

## Why we bundle it

Older ConPTY hosts render a snapshot of the console instead of forwarding output
in order. Shell markers can arrive before the text they describe. That can make
`wait command` return early or leave text out of the captured command output.
Microsoft replaced that renderer with synchronous console API-to-VT translation
in [microsoft/terminal#17510](https://github.com/microsoft/terminal/pull/17510),
which shipped in the 1.22 release.

Bundling a known host gives tui-test the same transport on supported Windows
versions. The shared Rust engine loads it before opening a PTY, so the CLI and
the native bindings use the same implementation. Normal CLI requests talk to the
daemon and do not extract or load these files.

## Files and loading

Each architecture has a gzip-compressed `conpty.dll` and `OpenConsole.exe`.
The DLL must match the process architecture; the host must match the native
Windows architecture. Following Microsoft's NuGet layout, x64 processes carry
both x64 and ARM64 hosts, and x86 processes carry all three hosts.
The engine also pins the native host as `OpenConsole.exe` beside `conpty.dll`,
because ConPTY prefers that location. It hard-links the verified host there
without storing another copy.

The cache is under `%LOCALAPPDATA%\tui-test\conpty\<version>-<architecture>`.
The engine extracts the selected files, checks their SHA-256 hashes, and loads
the DLL by absolute path. It keeps the files open for the lifetime of the
process. A corrupt cache or a conflicting ConPTY DLL is an error.

The bundle must be initialized before portable-pty opens its first PTY in the
process. Applications that also use portable-pty directly should call
`tui_test::terminal::conpty::initialize()` first. It cannot replace an
implementation that portable-pty has already cached.

## Updating

`manifest.json` records the package URL, version, package checksum, and checksums
of the original binaries. Compression does not change their signatures.

From the repository root, reproduce this bundle with:

```powershell
Push-Location .github\scripts
npm ci --ignore-scripts
Pop-Location
node .github\scripts\vendor-conpty.mjs --version 1.25.260303002-preview --sha256 7e477694e4095717c29725f7191ef13a5dda125e3a4d28a5fcc765a13dc9d967
```

For an update, review the new package and checksum, run the script with the new
values, update this README, and run the Windows ConPTY and shell-integration
tests. Keep the Microsoft license notice with the assets.
