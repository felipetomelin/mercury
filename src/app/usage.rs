//! Per-pane usage tracking: Claude/Codex token totals, context fill and
//! (when the agent reports it) account-level limits, for the sidebar gauges
//! and the Usage view (see `docs/dev/usage-ui-design.md`).
//!
//! Follows the `git_refresh.rs` pattern: the main loop never touches a
//! transcript/rollout file directly. `start_usage_refresh_if_due` spawns a
//! background thread that reads whatever changed and reports back through
//! `AppEvent::UsageRefreshed`; `scripts/test_ui_hot_path_architecture.py`
//! enforces that no file I/O happens on the render path.
//!
//! Pane -> session resolution never scrapes the screen (D2): it only uses the
//! session id the agent's own hooks already reported
//! (`TerminalState.persisted_agent_session`) plus the pane's cwd, fed into
//! `mercury_usage::locate`. A pane without a hook-reported session simply has
//! no tracked usage ("n/a"), never an invented number.
//!
//! The statusline bridge (D10, `usage.report_statusline`) is the exact source
//! for context fill, session cost and Claude's account windows: its snapshot
//! is kept per terminal and overlaid onto the transcript-derived usage of the
//! same session after every refresh (D8 hierarchy). Cost deltas go to the
//! append-only ledger (D9, `src/app/usage_ledger.rs`), which is replayed at
//! startup so USD budgets over past windows survive a restart.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mercury_usage::{
    claude, locate, ClaudeTranscript, CodexRollout, CostLedgerEntry, CostObservation, JsonlTail,
    Provider, StatuslineSnapshot,
};
use time::OffsetDateTime;

use super::usage_ledger::CostLedgerHandle;
use super::{App, AppState};
use crate::agent_resume::AgentSessionRefKind;
use crate::config::UsageConfig;
use crate::detect::AgentState;
use crate::events::{AppEvent, TerminalUsageUpdate};
use crate::layout::PaneId;
use crate::protocol;
use crate::terminal::{TerminalId, TerminalState};

/// At most one applied update per second per session (D4), independent of how
/// often the underlying file actually changes.
const APPLY_THROTTLE: Duration = Duration::from_secs(1);
/// How many day directories back `locate::codex_rollout_path` looks before
/// giving up. Fixed until `[usage]` config exists (implementation step 5 of
/// Fase 2); see `docs/dev/usage-ui-design.md` D7/D9.
const CODEX_ROLLOUT_MAX_DAYS: usize = 35;
/// How far back ledger cost entries are kept in memory for budget evaluation
/// (D9: "limitado a 35 dias"); longer rolling budget windows extend it.
const COST_LEDGER_MIN_HORIZON: time::Duration = time::Duration::days(35);
const CODEX_ACTIVE_REFRESH: Duration = Duration::from_secs(60);
const CODEX_IDLE_REFRESH: Duration = Duration::from_secs(5 * 60);

/// What a tracked pane resolved to: the hook-reported agent/session id plus
/// the pane's cwd, everything the background thread needs to find the file.
#[derive(Clone)]
struct TrackedSession {
    agent: String,
    session_id: String,
    cwd: PathBuf,
}

#[derive(Clone, Default)]
struct ClaudeSessionTail {
    transcript_path: PathBuf,
    main_tail: JsonlTail,
    main: ClaudeTranscript,
    subagents: HashMap<PathBuf, (JsonlTail, ClaudeTranscript)>,
}

#[derive(Clone, Default)]
struct CodexSessionTail {
    rollout_path: PathBuf,
    tail: JsonlTail,
    rollout: CodexRollout,
}

#[derive(Clone)]
enum SessionTail {
    Claude(ClaudeSessionTail),
    Codex(CodexSessionTail),
}

#[derive(Default)]
struct AccountProbe {
    status: String,
    detail: String,
    failures: u32,
    identity: Option<String>,
    in_flight: bool,
}

fn probe_result(probe: &mut AccountProbe, error: Option<&str>, has_sample: bool) -> Duration {
    probe.in_flight = false;
    if let Some(error) = error {
        probe.failures = probe.failures.saturating_add(1);
        let lower = error.to_ascii_lowercase();
        let message = if lower.contains("login") || lower.contains("auth") {
            "Sem login · conecte a conta no CLI"
        } else if lower.contains("no such file") || lower.contains("os error 2") {
            "CLI ausente · confira a instalação"
        } else if lower.contains("versão")
            || lower.contains("unsupported")
            || lower.contains("method not found")
        {
            "Versão incompatível · atualize o CLI"
        } else if lower.contains("alterada") || lower.contains("identidade") {
            "Conta alterada · consulte novamente"
        } else if lower.contains("desatualizada") {
            "Fonte desatualizada · tente consultar novamente"
        } else if lower.contains("timed out")
            || lower.contains("expirou")
            || lower.contains("indisponível")
        {
            "Consulta expirou · tente atualizar"
        } else {
            "Falha na consulta · tente atualizar"
        };
        probe.status = if has_sample {
            "desatualizado"
        } else {
            "indisponível"
        }
        .into();
        probe.detail = message.into();
        Duration::from_secs(
            (60u64.saturating_mul(1u64 << probe.failures.saturating_sub(1).min(5))).min(1800),
        )
    } else {
        probe.failures = 0;
        probe.status = "atualizado".into();
        probe.detail = "Conta conectada · perfil atual do CLI".into();
        CODEX_IDLE_REFRESH
    }
}

/// Lives on `App` (D1). Owns the last known usage per terminal (for the
/// sidebar gauge) plus the incremental file readers, which persist across
/// polls so a long-running session is never re-read from the start.
pub(crate) struct UsageService {
    sessions: HashMap<TerminalId, mercury_usage::SessionUsage>,
    limits: BTreeMap<Provider, mercury_usage::ProviderLimits>,
    tails: Arc<Mutex<HashMap<TerminalId, SessionTail>>>,
    next_poll_at: HashMap<TerminalId, Instant>,
    last_applied_at: HashMap<TerminalId, Instant>,
    claude_home: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    in_flight: bool,
    /// `[usage].poll_active_ms`/`poll_idle_ms` (D3); replaces what used to be
    /// hardcoded `POLL_ACTIVE_INTERVAL`/`POLL_IDLE_INTERVAL` constants.
    poll_active: Duration,
    poll_idle: Duration,
    /// Resolved `[[usage.budget]]` entries (invalid entries are dropped at
    /// config-parse time, see `UsageConfig::resolved_budgets`; their
    /// diagnostics surface through `Config::collect_diagnostics`, not here).
    budgets: Vec<mercury_usage::Budget>,
    /// `[usage].enabled`. When false nothing is polled, reported or recorded.
    enabled: bool,
    /// Latest statusline snapshot per terminal (D10), overlaid onto that
    /// terminal's transcript-derived usage when the session ids match.
    statuslines: HashMap<TerminalId, StatuslineSnapshot>,
    /// App-server totals win over rollout data only for the matching thread.
    codex_live_costs: HashMap<TerminalId, (String, f64)>,
    codex_live_limits: Option<mercury_usage::ProviderLimits>,
    codex_binary: PathBuf,
    codex_next_refresh_at: Option<Instant>,
    codex_last_threads: Vec<String>,
    codex_was_working: bool,
    codex_refresh_in_flight: bool,
    account_probes: BTreeMap<Provider, AccountProbe>,
    claude_refresh_in_flight: bool,
    claude_probe_requested: bool,
    claude_next_refresh_at: Option<Instant>,
    account_manual_refresh_at: Option<Instant>,

    history: Vec<mercury_usage::history_scan::HistoricalSession>,
    history_scan_started: bool,
    /// Ledger cost entries inside the budget horizon (D9): replayed at
    /// startup plus every new delta, fed to budgets as cost-only records.
    cost_entries: Vec<CostLedgerEntry>,
    /// Where the ledger lives and how to report back, once the app allows a
    /// persistent ledger (`App::new` with a persisting policy).
    cost_ledger_target: Option<(PathBuf, tokio::sync::mpsc::Sender<AppEvent>)>,
    cost_ledger: Option<CostLedgerHandle>,
}

impl UsageService {
    pub(crate) fn route_limits(&self) -> Vec<mercury_usage::ProviderLimits> {
        self.limits.values().cloned().collect()
    }

    pub(crate) fn route_codex_refresh_needed(&self, ttl_seconds: i64, now: OffsetDateTime) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(limits) = self.limits.get(&Provider::Codex) else {
            return true;
        };
        let Some(observed_at) = limits.observed_at else {
            return true;
        };
        let age = now - observed_at;
        age.is_negative()
            || age > time::Duration::seconds(ttl_seconds)
            || limits
                .limits
                .iter()
                .any(|limit| limit.resets_at.is_some_and(|reset| reset <= now))
    }

    pub(crate) fn route_codex_binary(&self) -> PathBuf {
        self.codex_binary.clone()
    }

    #[cfg(test)]
    pub(crate) fn set_route_codex_binary_for_test(&mut self, binary: PathBuf) {
        self.codex_binary = binary;
    }

    pub(crate) fn apply_route_codex_limits(
        &mut self,
        limits: mercury_usage::ProviderLimits,
    ) -> bool {
        if !self.enabled || limits.provider != Provider::Codex {
            return false;
        }
        if self
            .limits
            .get(&Provider::Codex)
            .and_then(|current| current.observed_at)
            .zip(limits.observed_at)
            .is_some_and(|(current, received)| current > received)
        {
            return false;
        }
        let changed = self.limits.get(&Provider::Codex) != Some(&limits);
        self.limits.insert(Provider::Codex, limits.clone());
        self.codex_live_limits = Some(limits);
        changed
    }

    pub(crate) fn route_budgets(&self) -> &[mercury_usage::Budget] {
        if self.enabled {
            &self.budgets
        } else {
            &[]
        }
    }
}

