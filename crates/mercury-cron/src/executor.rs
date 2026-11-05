//! Declarative execution plans. The server runs these off-thread, enforces
//! timeout/output limits, and owns the pane API; this module never spawns a
//! shell or places a headless prompt in argv.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::{Job, JobTarget};

const MAX_PROMPT_FILE: u64 = 128 * 1024;
const MAX_PROMPT_TOTAL: usize = 512 * 1024;
const MAX_OUTPUT_STREAM: usize = 256 * 1024;

/// Checks the repository precondition of `codex exec` without bypassing its
/// safety check. Git handles ordinary repositories and linked worktrees.
pub fn codex_workdir_is_git_repo(workdir: Option<&Path>) -> bool {
    let path = match workdir {
        Some(path) => path.to_path_buf(),
        None => match std::env::current_dir() {
            Ok(path) => path,
            Err(_) => return false,
        },
    };
    if !path.is_dir() {
        return false;
    }
    StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout.trim_ascii() == b"true")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub workdir: Option<PathBuf>,
    pub timeout_seconds: u64,
    pub output_mode: CommandOutputMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutputMode {
    Script,
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionPlan {
    Command(CommandPlan),
    Pane {
        pane_id: String,
        prompt: String,
        timeout_seconds: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub text: String,
    pub truncated: bool,
    /// Provider-reported usage only; never inferred from text or process output size.
    pub observed_usage: Option<ObservedUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// None means the provider did not report a usable cost.
    pub cost_microusd: Option<u64>,
}

fn parse_usage(
    value: &serde_json::Value,
    cost: Option<&serde_json::Value>,
) -> Option<ObservedUsage> {
    const MAX_TOKENS: u64 = 10_000_000;
    const MAX_COST_MICROUSD: f64 = 1_000_000_000.0;
    let input_tokens = value.get("input_tokens")?.as_u64()?;
    let output_tokens = value.get("output_tokens")?.as_u64()?;
    if input_tokens > MAX_TOKENS || output_tokens > MAX_TOKENS {
        return None;
    }
    let cost_microusd = cost
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .and_then(|value| {
            let micros = value * 1_000_000.0;
            (micros <= MAX_COST_MICROUSD).then(|| micros.round() as u64)
        });
    Some(ObservedUsage {
        input_tokens,
        output_tokens,
        cost_microusd,
    })
}

async fn read_bounded(mut stream: impl AsyncRead + Unpin) -> Result<(String, bool), String> {
    let mut stored = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|_| "read process output failed")?;
        if count == 0 {
            break;
        }
        let keep = count.min(MAX_OUTPUT_STREAM.saturating_sub(stored.len()));
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
    Ok((String::from_utf8_lossy(&stored).into_owned(), truncated))
}

#[derive(Default)]
struct CodexCapture {
    last_message: Option<String>,
    completed: bool,
    failed: bool,
    malformed: bool,
    truncated: bool,
    observed_usage: Option<ObservedUsage>,
}

impl CodexCapture {
    fn accept_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) else {
            self.malformed = true;
            return;
        };
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("item.completed")
                if event
                    .pointer("/item/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("agent_message") =>
            {
                if let Some(text) = event
                    .pointer("/item/text")
                    .and_then(serde_json::Value::as_str)
                {
                    self.last_message = Some(text.to_owned());
                }
            }
            Some("turn.completed") => {
                self.completed = true;
                self.observed_usage = event
                    .get("usage")
                    .and_then(|usage| parse_usage(usage, None));
            }
            Some("turn.failed" | "error") => self.failed = true,
            _ => {}
        }
    }
}

async fn read_codex_events(mut stream: impl AsyncRead + Unpin) -> Result<CodexCapture, String> {
    let mut capture = CodexCapture::default();
    let mut line = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut skipping = false;
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|_| "read process output failed")?;
        if count == 0 {
            break;
        }
        for &byte in &buffer[..count] {
            if byte == b'\n' {
                if !skipping {
                    capture.accept_line(&line);
                }
                line.clear();
                skipping = false;
            } else if !skipping {
                if line.len() < MAX_OUTPUT_STREAM {
                    line.push(byte);
                } else {
                    line.clear();
                    skipping = true;
                    capture.truncated = true;
                }
            }
        }
    }
    if !skipping {
        capture.accept_line(&line);
    }
    Ok(capture)
}

