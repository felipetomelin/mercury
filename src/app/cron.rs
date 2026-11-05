//! Server-owned cron orchestration. The scheduler and ledger stay on the App
//! thread; only a bounded process runner is sent to Tokio background tasks.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use crate::api::client::{ApiClient, ConnectionTarget};
use crate::api::schema::{
    AgentPromptParams, AgentPromptWaitOptions, AgentReadParams, AgentTarget, Method, ReadFormat,
    ReadSource, Request,
};
#[cfg(test)]
use mercury_cron::executor::ObservedUsage;
use mercury_cron::executor::{self, CommandOutput, ExecutionPlan};
use mercury_cron::scheduler::{ReservedRun, Scheduler};
use mercury_cron::storage::{PendingLearnUsage, SqliteLedger};
use mercury_cron::{Delivery, JobTarget};
use mercury_learn::metrics::{MetricPhase, TaskObservation, TaskOutcome};
use mercury_learn::storage::Storage as LearnStorage;
use mercury_learn::transcript::Provider;
use time::OffsetDateTime;
use tracing::{info, warn};

use super::App;
use crate::events::AppEvent;

pub(crate) struct CronService {
    pub(crate) scheduler: Scheduler,
    pub(crate) output_dir: PathBuf,
    ledger_path: PathBuf,
}

static LEARN_USAGE_SYNC: OnceLock<Mutex<()>> = OnceLock::new();
pub(super) const LEARN_USAGE_RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);

struct DeliveryMessage {
    job_id: String,
    run_id: String,
    title: String,
    body: String,
    destinations: Vec<Delivery>,
}

impl CronService {
    fn open(root: &Path, now: OffsetDateTime) -> Result<Self, String> {
        let scheduler = Scheduler::open(
            &root.join("jobs.toml"),
            &root.join("executions.db"),
            &root.join("scheduler.lock"),
            now,
        )?;
        let output_dir = root.join("output");
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("create cron output directory: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output_dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("restrict cron output directory: {error}"))?;
        }
        Ok(Self {
            scheduler,
            output_dir,
            ledger_path: root.join("executions.db"),
        })
    }

    fn persist_output(&self, run: &ReservedRun, body: &str) -> Result<PathBuf, String> {
        let path = self.output_dir.join(format!("{}.log", run.id));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("open cron output: {error}"))?;
        file.write_all(body.as_bytes())
            .map_err(|error| format!("write cron output: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync cron output: {error}"))?;
        Ok(path)
    }

    fn finish(
        &mut self,
        run: &ReservedRun,
        result: Result<CommandOutput, String>,
        now: OffsetDateTime,
        learn_enabled: bool,
        learn_target_id: Option<&str>,
    ) -> Result<Option<DeliveryMessage>, String> {
        let learn_usage = if learn_enabled {
            match &run.job.target {
                JobTarget::Headless { provider, .. } => result
                    .as_ref()
                    .ok()
                    .and_then(|output| output.observed_usage)
                    .map(|usage| (provider.as_str(), usage, learn_target_id)),
                _ => None,
            }
        } else {
            None
        };
        let (success, body, error_class) = match result {
            Ok(output) => {
                let marker_failure = output.text.contains("[CRON_FAILURE]");
                let error_class = if marker_failure {
                    Some("cron_failure")
                } else if !output.success {
                    Some("nonzero_exit")
                } else {
                    None
                };
                let mut body = output.text;
                if output.truncated {
                    body.push_str("\n[output truncated]\n");
                }
                (output.success && !marker_failure, body, error_class)
            }
            Err(error) => {
                let class = if error == "process timeout" {
                    "process_timeout"
                } else {
                    "process_error"
                };
                (false, error, Some(class))
            }
        };
        let body = redact_configured_secrets(&body, &run.job.deliver);
        let path = match self.persist_output(run, &body) {
            Ok(path) => path,
            Err(error) => {
                self.scheduler
                    .finish(run, now, false, None, Some("output_write_failed"), None)?;
                return Err(error);
            }
        };
        let incident =
            self.scheduler
                .finish(run, now, success, Some(&path), error_class, learn_usage)?;
        let should_notify = if let Some(incident) = incident {
            if incident.should_notify {
                warn!(job_id = %run.job.id, incident_id = %incident.incident.id,
                    fingerprint = %incident.incident.fingerprint, "cron job failed");
            }
            incident.should_notify
        } else {
            info!(job_id = %run.job.id, run_id = %run.id, "cron job succeeded");
            true
        };
        if !should_notify || body.contains("[SILENT]") {
            return Ok(None);
        }
        Ok(Some(DeliveryMessage {
            job_id: run.job.id.clone(),
            run_id: run.id.clone(),
            title: format!("Mercury cron: {}", run.job.name),
            body: format!(
                "Run {}: {}\n{}",
                run.id,
                if success { "success" } else { "failure" },
                body
            ),
            destinations: run.job.deliver.clone(),
        }))
    }
}