impl UsageService {
    pub(crate) fn new(config: &UsageConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            limits: BTreeMap::new(),
            tails: Arc::new(Mutex::new(HashMap::new())),
            next_poll_at: HashMap::new(),
            last_applied_at: HashMap::new(),
            claude_home: locate::default_claude_home(),
            codex_home: locate::default_codex_home(),
            in_flight: false,
            poll_active: config.poll_active(),
            poll_idle: config.poll_idle(),
            budgets: config.resolved_budgets().0,
            enabled: config.enabled,
            statuslines: HashMap::new(),
            codex_live_costs: HashMap::new(),
            codex_live_limits: None,
            codex_binary: PathBuf::from("codex"),
            codex_next_refresh_at: None,
            codex_last_threads: Vec::new(),
            codex_was_working: false,
            codex_refresh_in_flight: false,
            account_probes: BTreeMap::new(),
            claude_refresh_in_flight: false,
            claude_probe_requested: false,
            claude_next_refresh_at: None,
            account_manual_refresh_at: None,

            history: Vec::new(),
            history_scan_started: false,
            cost_entries: Vec::new(),
            cost_ledger_target: None,
            cost_ledger: None,
        }
    }

    /// Allows this service to own the persistent cost ledger at `path`
    /// (`None`: no home directory, nothing is persisted). Started right away
    /// when usage is enabled, or later by `apply_config` if it gets enabled.
    pub(crate) fn enable_cost_ledger(
        &mut self,
        path: Option<PathBuf>,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    ) {
        let Some(path) = path else {
            return;
        };
        self.cost_ledger_target = Some((path, event_tx));
        self.start_cost_ledger_if_allowed();
    }

    fn start_cost_ledger_if_allowed(&mut self) {
        if !self.enabled || self.cost_ledger.is_some() {
            return;
        }
        let Some((path, event_tx)) = self.cost_ledger_target.clone() else {
            return;
        };
        let horizon_start = OffsetDateTime::now_utc() - self.cost_horizon();
        self.cost_ledger = Some(CostLedgerHandle::spawn(path, horizon_start, event_tx));
    }

    /// The longest span any configured budget can look back over, at least
    /// [`COST_LEDGER_MIN_HORIZON`] (covers every calendar period).
    fn cost_horizon(&self) -> time::Duration {
        self.budgets
            .iter()
            .filter_map(|budget| match budget.window {
                mercury_usage::UsageWindow::Rolling { minutes } => {
                    Some(time::Duration::minutes(i64::from(minutes)))
                }
                _ => None,
            })
            .fold(COST_LEDGER_MIN_HORIZON, |longest, span| longest.max(span))
    }

    /// Applies a live `[usage]` reload (poll cadence + budgets); called from
    /// `App::apply_live_config` the same way every other section is.
    pub(crate) fn apply_config(&mut self, config: &UsageConfig) {
        self.poll_active = config.poll_active();
        self.poll_idle = config.poll_idle();
        self.budgets = config.resolved_budgets().0;
        self.enabled = config.enabled;
        if !self.enabled {
            // Disabling hides everything already shown; nothing is polled or
            // recorded until it is enabled again (the ledger thread, if
            // running, simply receives no more observations).
            self.sessions.clear();
            self.limits.clear();
            self.statuslines.clear();
            self.codex_live_costs.clear();
            self.codex_live_limits = None;
            self.codex_next_refresh_at = None;
            self.codex_last_threads.clear();
            self.codex_was_working = false;
            self.history.clear();
            self.history_scan_started = false;
            self.next_poll_at.clear();
        }
        self.start_cost_ledger_if_allowed();
    }

    pub(crate) fn poll_interval(&self, state: AgentState) -> Duration {
        match state {
            AgentState::Working => self.poll_active,
            AgentState::Idle | AgentState::Blocked | AgentState::Unknown => self.poll_idle,
        }
    }

    /// Earliest moment a configured calendar budget resets as a whole (D3/D7
    /// passo 5): scheduled into the same adaptive-polling wakeup as token
    /// polling so the Usage view recomputes right at the boundary instead of
    /// waiting for the next unrelated event.
    fn next_budget_reset_deadline(&self, now: Instant) -> Option<Instant> {
        if self.budgets.is_empty() {
            return None;
        }
        let now_time = OffsetDateTime::now_utc();
        self.budgets
            .iter()
            .filter_map(|budget| budget.window.next_reset(now_time))
            .filter(|reset| *reset > now_time)
            .filter_map(|reset| {
                let seconds = (reset - now_time).as_seconds_f64();
                (seconds.is_finite() && seconds > 0.0)
                    .then(|| now + Duration::from_secs_f64(seconds))
            })
            .min()
    }

    /// Wire projection of one pane's usage (`ClientShellAgent.usage`); `None`
    /// means "n/a", never a fabricated number.
    pub(crate) fn agent_usage_wire(
        &self,
        terminal_id: &TerminalId,
    ) -> Option<protocol::ClientShellAgentUsage> {
        if !self.enabled {
            return None;
        }
        self.sessions.get(terminal_id).map(session_usage_to_wire)
    }

    /// Wire projection of account-level usage (`ClientShellSnapshot.usage`);
    /// `None` while nothing has been observed yet and no budget is configured
    /// (a configured budget is worth showing even before it has any usage).
    /// `records` are the in-scope `UsageRecord`s budgets evaluate against —
    /// built from tracked sessions only (see `App::usage_summary_wire`; the
    /// historical scan of panes not currently attached is deferred, D9).
    pub(crate) fn summary_wire(
        &self,
        records: &[mercury_usage::UsageRecord],
    ) -> Option<protocol::ClientShellUsageSummary> {
        if !self.enabled {
            return None;
        }
        let now = OffsetDateTime::now_utc();
        let limits: Vec<mercury_usage::ProviderLimits> = self.limits.values().cloned().collect();
        let budgets = self
            .budgets
            .iter()
            .map(|budget| {
                let status = budget.evaluate(records, &limits, now);
                budget_status_to_wire(budget, &status)
            })
            .collect();
        let providers = [Provider::Claude, Provider::Codex]
            .into_iter()
            .map(|provider| {
                let mut wire = self.limits.get(&provider).map_or_else(
                    || protocol::ClientShellProviderUsage {
                        status: "consultando".into(),
                        detail: "Aguardando consulta da conta".into(),
                        observed_at_unix_ms: None,
                        provider: provider.as_str().to_owned(),
                        limits: Vec::new(),
                        credits: None,
                        plan_type: None,
                    },
                    provider_limits_to_wire,
                );
                if let Some(probe) = self.account_probes.get(&provider) {
                    wire.status = probe.status.clone();
                    wire.detail = probe.detail.clone();
                    if probe.in_flight {
                        wire.detail.push_str(" · atualizando…");
                    }
                }
                if wire.status == "atualizado"
                    && (wire.observed_at_unix_ms.is_some_and(|at| {
                        offsetdatetime_to_unix_ms(now).saturating_sub(at) > 6 * 60 * 1000
                    }) || wire.limits.iter().any(|limit| {
                        limit
                            .resets_at_unix_ms
                            .is_some_and(|at| at <= offsetdatetime_to_unix_ms(now))
                    }))
                {
                    wire.status = "desatualizado".into();
                    wire.detail = "Aguardando amostra atual da conta · r atualizar".into();
                }
                wire
            })
            .collect();
        Some(protocol::ClientShellUsageSummary {
            providers,
            budgets,
            activity: Some(activity_wire(records, now)),
        })
    }
}

fn activity_wire(
    records: &[mercury_usage::UsageRecord],
    now: OffsetDateTime,
) -> protocol::ClientShellUsageActivity {
    let today = now.unix_timestamp().div_euclid(86400);
    let mut activity = protocol::ClientShellUsageActivity {
        daily_tokens: vec![0; 7],
        ..Default::default()
    };
    let mut last: BTreeMap<Provider, i64> = BTreeMap::new();
    for record in records {
        let day = record.at.unix_timestamp().div_euclid(86400);
        let age = today - day;
        if record.at > now {
            continue;
        }
        if record.tokens.total() > 0 {
            last.entry(record.provider)
                .and_modify(|t| *t = (*t).max(record.at.unix_timestamp()))
                .or_insert(record.at.unix_timestamp());
        }
        if (0..7).contains(&age) {
            let slot = &mut activity.daily_tokens[6 - age as usize];
            *slot = slot.saturating_add(record.tokens.total());
        }
        if age == 0 {
            activity.tokens_today = activity.tokens_today.saturating_add(record.tokens.total());
            activity.input_today = activity.input_today.saturating_add(record.tokens.prompt());
            activity.cache_read_today = activity
                .cache_read_today
                .saturating_add(record.tokens.cache_read);
            if let Some(cost) = record.cost_usd.filter(|v| v.is_finite() && *v >= 0.0) {
                let amount = (cost * 1_000_000.0).round().clamp(0.0, u64::MAX as f64) as u64;
                activity.cost_microusd_today = Some(
                    activity
                        .cost_microusd_today
                        .unwrap_or(0)
                        .saturating_add(amount),
                );
            }
        }
    }
    activity.last_activity = last
        .into_iter()
        .map(|(p, t)| (p.as_str().into(), t.saturating_mul(1000)))
        .collect();
    activity
}

fn offsetdatetime_to_unix_ms(at: OffsetDateTime) -> i64 {
    (at.unix_timestamp_nanos() / 1_000_000) as i64
}

/// Scales a `BudgetStatus` amount (USD, a raw token count, or a provider
/// percent) into the wire's integer units (D4: the snapshot derives `Eq`).
fn scale_budget_amount(value: f64, unit: protocol::ClientShellBudgetUnit) -> u64 {
    let scaled = match unit {
        protocol::ClientShellBudgetUnit::Usd => value * 1_000_000.0,
        protocol::ClientShellBudgetUnit::Tokens => value,
        protocol::ClientShellBudgetUnit::Percent => value * 10.0,
        protocol::ClientShellBudgetUnit::Unknown => value,
    };
    scaled.round().clamp(0.0, u64::MAX as f64) as u64
}

