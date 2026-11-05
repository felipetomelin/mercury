//! Single-owner bounded Learn dispatch. Session/turn/week signal producers
//! are connected separately; this worker never runs on the PTY/UI loop.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use mercury_learn::curator;
use mercury_learn::storage::{transcript_fingerprint, Storage, StorageError, TriggerStatus};
use mercury_learn::transcript::{self, Limits, Provider, Role};
use mercury_learn::{
    pipeline, policy::Policy, reviewer::ClaudeReviewer, reviewer::ReviewerConfig, Changes,
};
use time::{Date, Duration as TimeDuration, OffsetDateTime, Time};

const QUEUE_CAPACITY: usize = 8;

#[derive(Debug, Clone)]
pub struct ObservedTranscript {
    pub session_id: String,
    pub path: PathBuf,
    /// Reject a process-exit event observed before the SessionStart report.
    pub accepted_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerOrigin {
    ObservedSession,
    InternalWeekly,
    InternalCurator,
    Learn,
    Cron,
}

#[derive(Debug, Clone)]
pub struct ReviewTrigger {
    /// Stable `provider:session:reason[:turn]` or `weekly:yyyy-mm-dd` key.
    key: String,
    origin: TriggerOrigin,
    session: Option<ReviewSession>,
    probe_every: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ReviewSession {
    pub provider: Provider,
    pub session_id: String,
    /// Lexically checked here; the transcript reader must still validate
    /// on-disk identity, symlinks and complete conversational content.
    pub transcript_path: PathBuf,
    /// Exact count required for a turn milestone; `None` for session end.
    pub milestone_turns: Option<usize>,
    pub expected_fingerprint: Option<String>,
}

impl ReviewTrigger {
    pub fn session_end(
        provider: Provider,
        session_id: &str,
        transcript_path: &Path,
    ) -> Option<Self> {
        let (prefix, session) = session_source(provider, session_id, transcript_path, None)?;
        Some(Self {
            key: format!("{prefix}:end"),
            origin: TriggerOrigin::ObservedSession,
            session: Some(session),
            probe_every: None,
        })
    }

    pub fn turn_milestone(
        provider: Provider,
        session_id: &str,
        transcript_path: &Path,
        exact_user_turns: Option<usize>,
        every: u32,
    ) -> Option<Self> {
        let turns = exact_user_turns?;
        if !(1..=1000).contains(&every)
            || turns == 0
            || turns > 1_000_000
            || turns % every as usize != 0
        {
            return None;
        }
        let (prefix, session) = session_source(provider, session_id, transcript_path, Some(turns))?;
        Some(Self {
            key: format!("{prefix}:turn:{turns}"),
            origin: TriggerOrigin::ObservedSession,
            session: Some(session),
            probe_every: None,
        })
    }

    pub fn turn_probe(
        provider: Provider,
        session_id: &str,
        transcript_path: &Path,
        every: u32,
    ) -> Option<Self> {
        if !(1..=1000).contains(&every) {
            return None;
        }
        let (prefix, session) = session_source(provider, session_id, transcript_path, None)?;
        Some(Self {
            key: format!("{prefix}:turn:probe"),
            origin: TriggerOrigin::ObservedSession,
            session: Some(session),
            probe_every: Some(every),
        })
    }

    /// `due_date` comes from the internal weekly scheduler, not user content.
    pub fn weekly_due(
        due_date: Date,
        provider: Provider,
        session_id: &str,
        transcript_path: &Path,
        fingerprint: &str,
    ) -> Option<Self> {
        let (_, mut session) = session_source(provider, session_id, transcript_path, None)?;
        session.expected_fingerprint = Some(fingerprint.to_owned());
        Some(Self {
            key: format!("weekly:{due_date}"),
            origin: TriggerOrigin::InternalWeekly,
            session: Some(session),
            probe_every: None,
        })
    }

