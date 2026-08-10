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
}

function Global:__su_seq([string]$m) { return "$([char]0x1b)]$m$([char]0x07)" }

function Global:Prompt() {
    # Capture $? and $LASTEXITCODE before any other statement disturbs them.
    $ok = $global:?
    $lastExit = $global:LASTEXITCODE

    # -Off so a $null Get-History result doesn't throw under StrictMode.
    Set-StrictMode -Off
    $lastHistory = Get-History -Count 1

    $out = ""
    if ($Global:__su_state.LastHistoryId -ne -1 -and
        (-not $Global:__su_state.HasPSReadLine -or $Global:__su_state.IsInExecution)) {
        $Global:__su_state.IsInExecution = $false
        if ($null -ne $lastHistory -and $lastHistory.Id -ne $Global:__su_state.LastHistoryId) {
            $code = if ($ok) { 0 } elseif ($null -ne $lastExit -and $lastExit -ne 0) { $lastExit } else { 1 }
            $out += __su_seq "133;D;$code"
        }
        else {
            $out += __su_seq "133;D"
        }
    }

    $out += __su_seq "133;A"
    if ($pwd.Provider.Name -eq 'FileSystem') {
        $out += __su_seq "7;file://$([System.Environment]::MachineName)/$($pwd.ProviderPath -replace '\\', '/')"
    }
    # Markers bracket the prompt so the command region (after B) excludes "> ".
    $out += "> "
    $out += __su_seq "133;B"

    $Global:__su_state.LastHistoryId = $lastHistory.Id
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
        $Global:__su_state.IsInExecution = $true
        [Console]::Write((__su_seq "133;C"))
        $commandLine
    }
}