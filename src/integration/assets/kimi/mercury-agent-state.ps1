# installed by mercury
# managed by mercury; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# MERCURY_INTEGRATION_ID=kimi
# MERCURY_INTEGRATION_VERSION=7

param([string]$Action = "")

if (@("session", "working", "blocked", "idle") -notcontains $Action) { exit 0 }
if ($env:MERCURY_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:MERCURY_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    $payload = $null
}

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sessionId = if ($null -ne $payload -and -not [string]::IsNullOrWhiteSpace($payload.session_id)) { $payload.session_id } else { $null }
$mercury = if ([string]::IsNullOrWhiteSpace($env:MERCURY_BIN_PATH)) { "mercury" } else { $env:MERCURY_BIN_PATH }

try {
    if ($Action -eq "session") {
        if ([string]::IsNullOrWhiteSpace($sessionId)) { exit 0 }
        & $mercury pane report-agent-session $env:MERCURY_PANE_ID --source mercury:kimi --agent kimi --agent-session-id $sessionId --session-start-source startup --seq $seq 2>$null | Out-Null
    } else {
        if ([string]::IsNullOrWhiteSpace($sessionId)) {
            & $mercury pane report-agent $env:MERCURY_PANE_ID --source mercury:kimi --agent kimi --state $Action --seq $seq 2>$null | Out-Null
        } else {
            & $mercury pane report-agent $env:MERCURY_PANE_ID --source mercury:kimi --agent kimi --state $Action --agent-session-id $sessionId --seq $seq 2>$null | Out-Null
        }
    }
} catch {
}
