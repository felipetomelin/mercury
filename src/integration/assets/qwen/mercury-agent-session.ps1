# installed by mercury
# managed by mercury; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# MERCURY_INTEGRATION_ID=qwen
# MERCURY_INTEGRATION_VERSION=1

param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:MERCURY_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:MERCURY_PANE_ID)) { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:MERCURY_SOCKET_PATH)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    $payload = $null
}

if ($null -eq $payload -or [string]::IsNullOrWhiteSpace($payload.session_id)) { exit 0 }

$seq = [DateTime]::UtcNow.Ticks
$mercury = if ([string]::IsNullOrWhiteSpace($env:MERCURY_BIN_PATH)) { "mercury" } else { $env:MERCURY_BIN_PATH }
$commandArgs = @(
    "pane", "report-agent-session", $env:MERCURY_PANE_ID,
    "--source", "mercury:qwen", "--agent", "qwen",
    "--agent-session-id", [string]$payload.session_id,
    "--seq", [string]$seq
)
if ($payload.source -in @("startup", "resume", "clear", "compact", "branch")) {
    $commandArgs += @("--session-start-source", [string]$payload.source)
}
try {
    & $mercury @commandArgs 2>$null | Out-Null
} catch {
}