fn redact_configured_secrets(body: &str, destinations: &[Delivery]) -> String {
    let mut result = body.to_owned();
    for destination in destinations {
        let names: Vec<&str> = match destination {
            Delivery::Teams { webhook_env } => vec![webhook_env],
            _ => Vec::new(),
        };
        for name in names {
            if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
                result = result.replace(&value.to_string_lossy().to_string(), "[REDACTED]");
            }
        }
    }
    result
}

fn default_cron_root() -> Result<PathBuf, String> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or("cannot locate user data directory for cron")?;
    Ok(data_home.join("mercury").join("cron"))
}

fn record_headless_observation(
    root: &Path,
    observation: &PendingLearnUsage,
) -> Result<(), mercury_learn::storage::StorageError> {
    let provider = match observation.provider.as_str() {
        "claude" => Provider::Claude,
        "codex" => Provider::Codex,
        _ => return Err(mercury_learn::storage::StorageError::Corrupt),
    };
    let mut storage = LearnStorage::open(root)?;
    storage.record_task_observation(TaskObservation {
        observation_id: &observation.run_id,
        task_key: &match &observation.learn_target_id {
            Some(target_id) => format!("cron:{}:target:{target_id}", observation.job_id),
            None => format!("cron:{}", observation.job_id),
        },
        provider,
        phase: MetricPhase::Observed,
        input_tokens: observation.usage.input_tokens,
        output_tokens: observation.usage.output_tokens,
        cost_microusd: observation.usage.cost_microusd,
        outcome: Some(if observation.succeeded {
            TaskOutcome::Succeeded
        } else {
            TaskOutcome::Failed
        }),
        observed_at_ms: observation.observed_at_ms,
    })?;
    Ok(())
}

fn reconcile_learn_usage(ledger_path: &Path, learn_root: &Path) -> Result<usize, String> {
    let ledger = SqliteLedger::open(ledger_path)?;
    let mut delivered = 0;
    loop {
        let pending = ledger.pending_learn_usage(64)?;
        if pending.is_empty() {
            return Ok(delivered);
        }
        for observation in pending {
            record_headless_observation(learn_root, &observation)
                .map_err(|_| "Learn metric ledger is unavailable".to_owned())?;
            ledger.ack_learn_usage(&observation.run_id)?;
            delivered += 1;
        }
    }
}

fn pane_request(
    client: &ApiClient,
    method: Method,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let response = client
        .request_value_with_timeout(
            &Request {
                id: "cron:pane".into(),
                method,
            },
            timeout,
        )
        .map_err(|_| "pane API request failed".to_owned())?;
    if let Some(code) = response["error"]["code"].as_str() {
        return Err(format!("pane API rejected request: {code}"));
    }
    Ok(response)
}