fn budget_scope_label(scope: &mercury_usage::Scope) -> String {
    match scope {
        mercury_usage::Scope::Global => "global".to_string(),
        mercury_usage::Scope::Provider(provider) => format!("provider: {}", provider.as_str()),
        mercury_usage::Scope::Workspace(workspace) => format!("workspace: {workspace}"),
        mercury_usage::Scope::Agent(agent) => format!("agent: {agent}"),
    }
}

fn budget_status_to_wire(
    budget: &mercury_usage::Budget,
    status: &mercury_usage::BudgetStatus,
) -> protocol::ClientShellUsageBudget {
    let unit = match &budget.limit {
        mercury_usage::Limit::Usd(_) => protocol::ClientShellBudgetUnit::Usd,
        mercury_usage::Limit::Tokens { .. } => protocol::ClientShellBudgetUnit::Tokens,
        mercury_usage::Limit::Percent(_) => protocol::ClientShellBudgetUnit::Percent,
    };
    let level = match status.level {
        mercury_usage::BudgetLevel::Ok => protocol::ClientShellBudgetLevel::Ok,
        mercury_usage::BudgetLevel::Warn => protocol::ClientShellBudgetLevel::Warn,
        mercury_usage::BudgetLevel::Exceeded => protocol::ClientShellBudgetLevel::Exceeded,
        mercury_usage::BudgetLevel::Unavailable => protocol::ClientShellBudgetLevel::Unavailable,
    };
    protocol::ClientShellUsageBudget {
        name: status.name.clone(),
        scope: budget_scope_label(&budget.scope),
        level,
        used_permille: status
            .fraction
            .map(|fraction| (fraction * 1000.0).round().clamp(0.0, 10_000.0) as u16),
        used_scaled: scale_budget_amount(status.used, unit),
        limit_scaled: scale_budget_amount(status.limit, unit),
        unit,
        warn_threshold: status.warn_threshold,
        resets_at_unix_ms: status.resets_at.map(offsetdatetime_to_unix_ms),
        forecast_exhausted_at_unix_ms: status.forecast_exhausted_at.map(offsetdatetime_to_unix_ms),
        partial: status.partial,
    }
}

/// Builds the `UsageRecord`s every configured budget evaluates against, from
/// the events of every currently tracked session (D2: only panes with a
/// hook-reported session are tracked; nothing is fabricated for the rest).
/// `agent` is the pane's custom agent name (`terminal.agent_name`, e.g.
/// "reviewer") and `pane` is the terminal id, so `Scope::Agent` can match
/// either a named agent or a specific pane (per `mercury_usage::budget::Scope::matches`).
/// Token records never carry cost (transcripts have none); cost comes only
/// from the ledger (`cost_entries`, D9) as separate cost-only records of the
/// same session, so a USD budget counts reported spend — including spend from
/// before a restart — and stays `partial` for sessions with no cost source.
fn collect_usage_records(
    tails: &HashMap<TerminalId, SessionTail>,
    state: &AppState,
    sessions: &HashMap<TerminalId, mercury_usage::SessionUsage>,
    cost_entries: &[CostLedgerEntry],
    history: &[mercury_usage::history_scan::HistoricalSession],
) -> Vec<mercury_usage::UsageRecord> {
    let mut records = Vec::new();
    let active: HashSet<(Provider, String)> = tails
        .keys()
        .filter_map(|terminal_id| {
            let tracked = state
                .terminals
                .get(terminal_id)
                .and_then(tracked_session_for)?;
            let provider = match tracked.agent.as_str() {
                "claude" => Provider::Claude,
                "codex" => Provider::Codex,
                _ => return None,
            };
            Some((provider, tracked.session_id))
        })
        .collect();
    for session in history {
        if active.contains(&(session.provider, session.session_id.clone())) {
            continue;
        }
        let workspace = session.cwd.as_ref().and_then(|cwd| {
            let mut matches = state
                .workspaces
                .iter()
                .filter(|workspace| workspace.identity_cwd == *cwd);
            let match_one = matches.next()?;
            matches
                .next()
                .is_none()
                .then(|| match_one.display_name_from_terminals(&state.terminals))
        });
        for event in &session.events {
            records.push(mercury_usage::UsageRecord {
                at: event.at,
                provider: session.provider,
                session_id: session.session_id.clone(),
                workspace: workspace.clone(),
                agent: None,
                pane: None,
                tokens: event.tokens,
                cost_usd: None,
            });
        }
    }
    // A ledger entry belongs to whichever pane currently runs its session
    // (lets `Scope::Agent` match a pane id); past sessions have no pane.
    let pane_by_session: HashMap<(Provider, &str), &TerminalId> = sessions
        .iter()
        .map(|(terminal_id, usage)| ((usage.provider, usage.session_id.as_str()), terminal_id))
        .collect();
    for entry in cost_entries {
        let live = pane_by_session
            .get(&(entry.provider, entry.session_id.as_str()))
            .copied();
        records.push(mercury_usage::UsageRecord {
            at: entry.at,
            provider: entry.provider,
            session_id: entry.session_id.clone(),
            workspace: entry.workspace.clone().or_else(|| {
                live.and_then(|terminal_id| workspace_label_for_terminal(state, terminal_id))
            }),
            agent: entry.agent.clone().or_else(|| {
                live.and_then(|terminal_id| state.terminals.get(terminal_id))
                    .and_then(|terminal| terminal.agent_name.clone())
            }),
            pane: live.map(|terminal_id| terminal_id.as_str().to_string()),
            tokens: mercury_usage::TokenCounts::default(),
            cost_usd: Some(entry.cost_delta_usd),
        });
    }
    for (terminal_id, tail) in tails {
        let Some(terminal) = state.terminals.get(terminal_id) else {
            continue;
        };
        let agent = terminal.agent_name.clone();
        let pane = Some(terminal_id.as_str().to_string());
        let workspace = workspace_label_for_terminal(state, terminal_id);
        let push_events = |records: &mut Vec<mercury_usage::UsageRecord>,
                           provider: Provider,
                           events: &[mercury_usage::UsageEvent]| {
            for event in events {
                records.push(mercury_usage::UsageRecord {
                    at: event.at,
                    provider,
                    session_id: event.session_id.clone(),
                    workspace: workspace.clone(),
                    agent: agent.clone(),
                    pane: pane.clone(),
                    tokens: event.tokens,
                    cost_usd: None,
                });
            }
        };
        match tail {
            SessionTail::Claude(claude_tail) => {
                push_events(&mut records, Provider::Claude, claude_tail.main.events());
                for (_, subagent) in claude_tail.subagents.values() {
                    push_events(&mut records, Provider::Claude, subagent.events());
                }
            }
            SessionTail::Codex(codex_tail) => {
                push_events(&mut records, Provider::Codex, codex_tail.rollout.events());
            }
        }
    }
    records
}

fn workspace_label_for_terminal(state: &AppState, terminal_id: &TerminalId) -> Option<String> {
    state
        .workspaces
        .iter()
        .find(|workspace| {
            workspace.tabs.iter().any(|tab| {
                tab.panes
                    .values()
                    .any(|pane| &pane.attached_terminal_id == terminal_id)
            })
        })
        .map(|workspace| workspace.display_name_from_terminals(&state.terminals))
}

fn session_usage_to_wire(usage: &mercury_usage::SessionUsage) -> protocol::ClientShellAgentUsage {
    protocol::ClientShellAgentUsage {
        model: usage.model.clone(),
        context_permille: usage.context.and_then(|context| context.permille()),
        context_exact: usage.context.is_some_and(|context| context.exact),
        context_window: usage.context.and_then(|context| context.window_tokens),
        tokens_in: usage.totals.input,
        tokens_out: usage.totals.output,
        tokens_cache_read: usage.totals.cache_read,
        tokens_cache_write: usage.totals.cache_write,
        cost_micros_usd: usage
            .cost_usd
            .map(|usd| (usd * 1_000_000.0).round().clamp(0.0, u64::MAX as f64) as u64),
        updated_at_unix_ms: usage
            .updated_at
            .map(|at| (at.unix_timestamp_nanos() / 1_000_000) as i64),
    }
}

fn provider_limits_to_wire(
    limits: &mercury_usage::ProviderLimits,
) -> protocol::ClientShellProviderUsage {
    protocol::ClientShellProviderUsage {
        status: "atualizado".into(),
        detail: "Fonte de sessão · conta ainda não confirmada".into(),
        observed_at_unix_ms: limits.observed_at.map(offsetdatetime_to_unix_ms),
        provider: limits.provider.as_str().to_owned(),
        limits: limits
            .limits
            .iter()
            .map(|limit| protocol::ClientShellProviderLimit {
                key: limit.key.clone(),
                used_permille: (limit.used_percent * 10.0).round().clamp(0.0, 10_000.0) as u16,
                window_minutes: limit.window_minutes,
                resets_at_unix_ms: limit
                    .resets_at
                    .map(|at| (at.unix_timestamp_nanos() / 1_000_000) as i64),
            })
            .collect(),
        credits: limits
            .credits
            .as_ref()
            .map(|credits| protocol::ClientShellProviderCredits {
                has_credits: credits.has_credits,
                unlimited: credits.unlimited,
                balance: credits.balance.clone(),
            }),
        plan_type: limits.plan_type.clone(),
    }
}

/// Only hook-reported Claude/Codex session ids are trackable (D2); `pi`/`omp`
/// report a path instead of an id and have no usage reader yet, and any other
/// agent has no known file format.
fn tracked_session_for(terminal: &TerminalState) -> Option<TrackedSession> {
    let persisted = terminal.persisted_agent_session.as_ref()?;
    if persisted.session_ref.kind != AgentSessionRefKind::Id {
        return None;
    }
    if persisted.agent != "claude" && persisted.agent != "codex" {
        return None;
    }
    Some(TrackedSession {
        agent: persisted.agent.clone(),
        session_id: persisted.session_ref.value.clone(),
        cwd: terminal.cwd.clone(),
    })
}

