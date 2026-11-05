# installed by mercury
# managed by mercury; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# MERCURY_INTEGRATION_ID=codex
# MERCURY_INTEGRATION_VERSION=9

param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:MERCURY_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:MERCURY_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    exit 0
}

if ($payload.hook_event_name -and $payload.hook_event_name -ne "SessionStart") { exit 0 }

$sessionId = $payload.session_id
if ([string]::IsNullOrWhiteSpace($sessionId)) { exit 0 }
if (-not [string]::IsNullOrWhiteSpace($env:CODEX_THREAD_ID) -and $env:CODEX_THREAD_ID -ne $sessionId) { exit 0 }

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$mercury = if ([string]::IsNullOrWhiteSpace($env:MERCURY_BIN_PATH)) { "mercury" } else { $env:MERCURY_BIN_PATH }
try {
    $args = @(
        "pane",
        "report-agent-session",
        $env:MERCURY_PANE_ID,
        "--source",
        "mercury:codex",
        "--agent",
        "codex",
        "--seq",
        "$seq",
        "--agent-session-id",
        "$sessionId"
    )
    if (-not [string]::IsNullOrWhiteSpace($payload.transcript_path)) {
        $args += @("--agent-session-path", "$($payload.transcript_path)")
    }
    if ($payload.hook_event_name -eq "SessionStart" -and $payload.source -is [string] -and -not [string]::IsNullOrWhiteSpace($payload.source)) {
        $args += @("--session-start-source", "$($payload.source)")
    }
    & $mercury @args 2>$null | Out-Null
} catch {
}
