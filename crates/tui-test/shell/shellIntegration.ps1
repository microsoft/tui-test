if ((Test-Path variable:global:__su_state) -and $null -ne $Global:__su_state.OriginalPrompt) {
    return
}

if ($ExecutionContext.SessionState.LanguageMode -ne "FullLanguage") {
    return
}

$Global:__su_state = @{
    OriginalPrompt = $function:Prompt
    LastHistoryId  = -1
    IsInExecution  = $false
    HasPSReadLine  = $false
    LastError      = $null
    LastNativeExit = $null
}

function Global:__su_seq([string]$m) { return "$([char]0x1b)]$m$([char]0x07)" }

function Global:Prompt() {
    # Capture $? before invoking anything.
    $ok = $global:?

    # LASTEXITCODE may not exist yet; Get-History may return $null.
    Set-StrictMode -Off
    $lastExit = $global:LASTEXITCODE
    $lastError = $global:Error[0]
    $lastHistory = Get-History -Count 1

    $out = ""
    if ($Global:__su_state.LastHistoryId -ne -1 -and
        (-not $Global:__su_state.HasPSReadLine -or $Global:__su_state.IsInExecution)) {
        $Global:__su_state.IsInExecution = $false
        if ($null -ne $lastHistory -and $lastHistory.Id -ne $Global:__su_state.LastHistoryId) {
            # LASTEXITCODE survives cmdlets and throw. Only use it for a native
            # failure, not for a new PowerShell error from the current command.
            $errorException = if ($lastError -is [System.Management.Automation.ErrorRecord]) {
                $lastError.Exception
            } else {
                $lastError
            }
            $powerShellError = $null -ne $lastError -and
                -not [object]::ReferenceEquals($lastError, $Global:__su_state.LastError) -and
                $lastError.FullyQualifiedErrorId -ne 'NativeCommandError' -and
                ($null -eq $errorException -or $errorException.GetType().FullName -ne 'System.Management.Automation.NativeCommandExitException')
            # Native execution assigns a new boxed exit code, even for repeated
            # failures with the same value. Compare identity so Ignore errors
            # (which do not enter $Error) cannot borrow a previous command's code.
            # This also leaves the user's LASTEXITCODE intact.
            $ranNative = -not [object]::ReferenceEquals($lastExit, $Global:__su_state.LastNativeExit)
            $code = if ($ok) { 0 } elseif ($powerShellError) { 1 } elseif ($ranNative -and $null -ne $lastExit -and $lastExit -ne 0) { $lastExit } else { 1 }
            $out += __su_seq "133;D;$code"
        }
        else {
            $out += __su_seq "133;D"
        }
    }

    $out += __su_seq "133;A"
    if ($pwd.Provider.Name -eq 'FileSystem') {
        $path = $pwd.ProviderPath
        if ([System.IO.Path]::DirectorySeparatorChar -eq '\') {
            $path = $path.Replace('\', '/')
        }
        $path = [System.Uri]::EscapeDataString($path).Replace('%2F', '/').Replace('%3A', ':')
        if (-not $path.StartsWith('/')) { $path = '/' + $path }
        $out += __su_seq "7;file://$([System.Environment]::MachineName)$path"
    }
    # Markers bracket the prompt so the command region (after B) excludes "> ".
    $out += "> "
    $out += __su_seq "133;B"

    $Global:__su_state.LastHistoryId = $lastHistory.Id
    $Global:__su_state.LastError = $global:Error[0]
    $Global:__su_state.LastNativeExit = $lastExit
    return $out
}

if (-not (Get-Module -Name PSReadLine)) {
    try { Import-Module PSReadLine -ErrorAction SilentlyContinue } catch {}
}
if (Get-Module -Name PSReadLine) {
    $Global:__su_state.HasPSReadLine = $true
    $Global:__su_state.OriginalReadLine = $function:PSConsoleHostReadLine
    function Global:PSConsoleHostReadLine {
        $commandLine = $Global:__su_state.OriginalReadLine.Invoke()
        Set-StrictMode -Off
        $Global:__su_state.IsInExecution = $true
        $Global:__su_state.LastError = if ($global:Error.Count -gt 0) { $global:Error[0] } else { $null }
        $Global:__su_state.LastNativeExit = $global:LASTEXITCODE
        [Console]::Write((__su_seq "133;C"))
        $commandLine
    }
}