impl App {
    /// Rebuilds historical token records once per enabled server lifetime.
    /// The scan is deliberately off-thread; only its in-memory result reaches
    /// the snapshot/render path.
    pub(crate) fn start_usage_history_scan_if_needed(&mut self) {
        if !self.usage.enabled || !self.policy.background_updates || self.usage.history_scan_started
        {
            return;
        }
        self.usage.history_scan_started = true;
        let claude_home = self.usage.claude_home.clone();
        let codex_home = self.usage.codex_home.clone();
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let sessions = mercury_usage::history_scan::scan(
                claude_home.as_deref(),
                codex_home.as_deref(),
                Duration::from_secs(35 * 24 * 60 * 60),
            );
            let _ = event_tx.blocking_send(AppEvent::UsageHistoryScanned { sessions });
        });
    }

    pub(crate) fn apply_usage_history_scanned(
        &mut self,
        sessions: Vec<mercury_usage::history_scan::HistoricalSession>,
    ) -> bool {
        if !self.usage.enabled {
            return false;
        }
        self.usage.history = sessions;
        !self.usage.budgets.is_empty()
    }

    /// Polls account limits even without a tracked Codex pane. Per-thread
    /// costs are requested only for hook-identified threads. The child process
    /// and all JSON I/O live on a background thread.
    pub(crate) fn start_codex_usage_refresh_if_due(&mut self, now: Instant) {
        self.start_claude_account_usage(now);
        if !self.usage.enabled || !self.policy.background_updates {
            return;
        }
        let mut threads = Vec::new();
        let mut working = false;
        for terminal in self.state.terminals.values() {
            let Some(tracked) = tracked_session_for(terminal) else {
                continue;
            };
            if tracked.agent == "codex" {
                threads.push(tracked.session_id);
                working |= terminal.state == AgentState::Working;
            }
        }
        threads.sort();
        threads.dedup();
        if threads.is_empty() {
            self.usage.codex_live_costs.clear();
        }
        if self.usage.codex_refresh_in_flight {
            return;
        }
        let changed =
            threads != self.usage.codex_last_threads || working != self.usage.codex_was_working;
        if !changed
            && self
                .usage
                .codex_next_refresh_at
                .is_some_and(|due| now < due)
        {
            return;
        }
        self.usage.codex_last_threads = threads.clone();
        self.usage.codex_was_working = working;
        self.usage.codex_next_refresh_at = Some(
            now + if working {
                CODEX_ACTIVE_REFRESH
            } else {
                CODEX_IDLE_REFRESH
            },
        );
        self.usage.codex_refresh_in_flight = true;
        self.usage
            .account_probes
            .entry(Provider::Codex)
            .or_default()
            .in_flight = true;
        let binary = if self.usage.codex_binary == std::path::Path::new("codex") {
            super::usage_accounts::executable("codex")
        } else {
            self.usage.codex_binary.clone()
        };
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let (limits, costs, identity, error) =
                match super::usage_codex_app_server::read_snapshot(&binary, &threads) {
                    Ok(read) => (read.limits, read.costs, read.identity, None),
                    Err(err) => (None, Vec::new(), None, Some(err.to_string())),
                };
            let _ = event_tx.blocking_send(AppEvent::CodexUsageRefreshed {
                identity,
                limits,
                costs,
                error,
            });
        });
    }

    /// Starts a background usage refresh if any tracked pane is due, exactly
    /// following `git_refresh.rs`'s `std::thread::spawn` + `blocking_send`
    /// pattern so no I/O ever runs on the render path.
    pub(crate) fn start_usage_refresh_if_due(&mut self, now: Instant) {
        if self.usage.in_flight {
            return;
        }
        let targets = self.usage_poll_targets_due(now);
        if targets.is_empty() {
            return;
        }
        self.usage.in_flight = true;
        let event_tx = self.event_tx.clone();
        let tails = Arc::clone(&self.usage.tails);
        let claude_home = self.usage.claude_home.clone();
        let codex_home = self.usage.codex_home.clone();
        std::thread::spawn(move || {
            let (sessions, limits) = refresh_usage_targets(
                &tails,
                claude_home.as_deref(),
                codex_home.as_deref(),
                targets,
            );
            let _ = event_tx.blocking_send(AppEvent::UsageRefreshed { sessions, limits });
        });
    }

    /// Collects tracked panes whose adaptive-cadence deadline (D3) is due,
    /// and prunes bookkeeping/cached readers for panes that stopped being
    /// trackable (agent detached, hook authority lost, pane closed).
    fn usage_poll_targets_due(&mut self, now: Instant) -> Vec<(TerminalId, TrackedSession)> {
        if !self.usage.enabled {
            return Vec::new();
        }
        let mut due = Vec::new();
        let mut live = HashSet::new();
        for (terminal_id, terminal) in &self.state.terminals {
            let Some(tracked) = tracked_session_for(terminal) else {
                continue;
            };
            live.insert(terminal_id.clone());
            let interval = self.usage.poll_interval(terminal.state);
            let deadline = *self
                .usage
                .next_poll_at
                .entry(terminal_id.clone())
                .or_insert(now);
            if now >= deadline {
                due.push((terminal_id.clone(), tracked));
                self.usage
                    .next_poll_at
                    .insert(terminal_id.clone(), now + interval);
            }
        }
        self.usage.next_poll_at.retain(|id, _| live.contains(id));
        self.usage.last_applied_at.retain(|id, _| live.contains(id));
        self.usage.statuslines.retain(|id, _| live.contains(id));
        let stale: Vec<TerminalId> = self
            .usage
            .sessions
            .keys()
            .filter(|id| !live.contains(*id))
            .cloned()
            .collect();
        for terminal_id in stale {
            self.usage.sessions.remove(&terminal_id);
        }
        if let Ok(mut tails) = self.usage.tails.lock() {
            tails.retain(|id, _| live.contains(id));
        }
        due
    }

    /// Lowers a terminal's next-poll deadline to "now" so a hook/state-change
    /// event is picked up on the next scheduled-tasks tick instead of waiting
    /// out the adaptive cadence (D3). Cheap bookkeeping only; the actual
    /// refresh still happens in the background thread above.
    pub(crate) fn request_immediate_usage_refresh(&mut self, terminal_id: &TerminalId) {
        self.usage
            .next_poll_at
            .insert(terminal_id.clone(), Instant::now());
    }

    /// Same as [`Self::request_immediate_usage_refresh`], resolved from a
    /// pane id (hook/state-change events carry a `PaneId`, not a `TerminalId`).
    pub(crate) fn request_immediate_usage_refresh_for_pane(&mut self, pane_id: PaneId) {
        let Some(terminal_id) = self.state.workspaces.iter().find_map(|workspace| {
            workspace
                .pane_state(pane_id)
                .map(|pane| pane.attached_terminal_id.clone())
        }) else {
            return;
        };
        self.request_immediate_usage_refresh(&terminal_id);
    }

    pub(crate) fn usage_next_poll_deadline(&self) -> Option<Instant> {
        let poll = self.usage.next_poll_at.values().copied().min();
        let budget_reset = self.usage.next_budget_reset_deadline(Instant::now());
        [
            poll,
            budget_reset,
            self.usage.codex_next_refresh_at,
            self.usage.claude_next_refresh_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Applies a completed background refresh (D4 throttle: at most one
    /// applied update per second per session). Returns whether the visible
    /// usage state actually changed, so the caller knows to re-render.
    pub(crate) fn apply_usage_refreshed(
        &mut self,
        sessions: Vec<TerminalUsageUpdate>,
        limits: Vec<mercury_usage::ProviderLimits>,
    ) -> bool {
        self.usage.in_flight = false;
        if !self.usage.enabled {
            return false;
        }
        let now = Instant::now();
        let mut changed = false;
        for update in sessions {
            let current = self
                .state
                .terminals
                .get(&update.terminal_id)
                .and_then(tracked_session_for);
            let belongs_to_current = match (&current, &update.usage) {
                (Some(tracked), Some(usage)) => {
                    (tracked.agent == "claude" && usage.provider == Provider::Claude)
                        || (tracked.agent == "codex"
                            && usage.provider == Provider::Codex
                            && tracked.session_id == usage.session_id)
                }
                (_, None) => true,
                _ => false,
            };
            if !belongs_to_current {
                continue;
            }
            if self
                .usage
                .last_applied_at
                .get(&update.terminal_id)
                .is_some_and(|at| now.saturating_duration_since(*at) < APPLY_THROTTLE)
            {
                continue;
            }
            match update.usage {
                Some(mut usage) => {
                    // D8: the statusline is exact; it wins over the
                    // transcript estimate for the same session.
                    if let Some(statusline) = self.usage.statuslines.get(&update.terminal_id) {
                        statusline.apply_to(&mut usage);
                    }
                    if usage.provider == Provider::Codex {
                        if let Some((session_id, cost)) =
                            self.usage.codex_live_costs.get(&update.terminal_id)
                        {
                            if *session_id == usage.session_id {
                                usage.cost_usd = Some(*cost);
                            }
                        }
                    }
                    if self.usage.sessions.get(&update.terminal_id) != Some(&usage) {
                        changed = true;
                    }
                    self.usage
                        .sessions
                        .insert(update.terminal_id.clone(), usage);
                }
                None => {
                    changed |= self.usage.sessions.remove(&update.terminal_id).is_some();
                }
            }
            self.usage.last_applied_at.insert(update.terminal_id, now);
        }
        for limit in limits {
            if self
                .usage
                .account_probes
                .get(&limit.provider)
                .is_some_and(|p| p.identity.is_some() || p.status == "indisponível")
                || (limit.provider == Provider::Codex && self.usage.codex_live_limits.is_some())
            {
                continue;
            }
            if self.usage.limits.get(&limit.provider) != Some(&limit) {
                changed = true;
            }
            self.usage.limits.insert(limit.provider, limit);
        }
        changed
    }

    /// Applies an app-server snapshot only to panes still running the
    /// requested Codex thread. Cost totals also flow through the existing
    /// ledger, which computes deltas across restarts.
    pub(crate) fn apply_codex_usage_refreshed(
        &mut self,
        limits: Option<mercury_usage::ProviderLimits>,
        costs: Vec<(String, f64)>,
        error: Option<String>,
    ) -> bool {
        self.usage.codex_refresh_in_flight = false;
        if !self.usage.enabled {
            return false;
        }
        let has_sample = self.usage.codex_live_limits.is_some();
        let probe = self
            .usage
            .account_probes
            .entry(Provider::Codex)
            .or_default();
        let delay = probe_result(probe, error.as_deref(), has_sample);
        if let Some(error) = error {
            let lost_login = error.to_ascii_lowercase().contains("auth")
                || error.to_ascii_lowercase().contains("login");
            self.usage.codex_next_refresh_at = Some(Instant::now() + delay);
            if lost_login {
                self.usage.codex_live_limits = None;
                self.usage.limits.remove(&Provider::Codex);
            }
            return true;
        }
        let mut changed = false;
        if let Some(limits) = limits {
            changed |= self.usage.limits.get(&Provider::Codex) != Some(&limits);
            self.usage.limits.insert(Provider::Codex, limits.clone());
            self.usage.codex_live_limits = Some(limits);
        } else if self.usage.codex_live_limits.take().is_some() {
            changed |= self.usage.limits.remove(&Provider::Codex).is_some();
        }
        let targets: Vec<_> = self
            .state
            .terminals
            .iter()
            .filter_map(|(terminal_id, terminal)| {
                let tracked = tracked_session_for(terminal)?;
                (tracked.agent == "codex").then_some((terminal_id.clone(), tracked))
            })
            .collect();
        let observed_at = OffsetDateTime::now_utc();
        for (session_id, cost) in costs {
            for (terminal_id, tracked) in &targets {
                if tracked.session_id != session_id {
                    continue;
                }
                self.usage
                    .codex_live_costs
                    .insert(terminal_id.clone(), (session_id.clone(), cost));
                if let Some(usage) = self.usage.sessions.get_mut(terminal_id) {
                    if usage.session_id == session_id && usage.cost_usd != Some(cost) {
                        usage.cost_usd = Some(cost);
                        usage.updated_at = usage.updated_at.max(Some(observed_at));
                        changed = true;
                    }
                }
                if let Some(ledger) = self.usage.cost_ledger.as_ref() {
                    ledger.observe(CostObservation {
                        at: observed_at,
                        provider: Provider::Codex,
                        session_id: session_id.clone(),
                        cwd: Some(tracked.cwd.display().to_string()),
                        workspace: workspace_label_for_terminal(&self.state, terminal_id),
                        agent: self
                            .state
                            .terminals
                            .get(terminal_id)
                            .and_then(|terminal| terminal.agent_name.clone()),
                        total_usd: cost,
                    });
                }
            }
        }
        changed
    }

    pub(crate) fn update_codex_identity(&mut self, identity: Option<String>) {
        let probe = self
            .usage
            .account_probes
            .entry(Provider::Codex)
            .or_default();
        if identity.is_some() && probe.identity != identity {
            self.usage.codex_live_limits = None;
            self.usage.limits.remove(&Provider::Codex);
            probe.identity = identity;
        }
    }

    fn start_claude_account_usage(&mut self, now: Instant) {
        if !self.usage.claude_probe_requested
            || !self.usage.enabled
            || !self.policy.background_updates
            || self.usage.claude_refresh_in_flight
            || self
                .usage
                .claude_next_refresh_at
                .is_some_and(|due| now < due)
        {
            return;
        }
        self.usage.claude_refresh_in_flight = true;
        self.usage
            .account_probes
            .entry(Provider::Claude)
            .or_default()
            .in_flight = true;
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let (limits, identity, error) = match super::usage_accounts::read_claude() {
                Ok((limits, key)) => (Some(limits), Some(key), None),
                Err(e) => (None, None, Some(e.to_string())),
            };
            let _ = tx.blocking_send(AppEvent::ClaudeAccountUsageRefreshed {
                limits,
                identity,
                error,
            });
        });
    }

    pub(crate) fn apply_claude_account_usage(
        &mut self,
        limits: Option<mercury_usage::ProviderLimits>,
        identity: Option<String>,
        error: Option<String>,
    ) -> bool {
        self.usage.claude_refresh_in_flight = false;
        if !self.usage.enabled {
            return false;
        }
        let probe = self
            .usage
            .account_probes
            .entry(Provider::Claude)
            .or_default();
        let clear = error.as_ref().is_some_and(|e| {
            ["login", "auth", "alterada", "identidade"]
                .iter()
                .any(|k| e.to_ascii_lowercase().contains(k))
        }) || (identity.is_some() && probe.identity != identity);
        if clear {
            self.usage.limits.remove(&Provider::Claude);
        }
        if identity.is_some() {
            probe.identity = identity;
        }
        let delay = probe_result(
            probe,
            error.as_deref(),
            self.usage.limits.contains_key(&Provider::Claude),
        );
        self.usage.claude_next_refresh_at = Some(Instant::now() + delay);
        if let Some(limits) = limits {
            self.usage.limits.insert(Provider::Claude, limits);
        }
        true
    }

    pub(crate) fn refresh_account_usage(&mut self) {
        self.usage.claude_probe_requested = true;
        let now = Instant::now();
        // A manual refresh can shorten the normal cadence, but never bypass
        // the in-flight guard or issue more than one probe per ten seconds.
        if self
            .usage
            .account_manual_refresh_at
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(10))
        {
            return;
        }
        if !self.usage.account_probes.values().any(|p| p.in_flight) {
            self.usage.account_manual_refresh_at = Some(now);
            if self
                .usage
                .account_probes
                .get(&Provider::Codex)
                .is_none_or(|p| p.failures == 0)
            {
                self.usage.codex_next_refresh_at = None;
            }
            if self
                .usage
                .account_probes
                .get(&Provider::Claude)
                .is_none_or(|p| p.failures == 0)
            {
                self.usage.claude_next_refresh_at = None;
            }
            self.start_codex_usage_refresh_if_due(now);
        }
    }

    /// Wire projection of account-level usage (`ClientShellSnapshot.usage`),
    /// including budget evaluation (Fase 2 passo 5). Locking `self.usage.tails`
    /// here is in-memory bookkeeping already populated by the background
    /// thread, not file I/O, so it stays within the hot-path rules enforced
    /// by `scripts/test_ui_hot_path_architecture.py`.
    pub(crate) fn usage_summary_wire(&self) -> Option<protocol::ClientShellUsageSummary> {
        let records = self.usage_records().unwrap_or_default();
        self.usage.summary_wire(&records)
    }

    /// In-memory usage snapshot for routing policy; never reads transcript files.
    pub(crate) fn usage_records(&self) -> Result<Vec<mercury_usage::UsageRecord>, ()> {
        self.usage
            .tails
            .lock()
            .map(|tails| {
                collect_usage_records(
                    &tails,
                    &self.state,
                    &self.usage.sessions,
                    &self.usage.cost_entries,
                    &self.usage.history,
                )
            })
            .map_err(|_| ())
    }

    /// `usage.report_statusline` (D10): stores the exact snapshot for this
    /// terminal, forwards its cumulative cost to the ledger thread (D9), and
    /// applies it right away unless this terminal was updated less than
    /// [`APPLY_THROTTLE`] ago (D4) — in that case the next poll is pulled in
    /// so the stored snapshot is applied as soon as the throttle allows.
    /// Returns whether visible usage changed.
    pub(crate) fn apply_statusline_report(
        &mut self,
        terminal_id: &TerminalId,
        snapshot: StatuslineSnapshot,
    ) -> bool {
        if !self.usage.enabled {
            return false;
        }
        let matches_hook = self
            .state
            .terminals
            .get(terminal_id)
            .and_then(tracked_session_for)
            .is_some_and(|tracked| {
                tracked.agent == "claude" && tracked.session_id == snapshot.session_id
            });
        if !matches_hook {
            return false;
        }
        if let (Some(total_usd), Some(ledger)) =
            (snapshot.cost_usd, self.usage.cost_ledger.as_ref())
        {
            ledger.observe(CostObservation {
                at: snapshot.observed_at,
                provider: Provider::Claude,
                session_id: snapshot.session_id.clone(),
                cwd: snapshot.cwd.clone(),
                workspace: workspace_label_for_terminal(&self.state, terminal_id),
                agent: self
                    .state
                    .terminals
                    .get(terminal_id)
                    .and_then(|terminal| terminal.agent_name.clone()),
                total_usd,
            });
        }
        let limits = snapshot.provider_limits().unwrap_or_else(|| {
            // A valid Claude statusline without rate_limits is evidence that
            // this account does not currently report them. Keep the provider
            // visible as n/a instead of retaining an older percentage.
            mercury_usage::ProviderLimits {
                provider: Provider::Claude,
                limits: Vec::new(),
                credits: None,
                plan_type: None,
                limit_reached_type: None,
                observed_at: Some(snapshot.observed_at),
            }
        });
        self.usage.statuslines.insert(terminal_id.clone(), snapshot);

        // Provider limits are account-level data. A recent pane usage poll may
        // defer the session overlay, but must not hide a fresh rate-limit
        // report from the Usage view until another statusline arrives.
        let mut changed = false;
        // The fresh timestamp is retained for budget freshness, but it does
        // not alter the wire or budget values. Avoid a render on every
        // identical statusline refresh.
        if !self
            .usage
            .account_probes
            .get(&Provider::Claude)
            .is_some_and(|p| p.identity.is_some() || p.status == "indisponível")
        {
            changed |= self
                .usage
                .limits
                .get(&limits.provider)
                .is_none_or(|previous| {
                    let mut comparable = previous.clone();
                    comparable.observed_at = limits.observed_at;
                    comparable != limits
                });
            self.usage.limits.insert(limits.provider, limits);
        }

        let now = Instant::now();
        if let Some(last) = self.usage.last_applied_at.get(terminal_id).copied() {
            if now.saturating_duration_since(last) < APPLY_THROTTLE {
                if let Some(deadline) = self.usage.next_poll_at.get_mut(terminal_id) {
                    *deadline = (*deadline).min(last + APPLY_THROTTLE);
                }
                return changed;
            }
        }

        if let (Some(session), Some(snapshot)) = (
            self.usage.sessions.get_mut(terminal_id),
            self.usage.statuslines.get(terminal_id),
        ) {
            let before = session.clone();
            snapshot.apply_to(session);
            changed |= *session != before;
        }
        self.usage.last_applied_at.insert(terminal_id.clone(), now);
        changed
    }

    /// `AppEvent::UsageCostRecorded`: new or replayed ledger entries (D9).
    /// Only matters to the Usage view when a USD budget is configured.
    pub(crate) fn apply_usage_cost_recorded(&mut self, entries: Vec<CostLedgerEntry>) -> bool {
        if entries.is_empty() {
            return false;
        }
        self.usage.cost_entries.extend(entries);
        let horizon_start = OffsetDateTime::now_utc() - self.usage.cost_horizon();
        self.usage
            .cost_entries
            .retain(|entry| entry.at >= horizon_start);
        self.usage.enabled
            && self
                .usage
                .budgets
                .iter()
                .any(|budget| matches!(budget.limit, mercury_usage::Limit::Usd(_)))
    }
}

