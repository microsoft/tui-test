#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateSet('x64', 'ARM64')]
    [string] $Architecture = 'x64',
    [string] $NuGetSource = 'https://api.nuget.org/v3/index.json',
    [switch] $DeveloperEnvironment
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$native = Join-Path $root 'crates\tui-test\native\windows-terminal'
$build = Join-Path $root 'target\windows-terminal'
$source = Join-Path $build 'source'
$revision = (Get-Content (Join-Path $native 'upstream-revision.txt') -Raw).Trim()
$vcpkgRevision = '927f62e4b8838bd7e441e9c45103a16ffd75007e'
$vcpkg = Join-Path $build 'vcpkg'
$platform = $Architecture.ToLowerInvariant()
$triplet = "$platform-windows-static"
$output = Join-Path $build $platform
New-Item -ItemType Directory -Force $build, $output | Out-Null
$env:TEMP = Join-Path $build 'scratch'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null

if (!$DeveloperEnvironment) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (!(Test-Path $vswhere)) { throw 'Visual Studio Installer (vswhere.exe) is required.' }
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$vs) { throw 'Install Visual Studio 2022 or newer with the MSVC v143 C++ tools and Windows SDK 10.0.26100.0.' }
    $devcmd = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
    $env:PATH = "$(Split-Path $vswhere -Parent);$env:PATH"
    $pwsh = (Get-Process -Id $PID).Path
    # Keep toolchain setup and all build commands in the same cmd.exe process.
    $command = "call `"$devcmd`" -no_logo -arch=$platform -host_arch=x64 && `"$pwsh`" -NoLogo -NoProfile -File `"$PSCommandPath`" -Architecture $Architecture -NuGetSource `"$NuGetSource`" -DeveloperEnvironment"
    & $env:ComSpec /d /s /c $command
    exit $LASTEXITCODE
}

function Invoke-Checked {
    param([string] $Program, [string[]] $Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program exited with code $LASTEXITCODE" }
}

function Get-PinnedSource {
    param([string] $Path, [string] $Repository, [string] $Commit)
    if (!(Test-Path (Join-Path $Path '.git'))) {
        New-Item -ItemType Directory -Force $Path | Out-Null
        Invoke-Checked git @('-C', $Path, 'init', '--quiet')
        Invoke-Checked git @('-C', $Path, 'remote', 'add', 'origin', $Repository)
        Invoke-Checked git @('-C', $Path, '-c', 'core.longpaths=true', 'fetch', '--quiet', '--depth=1', 'origin', $Commit)
        Invoke-Checked git @('-C', $Path, '-c', 'core.longpaths=true', 'checkout', '--quiet', '--detach', $Commit)
    }
    $actual = (& git -C $Path rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -ne $Commit) { throw "$Path must be checked out at $Commit (found $actual)." }
    Invoke-Checked git @('-C', $Path, 'config', 'core.longpaths', 'true')
    $changes = & git -C $Path status --porcelain --untracked-files=no
    if ($LASTEXITCODE -ne 0 -or $changes) { throw "Refusing to build modified upstream sources at $Path." }
}

Push-Location $root
try {
    $receipt = Join-Path $output 'build-info.txt'
    if (Test-Path $receipt) { Remove-Item -LiteralPath $receipt }
    Get-PinnedSource $source 'https://github.com/microsoft/terminal.git' $revision
    Get-PinnedSource $vcpkg 'https://github.com/microsoft/vcpkg.git' $vcpkgRevision
    Invoke-Checked nuget @('restore', (Join-Path $native 'packages.config'), '-PackagesDirectory', (Join-Path $source 'packages'), '-Source', $NuGetSource, '-NonInteractive', '-Verbosity', 'quiet')
    if (!(Test-Path (Join-Path $vcpkg 'vcpkg.exe'))) {
        Invoke-Checked (Join-Path $vcpkg 'bootstrap-vcpkg.bat') @('-disableMetrics')
    }
    $env:VCPKG_DISABLE_METRICS = '1'
    $env:VCPKG_ROOT = $vcpkg
    $toolset = if ([version]$env:VisualStudioVersion -ge [version]'18.0') { 'v145' } else { 'v143' }
    $env:TUI_TEST_WINDOWS_TERMINAL_TOOLSET = $toolset
    $env:VCPKG_DOWNLOADS = Join-Path $build 'downloads'
    $env:VCPKG_DEFAULT_BINARY_CACHE = Join-Path $build 'binary-cache'
    New-Item -ItemType Directory -Force $env:VCPKG_DOWNLOADS, $env:VCPKG_DEFAULT_BINARY_CACHE | Out-Null
    $installed = Join-Path $source "obj\$Architecture\vcpkg"
    Invoke-Checked (Join-Path $vcpkg 'vcpkg.exe') @(
        'install', "--x-manifest-root=$source", "--x-install-root=$installed",
        "--triplet=$triplet", "--overlay-triplets=$(Join-Path $native 'triplets')",
        "--x-buildtrees-root=$(Join-Path $root 'target\wt-buildtrees')", '--disable-metrics'
    )
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $properties = @(
        '/p:Configuration=Release', "/p:Platform=$Architecture", "/p:PlatformToolset=$toolset",
        '/p:WindowsTargetPlatformVersion=10.0.26100.0', '/p:WholeProgramOptimization=false',
        "/p:SolutionDir=$source\", "/p:VcpkgRoot=$vcpkg\", "/p:TerminalSourceDir=$source",
        "/p:VcpkgManifestRoot=$source\", "/p:VcpkgInstalledDir=$installed\",
        '/p:VcpkgManifestInstall=false', '/p:BuildingInsideVisualStudio=false'
    )
    Invoke-Checked msbuild (@(
        (Join-Path $native 'headless-core.vcxproj'),
        '/t:Build', '/m', '/v:minimal', '/nologo', "/bl:$(Join-Path $build "core-$stamp.binlog")"
    ) + $properties)
    Invoke-Checked msbuild @(
        (Join-Path $native 'bridge.vcxproj'), '/t:Build', '/m', '/v:minimal', '/nologo',
        '/p:Configuration=Release', "/p:Platform=$Architecture",
        "/p:PlatformToolset=$toolset",
        "/p:TerminalSourceDir=$source", "/p:NativeOutputDir=$output", "/p:VcpkgTriplet=$triplet",
        "/bl:$(Join-Path $build "bridge-$stamp.binlog")"
    )
    Copy-Item (Join-Path $source 'LICENSE') (Join-Path $output 'Windows-Terminal-LICENSE')
    if (Test-Path (Join-Path $source 'NOTICE')) {
        Copy-Item (Join-Path $source 'NOTICE') (Join-Path $output 'Windows-Terminal-NOTICE')
    }
    $licenses = Join-Path $output 'licenses'
    New-Item -ItemType Directory -Force $licenses | Out-Null
    foreach ($package in 'fmt', 'ms-gsl') {
        Copy-Item (Join-Path $installed "$triplet\share\$package\copyright") (Join-Path $licenses "$package-LICENSE")
    }
    foreach ($package in 'Microsoft.Windows.CppWinRT.3.0.260818.1', 'Microsoft.Windows.ImplementationLibrary.1.0.250325.1') {
        Copy-Item (Join-Path $source "packages\$package\LICENSE") (Join-Path $licenses "$package-LICENSE")
    }
    $inputs = Join-Path $output 'inputs'
    New-Item -ItemType Directory -Force $inputs | Out-Null
    foreach ($input in 'bridge.h', 'bridge.cpp', 'bridge.vcxproj', 'headless-core.vcxproj', 'packages.config', 'upstream-revision.txt') {
        Copy-Item (Join-Path $native $input) (Join-Path $inputs $input)
    }
    @("upstream=$revision", "vcpkg=$vcpkgRevision", "architecture=$platform", "toolset=$toolset", 'abi=1', 'crt=static') |
        Set-Content $receipt -Encoding utf8NoBOM
    Write-Host "Built $output\tui_test_windows_terminal.lib"
    Write-Host "Use cargo build --features windows-terminal (or set TUI_TEST_WINDOWS_TERMINAL_LIB_DIR=$output)."
}
finally {
    Pop-Location
}
