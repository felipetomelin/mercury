# installed by mercury
# managed by mercury; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# MERCURY_INTEGRATION_ID=droid
# MERCURY_INTEGRATION_VERSION=3

param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:MERCURY_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:MERCURY_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    $payload = $null
}

if ($null -eq $payload -or [string]::IsNullOrWhiteSpace($payload.session_id)) { exit 0 }

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$mercury = if ([string]::IsNullOrWhiteSpace($env:MERCURY_BIN_PATH)) { "mercury" } else { $env:MERCURY_BIN_PATH }
try {
    & $mercury pane report-agent-session $env:MERCURY_PANE_ID --source mercury:droid --agent droid --agent-session-id $payload.session_id --seq $seq 2>$null | Out-Null
} catch {
}