/// Runs in the background thread: reads only what changed for every due
/// target and reports the resulting [`mercury_usage::SessionUsage`] (or
/// `None` if the file could not be resolved/read this round).
fn refresh_usage_targets(
    tails: &Arc<Mutex<HashMap<TerminalId, SessionTail>>>,
    claude_home: Option<&Path>,
    codex_home: Option<&Path>,
    targets: Vec<(TerminalId, TrackedSession)>,
) -> (Vec<TerminalUsageUpdate>, Vec<mercury_usage::ProviderLimits>) {
    let mut sessions = Vec::with_capacity(targets.len());
    let mut limits = Vec::new();
    let Ok(mut tails) = tails.lock() else {
        return (sessions, limits);
    };
    for (terminal_id, tracked) in targets {
        let usage = match tracked.agent.as_str() {
            "claude" => refresh_claude_session(&mut tails, &terminal_id, claude_home, &tracked),
            "codex" => {
                refresh_codex_session(&mut tails, &terminal_id, codex_home, &tracked, &mut limits)
            }
            _ => None,
        };
        sessions.push(TerminalUsageUpdate { terminal_id, usage });
    }
    (sessions, limits)
}

fn refresh_claude_session(
    tails: &mut HashMap<TerminalId, SessionTail>,
    terminal_id: &TerminalId,
    claude_home: Option<&Path>,
    tracked: &TrackedSession,
) -> Option<mercury_usage::SessionUsage> {
    let claude_home = claude_home?;
    let transcript_path =
        locate::claude_transcript_path(claude_home, Some(&tracked.cwd), &tracked.session_id)?;

    let stale = !matches!(
        tails.get(terminal_id),
        Some(SessionTail::Claude(tail)) if tail.transcript_path == transcript_path
    );
    if stale {
        tails.insert(
            terminal_id.clone(),
            SessionTail::Claude(ClaudeSessionTail {
                transcript_path: transcript_path.clone(),
                ..Default::default()
            }),
        );
    }
    let Some(SessionTail::Claude(tail)) = tails.get_mut(terminal_id) else {
        unreachable!("a Claude tail was just inserted above")
    };

    read_claude_file(&transcript_path, &mut tail.main_tail, &mut tail.main);
    let subagent_paths = locate::claude_subagent_transcripts(&transcript_path);
    tail.subagents
        .retain(|path, _| subagent_paths.contains(path));
    for subagent_path in &subagent_paths {
        let (sub_tail, sub_transcript) = tail.subagents.entry(subagent_path.clone()).or_default();
        read_claude_file(subagent_path, sub_tail, sub_transcript);
    }

    let subagents: Vec<ClaudeTranscript> =
        tail.subagents.values().map(|(_, t)| t.clone()).collect();
    claude::merge_session(&tail.main, &subagents)
}

