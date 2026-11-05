//! `mercury statusline` — the Claude Code statusline bridge (D10 in
//! `docs/dev/usage-ui-design.md`).
//!
//! Installed as Claude Code's `statusLine` command by
//! `mercury integration install claude` (opt-in). On every statusline
//! refresh Claude Code pipes a JSON payload into it; the bridge:
//!
//! 1. forwards the payload to the Mercury server owning this pane
//!    (`usage.report_statusline`, pane from `MERCURY_PANE_ID`, socket from
//!    `MERCURY_SOCKET_PATH`) — best effort, short timeouts, never visible;
//! 2. runs the user's previous `statusLine` command (recorded in the chain
//!    file at install time) with the very same stdin, its stdout/stderr
//!    inherited so the output is byte-identical, and exits with its status.
//!
//! The bridge must never break or change the user's prompt: every failure
//! on the reporting side is swallowed, and without a previous command it
//! prints nothing (exactly what Claude Code shows without a statusLine).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::api::client::{ApiClient, ConnectionTarget};
use crate::api::schema::{Method, Request, UsageReportStatuslineParams};

/// Set for the chained command; a bridge that finds itself nested this deep
/// (a previous statusLine that is itself a bridge, possibly in a cycle) stops
/// chaining instead of recursing forever.
pub(crate) const STATUSLINE_DEPTH_ENV_VAR: &str = "MERCURY_STATUSLINE_DEPTH";
const MAX_CHAIN_DEPTH: u32 = 4;
/// Claude Code payloads are a few KB; anything this large is not one and is
/// not forwarded (the API also caps request lines at 1 MiB).
const MAX_REPORT_BYTES: usize = 512 * 1024;
/// Upper bound on stdin we buffer to replay into the chained command.
const MAX_STDIN_BYTES: u64 = 8 * 1024 * 1024;
const REPORT_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn run_statusline_command(args: &[String]) -> std::io::Result<i32> {
    let chain_file = match parse_args(args) {
        Ok(chain_file) => chain_file,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: mercury statusline [--chain-file PATH]");
            return Ok(2);
        }
    };
    let depth = std::env::var(STATUSLINE_DEPTH_ENV_VAR)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    if depth >= MAX_CHAIN_DEPTH {
        return Ok(0);
    }

    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_STDIN_BYTES)
        .read_to_end(&mut input)?;

    // Only the outermost bridge reports; a nested one would duplicate it.
    let reporter = (depth == 0)
        .then(|| report_target(&input))
        .flatten()
        .map(|(socket, request)| std::thread::spawn(move || send_report(&socket, &request)));

    let previous = chain_file.as_deref().and_then(|path| {
        crate::integration::statusline_previous_command(path)
            .ok()
            .flatten()
    });
    let code = match previous {
        Some(command) => run_previous(&command, &input, depth + 1),
        None => 0,
    };

    if let Some(reporter) = reporter {
        let _ = reporter.join();
    }
    Ok(code)
}

fn parse_args(args: &[String]) -> Result<Option<PathBuf>, String> {
    let mut chain_file = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--chain-file=") {
            chain_file = Some(PathBuf::from(value));
        } else if arg == "--chain-file" {
            let value = iter
                .next()
                .ok_or_else(|| "--chain-file needs a path".to_string())?;
            chain_file = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }
    Ok(chain_file)
}

