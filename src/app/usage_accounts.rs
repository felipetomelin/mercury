//! Metadata-only CLI probes. No user message or model request is ever sent.
use mercury_usage::{Provider, ProviderLimit, ProviderLimits};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

const MAX_OUTPUT: u64 = 1024 * 1024;

pub(super) fn executable(name: &str) -> PathBuf {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let path = dir.join(name);
            if path.is_file() {
                return path;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".local/bin").join(name);
        if path.is_file() {
            return path;
        }
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let path = Path::new(dir).join(name);
        if path.is_file() {
            return path;
        }
    }
    name.into()
}

pub(super) fn private_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("MERCURY_")
            || ["CLAUDECODE", "CODEX_THREAD_ID", "CLAUDE_CODE_ENTRYPOINT"].contains(&name.as_ref())
        {
            command.env_remove(key);
        }
    }
    command
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

pub(super) fn finish(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn channel(child: &mut Child) -> io::Result<mpsc::Receiver<Value>> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("CLI stdout absent"))?;
    let (tx, rx) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout.take(MAX_OUTPUT)).lines() {
            let Ok(line) = line else { break };
            if let Ok(value) = serde_json::from_str(&line) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        }
    });
    Ok(rx)
}

fn ask(
    child: &mut Child,
    rx: &mpsc::Receiver<Value>,
    id: &str,
    request: Value,
) -> io::Result<Value> {
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("CLI stdin absent"))?;
    serde_json::to_writer(
        &mut *stdin,
        &json!({"type":"control_request", "request_id":id,"request":request}),
    )?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let value = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| {
                io::Error::new(
                    if matches!(e, mpsc::RecvTimeoutError::Timeout) {
                        io::ErrorKind::TimedOut
                    } else {
                        io::ErrorKind::UnexpectedEof
                    },
                    "Consulta Claude indisponível",
                )
            })?;
        let response = &value["response"];
        if value["type"] != "control_response" || response["request_id"] != id {
            continue;
        }
        if response["subtype"] != "success" {
            let error = response["error"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            return Err(io::Error::other(
                if error.contains("unsupported") || error.contains("unknown") {
                    "Versão do Claude sem get_usage"
                } else if error.contains("auth") || error.contains("login") {
                    "Conta sem login"
                } else {
                    "Consulta Claude falhou"
                },
            ));
        }
        return Ok(response["response"].clone());
    }
}

fn claude_command(binary: &Path) -> Command {
    let mut c = private_command(binary);
    c.env_remove("CLAUDECODE").env("DISABLE_AUTOUPDATER", "1");
    // Only the selected CLI profile supplies authentication. Inherited API keys
    // must not silently replace its subscription account.
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k.starts_with("ANTHROPIC_") || k.starts_with("CLAUDE_CODE_USE_") {
            c.env_remove(key);
        }
    }
    c
}

fn identity(binary: &Path) -> io::Result<String> {
    let mut child = claude_command(binary)
        .args(["auth", "status", "--json"])
        .spawn()?;
    let result = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("CLI stdout absent"))?;
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut data = String::new();
            let _ = stdout.take(MAX_OUTPUT).read_to_string(&mut data);
            let _ = tx.send(data);
        });
        let data = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Consulta Claude expirou"))?;
        let value: Value =
            serde_json::from_str(&data).map_err(|_| io::Error::other("Login Claude inválido"))?;
        if value["loggedIn"].as_bool() == Some(false) {
            return Err(io::Error::other("Conta sem login"));
        }
        let key = ["accountUuid", "accountId", "email", "orgId"]
            .iter()
            .filter_map(|k| value[*k].as_str())
            .collect::<Vec<_>>()
            .join(":");
        if key.is_empty() {
            return Err(io::Error::other("Identidade da conta indisponível"));
        }
        Ok(mercury_knowledge::hash(key.as_bytes()))
    })();
    finish(&mut child);
    result
}

pub(super) fn parse_claude(value: &Value, now: OffsetDateTime) -> io::Result<ProviderLimits> {
    if value["rate_limits_available"].as_bool().is_none() {
        return Err(io::Error::other(
            "Versão do Claude sem get_usage estruturado",
        ));
    }
    let mut limits = Vec::new();
    for (key, minutes) in [
        ("five_hour", 300),
        ("seven_day", 10080),
        ("seven_day_opus", 10080),
        ("seven_day_sonnet", 10080),
    ] {
        let w = &value["rate_limits"][key];
        let Some(pct) = w["utilization"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
        else {
            continue;
        };
        let reset = w["resets_at"].as_str().and_then(|v| {
            OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339).ok()
        });
        limits.push(ProviderLimit {
            key: key.into(),
            used_percent: pct,
            window_minutes: Some(minutes),
            resets_at: reset,
            used_usd: None,
            limit_usd: None,
            period: None,
        });
    }
    Ok(ProviderLimits {
        provider: Provider::Claude,
        limits,
        credits: None,
        plan_type: value["subscription_type"].as_str().map(str::to_owned),
        limit_reached_type: None,
        observed_at: Some(now),
    })
}