fn refresh_codex_session(
    tails: &mut HashMap<TerminalId, SessionTail>,
    terminal_id: &TerminalId,
    codex_home: Option<&Path>,
    tracked: &TrackedSession,
    limits_out: &mut Vec<mercury_usage::ProviderLimits>,
) -> Option<mercury_usage::SessionUsage> {
    let codex_home = codex_home?;
    let rollout_path =
        locate::codex_rollout_path(codex_home, &tracked.session_id, CODEX_ROLLOUT_MAX_DAYS)?;

    let stale = !matches!(
        tails.get(terminal_id),
        Some(SessionTail::Codex(tail)) if tail.rollout_path == rollout_path
    );
    if stale {
        tails.insert(
            terminal_id.clone(),
            SessionTail::Codex(CodexSessionTail {
                rollout_path: rollout_path.clone(),
                ..Default::default()
            }),
        );
    }
    let Some(SessionTail::Codex(tail)) = tails.get_mut(terminal_id) else {
        unreachable!("a Codex tail was just inserted above")
    };

    read_codex_file(&rollout_path, &mut tail.tail, &mut tail.rollout);
    if let Some(limits) = tail.rollout.limits() {
        limits_out.push(limits.clone());
    }
    tail.rollout.session_usage()
}

/// Reads whatever was appended to `path` since `tail` last looked (D3's
/// `JsonlTail`). If the file shrank (truncated/replaced session), the parser
/// state is dropped *before* re-reading so the fresh content is not layered
/// on top of stale totals.
fn read_claude_file(path: &Path, tail: &mut JsonlTail, transcript: &mut ClaudeTranscript) {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() < tail.offset()) {
        transcript.reset();
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let _ = tail.read_new(&mut file, |line| {
        transcript.ingest_line(line);
    });
}