    fn weekly_curation(due_date: Date) -> Self {
        Self {
            key: format!("curation:weekly:{due_date}"),
            origin: TriggerOrigin::InternalCurator,
            session: None,
            probe_every: None,
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn session(&self) -> Option<&ReviewSession> {
        self.session.as_ref()
    }
}

fn session_source(
    provider: Provider,
    session_id: &str,
    transcript_path: &Path,
    milestone_turns: Option<usize>,
) -> Option<(String, ReviewSession)> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    if !transcript_path.is_absolute()
        || transcript_path.as_os_str().len() > 4096
        || transcript_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let prefix = format!(
        "{}:{session_id}",
        match provider {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    );
    Some((
        prefix,
        ReviewSession {
            provider,
            session_id: session_id.to_owned(),
            transcript_path: transcript_path.to_path_buf(),
            milestone_turns,
            expected_fingerprint: None,
        },
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitResult {
    Queued,
    Disabled,
    ExcludedOrigin,
    InvalidKey,
    QueueFull,
    WorkerClosed,
}

pub struct LearnWorker {
    sender: Option<SyncSender<ReviewTrigger>>,
    enabled: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    weekly_stop: Option<mpsc::Sender<()>>,
    weekly_handle: Option<std::thread::JoinHandle<()>>,
}

impl LearnWorker {
    /// Builds the real handler on the worker thread. The executable and
    /// private workdir are resolved by the server, not by a model response.
    pub fn start_reviewing(
        root: &Path,
        config: &crate::config::LearnConfig,
        reviewer_config: ReviewerConfig,
        app_events: Option<tokio::sync::mpsc::Sender<crate::events::AppEvent>>,
    ) -> Result<Self, String> {
        config.validate().map_err(str::to_owned)?;
        let policy = Policy::new(config.target_policies())
            .map_err(|_| "invalid Learn target policy".to_string())?;
        let reviewer = ClaudeReviewer::new(reviewer_config)
            .map_err(|_| "invalid Learn reviewer configuration".to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Learn reviewer runtime unavailable".to_string())?;
        let handler_root = root.to_path_buf();
        let config_target_kinds = config
            .targets
            .iter()
            .map(|target| {
                (
                    target.id.clone(),
                    mercury_learn::policy::TargetKind::from(target.kind),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let mut worker = Self::start(root, config.enabled, move |trigger, gate| {
            if trigger.origin == TriggerOrigin::InternalCurator {
                return run_weekly_curation(&handler_root, &policy, trigger.key(), gate);
            }
            let Some(session) = trigger.session() else {
                return false;
            };
            if let Some(expected) = session.expected_fingerprint.as_deref() {
                if transcript_fingerprint(&session.transcript_path)
                    .ok()
                    .as_deref()
                    != Some(expected)
                {
                    return false;
                }
            }
            let Ok(mut storage) = Storage::open(&handler_root) else {
                return false;
            };
            if trigger.origin == TriggerOrigin::ObservedSession && session.milestone_turns.is_none()
            {
                let before = match transcript_fingerprint(&session.transcript_path) {
                    Ok(fingerprint) => fingerprint,
                    Err(_) => return false,
                };
                if transcript::read(
                    &session.transcript_path,
                    session.provider,
                    &session.session_id,
                    Limits::default(),
                )
                .is_err()
                    || storage
                        .record_completed_session(
                            session.provider,
                            &session.session_id,
                            &session.transcript_path,
                            &before,
                        )
                        .is_err()
                {
                    return false;
                }
            }
            let mut changes = Changes::new(storage);
            let outcome = runtime.block_on(pipeline::review_session(
                pipeline::SessionReview {
                    provider: session.provider,
                    session_id: &session.session_id,
                    transcript_path: &session.transcript_path,
                    milestone_turns: session.milestone_turns,
                },
                &policy,
                || gate.load(Ordering::Acquire),
                &reviewer,
                &mut changes,
            ));
            let Ok(outcome) = outcome else {
                return false;
            };
            let mut applied_kinds = Vec::with_capacity(outcome.applied_ids.len());
            for id in &outcome.applied_ids {
                let Ok(Some(record)) = changes.storage().get(id) else {
                    return false;
                };
                let Some(kind) = config_target_kinds.get(&record.target_id) else {
                    return false;
                };
                applied_kinds.push(*kind);
            }
            let route_changed =
                applied_kinds.contains(&mercury_learn::policy::TargetKind::ConfigSection);
            let cron_changed = applied_kinds.contains(&mercury_learn::policy::TargetKind::CronJob);
            if route_changed {
                let Some(events) = &app_events else {
                    return false;
                };
                if events
                    .try_send(crate::events::AppEvent::LearnRouteChanged)
                    .is_err()
                {
                    return false;
                }
            }
            if cron_changed {
                let Some(events) = &app_events else {
                    return false;
                };
                if events
                    .try_send(crate::events::AppEvent::LearnCronChanged)
                    .is_err()
                {
                    return false;
                }
            }
            true
        })
        .map_err(|_| "Learn worker storage unavailable".to_string())?;
        if config.weekly && config.enabled {
            worker.start_weekly_with_clock(root, OffsetDateTime::now_utc)?;
        }
        Ok(worker)
    }

    /// The handler runs on one dedicated thread after a durable claim. It
    /// must return false on any review/apply failure, without logging secrets.
    pub fn start(
        root: &Path,
        enabled: bool,
        handler: impl Fn(ReviewTrigger, &AtomicBool) -> bool + Send + 'static,
    ) -> Result<Self, StorageError> {
        let mut storage = Storage::open(root)?;
        let enabled = Arc::new(AtomicBool::new(enabled));
        let worker_enabled = Arc::clone(&enabled);
        let (sender, receiver) = mpsc::sync_channel::<ReviewTrigger>(QUEUE_CAPACITY);
        let handle = std::thread::Builder::new()
            .name("mercury-learn".into())
            .spawn(move || {
                while let Ok(trigger) = receiver.recv() {
                    if !worker_enabled.load(Ordering::Acquire) {
                        continue;
                    }
                    let Some(trigger) = resolve_turn_probe(trigger) else {
                        continue;
                    };
                    match storage.claim_trigger(&trigger.key) {
                        Ok(true) => {
                            let succeeded =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    worker_enabled.load(Ordering::Acquire)
                                        && handler(trigger.clone(), &worker_enabled)
                                }))
                                .unwrap_or(false);
                            let status = if succeeded {
                                TriggerStatus::Succeeded
                            } else {
                                TriggerStatus::Failed
                            };
                            if storage.finish_trigger(&trigger.key, status).is_err() {
                                tracing::warn!("learn trigger could not be finalized");
                            }
                        }
                        Ok(false) => {} // already claimed, even across restarts
                        Err(_) => tracing::warn!("learn trigger could not be claimed"),
                    }
                }
            })
            .map_err(StorageError::Io)?;
        Ok(Self {
            sender: Some(sender),
            enabled,
            handle: Some(handle),
            weekly_stop: None,
            weekly_handle: None,
        })
    }

    fn start_weekly_with_clock(
        &mut self,
        root: &Path,
        clock: impl Fn() -> OffsetDateTime + Send + 'static,
    ) -> Result<(), String> {
        let (stop, receiver) = mpsc::channel();
        let sender = self
            .sender
            .as_ref()
            .ok_or("Learn worker unavailable")?
            .clone();
        let enabled = Arc::clone(&self.enabled);
        let root = root.to_path_buf();
        let handle = std::thread::Builder::new()
            .name("mercury-learn-weekly".into())
            .spawn(move || {
                while enabled.load(Ordering::Acquire) {
                    let now = clock();
                    if let Ok(Some(trigger)) = weekly_candidate(&root, now) {
                        let _ = sender.try_send(trigger);
                    }
                    let _ = sender.try_send(ReviewTrigger::weekly_curation(week_start(now)));
                    if receiver
                        .recv_timeout(std::time::Duration::from_secs(15 * 60))
                        .is_ok()
                    {
                        break;
                    }
                }
            })
            .map_err(|_| "Learn weekly scheduler unavailable".to_string())?;
        self.weekly_stop = Some(stop);
        self.weekly_handle = Some(handle);
        Ok(())
    }

    pub fn submit(&self, trigger: ReviewTrigger) -> SubmitResult {
        if !self.enabled.load(Ordering::Acquire) {
            return SubmitResult::Disabled;
        }
        if matches!(trigger.origin, TriggerOrigin::Learn | TriggerOrigin::Cron) {
            return SubmitResult::ExcludedOrigin;
        }
        if !valid_trigger_key(&trigger.key) {
            return SubmitResult::InvalidKey;
        }
        let Some(sender) = self.sender.as_ref() else {
            return SubmitResult::WorkerClosed;
        };
        match sender.try_send(trigger) {
            Ok(()) => SubmitResult::Queued,
            Err(TrySendError::Full(_)) => SubmitResult::QueueFull,
            Err(TrySendError::Disconnected(_)) => SubmitResult::WorkerClosed,
        }
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }

    pub fn shutdown(mut self) {
        self.disable();
        self.weekly_stop.take();
        if let Some(handle) = self.weekly_handle.take() {
            let _ = handle.join();
        }
        self.sender.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LearnWorker {
    fn drop(&mut self) {
        self.disable();
        self.weekly_stop.take();
        self.sender.take();
    }
}

fn valid_trigger_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}

fn resolve_turn_probe(trigger: ReviewTrigger) -> Option<ReviewTrigger> {
    let Some(every) = trigger.probe_every else {
        return Some(trigger);
    };
    let session = trigger.session.as_ref()?;
    let before = transcript_fingerprint(&session.transcript_path).ok()?;
    let snapshot = transcript::read(
        &session.transcript_path,
        session.provider,
        &session.session_id,
        Limits::default(),
    )
    .ok()?;
    let turns = snapshot.user_turns?;
    if snapshot.messages.last()?.role != Role::Assistant {
        return None;
    }
    if transcript_fingerprint(&session.transcript_path).ok()? != before {
        return None;
    }
    let mut milestone = ReviewTrigger::turn_milestone(
        session.provider,
        &session.session_id,
        &session.transcript_path,
        Some(turns),
        every,
    )?;
    milestone.session.as_mut()?.expected_fingerprint = Some(before);
    Some(milestone)
}

fn weekly_candidate(
    root: &Path,
    now: OffsetDateTime,
) -> Result<Option<ReviewTrigger>, StorageError> {
    let due_date = week_start(now);
    let start_ms = (due_date - TimeDuration::days(7))
        .with_time(Time::MIDNIGHT)
        .assume_utc()
        .unix_timestamp_nanos()
        / 1_000_000;
    let end_ms = due_date
        .with_time(Time::MIDNIGHT)
        .assume_utc()
        .unix_timestamp_nanos()
        / 1_000_000;
    let storage = Storage::open(root)?;
    for candidate in storage.recent_completed_sessions(64)? {
        if (candidate.completed_at_ms as i128) < start_ms
            || (candidate.completed_at_ms as i128) >= end_ms
        {
            continue;
        }
        let Some(expected) = candidate.fingerprint else {
            continue;
        };
        if transcript_fingerprint(&candidate.transcript_path)
            .ok()
            .as_deref()
            != Some(expected.as_str())
        {
            continue;
        }
        if transcript::read(
            &candidate.transcript_path,
            candidate.provider,
            &candidate.session_id,
            Limits::default(),
        )
        .is_err()
            || transcript_fingerprint(&candidate.transcript_path)
                .ok()
                .as_deref()
                != Some(expected.as_str())
        {
            continue;
        }
        if let Some(trigger) = ReviewTrigger::weekly_due(
            due_date,
            candidate.provider,
            &candidate.session_id,
            &candidate.transcript_path,
            &expected,
        ) {
            return Ok(Some(trigger));
        }
    }
    Ok(None)
}

fn week_start(now: OffsetDateTime) -> Date {
    now.date() - TimeDuration::days(now.weekday().number_days_from_monday() as i64)
}

#[cfg(unix)]
fn run_weekly_curation(root: &Path, policy: &Policy, source: &str, gate: &AtomicBool) -> bool {
    if !gate.load(Ordering::Acquire) {
        return false;
    }
    let Ok(plans) = curator::plan_exact_duplicate_cleanup(policy, true, source) else {
        return false;
    };
    let Ok(storage) = Storage::open(root) else {
        return false;
    };
    let mut changes = Changes::new(storage);
    for plan in plans {
        if curator::apply_plan(policy, || gate.load(Ordering::Acquire), &mut changes, plan).is_err()
        {
            return false;
        }
    }
    true
}

#[cfg(not(unix))]
fn run_weekly_curation(_: &Path, _: &Policy, _: &str, _: &AtomicBool) -> bool {
    false
}

impl super::App {
    pub(crate) fn enable_learn(&mut self) -> Result<(), String> {
        if !self.learn_config.enabled || self.learn.is_some() {
            return Ok(());
        }
        #[cfg(not(unix))]
        return Err("Learn automatic writes are not supported on this platform".into());

        #[cfg(unix)]
        {
            let data_dir = crate::session::data_dir();
            let workdir = data_dir.join("learn-reviewer");
            mercury_learn::storage::ensure_private_dir(&workdir)
                .map_err(|_| "Learn private reviewer directory unavailable".to_string())?;
            let executable = find_claude_executable()
                .ok_or_else(|| "Claude CLI is not available for Learn".to_string())?;
            let reviewer_config = ReviewerConfig {
                executable,
                workdir,
                model: self.learn_config.model.clone(),
                max_budget_usd: self.learn_config.max_budget_usd,
                timeout: Duration::from_secs(self.learn_config.timeout_secs),
                max_input_bytes: 256 * 1024,
                max_stdout_bytes: 64 * 1024,
                max_stderr_bytes: 8 * 1024,
            };
            let worker = LearnWorker::start_reviewing(
                &data_dir.join("learn"),
                &self.learn_config,
                reviewer_config,
                Some(self.event_tx.clone()),
            )?;
            self.learn = Some(worker);
            Ok(())
        }
    }
}

#[cfg(unix)]
fn find_claude_executable() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|directory| {
        let candidate = directory.join("claude");
        let metadata = std::fs::metadata(&candidate).ok()?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
        candidate
            .canonicalize()
            .ok()
            .filter(|path| path.is_absolute())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "mercury-learn-worker-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(root)
        }

        fn data(&self) -> std::path::PathBuf {
            self.0.join("learn")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn trigger(key: &str, origin: TriggerOrigin) -> ReviewTrigger {
        ReviewTrigger {
            key: key.into(),
            origin,
            session: None,
            probe_every: None,
        }
    }

    #[test]
    fn typed_trigger_keys_require_stable_identity_and_exact_turn_count() {
        let transcript = Path::new("/tmp/session-1.jsonl");
        let end = ReviewTrigger::session_end(Provider::Claude, "session-1", transcript).unwrap();
        assert_eq!(end.key(), "claude:session-1:end");
        assert_eq!(end.session().unwrap().transcript_path, transcript);
        assert!(ReviewTrigger::session_end(Provider::Claude, "../escape", transcript).is_none());
        assert!(ReviewTrigger::session_end(
            Provider::Claude,
            "session-1",
            Path::new("relative.jsonl")
        )
        .is_none());
        assert!(ReviewTrigger::session_end(
            Provider::Claude,
            "session-1",
            Path::new("/tmp/../session-1.jsonl")
        )
        .is_none());
        assert!(
            ReviewTrigger::turn_milestone(Provider::Codex, "session-1", transcript, None, 12)
                .is_none()
        );
        assert!(ReviewTrigger::turn_milestone(
            Provider::Codex,
            "session-1",
            transcript,
            Some(11),
            12
        )
        .is_none());
        assert_eq!(
            ReviewTrigger::turn_milestone(Provider::Codex, "session-1", transcript, Some(12), 12)
                .unwrap()
                .key(),
            "codex:session-1:turn:12"
        );
        assert_eq!(
            ReviewTrigger::weekly_due(
                Date::from_calendar_date(2026, time::Month::October, 5).unwrap(),
                Provider::Claude,
                "session-1",
                transcript,
                "fingerprint"
            )
            .unwrap()
            .key(),
            "weekly:2026-10-05"
        );
    }

    #[test]
    fn turn_probe_requires_a_complete_exact_milestone_before_claim() {
        let fixture = Fixture::new();
        let transcript = fixture.0.join("rollout.jsonl");
        let meta = "{\"type\":\"session_meta\",\"payload\":{\"session_id\":\"session-1\"}}\n";
        let user = |id: &str| {
            format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"id\":\"{id}\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"hello\"}}]}}}}\n"
        )
        };
        let assistant = |id: &str| {
            format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"id\":\"{id}\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"done\"}}]}}}}\n"
        )
        };
        let probe =
            || ReviewTrigger::turn_probe(Provider::Codex, "session-1", &transcript, 2).unwrap();
        fs::write(
            &transcript,
            format!("{meta}{}{}{}", user("u1"), assistant("a1"), user("u2")),
        )
        .unwrap();
        assert!(resolve_turn_probe(probe()).is_none());
        fs::write(
            &transcript,
            format!(
                "{meta}{}{}{}{}",
                user("u1"),
                assistant("a1"),
                user("u2"),
                assistant("a2")
            ),
        )
        .unwrap();
        let milestone = resolve_turn_probe(probe()).unwrap();
        assert_eq!(milestone.key(), "codex:session-1:turn:2");
        assert_eq!(milestone.session().unwrap().milestone_turns, Some(2));
        assert!(milestone.session().unwrap().expected_fingerprint.is_some());
        let (sent, received) = mpsc::channel();
        let worker = LearnWorker::start(&fixture.data(), true, move |trigger, _| {
            sent.send(trigger.key().to_owned()).unwrap();
            true
        })
        .unwrap();
        assert_eq!(worker.submit(probe()), SubmitResult::Queued);
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            milestone.key()
        );
        assert_eq!(worker.submit(probe()), SubmitResult::Queued);
        worker.shutdown();
        assert!(received.try_recv().is_err());
        assert_eq!(
            Storage::open(&fixture.data())
                .unwrap()
                .trigger_status(milestone.key())
                .unwrap(),
            Some(TriggerStatus::Succeeded)
        );
        fs::write(
            &transcript,
            format!(
                "{meta}{}{}{}{}{{broken",
                user("u1"),
                assistant("a1"),
                user("u2"),
                assistant("a2")
            ),
        )
        .unwrap();
        assert!(resolve_turn_probe(probe()).is_none());
    }

    #[test]
    fn worker_deduplicates_across_restart_and_excludes_recursive_origins() {
        let fixture = Fixture::new();
        let (done_tx, done_rx) = mpsc::channel();
        let first = LearnWorker::start(&fixture.data(), true, move |_, _| {
            done_tx.send(()).unwrap();
            true
        })
        .unwrap();
        assert_eq!(
            first.submit(trigger("codex:session-1:end", TriggerOrigin::Learn)),
            SubmitResult::ExcludedOrigin
        );
        assert_eq!(
            first.submit(trigger("codex:session-1:end", TriggerOrigin::Cron)),
            SubmitResult::ExcludedOrigin
        );
        assert_eq!(
            first.submit(trigger(
                "codex:session-1:end",
                TriggerOrigin::ObservedSession
            )),
            SubmitResult::Queued
        );
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        first.shutdown();

        let (again_tx, again_rx) = mpsc::channel();
        let second = LearnWorker::start(&fixture.data(), true, move |_, _| {
            again_tx.send(()).unwrap();
            true
        })
        .unwrap();
        assert_eq!(
            second.submit(trigger(
                "codex:session-1:end",
                TriggerOrigin::ObservedSession
            )),
            SubmitResult::Queued
        );
        second.shutdown();
        assert!(again_rx.try_recv().is_err());
        assert_eq!(
            Storage::open(&fixture.data())
                .unwrap()
                .trigger_status("codex:session-1:end")
                .unwrap(),
            Some(TriggerStatus::Succeeded)
        );
    }

    #[test]
    fn worker_queues_nonblocking_and_rejects_invalid_or_disabled_signals() {
        let fixture = Fixture::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = LearnWorker::start(&fixture.data(), true, move |_, _| {
            entered_tx.send(()).unwrap();
            let _ = release_rx.recv();
            true
        })
        .unwrap();
        assert_eq!(
            worker.submit(trigger("weekly:2026-10-05", TriggerOrigin::InternalWeekly)),
            SubmitResult::Queued
        );
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for index in 0..QUEUE_CAPACITY {
            assert_eq!(
                worker.submit(trigger(
                    &format!("codex:session-{index}:end"),
                    TriggerOrigin::ObservedSession
                )),
                SubmitResult::Queued
            );
        }
        assert_eq!(
            worker.submit(trigger(
                "codex:overflow:end",
                TriggerOrigin::ObservedSession
            )),
            SubmitResult::QueueFull
        );
        assert_eq!(
            worker.submit(trigger("bad/key", TriggerOrigin::ObservedSession)),
            SubmitResult::InvalidKey
        );
        for _ in 0..=QUEUE_CAPACITY {
            release_tx.send(()).unwrap();
        }
        worker.shutdown();

        let disabled = LearnWorker::start(&fixture.data(), false, |_, _| true).unwrap();
        assert_eq!(
            disabled.submit(trigger(
                "codex:disabled:end",
                TriggerOrigin::ObservedSession
            )),
            SubmitResult::Disabled
        );
        disabled.shutdown();
    }

    #[cfg(unix)]
    #[test]
    fn configured_worker_reviews_fixture_session_and_records_result() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        let target = fixture.0.join("AGENTS.md");
        fs::write(&target, b"Before").unwrap();
        let transcript = fixture.0.join("session-1.jsonl");
        fs::write(
            &transcript,
            "{\"type\":\"user\",\"sessionId\":\"session-1\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"Remember this preference\"}}\n",
        )
        .unwrap();
        let script = fixture.0.join("fake-claude");
        let response = serde_json::json!({
            "type":"result", "subtype":"success", "is_error":false,
            "total_cost_usd":0.01,
            "structured_output":{"proposals":[{
                "target_id":"instructions", "content":"After",
                "reason":"Remember preference", "evidence":"User requested it"
            }]}
        });
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", response),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let config_text = format!(
            "[learn]\nenabled = true\n[[learn.target]]\nid = \"instructions\"\npath = \"{}\"\nkind = \"instructions\"\nmax_bytes = 4096\n",
            target.display()
        );
        let config: crate::config::Config = toml::from_str(&config_text).unwrap();
        let reviewer_config = ReviewerConfig {
            executable: script,
            workdir: fixture.0.clone(),
            model: "haiku".into(),
            max_budget_usd: 0.1,
            timeout: Duration::from_secs(2),
            max_input_bytes: 128 * 1024,
            max_stdout_bytes: 8 * 1024,
            max_stderr_bytes: 1024,
        };
        let worker =
            LearnWorker::start_reviewing(&fixture.data(), &config.learn, reviewer_config, None)
                .unwrap();
        let trigger =
            ReviewTrigger::session_end(Provider::Claude, "session-1", &transcript).unwrap();
        assert_eq!(worker.submit(trigger), SubmitResult::Queued);

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let status = Storage::open(&fixture.data())
                .unwrap()
                .trigger_status("claude:session-1:end")
                .unwrap();
            if status == Some(TriggerStatus::Succeeded) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Learn review timed out: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(&target).unwrap(), b"After");
        let invalid_transcript = fixture.0.join("session-2.jsonl");
        fs::write(
            &invalid_transcript,
            "{\"type\":\"user\",\"sessionId\":\"other-session\",\"uuid\":\"u2\",\"message\":{\"role\":\"user\",\"content\":\"ignore\"}}\n",
        )
        .unwrap();
        assert_eq!(
            worker.submit(
                ReviewTrigger::session_end(Provider::Claude, "session-2", &invalid_transcript)
                    .unwrap()
            ),
            SubmitResult::Queued
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let status = Storage::open(&fixture.data())
                .unwrap()
                .trigger_status("claude:session-2:end")
                .unwrap();
            if status == Some(TriggerStatus::Failed) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "invalid session timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        worker.shutdown();
        let completed = Storage::open(&fixture.data())
            .unwrap()
            .recent_completed_sessions(1)
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].provider, Provider::Claude);
        assert_eq!(completed[0].session_id, "session-1");
        assert_eq!(completed[0].transcript_path, transcript);
        let this_week = OffsetDateTime::from_unix_timestamp_nanos(
            completed[0].completed_at_ms as i128 * 1_000_000,
        )
        .unwrap();
        assert!(weekly_candidate(&fixture.data(), this_week)
            .unwrap()
            .is_none());
        let next_monday = this_week.date()
            - TimeDuration::days(this_week.weekday().number_days_from_monday() as i64)
            + TimeDuration::days(7);
        let next_week = next_monday.with_time(Time::MIDNIGHT).assume_utc();
        assert_eq!(
            weekly_candidate(&fixture.data(), next_week)
                .unwrap()
                .unwrap()
                .key(),
            format!("weekly:{next_monday}")
        );
        let mut weekly_worker = LearnWorker::start(&fixture.data(), true, |trigger, _| {
            trigger.origin == TriggerOrigin::InternalWeekly && trigger.session().is_some()
        })
        .unwrap();
        weekly_worker
            .start_weekly_with_clock(&fixture.data(), move || next_week)
            .unwrap();
        let weekly_key = format!("weekly:{next_monday}");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let status = Storage::open(&fixture.data())
                .unwrap()
                .trigger_status(&weekly_key)
                .unwrap();
            if status == Some(TriggerStatus::Succeeded) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "weekly review timed out: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        weekly_worker.shutdown();
        let duplicate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&duplicate_calls);
        let mut restarted = LearnWorker::start(&fixture.data(), true, move |_, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            true
        })
        .unwrap();
        restarted
            .start_weekly_with_clock(&fixture.data(), move || next_week)
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        restarted.shutdown();
        assert_eq!(duplicate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            Storage::open(&fixture.data())
                .unwrap()
                .trigger_status(&weekly_key)
                .unwrap(),
            Some(TriggerStatus::Succeeded)
        );
        fs::write(&transcript, b"changed after completion").unwrap();
        assert!(weekly_candidate(&fixture.data(), next_week)
            .unwrap()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn route_and_cron_review_notify_app_after_durable_apply() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        let target = fixture.0.join("config.toml");
        fs::write(&target, b"[route]\nenabled = false\n").unwrap();
        let jobs = fixture.0.join("jobs.toml");
        let now = time::OffsetDateTime::now_utc();
        let mut scheduler = mercury_cron::scheduler::Scheduler::open(
            &jobs,
            &fixture.0.join("executions.db"),
            &fixture.0.join("scheduler.lock"),
            now,
        )
        .unwrap();
        let job = mercury_cron::Job::new(
            "fixture".into(),
            "every 1h",
            time::UtcOffset::UTC,
            now,
            mercury_cron::JobTarget::Script {
                program: "/usr/bin/true".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        let job_id = job.id.clone();
        scheduler.add(job).unwrap();
        let second = mercury_cron::Job::new(
            "second fixture".into(),
            "every 2h",
            time::UtcOffset::UTC,
            now,
            mercury_cron::JobTarget::Script {
                program: "/usr/bin/true".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        let second_id = second.id.clone();
        scheduler.add(second).unwrap();
        let transcript = fixture.0.join("session-route.jsonl");
        fs::write(
            &transcript,
            "{\"type\":\"user\",\"sessionId\":\"session-route\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"Prefer automatic routing\"}}\n",
        )
        .unwrap();
        let script = fixture.0.join("fake-claude-route");
        let response = serde_json::json!({
            "type":"result", "subtype":"success", "is_error":false,
            "total_cost_usd":0.01,
            "structured_output":{"proposals":[{
                "target_id":"routing", "content":"[route]\nenabled = true\n",
                "reason":"Confirmed routing preference", "evidence":"User requested it"
            }, {
                "target_id":job_id.clone(), "content":"{\"enabled\":false}",
                "reason":"Disable idle fixture", "evidence":"User requested it"
            }, {
                "target_id":second_id.clone(), "content":"{\"timeout_seconds\":90}",
                "reason":"Bound second fixture", "evidence":"User requested it"
            }]}
        });
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", response),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let config_text = format!(
            "[learn]\nenabled = true\nweekly = false\n[[learn.target]]\nid = 'routing'\npath = '{}'\nkind = 'config-section'\nmax_bytes = 4096\n[[learn.target]]\nid = '{}'\npath = '{}'\nkind = 'cron-job'\nmax_bytes = 4096\n[[learn.target]]\nid = '{}'\npath = '{}'\nkind = 'cron-job'\nmax_bytes = 4096\n",
            target.display(), job_id, jobs.display(), second_id, jobs.display()
        );
        let config: crate::config::Config = toml::from_str(&config_text).unwrap();
        let reviewer_config = ReviewerConfig {
            executable: script,
            workdir: fixture.0.clone(),
            model: "haiku".into(),
            max_budget_usd: 0.1,
            timeout: Duration::from_secs(2),
            max_input_bytes: 128 * 1024,
            max_stdout_bytes: 8 * 1024,
            max_stderr_bytes: 1024,
        };
        let (events, mut receiver) = tokio::sync::mpsc::channel(4);
        let worker = LearnWorker::start_reviewing(
            &fixture.data(),
            &config.learn,
            reviewer_config,
            Some(events),
        )
        .unwrap();
        let trigger =
            ReviewTrigger::session_end(Provider::Claude, "session-route", &transcript).unwrap();
        assert_eq!(worker.submit(trigger), SubmitResult::Queued);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut route_seen = false;
        let mut cron_seen = false;
        loop {
            while let Ok(event) = receiver.try_recv() {
                route_seen |= matches!(event, crate::events::AppEvent::LearnRouteChanged);
                cron_seen |= matches!(event, crate::events::AppEvent::LearnCronChanged);
            }
            if route_seen && cron_seen {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Learn reload events timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(fs::read_to_string(&target)
            .unwrap()
            .contains("enabled = true"));
        scheduler.reload_jobs().unwrap();
        assert!(
            !scheduler
                .jobs()
                .iter()
                .find(|job| job.id == job_id)
                .unwrap()
                .enabled
        );
        assert_eq!(
            scheduler
                .jobs()
                .iter()
                .find(|job| job.id == second_id)
                .unwrap()
                .timeout_seconds,
            90
        );
        worker.shutdown();
    }

    #[cfg(unix)]
    #[test]
    fn weekly_curator_deduplicates_allowlisted_skill_without_model_or_transcript() {
        let fixture = Fixture::new();
        let first = fixture.0.join("skills/a/SKILL.md");
        let second = fixture.0.join("skills/b/SKILL.md");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        let original = b"---\nname: shared\ndescription: same\n---\nUse this.\n";
        fs::write(&first, original).unwrap();
        fs::write(&second, original).unwrap();
        let config_text = format!(
            "[learn]\nenabled = true\n[[learn.target]]\nid = \"first\"\npath = \"{}\"\nkind = \"skill\"\nmax_bytes = 4096\n[[learn.target]]\nid = \"second\"\npath = \"{}\"\nkind = \"skill\"\nmax_bytes = 4096\n",
            first.display(), second.display()
        );
        let config: crate::config::Config = toml::from_str(&config_text).unwrap();
        let reviewer_config = ReviewerConfig {
            executable: PathBuf::from("/usr/bin/true"),
            workdir: fixture.0.clone(),
            model: "haiku".into(),
            max_budget_usd: 0.1,
            timeout: Duration::from_secs(2),
            max_input_bytes: 128 * 1024,
            max_stdout_bytes: 8 * 1024,
            max_stderr_bytes: 1024,
        };
        let worker =
            LearnWorker::start_reviewing(&fixture.data(), &config.learn, reviewer_config, None)
                .unwrap();
        let key = format!("curation:weekly:{}", week_start(OffsetDateTime::now_utc()));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let status = Storage::open(&fixture.data())
                .unwrap()
                .trigger_status(&key)
                .unwrap();
            if status == Some(TriggerStatus::Succeeded) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "weekly curation timed out: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        worker.shutdown();
        assert_eq!(fs::read(&first).unwrap(), original);
        assert!(fs::read_to_string(&second)
            .unwrap()
            .contains("Archived skill"));
        let storage = Storage::open(&fixture.data()).unwrap();
        let records = storage.list_recent(10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].target_id, "second");
        let mut changes = Changes::new(storage);
        changes.undo(&records[0].id).unwrap();
        assert_eq!(fs::read(&second).unwrap(), original);
    }
}