fn claude_output(raw: &str, exit_success: bool, truncated: bool) -> CommandOutput {
    let parsed = serde_json::from_str::<serde_json::Value>(raw).ok();
    let result = parsed
        .as_ref()
        .and_then(|value| value.get("result"))
        .and_then(serde_json::Value::as_str);
    let is_error = parsed
        .as_ref()
        .and_then(|value| value.get("is_error"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let success = exit_success && !truncated && !is_error && result.is_some();
    CommandOutput {
        success,
        exit_code: None,
        text: if success {
            result.unwrap_or_default().to_owned()
        } else {
            "claude headless run failed or returned invalid output".into()
        },
        truncated,
        observed_usage: success
            .then(|| {
                parsed.as_ref().and_then(|value| {
                    value
                        .get("usage")
                        .and_then(|usage| parse_usage(usage, value.get("total_cost_usd")))
                })
            })
            .flatten(),
    }
}

fn codex_output(capture: CodexCapture, exit_success: bool) -> CommandOutput {
    let success = exit_success
        && capture.completed
        && !capture.failed
        && !capture.malformed
        && capture.last_message.is_some();
    CommandOutput {
        success,
        exit_code: None,
        text: if success {
            capture.last_message.unwrap_or_default()
        } else {
            "codex headless run failed or returned invalid output".into()
        },
        truncated: capture.truncated && !success,
        observed_usage: success.then_some(capture.observed_usage).flatten(),
    }
}

/// Runs one explicit binary; no shell interpolation or unbounded stdout
/// buffering. Callers persist the bounded result and classify delivery.
pub async fn run_command(plan: &CommandPlan) -> Result<CommandOutput, String> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(if plan.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(workdir) = &plan.workdir {
        if !workdir.is_dir() {
            return Err("workdir does not exist or is not a directory".into());
        }
        command.current_dir(workdir);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("start process: {error}"))?;
    let stdout = child.stdout.take().ok_or("process stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("process stderr unavailable")?;
    let output_mode = plan.output_mode;
    let stdout_reader = tokio::spawn(async move {
        match output_mode {
            CommandOutputMode::Codex => read_codex_events(stdout)
                .await
                .map(|capture| (String::new(), false, Some(capture))),
            _ => read_bounded(stdout)
                .await
                .map(|(text, truncated)| (text, truncated, None)),
        }
    });
    let stderr_reader = tokio::spawn(read_bounded(stderr));
    let execution = async {
        if let Some(input) = &plan.stdin {
            let mut stdin = child.stdin.take().ok_or("process stdin unavailable")?;
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|_| "write process stdin failed")?;
            stdin
                .shutdown()
                .await
                .map_err(|_| "close process stdin failed")?;
        }
        child
            .wait()
            .await
            .map_err(|error| format!("wait for process: {error}"))
    };
    let status = match timeout(Duration::from_secs(plan.timeout_seconds), execution).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            let _ = child.kill().await;
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill().await;
            return Err("process timeout".into());
        }
    };
    let (mut stdout, stdout_truncated, codex_capture) =
        stdout_reader.await.map_err(|_| "stdout reader failed")??;
    let (stderr, stderr_truncated) = stderr_reader.await.map_err(|_| "stderr reader failed")??;
    if output_mode == CommandOutputMode::Claude {
        let mut output = claude_output(&stdout, status.success(), stdout_truncated);
        output.exit_code = status.code();
        return Ok(output);
    }
    if let Some(capture) = codex_capture {
        let mut output = codex_output(capture, status.success());
        output.exit_code = status.code();
        return Ok(output);
    }
    if !stderr.is_empty() {
        if !stdout.is_empty() {
            stdout.push('\n');
        }
        stdout.push_str(&stderr);
    }
    Ok(CommandOutput {
        success: status.success(),
        exit_code: status.code(),
        text: stdout,
        truncated: stdout_truncated || stderr_truncated,
        observed_usage: None,
    })
}