pub(super) fn read_claude() -> io::Result<(ProviderLimits, String)> {
    let binary = executable("claude");
    read_claude_using(&binary, cached_observation)
}

fn read_claude_using(
    binary: &Path,
    observation: impl FnOnce() -> Option<OffsetDateTime>,
) -> io::Result<(ProviderLimits, String)> {
    let before = identity(binary)?;
    let mut child = claude_command(binary)
        .args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--setting-sources",
            "",
            "--settings",
            "{\"disableAllHooks\":true}",
        ])
        .spawn()?;
    let result = (|| {
        let rx = channel(&mut child)?;
        ask(&mut child, &rx, "init", json!({"subtype":"initialize"}))?;
        let result = ask(
            &mut child,
            &rx,
            "usage",
            json!({"subtype":"get_usage","skip_behaviors":true}),
        )?;
        parse_claude(&result, OffsetDateTime::now_utc())
    })();
    finish(&mut child);
    let mut limits = result?;
    if let Some(observed) = observation() {
        if OffsetDateTime::now_utc() - observed > time::Duration::minutes(5) {
            return Err(io::Error::other(
                "Fonte Claude desatualizada; tente consultar novamente",
            ));
        }
        limits.observed_at = Some(observed);
    }
    if before != identity(binary)? {
        return Err(io::Error::other("Conta alterada durante consulta"));
    }
    Ok((limits, before))
}

// Claude's configuration metadata contains the fetch time, not authentication
// tokens. Never read .credentials.json, auth.json or Keychain entries.
fn cached_observation() -> Option<OffsetDateTime> {
    let path = if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        PathBuf::from(dir).join(".claude.json")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".claude.json")
    };
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return None;
    }
    let data = std::fs::File::open(path).ok()?;
    let value: Value = serde_json::from_reader(data.take(4 * 1024 * 1024)).ok()?;
    let ms = value["cachedUsageUtilization"]["fetchedAtMs"].as_i64()?;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enterprise_without_windows_is_connected_and_has_no_invented_quota() {
        let read=parse_claude(&json!({"subscription_type":"enterprise","rate_limits_available":true,"rate_limits":null}),OffsetDateTime::UNIX_EPOCH).unwrap();
        assert_eq!(read.plan_type.as_deref(), Some("enterprise"));
        assert!(read.limits.is_empty());
        assert!(parse_claude(&json!({"session":{}}), OffsetDateTime::UNIX_EPOCH).is_err());
    }
    #[test]
    fn claude_reported_windows_require_valid_percentages() {
        let read = parse_claude(
            &json!({"rate_limits_available":true,"rate_limits":{
                "five_hour":{"utilization":42.5,"resets_at":"2026-10-31T12:00:00Z"},
                "seven_day":{"utilization":-1},"seven_day_opus":{"utilization":101}
            }}),
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(read.limits.len(), 1);
        assert_eq!(read.limits[0].used_percent, 42.5);
        assert_eq!(read.limits[0].window_minutes, Some(300));
        assert!(read.limits[0].resets_at.is_some());
    }
    #[cfg(unix)]
    #[test]
    fn claude_probe_uses_only_control_requests_and_does_not_send_a_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "m009-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let binary = root.join("claude");
        std::fs::write(&binary,r##"#!/bin/sh
if [ "$1" = auth ]; then
  printf '%s\n' '{' '"loggedIn":true,' '"email":"fixture@example.invalid"' '}'
  exit 0
fi
read -r init
case "$init" in *'"subtype":"initialize"'*) ;; *) exit 12;; esac
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
read -r usage
case "$usage" in *'"subtype":"get_usage"'*) ;; *) exit 13;; esac
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"usage","response":{"subscription_type":"enterprise","rate_limits_available":true,"rate_limits":null}}}'
"##).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (limits, key) = read_claude_using(&binary, || None).unwrap();
        assert_eq!(limits.plan_type.as_deref(), Some("enterprise"));
        assert!(!key.contains("fixture"));
        let stale = read_claude_using(&binary, || Some(OffsetDateTime::UNIX_EPOCH));
        assert!(stale.unwrap_err().to_string().contains("desatualizada"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