fn read_codex_file(path: &Path, tail: &mut JsonlTail, rollout: &mut CodexRollout) {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() < tail.offset()) {
        rollout.reset();
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let _ = tail.read_new(&mut file, |line| {
        rollout.ingest_line(line);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_resume::{AgentSessionRef, PersistedAgentSession};

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mercury-usage-service-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_claude_transcript(home: &Path, cwd: &Path, session_id: &str, body: &str) -> PathBuf {
        let path = home
            .join("projects")
            .join(locate::claude_project_slug(cwd))
            .join(format!("{session_id}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn claude_line(request: &str, output_tokens: u64, timestamp: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"sess","requestId":"{request}","timestamp":"{timestamp}","message":{{"id":"{request}","model":"claude-sonnet-5","usage":{{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":{output_tokens}}}}}}}"#
        )
    }

    fn app_with_claude_pane(claude_home: &Path, cwd: &Path, session_id: &str) -> (App, TerminalId) {
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        app.usage.claude_home = Some(claude_home.to_path_buf());
        app.usage.codex_home = None;
        let terminal_id = TerminalId::alloc();
        let mut terminal = TerminalState::new(terminal_id.clone(), cwd.to_path_buf());
        terminal.persisted_agent_session = Some(PersistedAgentSession {
            source: "mercury:claude".into(),
            agent: "claude".into(),
            session_ref: AgentSessionRef::id(session_id).unwrap(),
        });
        app.state.terminals.insert(terminal_id.clone(), terminal);
        (app, terminal_id)
    }

    fn app_with_codex_pane(session_id: &str) -> (App, TerminalId) {
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        let terminal_id = TerminalId::alloc();
        let mut terminal = TerminalState::new(terminal_id.clone(), PathBuf::from("/tmp"));
        terminal.persisted_agent_session = Some(PersistedAgentSession {
            source: "mercury:codex".into(),
            agent: "codex".into(),
            session_ref: AgentSessionRef::id(session_id).unwrap(),
        });
        app.state.terminals.insert(terminal_id.clone(), terminal);
        (app, terminal_id)
    }

    #[test]
    fn codex_live_cost_only_updates_matching_thread_and_survives_rollout_refresh() {
        let first = "11111111-2222-3333-4444-555555555551";
        let second = "11111111-2222-3333-4444-555555555552";
        let (mut app, terminal_id) = app_with_codex_pane(first);
        app.usage.sessions.insert(
            terminal_id.clone(),
            mercury_usage::SessionUsage {
                provider: Provider::Codex,
                session_id: first.into(),
                model: None,
                totals: mercury_usage::TokenCounts::default(),
                context: None,
                cost_usd: None,
                updated_at: None,
            },
        );
        assert!(!app.apply_codex_usage_refreshed(None, vec![(second.into(), 1.0)], None));
        assert_eq!(app.usage.sessions[&terminal_id].cost_usd, None);
        assert!(app.apply_codex_usage_refreshed(None, vec![(first.into(), 0.25)], None));
        assert_eq!(app.usage.sessions[&terminal_id].cost_usd, Some(0.25));

        let mut rollout = app.usage.sessions[&terminal_id].clone();
        rollout.cost_usd = None;
        app.apply_usage_refreshed(
            vec![TerminalUsageUpdate {
                terminal_id: terminal_id.clone(),
                usage: Some(rollout),
            }],
            Vec::new(),
        );
        assert_eq!(app.usage.sessions[&terminal_id].cost_usd, Some(0.25));

        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .persisted_agent_session
            .as_mut()
            .unwrap()
            .session_ref = AgentSessionRef::id(second).unwrap();
        assert!(!app.apply_codex_usage_refreshed(None, vec![(first.into(), 0.5)], None));
        assert_eq!(app.usage.sessions[&terminal_id].cost_usd, Some(0.25));

        let mut newer = app.usage.sessions[&terminal_id].clone();
        newer.session_id = second.into();
        newer.cost_usd = None;
        app.usage
            .sessions
            .insert(terminal_id.clone(), newer.clone());
        app.usage.last_applied_at.clear();
        let mut stale = newer.clone();
        stale.session_id = first.into();
        assert!(!app.apply_usage_refreshed(
            vec![TerminalUsageUpdate {
                terminal_id: terminal_id.clone(),
                usage: Some(stale),
            }],
            Vec::new(),
        ));
        assert_eq!(app.usage.sessions[&terminal_id], newer);
    }

    #[test]
    fn codex_app_server_failure_retries_soon_and_preserves_rollout_fallback() {
        let (mut app, _) = app_with_codex_pane("11111111-2222-3333-4444-555555555551");
        let rollout = mercury_usage::codex_app_server::account_limits(
            &serde_json::json!({"rateLimits":{"primary":null,"secondary":null,"planType":"business"}}),
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        app.usage.limits.insert(Provider::Codex, rollout.clone());
        let before = Instant::now();
        assert!(app.apply_codex_usage_refreshed(None, Vec::new(), Some("offline".into())));
        assert_eq!(app.usage.limits.get(&Provider::Codex), Some(&rollout));
        let deadline = app.usage.codex_next_refresh_at.unwrap();
        assert!(deadline >= before + CODEX_ACTIVE_REFRESH);
        assert!(deadline <= Instant::now() + CODEX_ACTIVE_REFRESH);
    }

    #[test]
    fn usage_summary_shows_both_providers_without_fabricating_limits() {
        let service = UsageService::new(&crate::config::UsageConfig::default());
        let summary = service.summary_wire(&[]).expect("usage enabled by default");
        assert_eq!(summary.providers.len(), 2);
        assert_eq!(summary.providers[0].provider, "claude");
        assert_eq!(summary.providers[1].provider, "codex");
        assert!(summary
            .providers
            .iter()
            .all(|provider| provider.limits.is_empty()));
    }

    #[test]
    fn account_failure_keeps_stale_quota_but_logout_and_identity_change_clear_it() {
        let (mut app, _) = app_with_codex_pane("11111111-2222-3333-4444-555555555551");
        let limits=mercury_usage::codex_app_server::account_limits(&serde_json::json!({"rateLimits":{"individualLimit":{"remainingPercent":23},"planType":"business"}}),OffsetDateTime::UNIX_EPOCH).unwrap();
        app.update_codex_identity(Some("first".into()));
        app.apply_codex_usage_refreshed(Some(limits.clone()), vec![], None);
        app.apply_codex_usage_refreshed(None, vec![], Some("timed out".into()));
        let wire = app.usage.summary_wire(&[]).unwrap();
        let codex = &wire.providers[1];
        assert_eq!(codex.status, "desatualizado");
        assert_eq!(codex.limits[0].used_permille, 770);
        assert_eq!(codex.observed_at_unix_ms, Some(0));
        app.update_codex_identity(Some("second".into()));
        assert!(!app.usage.limits.contains_key(&Provider::Codex));
        app.apply_codex_usage_refreshed(Some(limits), vec![], None);
        app.apply_codex_usage_refreshed(None, vec![], Some("auth login required".into()));
        assert!(app.usage.summary_wire(&[]).unwrap().providers[1]
            .limits
            .is_empty());
    }

    #[test]
    fn account_failures_back_off_and_success_restores_normal_cadence() {
        let mut probe = AccountProbe::default();
        assert_eq!(
            probe_result(&mut probe, Some("offline"), false),
            Duration::from_secs(60)
        );
        assert_eq!(
            probe_result(&mut probe, Some("offline"), false),
            Duration::from_secs(120)
        );
        for _ in 0..10 {
            probe_result(&mut probe, Some("offline"), false);
        }
        assert_eq!(
            probe_result(&mut probe, Some("offline"), false),
            Duration::from_secs(1800)
        );
        assert_eq!(probe_result(&mut probe, None, true), CODEX_IDLE_REFRESH);
        assert_eq!(probe.failures, 0);
    }

    #[test]
    fn local_activity_has_explicit_utc_window_and_does_not_invent_cost() {
        let now = OffsetDateTime::parse(
            "2026-10-29T00:05:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let record = mercury_usage::UsageRecord {
            at: now,
            provider: Provider::Codex,
            session_id: "fixture".into(),
            workspace: None,
            agent: None,
            pane: None,
            tokens: mercury_usage::TokenCounts {
                input: 10,
                cache_read: 90,
                output: 5,
                ..Default::default()
            },
            cost_usd: None,
        };
        let mut yesterday = record.clone();
        yesterday.at = now - time::Duration::minutes(10);
        let result = activity_wire(&[record, yesterday], now);
        assert_eq!(result.tokens_today, 105);
        assert_eq!(result.cache_read_today, 90);
        assert_eq!(result.daily_tokens[5..], [105, 105]);
        assert!(result.cost_microusd_today.is_none());
    }

    #[test]
    fn tracked_session_requires_claude_or_codex_id() {
        let mut terminal = TerminalState::new(TerminalId::alloc(), PathBuf::from("/tmp"));
        assert!(tracked_session_for(&terminal).is_none());

        terminal.persisted_agent_session = Some(PersistedAgentSession {
            source: "mercury:pi".into(),
            agent: "pi".into(),
            session_ref: AgentSessionRef::path("/tmp/session.jsonl").unwrap(),
        });
        assert!(tracked_session_for(&terminal).is_none());

        terminal.persisted_agent_session = Some(PersistedAgentSession {
            source: "mercury:claude".into(),
            agent: "claude".into(),
            session_ref: AgentSessionRef::id("abc-123").unwrap(),
        });
        let tracked = tracked_session_for(&terminal).unwrap();
        assert_eq!(tracked.agent, "claude");
        assert_eq!(tracked.session_id, "abc-123");
    }

    #[test]
    fn poll_interval_is_fast_only_while_working() {
        let service = UsageService::new(&crate::config::UsageConfig::default());
        assert_eq!(
            service.poll_interval(AgentState::Working),
            Duration::from_millis(1000)
        );
        assert_eq!(
            service.poll_interval(AgentState::Idle),
            Duration::from_millis(10_000)
        );
        assert_eq!(
            service.poll_interval(AgentState::Blocked),
            Duration::from_millis(10_000)
        );
        assert_eq!(
            service.poll_interval(AgentState::Unknown),
            Duration::from_millis(10_000)
        );
    }

    #[test]
    fn poll_interval_follows_configured_cadence() {
        let config = crate::config::UsageConfig {
            poll_active_ms: 250,
            poll_idle_ms: 2_000,
            ..crate::config::UsageConfig::default()
        };
        let service = UsageService::new(&config);
        assert_eq!(
            service.poll_interval(AgentState::Working),
            Duration::from_millis(250)
        );
        assert_eq!(
            service.poll_interval(AgentState::Idle),
            Duration::from_millis(2_000)
        );
    }

    #[test]
    fn refresh_reads_claude_transcript_for_tracked_pane() {
        let claude_home = temp_dir("claude");
        let cwd = temp_dir("cwd");
        let session_id = "11111111-2222-3333-4444-555555555555";
        write_claude_transcript(
            &claude_home,
            &cwd,
            session_id,
            &format!(
                "{}\n{}\n",
                claude_line("req_a", 10, "2026-10-05T14:00:00.000Z"),
                claude_line("req_b", 20, "2026-10-05T14:01:00.000Z"),
            ),
        );
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);

        let now = Instant::now();
        let targets = app.usage_poll_targets_due(now);
        assert_eq!(targets.len(), 1);
        let (tails, limits) = refresh_usage_targets(
            &app.usage.tails.clone(),
            app.usage.claude_home.as_deref(),
            app.usage.codex_home.as_deref(),
            targets,
        );
        assert!(limits.is_empty());
        let changed = app.apply_usage_refreshed(tails, limits);
        assert!(changed);

        let usage = app.usage.agent_usage_wire(&terminal_id).unwrap();
        assert_eq!(usage.tokens_out, 30);
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-5"));

        std::fs::remove_dir_all(&claude_home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn apply_throttle_drops_updates_within_one_second() {
        let claude_home = temp_dir("claude-throttle");
        let cwd = temp_dir("cwd-throttle");
        let session_id = "11111111-2222-3333-4444-555555555556";
        write_claude_transcript(
            &claude_home,
            &cwd,
            session_id,
            &format!("{}\n", claude_line("req_a", 10, "2026-10-05T14:00:00.000Z")),
        );
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        app.usage
            .last_applied_at
            .insert(terminal_id.clone(), Instant::now());

        let changed = app.apply_usage_refreshed(
            vec![TerminalUsageUpdate {
                terminal_id: terminal_id.clone(),
                usage: Some(mercury_usage::SessionUsage {
                    provider: Provider::Claude,
                    session_id: session_id.into(),
                    model: None,
                    totals: mercury_usage::TokenCounts::default(),
                    context: None,
                    cost_usd: None,
                    updated_at: None,
                }),
            }],
            Vec::new(),
        );
        assert!(!changed);
        assert!(app.usage.agent_usage_wire(&terminal_id).is_none());

        std::fs::remove_dir_all(&claude_home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn statusline_from_another_session_cannot_update_a_pane() {
        let claude_home = temp_dir("claude-statusline-mismatch");
        let cwd = temp_dir("cwd-statusline-mismatch");
        let (mut app, terminal_id) =
            app_with_claude_pane(&claude_home, &cwd, "11111111-2222-3333-4444-555555555560");
        let snapshot = mercury_usage::StatuslineSnapshot::from_value(
            &serde_json::json!({
                "session_id": "11111111-2222-3333-4444-555555555561",
                "cost": {"total_cost_usd": 10.0},
                "rate_limits": {"five_hour": {"used_percentage": 99.0}}
            }),
            OffsetDateTime::now_utc(),
        )
        .unwrap();

        assert!(!app.apply_statusline_report(&terminal_id, snapshot));
        assert!(!app.usage.statuslines.contains_key(&terminal_id));
        assert!(!app.usage.limits.contains_key(&Provider::Claude));
        std::fs::remove_dir_all(claude_home).ok();
        std::fs::remove_dir_all(cwd).ok();
    }

    #[test]
    fn statusline_provider_limits_update_while_pane_overlay_is_throttled() {
        let claude_home = temp_dir("claude-statusline-throttle");
        let cwd = temp_dir("cwd-statusline-throttle");
        let session_id = "11111111-2222-3333-4444-555555555562";
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        let last = Instant::now();
        app.usage.last_applied_at.insert(terminal_id.clone(), last);
        app.usage
            .next_poll_at
            .insert(terminal_id.clone(), last + Duration::from_secs(60));
        let snapshot = mercury_usage::StatuslineSnapshot::from_value(
            &serde_json::json!({
                "session_id": session_id,
                "rate_limits": {"five_hour": {"used_percentage": 61.0}}
            }),
            OffsetDateTime::now_utc(),
        )
        .unwrap();

        assert!(app.apply_statusline_report(&terminal_id, snapshot));
        let limits = app.usage.limits.get(&Provider::Claude).unwrap();
        assert_eq!(limits.limits[0].key, "five_hour");
        assert_eq!(limits.limits[0].used_percent, 61.0);
        assert_eq!(app.usage.next_poll_at[&terminal_id], last + APPLY_THROTTLE);
        let later = OffsetDateTime::now_utc() + time::Duration::seconds(1);
        let same_values = mercury_usage::StatuslineSnapshot::from_value(
            &serde_json::json!({
                "session_id": session_id,
                "rate_limits": {"five_hour": {"used_percentage": 61.0}}
            }),
            later,
        )
        .unwrap();
        assert!(!app.apply_statusline_report(&terminal_id, same_values));
        assert_eq!(app.usage.limits[&Provider::Claude].observed_at, Some(later));

        std::fs::remove_dir_all(claude_home).ok();
        std::fs::remove_dir_all(cwd).ok();
    }

    #[test]
    fn statusline_without_limits_replaces_stale_claude_limit_with_na() {
        let claude_home = temp_dir("claude-statusline-no-limits");
        let cwd = temp_dir("cwd-statusline-no-limits");
        let session_id = "11111111-2222-3333-4444-555555555563";
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        let first = mercury_usage::StatuslineSnapshot::from_value(
            &serde_json::json!({
                "session_id": session_id,
                "rate_limits": {"five_hour": {"used_percentage": 61.0}}
            }),
            OffsetDateTime::now_utc(),
        )
        .unwrap();
        assert!(app.apply_statusline_report(&terminal_id, first));
        assert_eq!(app.usage.limits[&Provider::Claude].limits.len(), 1);

        let no_limits = mercury_usage::StatuslineSnapshot::from_value(
            &serde_json::json!({"session_id": session_id}),
            OffsetDateTime::now_utc(),
        )
        .unwrap();
        assert!(app.apply_statusline_report(&terminal_id, no_limits));
        let provider = &app.usage_summary_wire().unwrap().providers[0];
        assert_eq!(provider.provider, "claude");
        assert!(provider.limits.is_empty());

        std::fs::remove_dir_all(claude_home).ok();
        std::fs::remove_dir_all(cwd).ok();
    }

    #[test]
    fn untracked_panes_are_pruned_from_sessions_and_tails() {
        let claude_home = temp_dir("claude-prune");
        let cwd = temp_dir("cwd-prune");
        let session_id = "11111111-2222-3333-4444-555555555557";
        write_claude_transcript(
            &claude_home,
            &cwd,
            session_id,
            &format!("{}\n", claude_line("req_a", 10, "2026-10-05T14:00:00.000Z")),
        );
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        let now = Instant::now();
        let targets = app.usage_poll_targets_due(now);
        let (tails, limits) = refresh_usage_targets(
            &app.usage.tails.clone(),
            app.usage.claude_home.as_deref(),
            app.usage.codex_home.as_deref(),
            targets,
        );
        app.apply_usage_refreshed(tails, limits);
        assert!(app.usage.agent_usage_wire(&terminal_id).is_some());

        app.state.terminals.remove(&terminal_id);
        app.usage_poll_targets_due(Instant::now());
        assert!(app.usage.agent_usage_wire(&terminal_id).is_none());
        assert!(app.usage.tails.lock().unwrap().is_empty());

        std::fs::remove_dir_all(&claude_home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    fn now_rfc3339() -> String {
        OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    }

    #[test]
    fn configured_budget_is_evaluated_end_to_end_against_tracked_sessions() {
        let claude_home = temp_dir("claude-budget");
        let cwd = temp_dir("cwd-budget");
        let session_id = "11111111-2222-3333-4444-555555555558";
        let now = now_rfc3339();
        write_claude_transcript(
            &claude_home,
            &cwd,
            session_id,
            &format!("{}\n", claude_line("req_a", 500, &now)),
        );
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        let budget_config: mercury_usage::BudgetConfig = toml::from_str(
            r#"
            name = "claude-tokens"
            scope = { provider = "claude" }
            window = { kind = "rolling", hours = 1 }
            limit = { tokens = 1000 }
            warn_at = [50]
            "#,
        )
        .unwrap();
        app.usage.budgets = vec![mercury_usage::Budget::try_from(budget_config).unwrap()];

        let targets = app.usage_poll_targets_due(Instant::now());
        let (tails, limits) = refresh_usage_targets(
            &app.usage.tails.clone(),
            app.usage.claude_home.as_deref(),
            app.usage.codex_home.as_deref(),
            targets,
        );
        app.apply_usage_refreshed(tails, limits);

        let summary = app.usage_summary_wire().expect("budget configured");
        assert_eq!(summary.budgets.len(), 1);
        let budget = &summary.budgets[0];
        assert_eq!(budget.name, "claude-tokens");
        // 10 input + 500 output = 510 tokens; crosses the 50% warn_at.
        assert_eq!(budget.used_scaled, 510);
        assert_eq!(budget.limit_scaled, 1000);
        assert_eq!(budget.level, protocol::ClientShellBudgetLevel::Warn);
        assert_eq!(budget.warn_threshold, Some(50));
        assert!(
            !budget.partial,
            "no cost source means no USD budget, so partial stays false for a token budget"
        );

        let _ = terminal_id;
        std::fs::remove_dir_all(&claude_home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn historical_session_counts_after_restart_but_not_twice_when_pane_is_tracked() {
        let claude_home = temp_dir("claude-history-budget");
        let cwd = temp_dir("cwd-history-budget");
        let session_id = "11111111-2222-3333-4444-555555555559";
        write_claude_transcript(
            &claude_home,
            &cwd,
            session_id,
            &format!("{}\n", claude_line("req_history", 500, &now_rfc3339())),
        );
        let (mut app, terminal_id) = app_with_claude_pane(&claude_home, &cwd, session_id);
        let budget_config: mercury_usage::BudgetConfig = toml::from_str(
            r#"
            name = "all-tokens"
            window = { kind = "rolling", hours = 1 }
            limit = { tokens = 1000 }
            "#,
        )
        .unwrap();
        app.usage.budgets = vec![mercury_usage::Budget::try_from(budget_config).unwrap()];
        let historical = mercury_usage::history_scan::scan(
            Some(&claude_home),
            None,
            Duration::from_secs(35 * 24 * 60 * 60),
        );
        assert_eq!(historical.len(), 1);
        assert!(app.apply_usage_history_scanned(historical));
        assert_eq!(
            app.usage_summary_wire().unwrap().budgets[0].used_scaled,
            510
        );

        let targets = app.usage_poll_targets_due(Instant::now());
        let (sessions, limits) = refresh_usage_targets(
            &app.usage.tails,
            app.usage.claude_home.as_deref(),
            app.usage.codex_home.as_deref(),
            targets,
        );
        app.apply_usage_refreshed(sessions, limits);
        assert_eq!(
            app.usage_summary_wire().unwrap().budgets[0].used_scaled,
            510
        );

        app.state.terminals.remove(&terminal_id);
        app.usage_poll_targets_due(Instant::now());
        assert_eq!(
            app.usage_summary_wire().unwrap().budgets[0].used_scaled,
            510
        );
        std::fs::remove_dir_all(claude_home).ok();
        std::fs::remove_dir_all(cwd).ok();
    }

    #[test]
    fn budgets_configured_but_not_yet_evaluated_still_surface_an_empty_summary() {
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        let budget_config: mercury_usage::BudgetConfig = toml::from_str(
            r#"
            name = "claude-monthly"
            window = { kind = "calendar", period = "monthly", reset_day = 1 }
            limit = { usd = 100 }
            "#,
        )
        .unwrap();
        app.usage.budgets = vec![mercury_usage::Budget::try_from(budget_config).unwrap()];

        let summary = app
            .usage_summary_wire()
            .expect("a configured budget is worth showing even with zero usage so far");
        assert_eq!(summary.budgets.len(), 1);
        assert_eq!(summary.budgets[0].used_scaled, 0);
    }

    #[test]
    fn next_budget_reset_deadline_schedules_a_wakeup_before_the_calendar_boundary() {
        let config = crate::config::UsageConfig::default();
        let mut service = UsageService::new(&config);
        assert!(service.next_budget_reset_deadline(Instant::now()).is_none());

        let budget_config: mercury_usage::BudgetConfig = toml::from_str(
            r#"
            name = "daily"
            window = { kind = "calendar", period = "daily", reset_hour = 0 }
            limit = { usd = 1 }
            "#,
        )
        .unwrap();
        service.budgets = vec![mercury_usage::Budget::try_from(budget_config).unwrap()];
        let now = Instant::now();
        let deadline = service
            .next_budget_reset_deadline(now)
            .expect("a daily calendar budget always has a next reset");
        assert!(deadline > now);
        // The next midnight UTC is never more than 24h away.
        assert!(deadline <= now + Duration::from_secs(24 * 60 * 60));
    }
}
