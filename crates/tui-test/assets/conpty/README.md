# Bundled ConPTY

These are the Microsoft-signed binaries from
[`Microsoft.Windows.Console.ConPTY` 1.22.250204002](https://dev.azure.com/shine-oss/terminal/_artifacts/feed/TerminalDependencies/NuGet/Microsoft.Windows.Console.ConPTY/overview/1.22.250204002).
Their Windows file version is `1.22.2502.4002`. The package supports Windows 10
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
python .github\scripts\vendor-conpty.py --version 1.22.250204002 --sha256 307c80b5fe6d4cf8d4ad300f88683725907c754f6fdb60652e311a141049b388
```

For an update, review the new package and checksum, run the script with the new
values, update this README, and run the Windows ConPTY and shell-integration
tests. Keep the Microsoft license notice with the assets.
