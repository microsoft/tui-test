$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repository = "microsoft/tui-test"
$binaryName = "tui-test.exe"

if ($env:OS -ne "Windows_NT") {
    throw "install.ps1 only supports Windows. Use install.sh on macOS or Linux."
}

$processorArchitecture = $env:PROCESSOR_ARCHITEW6432
if ([string]::IsNullOrWhiteSpace($processorArchitecture)) {
    $processorArchitecture = $env:PROCESSOR_ARCHITECTURE
}

switch ($processorArchitecture.ToUpperInvariant()) {
    "AMD64" { $architecture = "x86_64" }
    "X86_64" { $architecture = "x86_64" }
    "ARM64" { $architecture = "aarch64" }
    "AARCH64" { $architecture = "aarch64" }
    default { throw "Unsupported Windows architecture: $processorArchitecture" }
}

$target = "$architecture-pc-windows-msvc"
$asset = "tui-test-$target.zip"
$version = $env:TUI_TEST_VERSION
$token = $env:GITHUB_TOKEN
if ([string]::IsNullOrWhiteSpace($token)) {
    $token = $env:GH_TOKEN
}

if ([string]::IsNullOrWhiteSpace($version) -or $version -eq "latest") {
    $releaseUrl = "https://github.com/$repository/releases/latest/download"
}
elseif ($version -eq "beta") {
    $headers = @{
        Accept = "application/vnd.github+json"
        "X-GitHub-Api-Version" = "2022-11-28"
    }

    $releases = Invoke-RestMethod `
        -Uri "https://api.github.com/repos/$repository/releases?per_page=100" `
        -Headers $headers `
        -UseBasicParsing
    $latestBeta = $releases |
        Where-Object {
            $_.prerelease -and
            -not $_.draft -and
            $_.tag_name -match "^[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$"
        } |
        Select-Object -First 1
    if ($null -eq $latestBeta -or [string]::IsNullOrWhiteSpace($latestBeta.tag_name)) {
        throw "No beta release was found."
    }

    $version = $latestBeta.tag_name
    $releaseUrl = "https://github.com/$repository/releases/download/$version"
}
else {
    $releaseUrl = "https://github.com/$repository/releases/download/$version"
}

$downloadUrl = "$releaseUrl/$asset"

$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("tui-test-" + [Guid]::NewGuid())
$archivePath = Join-Path $tempDir $asset
$extractDir = Join-Path $tempDir "extract"

New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

try {
    $request = @{
        Uri = $downloadUrl
        OutFile = $archivePath
        UseBasicParsing = $true
    }
    if (-not [string]::IsNullOrWhiteSpace($token)) {
        $request.Headers = @{ Authorization = "Bearer $token" }
    }

    Write-Host "Downloading tui-test for $target..."
    Invoke-WebRequest @request
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force

    $files = @(Get-ChildItem -LiteralPath $extractDir -File -Recurse)
    if ($files.Count -ne 1 -or $files[0].Name -ne $binaryName) {
        throw "Downloaded archive has unexpected contents."
    }

    $installDir = $env:TUI_TEST_INSTALL_DIR
    if ([string]::IsNullOrWhiteSpace($installDir)) {
        if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
            throw "LOCALAPPDATA is not set. Set TUI_TEST_INSTALL_DIR to a writable directory."
        }
        $installDir = Join-Path $env:LOCALAPPDATA "Programs\tui-test\bin"
    }

    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $destination = Join-Path $installDir $binaryName
    $stagedDestination = Join-Path $installDir ".$binaryName.tmp"

    Copy-Item -LiteralPath $files[0].FullName -Destination $stagedDestination -Force
    Unblock-File -LiteralPath $stagedDestination
    Move-Item -LiteralPath $stagedDestination -Destination $destination -Force

    Write-Host "Installed tui-test to $destination"

    function Test-PathEntry {
        param(
            [string]$PathValue,
            [string]$Entry
        )

        if ([string]::IsNullOrWhiteSpace($PathValue)) {
            return $false
        }

        $normalizedEntry = $Entry.TrimEnd("\")
        foreach ($pathEntry in $PathValue.Split(";")) {
            if ($pathEntry.Trim().TrimEnd("\") -ieq $normalizedEntry) {
                return $true
            }
        }
        return $false
    }

    function Publish-EnvironmentChange {
        if (-not ("TuiTestInstaller.NativeMethods" -as [Type])) {
            Add-Type -Namespace TuiTestInstaller -Name NativeMethods -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(
    IntPtr hWnd,
    uint Msg,
    UIntPtr wParam,
    string lParam,
    uint fuFlags,
    uint uTimeout,
    out UIntPtr lpdwResult);
'@
        }

        $result = [UIntPtr]::Zero
        [TuiTestInstaller.NativeMethods]::SendMessageTimeout(
            [IntPtr]0xffff,
            0x1a,
            [UIntPtr]::Zero,
            "Environment",
            2,
            5000,
            [ref]$result
        ) | Out-Null
    }

    function Get-UserPath {
        $environmentKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment")
        if ($null -eq $environmentKey) {
            return $null
        }

        try {
            return $environmentKey.GetValue(
                "Path",
                $null,
                [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames
            )
        }
        finally {
            $environmentKey.Dispose()
        }
    }

    function Set-UserPath {
        param([string]$Value)

        $environmentKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
        if ($null -eq $environmentKey) {
            throw "Could not open the current user's environment registry key."
        }

        try {
            $valueKind = [Microsoft.Win32.RegistryValueKind]::String
            if ($Value.Contains("%")) {
                $valueKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
            }
            elseif ($null -ne $environmentKey.GetValue("Path")) {
                $valueKind = $environmentKey.GetValueKind("Path")
            }

            $environmentKey.SetValue("Path", $Value, $valueKind)
        }
        finally {
            $environmentKey.Dispose()
        }

        Publish-EnvironmentChange
    }

    if (-not (Test-PathEntry -PathValue $env:Path -Entry $installDir)) {
        $env:Path = "$installDir;$env:Path"
    }

    $skipPathUpdate = $env:TUI_TEST_NO_MODIFY_PATH -match "^(1|true|yes)$"
    if (-not $skipPathUpdate) {
        $userPath = Get-UserPath
        if (-not (Test-PathEntry -PathValue $userPath -Entry $installDir)) {
            if ([string]::IsNullOrWhiteSpace($userPath)) {
                $newUserPath = $installDir
            }
            else {
                $newUserPath = "$installDir;$userPath"
            }
            Set-UserPath -Value $newUserPath
            Write-Host "Added $installDir to PATH. Restart your shell."
        }
    }
    elseif (-not (Test-PathEntry -PathValue (Get-UserPath) -Entry $installDir)) {
        Write-Host "Add $installDir to PATH."
    }
}
finally {
    Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