/// `None` unless this runs inside a Mercury pane and stdin is a JSON object.
fn report_target(input: &[u8]) -> Option<(PathBuf, Request)> {
    let pane_id = std::env::var(crate::integration::MERCURY_PANE_ID_ENV_VAR)
        .ok()
        .filter(|value| !value.is_empty())?;
    let socket = std::env::var_os(crate::api::SOCKET_PATH_ENV_VAR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let request = build_report_request(pane_id, input)?;
    Some((socket, request))
}

fn build_report_request(pane_id: String, input: &[u8]) -> Option<Request> {
    if input.len() > MAX_REPORT_BYTES {
        return None;
    }
    let payload: serde_json::Value = serde_json::from_slice(input).ok()?;
    if !payload.is_object() {
        return None;
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    Some(Request {
        id: format!("mercury:statusline:{millis}"),
        method: Method::UsageReportStatusline(UsageReportStatuslineParams { pane_id, payload }),
    })
}

fn send_report(socket: &Path, request: &Request) {
    let client = ApiClient::for_target(ConnectionTarget::SocketPath(socket.to_path_buf()));
    // Best effort: an older server answers "unknown method", a stopped one
    // refuses the connection; neither may surface in the user's prompt.
    let _ = client.request_value_with_timeout(request, REPORT_TIMEOUT);
}

/// Runs the user's previous statusLine command the way Claude Code would
/// (through the shell), feeding it the same stdin. stdout/stderr are
/// inherited, so whatever it prints reaches Claude Code unchanged.
fn run_previous(command: &str, input: &[u8], depth: u32) -> i32 {
    let mut process = shell_command(command);
    process
        .env(STATUSLINE_DEPTH_ENV_VAR, depth.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(_) => return 1,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_vec();
        // Written from a thread so a command that prints before reading all
        // of stdin can never deadlock against us; a command that ignores
        // stdin just closes the pipe (the write error is irrelevant).
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    }
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> Command {
    let mut process = Command::new("/bin/sh");
    process.arg("-c").arg(command);
    process
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut process = Command::new(shell);
    process.arg("/C").arg(command);
    process
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chain_file_forms() {
        assert_eq!(parse_args(&[]).unwrap(), None);
        assert_eq!(
            parse_args(&["--chain-file".into(), "/x/y.json".into()]).unwrap(),
            Some(PathBuf::from("/x/y.json"))
        );
        assert_eq!(
            parse_args(&["--chain-file=/x/y.json".into()]).unwrap(),
            Some(PathBuf::from("/x/y.json"))
        );
        assert!(parse_args(&["--chain-file".into()]).is_err());
        assert!(parse_args(&["--bogus".into()]).is_err());
    }

    #[test]
    fn report_request_carries_the_payload_verbatim() {
        let input = br#"{"session_id":"abc","cost":{"total_cost_usd":1.5},"future":{"x":1}}"#;
        let request = build_report_request("p_1_2".into(), input).unwrap();
        let Method::UsageReportStatusline(params) = request.method else {
            panic!("wrong method");
        };
        assert_eq!(params.pane_id, "p_1_2");
        assert_eq!(
            params.payload,
            serde_json::from_slice::<serde_json::Value>(input).unwrap()
        );
        let wire = serde_json::to_value(Request {
            id: "x".into(),
            method: Method::UsageReportStatusline(params),
        })
        .unwrap();
        assert_eq!(wire["method"], "usage.report_statusline");
    }

    #[test]
    fn non_object_or_oversized_input_is_not_reported() {
        assert!(build_report_request("p".into(), b"not json").is_none());
        assert!(build_report_request("p".into(), b"[1,2]").is_none());
        let huge = format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_REPORT_BYTES));
        assert!(build_report_request("p".into(), huge.as_bytes()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn chained_command_gets_the_same_stdin_and_its_exit_code_is_kept() {
        let dir = std::env::temp_dir().join(format!(
            "mercury-statusline-chain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let captured = dir.join("stdin.json");
        let command = format!(
            "cat > {}; exit 3",
            crate::integration::shell_single_quote_for_tests(&captured.display().to_string())
        );
        let input = br#"{"session_id":"abc"}"#;
        assert_eq!(run_previous(&command, input, 1), 3);
        assert_eq!(std::fs::read(&captured).unwrap(), input);

        // The nested depth is visible to the chained command (recursion guard).
        let depth_file = dir.join("depth");
        let command = format!(
            "printf %s \"${STATUSLINE_DEPTH_ENV_VAR}\" > {}",
            crate::integration::shell_single_quote_for_tests(&depth_file.display().to_string())
        );
        assert_eq!(run_previous(&command, b"", 2), 0);
        assert_eq!(std::fs::read_to_string(&depth_file).unwrap(), "2");
        std::fs::remove_dir_all(&dir).ok();
    }
}
