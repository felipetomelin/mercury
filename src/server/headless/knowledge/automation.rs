use super::*;
#[cfg(test)]
mod tests;
use mercury_knowledge::automation::{BudgetedModel, Capture};
use std::{
    fs::File,
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};

pub(crate) struct AutomationWorker {
    sender: Option<SyncSender<crate::app::learn::ReviewSession>>,
    cancel: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl AutomationWorker {
    pub(crate) fn start(
        config: Config,
        route: mercury_router::RouteConfig,
        limits: Arc<Mutex<Vec<mercury_usage::ProviderLimits>>>,
    ) -> Result<Self> {
        Self::start_shared(root(), config, route, limits)
    }
    #[cfg(test)]
    fn start_at(
        root: std::path::PathBuf,
        config: Config,
        route: mercury_router::RouteConfig,
        limits: Vec<mercury_usage::ProviderLimits>,
    ) -> Result<Self> {
        Self::start_shared(root, config, route, Arc::new(Mutex::new(limits)))
    }
    fn start_shared(
        root: std::path::PathBuf,
        config: Config,
        route: mercury_router::RouteConfig,
        limits: Arc<Mutex<Vec<mercury_usage::ProviderLimits>>>,
    ) -> Result<Self> {
        config.validate()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(64);
        let handle = std::thread::Builder::new()
            .name("mercury-knowledge-auto".into())
            .spawn(move || {
                if let Err(error) = run(&root, config, route, limits, &gate, receiver) {
                    tracing::warn!(%error, "Knowledge automation unavailable");
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            cancel,
            handle: Some(handle),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_channel() -> (Self, mpsc::Receiver<crate::app::learn::ReviewSession>) {
        let (sender, receiver) = mpsc::sync_channel(8);
        (
            Self {
                sender: Some(sender),
                cancel: Arc::new(AtomicBool::new(false)),
                handle: None,
            },
            receiver,
        )
    }
    pub(crate) fn submit(&self, session: crate::app::learn::ReviewSession) -> bool {
        !self.cancel.load(Ordering::Acquire)
            && self
                .sender
                .as_ref()
                .is_some_and(|s| s.try_send(session).is_ok())
    }
    pub(crate) fn stop(&mut self, timeout: Duration) -> bool {
        self.cancel.store(true, Ordering::Release);
        self.sender.take();
        let deadline = Instant::now() + timeout;
        while self.handle.as_ref().is_some_and(|h| !h.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.handle.as_ref().is_some_and(|h| h.is_finished()) {
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
        self.handle.is_none()
    }
}
impl Drop for AutomationWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.sender.take();
    }
}

#[cfg(unix)]
fn lease(root: &Path) -> Result<File> {
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("automation.lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(Error::UnsafePath);
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(Error::Unavailable);
    }
    Ok(file)
}
#[cfg(not(unix))]
fn lease(_: &Path) -> Result<File> {
    Err(Error::Unavailable)
}

fn enqueue(root: &Path, session: crate::app::learn::ReviewSession, config: &Config) -> Result<()> {
    if !config.automation.capture {
        return Ok(());
    }
    let provider = match session.provider {
        mercury_learn::transcript::Provider::Claude => "claude",
        mercury_learn::transcript::Provider::Codex => "codex",
    };
    let fingerprint = mercury_learn::storage::transcript_fingerprint(&session.transcript_path)
        .map_err(|_| Error::Conflict)?;
    // Validate identity and complete transcript before recording trusted completion.
    mercury_learn::transcript::read(
        &session.transcript_path,
        session.provider,
        &session.session_id,
        Default::default(),
    )
    .map_err(|_| Error::Conflict)?;
    let mut capture = Capture::new(
        provider,
        &session.session_id,
        session.transcript_path.clone(),
        fingerprint.clone(),
    )?;
    capture.next_at = mercury_knowledge::now() + config.automation.debounce_secs;
    let index = Index::open(root)?;
    index.enqueue_capture(&capture)?;
    let mut storage = mercury_learn::storage::Storage::open(
        &root.parent().ok_or(Error::UnsafePath)?.join("learn"),
    )
    .map_err(|_| Error::Unavailable)?;
    storage
        .record_completed_session(
            session.provider,
            &session.session_id,
            &session.transcript_path,
            &fingerprint,
        )
        .map_err(|_| Error::Unavailable)?;
    Ok(())
}

fn run(
    root: &Path,
    config: Config,
    route: mercury_router::RouteConfig,
    limits: Arc<Mutex<Vec<mercury_usage::ProviderLimits>>>,
    cancel: &AtomicBool,
    receiver: mpsc::Receiver<crate::app::learn::ReviewSession>,
) -> Result<()> {
    let config = config.prepare_at(root)?;
    let index = Index::open(root)?;
    index.automation_setup()?;
    // A file lease also serializes generations across reload and process handoff.
    let _lease = loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        match lease(root) {
            Ok(file) => break file,
            Err(Error::Unavailable) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error),
        }
    };
    let mut next_poll = Instant::now();
    let mut pending_revision: Option<String> = None;
    while !cancel.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(session) => {
                if !cancel.load(Ordering::Acquire) {
                    if let Err(error) = enqueue(root, session, &config) {
                        let _=index.save_auto_state("runtime",&json!({"state":"error","error":error.to_string(),"updated":mercury_knowledge::now()}));
                    }
                    next_poll = next_poll
                        .min(Instant::now() + Duration::from_secs(config.automation.debounce_secs));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() < next_poll {
            continue;
        }
        let Ok(_serial) = SERIAL.try_lock() else {
            next_poll = Instant::now() + Duration::from_secs(1);
            continue;
        };
        next_poll = Instant::now() + Duration::from_secs(config.automation.poll_secs);
        let result = (|| {
            let mut index = Index::open(root)?;
            let scan = mercury_knowledge::scan(&config, cancel)?;
            let current = index.status()?.revision;
            if scan.revision != current {
                if pending_revision.as_ref() != Some(&scan.revision) {
                    pending_revision = Some(scan.revision);
                    next_poll =
                        Instant::now() + Duration::from_secs(config.automation.debounce_secs);
                    return Ok(());
                }
                index.replace(scan, cancel)?;
                pending_revision = None;
            }
            let pending = index.pending_captures(mercury_knowledge::now())?;
            let state = index.status()?;
            let graph = index.auto_state("graph")?;
            let signature = mercury_knowledge::hash(
                serde_json::to_string(&json!({"config":config,"route":format!("{route:?}")}))?
                    .as_bytes(),
            );
            let graph_context_changed = graph["revision"].as_str() != Some(&state.revision)
                || graph["config"].as_str() != Some(&signature);
            let graph_due = config.automation.graph
                && state.notes > 0
                && (state.model.is_none() || graph["config"].as_str() != Some(&signature))
                && (graph_context_changed
                    || graph["next_at"].as_u64().unwrap_or(0) <= mercury_knowledge::now())
                && (graph_context_changed
                    || graph["attempts"].as_u64().unwrap_or(0)
                        < config.automation.max_attempts as u64);
            if !config.allow_external || config.allowed_providers.is_empty() {
                index.save_auto_state("runtime",&json!({"state":"awaiting_external_configuration","updated":mercury_knowledge::now()}))?;
                return Ok(());
            }
            if !pending.is_empty() || graph_due {
                let latest_limits = limits.lock().map_err(|_| Error::Unavailable)?.clone();
                let model = match NativeModel::new(
                    &config,
                    &root.join("model"),
                    &route,
                    &latest_limits,
                    None,
                    cancel,
                ) {
                    Ok(model) => model,
                    Err(error) => {
                        for entry in pending.iter().take(8) {
                            fail_capture(&index, entry.clone(), &error, &config)?;
                        }
                        if graph_due {
                            let attempts = if graph_context_changed {
                                0
                            } else {
                                graph["attempts"].as_u64().unwrap_or(0) as u32
                            };
                            let (attempts, next_at) = retry(&error, attempts, &config);
                            index.save_auto_state("graph",&json!({"state":if attempts>=config.automation.max_attempts{"failed"}else{"retry"},"revision":state.revision,"attempts":attempts,"next_at":next_at,"error":error.to_string(),"config":signature}))?;
                        }
                        return Err(error);
                    }
                };
                let budget_index = Index::open(root)?;
                let budget = BudgetedModel {
                    model: &model,
                    index: &budget_index,
                    daily_limit: config.automation.daily_model_calls,
                };
                process_captures(&mut index, &config, &pending, &budget, cancel)?;
                // Managed publication is complete and journaled: index it immediately.
                index.sync(&config, cancel)?;
                if (graph_due || index.status()?.revision != state.revision)
                    && config.automation.graph
                    && index.status()?.notes > 0
                    && (index.status()?.model.is_none()
                        || graph["config"].as_str() != Some(&signature))
                {
                    let revision = index.status()?.revision;
                    let attempts = if graph["revision"].as_str() == Some(&revision) {
                        graph["attempts"].as_u64().unwrap_or(0) as u32
                    } else {
                        0
                    };
                    if let Err(error) = index.build_graph_verified(&budget, cancel, &config) {
                        if cancel.load(Ordering::Acquire) {
                            return Err(Error::Cancelled);
                        }
                        let (attempts, next_at) = retry(&error, attempts, &config);
                        index.save_auto_state("graph",&json!({"state":if attempts>=config.automation.max_attempts{"failed"}else{"retry"},"revision":revision,"attempts":attempts,"next_at":next_at,"error":error.to_string(),"config":signature}))?;
                        return Err(error);
                    }
                    index.save_auto_state("graph",&json!({"state":"current","revision":revision,"attempts":0,"updated":mercury_knowledge::now(),"config":signature}))?;
                }
            }
            index.save_auto_state(
                "runtime",
                &json!({"state":"watching","updated":mercury_knowledge::now()}),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            if !cancel.load(Ordering::Acquire) {
                let _=index.save_auto_state("runtime",&json!({"state":"error","error":error.to_string(),"updated":mercury_knowledge::now()}));
                next_poll = Instant::now() + Duration::from_secs(config.automation.retry_secs);
            }
        }
    }
    Ok(())
}

fn retry(error: &Error, attempts: u32, config: &Config) -> (u32, u64) {
    let now = mercury_knowledge::now();
    if matches!(
        error,
        Error::Invalid("automatic daily model budget exhausted")
    ) {
        (attempts, (now / 86400 + 1) * 86400 + 1)
    } else {
        (
            attempts + 1,
            now + config.automation.retry_secs * (1u64 << attempts.min(6)),
        )
    }
}

fn fail_capture(index: &Index, mut capture: Capture, error: &Error, config: &Config) -> Result<()> {
    let (attempts, next_at) = retry(error, capture.attempts, config);
    capture.attempts = attempts;
    capture.next_at = next_at;
    capture.error = Some(error.to_string());
    if attempts >= config.automation.max_attempts
        || matches!(
            error,
            Error::Conflict
                | Error::Invalid("automatic model input contains a recognizable secret")
                | Error::Invalid("automatic model output contains a recognizable secret")
        )
    {
        capture.state = "failed".into();
    }
    index.save_capture(&capture)
}

fn process_captures(
    index: &mut Index,
    config: &Config,
    pending: &[Capture],
    model: &dyn Model,
    cancel: &AtomicBool,
) -> Result<()> {
    for entry in pending.iter().take(8) {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let mut capture = entry.clone();
        let result = (|| {
            let source = format!(
                "auto:observed:{}:{}:{}",
                capture.provider, capture.session_id, capture.fingerprint
            );
            let proposal = match index.proposal_by_source(&source)? {
                Some(proposal) => proposal,
                None => {
                    let mut params = KnowledgeParams::new(Action::Capture);
                    params.provider = capture.provider.clone();
                    params.id = capture.session_id.clone();
                    params.title = format!("Aprendizado {} {}", capture.provider, &capture.id[..8]);
                    capture_transcript(
                        index,
                        &params,
                        &capture.transcript,
                        &capture.fingerprint,
                        &source,
                        model,
                        cancel,
                        true,
                    )?
                }
            };
            capture.proposal_id = Some(proposal.id.clone());
            index.save_capture(&capture)?;
            if cancel.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if ["undone", "rejected"].contains(&proposal.state.as_str()) {
                capture.state = "done".into();
                index.save_capture(&capture)?;
                return Ok(());
            }
            if config.automation.publish {
                if mercury_learn::storage::transcript_fingerprint(&capture.transcript)
                    .map_err(|_| Error::Conflict)?
                    != capture.fingerprint
                {
                    return Err(Error::Conflict);
                }
                index.review(config, &proposal.id, "accept")?;
                capture.state = "published".into();
                index.save_capture(&capture)?;
                index.sync(config, cancel)?;
            }
            capture.state = "done".into();
            capture.error = None;
            index.save_capture(&capture)?;
            Ok(())
        })();
        if let Err(error) = result {
            if cancel.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            fail_capture(index, capture, &error, config)?;
        }
    }
    Ok(())
}

impl crate::app::App {
    pub(crate) fn refresh_automation_limits(&self) {
        if let Ok(mut limits) = self.automation_limits.lock() {
            *limits = self.usage.route_limits();
        }
    }
    pub(crate) fn enable_knowledge_automation(&mut self) -> std::result::Result<(), String> {
        if !self.policy.background_updates
            || !self.knowledge_config.enabled
            || !self.knowledge_config.automation.enabled
            || self.knowledge_automation.is_some()
        {
            return Ok(());
        }
        self.knowledge_automation = Some(
            AutomationWorker::start(
                self.knowledge_config.clone(),
                self.route.clone(),
                self.automation_limits.clone(),
            )
            .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    pub(crate) fn capture_completed_session(
        &self,
        trigger: &crate::app::learn::ReviewTrigger,
    ) -> bool {
        let Some(session) = trigger.session() else {
            return false;
        };
        if !trigger.key().ends_with(":end") {
            return false;
        }
        self.knowledge_automation
            .as_ref()
            .is_some_and(|worker| worker.submit(session.clone()))
    }
}