fn run_pane(
    socket_path: PathBuf,
    pane_id: String,
    prompt: String,
    workdir: Option<PathBuf>,
    timeout_seconds: u64,
) -> Result<CommandOutput, String> {
    let client = ApiClient::for_target(ConnectionTarget::SocketPath(socket_path));
    let timeout = Duration::from_secs(timeout_seconds.saturating_add(5));
    if let Some(expected) = workdir {
        let agent = pane_request(
            &client,
            Method::AgentGet(AgentTarget {
                target: pane_id.clone(),
            }),
            Duration::from_secs(5),
        )?;
        let actual = agent["result"]["agent"]["foreground_cwd"]
            .as_str()
            .or_else(|| agent["result"]["agent"]["cwd"].as_str())
            .ok_or("pane workdir is unknown")?;
        let expected = fs::canonicalize(expected).map_err(|_| "pane workdir does not exist")?;
        let actual = fs::canonicalize(actual).map_err(|_| "pane workdir does not exist")?;
        if expected != actual {
            return Err("pane workdir differs from job workdir".into());
        }
    }
    let response = pane_request(
        &client,
        Method::AgentPrompt(AgentPromptParams {
            target: pane_id.clone(),
            text: prompt,
            wait: Some(AgentPromptWaitOptions {
                until: Vec::new(),
                timeout_ms: Some(timeout_seconds.saturating_mul(1000)),
                submission_deadline: None,
            }),
        }),
        timeout,
    )?;
    if response["result"]["type"] != "agent_prompted" {
        return Err("pane prompt returned an unexpected result".into());
    }
    let read = pane_request(
        &client,
        Method::AgentRead(AgentReadParams {
            target: pane_id,
            source: ReadSource::RecentUnwrapped,
            lines: Some(120),
            format: ReadFormat::Text,
            strip_ansi: true,
        }),
        Duration::from_secs(5),
    )?;
    let text = read["result"]["read"]["text"]
        .as_str()
        .ok_or("pane read returned no text")?;
    const MAX_PANE_OUTPUT: usize = 256 * 1024;
    let mut bytes = text.len().min(MAX_PANE_OUTPUT);
    while !text.is_char_boundary(bytes) {
        bytes -= 1;
    }
    Ok(CommandOutput {
        success: true,
        exit_code: None,
        text: text[..bytes].to_owned(),
        truncated: bytes < text.len(),
        observed_usage: None,
    })
}

fn truncate_chars(text: &str, max_bytes: usize) -> &str {
    let mut length = text.len().min(max_bytes);
    while !text.is_char_boundary(length) {
        length -= 1;
    }
    &text[..length]
}

pub(crate) fn validate_teams_url(
    raw: &str,
    allow_local_http: bool,
) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "invalid Teams webhook URL")?;
    let local_http = allow_local_http
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"));
    if (url.scheme() != "https" && !local_http)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Teams webhook URL must use HTTPS without embedded credentials".into());
    }
    Ok(url)
}

fn send_teams_with_client(
    client: &reqwest::blocking::Client,
    url: &str,
    title: &str,
    body: &str,
    allow_local_http: bool,
) -> Result<(), String> {
    let url = validate_teams_url(url, allow_local_http)?;
    let text = format!("{title}\n{}", truncate_chars(body, 4 * 1024));
    let payload = serde_json::to_vec(&serde_json::json!({ "text": text }))
        .map_err(|_| "cannot encode Teams payload")?;
    if payload.len() > 28 * 1024 {
        return Err("Teams payload exceeds size limit".into());
    }
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .body(payload)
        .send()
        .map_err(|_| "Teams webhook request failed")?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "Teams webhook returned HTTP {}",
            response.status().as_u16()
        ))
    }
}

fn required_secret_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("destination environment variable {name} is unset or invalid"))
}

fn deliver_one(
    socket_path: &Path,
    destination: &Delivery,
    title: &str,
    body: &str,
) -> Result<(), String> {
    match destination {
        Delivery::Local { notify: false } => Ok(()),
        Delivery::Local { notify: true } => {
            let body = truncate_chars(body, 200);
            match crate::platform::show_desktop_notification(title, Some(body)) {
                Ok(true) => Ok(()),
                Ok(false) => Err("desktop notification is unavailable".into()),
                Err(_) => Err("desktop notification failed".into()),
            }
        }
        Delivery::Pane { pane_id } => {
            let client =
                ApiClient::for_target(ConnectionTarget::SocketPath(socket_path.to_path_buf()));
            let response = pane_request(
                &client,
                Method::AgentPrompt(AgentPromptParams {
                    target: pane_id.clone(),
                    text: truncate_chars(body, 16 * 1024).to_owned(),
                    wait: None,
                }),
                Duration::from_secs(10),
            )?;
            if response["result"]["type"] == "agent_prompted" {
                Ok(())
            } else {
                Err("pane delivery returned an unexpected result".into())
            }
        }
        Delivery::Teams { webhook_env } => {
            let url = required_secret_env(webhook_env)?;
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "cannot initialize Teams HTTP client")?;
            send_teams_with_client(&client, &url, title, body, false)
        }
    }
}