pub fn plan(job: &Job) -> Result<ExecutionPlan, String> {
    job.validate()?;
    match &job.target {
        JobTarget::Script { program, args } => Ok(ExecutionPlan::Command(CommandPlan {
            program: program.clone(),
            args: args.clone(),
            stdin: None,
            workdir: job.workdir.clone(),
            timeout_seconds: job.timeout_seconds,
            output_mode: CommandOutputMode::Script,
        })),
        JobTarget::Pane { pane_id, prompt } => Ok(ExecutionPlan::Pane {
            pane_id: pane_id.clone(),
            prompt: prompt.clone(),
            timeout_seconds: job.timeout_seconds,
        }),
        JobTarget::Headless {
            provider,
            prompt,
            prompt_files,
            model,
            effort,
            skills,
        } => {
            let mut input = String::from(prompt.as_str());
            if !skills.is_empty() {
                input.push_str("\n\nRequested skills: ");
                input.push_str(&skills.join(", "));
            }
            for path in prompt_files {
                let mut content = String::new();
                File::open(path)
                    .map_err(|_| "cannot open prompt file")?
                    .take(MAX_PROMPT_FILE + 1)
                    .read_to_string(&mut content)
                    .map_err(|_| "cannot read UTF-8 prompt file")?;
                if content.len() as u64 > MAX_PROMPT_FILE
                    || input.len().saturating_add(content.len()) > MAX_PROMPT_TOTAL
                {
                    return Err("prompt files exceed size limit".into());
                }
                input.push_str("\n\n--- prompt file ---\n");
                input.push_str(&content);
            }
            if input.len() > MAX_PROMPT_TOTAL {
                return Err("headless prompt exceeds size limit".into());
            }
            if provider == "codex" && !codex_workdir_is_git_repo(job.workdir.as_deref()) {
                return Err("codex workdir is not a Git repository".into());
            }
            let (program, mut args) = match provider.as_str() {
                "claude" => (
                    "claude",
                    vec!["-p".into(), "--output-format".into(), "json".into()],
                ),
                "codex" => (
                    "codex",
                    vec!["exec".into(), "--json".into(), "--ephemeral".into()],
                ),
                _ => return Err("unsupported headless provider".into()),
            };
            if let Some(model) = model {
                args.extend(["--model".into(), model.clone()]);
            }
            if let Some(effort) = effort {
                if !matches!(effort.as_str(), "low" | "medium" | "high" | "xhigh" | "max") {
                    return Err("unsupported reasoning effort".into());
                }
                if provider == "claude" {
                    args.extend(["--effort".into(), effort.clone()]);
                } else {
                    args.extend([
                        "--config".into(),
                        format!("model_reasoning_effort={effort}"),
                    ]);
                }
            }
            if provider == "codex" {
                args.push("-".into());
            }
            Ok(ExecutionPlan::Command(CommandPlan {
                program: program.into(),
                args,
                stdin: Some(input),
                workdir: job.workdir.clone(),
                timeout_seconds: job.timeout_seconds,
                output_mode: if provider == "claude" {
                    CommandOutputMode::Claude
                } else {
                    CommandOutputMode::Codex
                },
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{OffsetDateTime, UtcOffset};

    fn job(target: JobTarget) -> Job {
        Job::new(
            "job".into(),
            "every 1h",
            UtcOffset::UTC,
            OffsetDateTime::now_utc(),
            target,
        )
        .unwrap()
    }

    #[test]
    fn codex_prompt_is_stdin_and_never_argv() {
        let job = job(JobTarget::Headless {
            provider: "codex".into(),
            prompt: "private task".into(),
            prompt_files: Vec::new(),
            model: Some("gpt-test".into()),
            effort: Some("high".into()),
            skills: Vec::new(),
        });
        let ExecutionPlan::Command(plan) = plan(&job).unwrap() else {
            panic!("expected command")
        };
        assert_eq!(plan.program, "codex");
        assert_eq!(plan.stdin.as_deref(), Some("private task"));
        assert_eq!(plan.output_mode, CommandOutputMode::Codex);
        assert!(!plan.args.join(" ").contains("private task"));
        assert_eq!(plan.args.last().unwrap(), "-");
    }

    #[test]
    fn codex_plan_requires_git_workdir_without_bypassing_cli_safety() {
        let root = std::env::temp_dir().join(format!("mercury-codex-git-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut job = job(JobTarget::Headless {
            provider: "codex".into(),
            prompt: "check".into(),
            prompt_files: Vec::new(),
            model: None,
            effort: None,
            skills: Vec::new(),
        });
        job.workdir = Some(root.clone());
        assert_eq!(
            plan(&job).unwrap_err(),
            "codex workdir is not a Git repository"
        );
        assert!(StdCommand::new("git")
            .arg("init")
            .arg(&root)
            .output()
            .unwrap()
            .status
            .success());
        let ExecutionPlan::Command(command) = plan(&job).unwrap() else {
            panic!("expected command");
        };
        assert!(!command
            .args
            .iter()
            .any(|arg| arg == "--skip-git-repo-check"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scripts_have_no_shell_or_stdin() {
        let job = job(JobTarget::Script {
            program: "echo".into(),
            args: vec!["hello".into()],
        });
        let ExecutionPlan::Command(plan) = plan(&job).unwrap() else {
            panic!("expected command")
        };
        assert_eq!(plan.program, "echo");
        assert_eq!(plan.args, vec!["hello"]);
        assert_eq!(plan.stdin, None);
        assert_eq!(plan.output_mode, CommandOutputMode::Script);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_executes_without_shell_and_captures_bounded_output() {
        let plan = CommandPlan {
            program: "/bin/echo".into(),
            args: vec!["hello".into()],
            stdin: None,
            workdir: None,
            timeout_seconds: 3,
            output_mode: CommandOutputMode::Script,
        };
        let result = run_command(&plan).await.unwrap();
        assert!(result.success);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.text.trim(), "hello");
        assert!(!result.truncated);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_timeout_stops_the_child() {
        let plan = CommandPlan {
            program: "/bin/sleep".into(),
            args: vec!["5".into()],
            stdin: None,
            workdir: None,
            timeout_seconds: 1,
            output_mode: CommandOutputMode::Script,
        };
        assert_eq!(run_command(&plan).await.unwrap_err(), "process timeout");
    }

    #[test]
    fn claude_json_extracts_only_result_and_rejects_errors() {
        let output = claude_output(
            r#"{"result":"final answer","session_id":"private-id","total_cost_usd":1.25,"is_error":false}"#,
            true,
            false,
        );
        assert!(output.success);
        assert_eq!(output.text, "final answer");
        let failed = claude_output(r#"{"result":"secret debug","is_error":true}"#, true, false);
        assert!(!failed.success);
        assert!(!failed.text.contains("secret"));
        assert!(!claude_output("not-json", true, false).success);
    }

    #[test]
    fn claude_usage_requires_complete_bounded_provider_counts() {
        let observed = claude_output(
            r#"{"result":"done","usage":{"input_tokens":120,"output_tokens":30},"total_cost_usd":0.000125}"#,
            true,
            false,
        );
        assert_eq!(
            observed.observed_usage,
            Some(ObservedUsage {
                input_tokens: 120,
                output_tokens: 30,
                cost_microusd: Some(125),
            })
        );
        let partial = claude_output(
            r#"{"result":"done","usage":{"input_tokens":120}}"#,
            true,
            false,
        );
        assert_eq!(partial.observed_usage, None);
        let oversized = claude_output(
            r#"{"result":"done","usage":{"input_tokens":10000001,"output_tokens":2}}"#,
            true,
            false,
        );
        assert_eq!(oversized.observed_usage, None);
        let failed = claude_output(
            r#"{"result":"done","is_error":true,"usage":{"input_tokens":1,"output_tokens":2}}"#,
            true,
            false,
        );
        assert_eq!(failed.observed_usage, None);
    }

    #[test]
    fn codex_usage_uses_completed_turn_only_and_rejects_failed_stream() {
        let mut capture = CodexCapture::default();
        capture.accept_line(
            br#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
        );
        capture.accept_line(
            br#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":20}}"#,
        );
        assert_eq!(
            codex_output(capture, true).observed_usage,
            Some(ObservedUsage {
                input_tokens: 100,
                output_tokens: 20,
                cost_microusd: None,
            })
        );
        let mut failed = CodexCapture::default();
        failed.accept_line(
            br#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
        );
        failed.accept_line(
            br#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":20}}"#,
        );
        failed.accept_line(b"malformed");
        assert_eq!(codex_output(failed, true).observed_usage, None);
    }

    #[tokio::test]
    async fn codex_jsonl_retains_final_message_after_large_intermediate_stream() {
        let (mut writer, reader) = tokio::io::duplex(8192);
        let producer = tokio::spawn(async move {
            writer
                .write_all(b"{\"type\":\"thread.started\"}\n")
                .await
                .unwrap();
            for _ in 0..5000 {
                writer
                    .write_all(b"{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"aggregated_output\":\"verbose\"}}\n")
                    .await
                    .unwrap();
            }
            writer
                .write_all(b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"final answer\"}}\n{\"type\":\"turn.completed\"}\n")
                .await
                .unwrap();
        });
        let capture = read_codex_events(reader).await.unwrap();
        producer.await.unwrap();
        let output = codex_output(capture, true);
        assert!(output.success);
        assert_eq!(output.text, "final answer");
        assert!(!output.truncated);
    }

    #[test]
    fn codex_jsonl_rejects_failed_or_missing_completion_without_leaking_events() {
        let mut capture = CodexCapture::default();
        capture.accept_line(
            br#"{"type":"item.completed","item":{"type":"agent_message","text":"secret text"}}"#,
        );
        capture.accept_line(br#"{"type":"turn.failed"}"#);
        let output = codex_output(capture, false);
        assert!(!output.success);
        assert!(!output.text.contains("secret"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_normalizes_claude_envelope() {
        let plan = CommandPlan {
            program: "/bin/echo".into(),
            args: vec![r#"{"result":"ready","session_id":"not-for-log"}"#.into()],
            stdin: None,
            workdir: None,
            timeout_seconds: 3,
            output_mode: CommandOutputMode::Claude,
        };
        let output = run_command(&plan).await.unwrap();
        assert!(output.success);
        assert_eq!(output.text, "ready");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_normalizes_codex_events() {
        let plan = CommandPlan {
            program: "/bin/echo".into(),
            args: vec![
                "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"ready\"}}\n{\"type\":\"turn.completed\"}".into(),
            ],
            stdin: None,
            workdir: None,
            timeout_seconds: 3,
            output_mode: CommandOutputMode::Codex,
        };
        let output = run_command(&plan).await.unwrap();
        assert!(output.success);
        assert_eq!(output.text, "ready");
    }
}
