//! Short-lived, read-only Codex app-server client. Called only from a
//! background thread; every child is bounded by a deadline and reaped.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mercury_usage::ProviderLimits;
use serde_json::{json, Value};
use time::OffsetDateTime;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const ROUTE_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(2);
const ROUTE_READ_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Default)]
pub(crate) struct CodexLiveRead {
    pub(crate) limits: Option<ProviderLimits>,
    pub(crate) costs: Vec<(String, f64)>,
    pub(crate) identity: Option<String>,
}

fn send(stdin: &mut impl Write, message: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *stdin, message)?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn response_id(value: &Value) -> Option<u64> {
    value.get("id")?.as_u64()
}

fn receive_until(
    rx: &mpsc::Receiver<Value>,
    deadline: Instant,
    mut accept: impl FnMut(Value) -> bool,
) -> io::Result<()> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex app-server timed out",
            ));
        }
        let value = rx.recv_timeout(remaining).map_err(|err| match err {
            mpsc::RecvTimeoutError::Timeout => {
                io::Error::new(io::ErrorKind::TimedOut, "Codex app-server timed out")
            }
            mpsc::RecvTimeoutError::Disconnected => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Codex app-server closed stdout",
            ),
        })?;
        if accept(value) {
            return Ok(());
        }
    }
}

/// Reads account limits and estimated cumulative USD for the listed thread
/// ids. A missing `codex` executable, auth error, null cost or malformed
/// response never fabricates numbers.
pub(crate) fn read_snapshot(binary: &Path, thread_ids: &[String]) -> io::Result<CodexLiveRead> {
    read_snapshot_with_timeouts(binary, thread_ids, INITIALIZE_TIMEOUT, READ_TIMEOUT)
}

/// A route only needs account limits, not per-thread costs. Keep its latency
/// bounded independently of the background Usage poll.
pub(crate) fn read_route_limits(binary: &Path) -> io::Result<Option<ProviderLimits>> {
    read_snapshot_with_timeouts(binary, &[], ROUTE_INITIALIZE_TIMEOUT, ROUTE_READ_TIMEOUT)
        .map(|read| read.limits)
}

fn read_snapshot_with_timeouts(
    binary: &Path,
    thread_ids: &[String],
    initialize_timeout: Duration,
    read_timeout: Duration,
) -> io::Result<CodexLiveRead> {
    let mut child = super::usage_accounts::private_command(binary)
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let outcome = read_from_child(&mut child, thread_ids, initialize_timeout, read_timeout);
    super::usage_accounts::finish(&mut child);
    outcome
}

fn read_from_child(
    child: &mut std::process::Child,
    thread_ids: &[String],
    initialize_timeout: Duration,
    read_timeout: Duration,
) -> io::Result<CodexLiveRead> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no app-server stdout"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no app-server stdin"))?;
    let (tx, rx) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout.take(1024 * 1024)).lines() {
            let Ok(line) = line else { break };
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        }
    });

    send(
        &mut stdin,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {"clientInfo": {"name": "mercury", "version": env!("CARGO_PKG_VERSION")}}
        }),
    )?;
    let mut initialize_error = None;
    receive_until(&rx, Instant::now() + initialize_timeout, |value| {
        if response_id(&value) != Some(1) {
            return false;
        }
        if let Some(error) = value.get("error") {
            initialize_error = Some(error.to_string());
        }
        true
    })?;
    if let Some(error) = initialize_error {
        return Err(io::Error::other(format!(
            "Codex app-server initialize failed: {error}"
        )));
    }
    send(&mut stdin, &json!({"method":"initialized","params":{}}))?;
    send(
        &mut stdin,
        &json!({"id":2,"method":"account/rateLimits/read","params":{}}),
    )?;
    let mut pending: HashMap<u64, String> = HashMap::new();
    for (index, thread_id) in thread_ids.iter().enumerate() {
        let id = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(3))
            .ok_or_else(|| io::Error::other("too many Codex threads"))?;
        pending.insert(id, thread_id.clone());
        send(
            &mut stdin,
            &json!({"id":id,"method":"account/usage/read","params":{"threadId":thread_id}}),
        )?;
    }
    let mut read = CodexLiveRead::default();
    let mut rate_pending = true;
    let deadline = Instant::now() + read_timeout;
    let mut rate_error = None;
    receive_until(&rx, deadline, |value| {
        let Some(id) = response_id(&value) else {
            return false;
        };
        if id == 2 {
            rate_pending = false;
            if let Some(error) = value.get("error") {
                rate_error = Some(error.to_string());
            }
            if let Some(result) = value.get("result") {
                read.identity = result["accountId"]
                    .as_str()
                    .map(|v| mercury_knowledge::hash(v.as_bytes()));
                read.limits = mercury_usage::codex_app_server::account_limits(
                    result,
                    OffsetDateTime::now_utc(),
                );
            }
        } else if let Some(thread_id) = pending.remove(&id) {
            if let Some(cost) = value.get("result").and_then(|result| {
                mercury_usage::codex_app_server::thread_total_usd(result, &thread_id)
            }) {
                read.costs.push((thread_id, cost));
            }
        }
        !rate_pending && pending.is_empty()
    })?;
    if let Some(error) = rate_error {
        return Err(io::Error::other(format!(
            "Codex rate-limits read failed: {error}"
        )));
    }
    Ok(read)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn stdio_round_trip_matches_results_by_request_id() {
        let path = std::env::temp_dir().join(format!(
            "mercury-fake-codex-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let script = "#!/bin/sh\nread init\nprintf '%s\\n' '{\"id\":1,\"result\":{}}'\nread initialized\nread limits\nread usage\nprintf '%s\\n' '{\"id\":3,\"result\":{\"threadUsage\":{\"threadId\":\"thread-1\",\"estimatedUsageUsdMicros\":42000}}}'\nprintf '%s\\n' '{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":null,\"secondary\":null,\"credits\":{\"hasCredits\":true,\"unlimited\":false},\"planType\":\"business\"}}}'\n";
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = read_snapshot(&path, &["thread-1".into()]).unwrap();
        assert_eq!(result.costs, vec![("thread-1".into(), 0.042)]);
        assert_eq!(
            result.limits.unwrap().plan_type.as_deref(),
            Some("business")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn initialize_timeout_reaps_child() {
        let path = std::env::temp_dir().join(format!(
            "mercury-fake-codex-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "#!/bin/sh\nread init\nwhile :; do :; done\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let error = read_snapshot_with_timeouts(
            &path,
            &[],
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_file(path).unwrap();
    }
}