fn deliver_message(socket_path: PathBuf, message: DeliveryMessage) {
    for destination in &message.destinations {
        if let Err(error) = deliver_one(&socket_path, destination, &message.title, &message.body) {
            warn!(job_id = %message.job_id, run_id = %message.run_id, destination = ?destination_kind(destination), %error, "cron delivery failed");
        }
    }
}

fn destination_kind(destination: &Delivery) -> &'static str {
    match destination {
        Delivery::Local { .. } => "local",
        Delivery::Pane { .. } => "pane",
        Delivery::Teams { .. } => "teams",
    }
}

impl App {
    pub(crate) fn enable_cron(&mut self) -> Result<(), String> {
        self.enable_cron_at(&default_cron_root()?)
    }

    pub(crate) fn enable_cron_at(&mut self, root: &Path) -> Result<(), String> {
        let now = OffsetDateTime::now_utc();
        let mut cron = CronService::open(root, now)?;
        let pending = cron.scheduler.recover_on_startup(now)?;
        self.cron = Some(cron);
        if self.learn_config.enabled {
            self.start_cron_learn_usage_sync();
        }
        self.cron_retry_at = None;
        self.cron_learn_usage_retry_at = self
            .learn_config
            .enabled
            .then(|| Instant::now() + LEARN_USAGE_RETRY_INTERVAL);
        for run in pending {
            self.dispatch_cron(run);
        }
        Ok(())
    }

    pub(crate) fn start_cron_learn_usage_sync(&self) {
        // Test/embedded apps must never reconcile against the user's global data.
        if !self.policy.background_updates {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Some(cron) = self.cron.as_ref() else {
            return;
        };
        let ledger_path = cron.ledger_path.clone();
        let learn_root = crate::session::data_dir().join("learn");
        runtime.spawn_blocking(move || {
            let lock = LEARN_USAGE_SYNC.get_or_init(|| Mutex::new(()));
            let _guard = match lock.try_lock() {
                Ok(guard) => guard,
                Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
                Err(TryLockError::WouldBlock) => return,
            };
            if reconcile_learn_usage(&ledger_path, &learn_root).is_err() {
                warn!("failed to reconcile pending Learn cron usage");
            }
        });
    }

    fn cron_learn_usage_retry_due(&mut self, now: Instant) -> bool {
        if !self.learn_config.enabled || self.cron.is_none() {
            self.cron_learn_usage_retry_at = None;
            return false;
        }
        let due = self
            .cron_learn_usage_retry_at
            .get_or_insert_with(|| now + LEARN_USAGE_RETRY_INTERVAL);
        if now < *due {
            return false;
        }
        *due = now + LEARN_USAGE_RETRY_INTERVAL;
        true
    }

    pub(crate) fn cron_deadline(&self, now: Instant) -> Option<Instant> {
        let due = self.cron.as_ref()?.scheduler.next_run_at()?;
        let remaining = due.saturating_sub(OffsetDateTime::now_utc().unix_timestamp());
        // Instant cannot represent arbitrarily distant RFC3339 one-shots on
        // every platform; revisit the absolute time at least once per day.
        let seconds = remaining.clamp(0, 24 * 60 * 60) as u64;
        now.checked_add(Duration::from_secs(seconds).max(Duration::from_millis(100)))
    }

    pub(crate) fn start_cron_due(&mut self) -> bool {
        if self.cron.is_none() && self.cron_retry_at.is_some_and(|at| Instant::now() >= at) {
            if let Err(error) = self.enable_cron() {
                tracing::debug!(%error, "cron scheduler waiting for handoff lock");
                self.cron_retry_at = Some(Instant::now() + Duration::from_secs(1));
            }
        }
        if self.cron_learn_usage_retry_due(Instant::now()) {
            self.start_cron_learn_usage_sync();
        }
        let Some(cron) = self.cron.as_mut() else {
            return false;
        };
        let pending = match cron.scheduler.take_due(OffsetDateTime::now_utc()) {
            Ok(pending) => pending,
            Err(error) => {
                warn!(%error, "cron scheduler tick failed");
                return false;
            }
        };
        let changed = !pending.is_empty();
        for run in pending {
            self.dispatch_cron(run);
        }
        changed
    }

    pub(crate) fn dispatch_cron(&mut self, run: ReservedRun) {
        let Some(cron) = self.cron.as_ref() else {
            return;
        };
        if let Err(error) = cron
            .scheduler
            .mark_running(&run.id, OffsetDateTime::now_utc())
        {
            warn!(run_id = %run.id, %error, "cron run could not start");
            return;
        }
        match executor::plan(&run.job) {
            Ok(ExecutionPlan::Command(plan)) => {
                let tx = self.event_tx.clone();
                tokio::spawn(async move {
                    let result = executor::run_command(&plan).await;
                    let _ = tx
                        .send(AppEvent::CronCompleted {
                            run: Box::new(run),
                            result,
                        })
                        .await;
                });
            }
            Ok(ExecutionPlan::Pane {
                pane_id,
                prompt,
                timeout_seconds,
            }) => {
                let socket_path = crate::api::socket_path();
                let workdir = run.job.workdir.clone();
                let tx = self.event_tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        run_pane(socket_path, pane_id, prompt, workdir, timeout_seconds)
                    })
                    .await
                    .unwrap_or_else(|_| Err("pane worker failed".into()));
                    let _ = tx
                        .send(AppEvent::CronCompleted {
                            run: Box::new(run),
                            result,
                        })
                        .await;
                });
            }
            Err(error) => self.handle_cron_completed(run, Err(error)),
        }
    }

    pub(crate) fn handle_cron_completed(
        &mut self,
        run: ReservedRun,
        result: Result<CommandOutput, String>,
    ) {
        let Some(cron) = self.cron.as_mut() else {
            return;
        };
        let had_usage = result
            .as_ref()
            .ok()
            .is_some_and(|output| output.observed_usage.is_some());
        let target_binding = self
            .learn_config
            .job_targets
            .get(&run.job.id)
            .and_then(|target_id| {
                self.learn_config
                    .targets
                    .iter()
                    .find(|target| &target.id == target_id)
                    .and_then(|target| {
                        mercury_learn::metrics::target_path_fingerprint(&target.path)
                            .map(|fingerprint| format!("{target_id}:{fingerprint}"))
                    })
            });
        let completion = cron.finish(
            &run,
            result,
            OffsetDateTime::now_utc(),
            self.learn_config.enabled,
            target_binding.as_deref(),
        );
        if self.learn_config.enabled && had_usage {
            self.start_cron_learn_usage_sync();
        }
        match completion {
            Ok(Some(message)) if !message.destinations.is_empty() => {
                let socket_path = crate::api::socket_path();
                tokio::task::spawn_blocking(move || deliver_message(socket_path, message));
            }
            Ok(_) => {}
            Err(error) => warn!(run_id = %run.id, %error, "failed to persist cron completion"),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::app::AppPolicy;
    use crate::config::Config;
    use mercury_cron::{Job, JobTarget};
    use mercury_learn::metrics::ComparisonState;
    use time::UtcOffset;

    #[test]
    fn completed_headless_usage_replays_after_restart_without_claiming_improvement() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("mercury-cron-usage-{}-{stamp}", std::process::id()));
        let job = Job::new(
            "measure".into(),
            "every 1h",
            UtcOffset::UTC,
            OffsetDateTime::now_utc(),
            JobTarget::Headless {
                provider: "claude".into(),
                prompt: "synthetic task".into(),
                prompt_files: Vec::new(),
                model: None,
                effort: None,
                skills: Vec::new(),
            },
        )
        .unwrap();
        let job_id = job.id.clone();
        let cron_root = root.join("cron");
        let learn_root = root.join("learn");
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let mut cron = CronService::open(&cron_root, now).unwrap();
        cron.scheduler.add(job).unwrap();
        let run = cron.scheduler.run_now(&job_id, now).unwrap();
        cron.scheduler.mark_running(&run.id, now).unwrap();
        let usage = ObservedUsage {
            input_tokens: 120,
            output_tokens: 30,
            cost_microusd: Some(125),
        };
        cron.finish(
            &run,
            Ok(CommandOutput {
                success: true,
                exit_code: Some(0),
                text: "synthetic result".into(),
                truncated: false,
                observed_usage: Some(usage),
            }),
            now,
            true,
            Some("project:0123456789abcdef"),
        )
        .unwrap();
        let ledger_path = cron.ledger_path.clone();
        let pending = cron.scheduler.ledger().pending_learn_usage(16).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].learn_target_id.as_deref(),
            Some("project:0123456789abcdef")
        );
        drop(cron);
        fs::write(root.join("invalid-learn-root"), b"file").unwrap();
        assert!(reconcile_learn_usage(&ledger_path, &root.join("invalid-learn-root")).is_err());
        assert_eq!(
            SqliteLedger::open(&ledger_path)
                .unwrap()
                .pending_learn_usage(16)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(reconcile_learn_usage(&ledger_path, &learn_root).unwrap(), 1);
        assert_eq!(reconcile_learn_usage(&ledger_path, &learn_root).unwrap(), 0);
        let storage = LearnStorage::open(&learn_root).unwrap();
        let comparison = storage
            .task_comparison(
                &format!("cron:{job_id}:target:project:0123456789abcdef"),
                Provider::Claude,
            )
            .unwrap();
        assert_eq!(comparison.observed.samples, 1);
        assert_eq!(comparison.observed.input_tokens, 120);
        assert_eq!(comparison.observed.cost_microusd, Some(125));
        assert_eq!(comparison.state, ComparisonState::InsufficientData);
        let recent = storage.recent_observed_metrics(16).unwrap();
        assert_eq!(recent[0].observed_at_ms, now.unix_timestamp() * 1000);
        drop(storage);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn due_script_runs_off_thread_and_finishes_in_ledger() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("mercury-app-cron-{}-{stamp}", std::process::id()));
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.enable_cron_at(&root).unwrap();
        let now = OffsetDateTime::now_utc();
        let mut job = Job::new(
            "echo".into(),
            "every 1h",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "/bin/echo".into(),
                args: vec!["cron-result".into()],
            },
        )
        .unwrap();
        job.next_run_at = Some(now.unix_timestamp() - 1);
        let job_id = job.id.clone();
        let occurrence = job.next_run_at.unwrap();
        app.cron.as_mut().unwrap().scheduler.add(job).unwrap();
        assert!(app.start_cron_due());
        let event = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = app
                    .event_rx
                    .recv()
                    .await
                    .expect("cron event channel closed");
                if matches!(event, AppEvent::CronCompleted { .. }) {
                    break event;
                }
            }
        })
        .await
        .unwrap();
        app.handle_internal_event_with_render_impact(event);
        let row = app
            .cron
            .as_ref()
            .unwrap()
            .scheduler
            .ledger()
            .find(&job_id, occurrence)
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "success");
        assert!(fs::read_to_string(row.output_path.unwrap())
            .unwrap()
            .contains("cron-result"));
        drop(app);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failure_marker_creates_incident_even_on_zero_exit() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mercury-app-cron-failure-{}-{stamp}",
            std::process::id()
        ));
        let now = OffsetDateTime::now_utc();
        let mut cron = CronService::open(&root, now).unwrap();
        let job = Job::new(
            "check".into(),
            "every 1h",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "/bin/echo".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        let job_id = job.id.clone();
        cron.scheduler.add(job).unwrap();
        let run = cron.scheduler.run_now(&job_id, now).unwrap();
        cron.scheduler.mark_running(&run.id, now).unwrap();
        let delivery = cron
            .finish(
                &run,
                Ok(CommandOutput {
                    success: true,
                    exit_code: Some(0),
                    text: "[CRON_FAILURE] check failed".into(),
                    truncated: false,
                    observed_usage: None,
                }),
                now,
                false,
                None,
            )
            .unwrap();
        assert!(delivery.is_some());
        let second = cron.scheduler.run_now(&job_id, now).unwrap();
        cron.scheduler.mark_running(&second.id, now).unwrap();
        let repeated = cron
            .finish(
                &second,
                Ok(CommandOutput {
                    success: true,
                    exit_code: Some(0),
                    text: "[CRON_FAILURE] check failed".into(),
                    truncated: false,
                    observed_usage: None,
                }),
                now,
                false,
                None,
            )
            .unwrap();
        assert!(
            repeated.is_none(),
            "repeated incident must not notify every run"
        );
        assert_eq!(
            cron.scheduler.ledger().list_runs(&job_id, 10).unwrap()[0].status,
            "failure"
        );
        assert_eq!(
            cron.scheduler.ledger().list_incidents(&job_id, 10).unwrap()[0].fingerprint,
            "cron_failure"
        );
        drop(cron);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn far_future_cron_deadline_is_bounded_to_one_day() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mercury-app-cron-future-{}-{stamp}",
            std::process::id()
        ));
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.enable_cron_at(&root).unwrap();
        let job = Job::new(
            "distant".into(),
            "9999-01-01T00:00:00Z",
            UtcOffset::UTC,
            OffsetDateTime::now_utc(),
            JobTarget::Script {
                program: "/bin/echo".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        app.cron.as_mut().unwrap().scheduler.add(job).unwrap();
        let now = Instant::now();
        let deadline = app.cron_deadline(now).unwrap();
        assert!(deadline.duration_since(now) <= Duration::from_secs(24 * 60 * 60));
        drop(app);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn learn_usage_retry_timer_is_bounded_and_respects_opt_out() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("mercury-cron-retry-{}-{stamp}", std::process::id()));
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.learn_config.enabled = false;
        app.enable_cron_at(&root).unwrap();
        let now = Instant::now();
        assert!(!app.cron_learn_usage_retry_due(now));
        assert!(app.cron_learn_usage_retry_at.is_none());

        app.learn_config.enabled = true;
        assert!(!app.cron_learn_usage_retry_due(now));
        let due = now + LEARN_USAGE_RETRY_INTERVAL;
        assert_eq!(app.cron_learn_usage_retry_at, Some(due));
        assert!(app
            .next_headless_loop_deadline_with_git_refresh(now, false, false)
            .is_some_and(|deadline| deadline <= due));
        assert!(!app.cron_learn_usage_retry_due(due - Duration::from_millis(1)));
        assert!(app.cron_learn_usage_retry_due(due));
        assert_eq!(
            app.cron_learn_usage_retry_at,
            Some(due + LEARN_USAGE_RETRY_INTERVAL)
        );

        app.learn_config.enabled = false;
        assert!(!app.cron_learn_usage_retry_due(due));
        assert!(app.cron_learn_usage_retry_at.is_none());
        drop(app);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pane_runner_prompts_existing_agent_and_captures_bounded_reply() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("mercury-pane-cron-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("api.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            for expected_method in ["agent.prompt", "agent.read"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], expected_method);
                assert_eq!(request["params"]["target"], "pane-1");
                let result = if expected_method == "agent.prompt" {
                    assert_eq!(request["params"]["text"], "run check");
                    assert_eq!(request["params"]["wait"]["timeout_ms"], 10_000);
                    serde_json::json!({"type":"agent_prompted"})
                } else {
                    serde_json::json!({"type":"pane_read","read":{"text":"check passed"}})
                };
                writeln!(
                    stream,
                    "{}",
                    serde_json::json!({"id":"cron:pane","result":result})
                )
                .unwrap();
            }
        });
        let output = run_pane(socket, "pane-1".into(), "run check".into(), None, 10).unwrap();
        assert!(output.success);
        assert_eq!(output.text, "check passed");
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn silent_run_keeps_log_but_suppresses_delivery() {
        let root = std::env::temp_dir().join(format!(
            "mercury-silent-cron-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let now = OffsetDateTime::now_utc();
        let mut cron = CronService::open(&root, now).unwrap();
        let job = Job::new(
            "silent".into(),
            "every 1h",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "/bin/echo".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        let job_id = job.id.clone();
        cron.scheduler.add(job).unwrap();
        let run = cron.scheduler.run_now(&job_id, now).unwrap();
        cron.scheduler.mark_running(&run.id, now).unwrap();
        let delivery = cron
            .finish(
                &run,
                Ok(CommandOutput {
                    success: true,
                    exit_code: Some(0),
                    text: "[SILENT] result".into(),
                    truncated: false,
                    observed_usage: None,
                }),
                now,
                false,
                None,
            )
            .unwrap();
        assert!(delivery.is_none());
        let row = cron.scheduler.ledger().get_run(&run.id).unwrap().unwrap();
        assert_eq!(row.status, "success");
        assert!(fs::read_to_string(row.output_path.unwrap())
            .unwrap()
            .contains("[SILENT]"));
        drop(cron);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pane_delivery_submits_result_without_waiting() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("mcd-{}-{:x}", std::process::id(), stamp as u64));
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("api.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "agent.prompt");
            assert_eq!(request["params"]["target"], "pane-2");
            assert_eq!(request["params"]["text"], "run finished");
            assert!(request["params"].get("wait").is_none());
            writeln!(
                stream,
                "{}",
                serde_json::json!({"id":"cron:pane","result":{"type":"agent_prompted"}})
            )
            .unwrap();
        });
        deliver_one(
            &socket,
            &Delivery::Pane {
                pane_id: "pane-2".into(),
            },
            "ignored",
            "run finished",
        )
        .unwrap();
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_destination_does_not_prevent_later_pane_delivery() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("mci-{}-{:x}", std::process::id(), stamp as u64));
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("api.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            for (target, result) in [
                (
                    "missing",
                    serde_json::json!({"error":{"code":"agent_not_found"}}),
                ),
                (
                    "ready",
                    serde_json::json!({"result":{"type":"agent_prompted"}}),
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["params"]["target"], target);
                let mut response = result;
                response["id"] = "cron:pane".into();
                writeln!(stream, "{response}").unwrap();
            }
        });
        deliver_message(
            socket,
            DeliveryMessage {
                job_id: "job".into(),
                run_id: "run".into(),
                title: "Cron".into(),
                body: "result".into(),
                destinations: vec![
                    Delivery::Pane {
                        pane_id: "missing".into(),
                    },
                    Delivery::Pane {
                        pane_id: "ready".into(),
                    },
                ],
            },
        );
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delivery_truncation_respects_utf8_boundaries() {
        assert_eq!(truncate_chars("a🙂b", 3), "a");
        assert_eq!(truncate_chars("a🙂b", 5), "a🙂");
    }

    #[test]
    fn teams_destination_requires_https_without_embedded_credentials() {
        assert!(validate_teams_url("https://example.com/webhook", false).is_ok());
        assert!(validate_teams_url("http://127.0.0.1:8080/webhook", false).is_err());
        assert!(validate_teams_url("https://user:pass@example.com/webhook", false).is_err());
    }

    #[test]
    fn teams_delivery_posts_bounded_json_to_local_simulated_webhook() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("POST /hook HTTP/1.1"));
            let mut content_length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    content_length = line
                        .split_once(':')
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                }
            }
            assert!(content_length <= 28 * 1024);
            let mut payload = vec![0; content_length];
            reader.read_exact(&mut payload).unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
            assert!(payload["text"]
                .as_str()
                .unwrap()
                .starts_with("Mercury cron\nRun"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n1"
            )
            .unwrap();
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        assert!(validate_teams_url(&url, false).is_err());
        send_teams_with_client(
            &client,
            &url,
            "Mercury cron",
            &format!("Run{}", "é".repeat(5000)),
            true,
        )
        .unwrap();
        server.join().unwrap();
    }
}
