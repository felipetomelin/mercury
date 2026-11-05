//! Internal app events delivered via channel.
//!
//! Background tasks (PTY child watchers, future hook listeners, etc.) send
//! events to the main loop through this channel. No polling needed.

use std::time::Instant;

use crate::detect::{Agent, AgentState};
use crate::layout::PaneId;
use crate::workspace::{GitStatusCacheEntry, WorkspaceGitStatus};

#[derive(Debug)]
pub struct ApiWorktreeAddRequest {
    pub id: String,
    pub operation_id: u64,
    pub checkout_key: std::path::PathBuf,
    pub source_workspace_id: Option<String>,
    pub source_existing_membership: Option<crate::workspace::WorktreeSpaceMembership>,
    pub source_checkout_path: std::path::PathBuf,
    pub source_repo_root: std::path::PathBuf,
    pub repo_key: String,
    pub repo_name: String,
    pub label: Option<String>,
    pub focus: bool,
    pub respond_to: std::sync::mpsc::Sender<String>,
}

#[derive(Debug)]
pub struct WorktreeAddResult {
    pub path: std::path::PathBuf,
    pub api_request: Option<ApiWorktreeAddRequest>,
    pub result: Result<(), String>,
}

#[derive(Debug)]
pub struct ApiWorktreeRemoveRequest {
    pub id: String,
    pub operation_id: u64,
    pub checkout_key: std::path::PathBuf,
    pub shutdown_panes: Vec<crate::layout::PaneId>,
    pub respond_to: std::sync::mpsc::Sender<String>,
}

#[derive(Debug)]
pub struct WorktreeRemoveResult {
    pub workspace_id: String,
    pub path: std::path::PathBuf,
    pub workspace: Option<Box<crate::api::schema::WorkspaceInfo>>,
    pub worktree: Option<Box<crate::api::schema::WorktreeInfo>>,
    pub forced: bool,
    pub api_request: Option<ApiWorktreeRemoveRequest>,
    pub result: Result<(), String>,
}

#[derive(Debug)]
pub struct WorktreeReadResult {
    // Keep the slot until completion is consumed, including time queued on the app loop.
    pub(crate) _permit: tokio::sync::OwnedSemaphorePermit,
    pub(crate) client_local: bool,
    pub(crate) request: crate::api::schema::Request,
    pub(crate) source_workspace_id: Option<String>,
    pub(crate) source_cwd: Option<std::path::PathBuf>,
    pub(crate) result: Result<WorktreeReadData, (String, String)>,
    pub(crate) respond_to: std::sync::mpsc::Sender<String>,
}

#[derive(Debug)]
pub(crate) struct WorktreeReadData {
    pub source_checkout_path: std::path::PathBuf,
    pub source_repo_root: std::path::PathBuf,
    pub repo_key: String,
    pub repo_name: String,
    pub entries: Vec<crate::worktree::ExistingWorktree>,
}

/// Result of a background usage refresh for one tracked terminal (`src/app/usage.rs`).
/// `usage: None` means the session's file could not be resolved or read this
/// round (e.g. hook not installed, or the file disappeared); the caller must
/// treat that as "n/a", never fabricate a number.
#[derive(Debug, Clone)]
pub struct TerminalUsageUpdate {
    pub terminal_id: crate::terminal::TerminalId,
    pub usage: Option<mercury_usage::SessionUsage>,
}

/// An event from a background task to the main loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A pane's child process exited.
    PaneDied {
        pane_id: PaneId,
        exit_reason: crate::platform::ChildExitReason,
    },
    /// A worktree-removal runtime could not be restored normally.
    WorktreeRuntimeRestoreFailed { pane_id: PaneId, operation_id: u64 },
    /// Process detection identified an agent before its screen state was confirmed.
    AgentProcessDetected {
        pane_id: PaneId,
        agent: Agent,
        observed_at: Instant,
    },
    /// The current Codex input screen is visible during managed startup.
    CodexPromptObserved { pane_id: PaneId, ready: bool },
    /// Fallback detector state changed in a pane.
    StateChanged {
        pane_id: PaneId,
        agent: Option<Agent>,
        state: AgentState,
        visible_blocker: bool,
        visible_working: bool,
        process_exited: bool,
        observed_at: Instant,
    },
    /// Hook-authoritative agent state was reported for a pane.
    HookStateReported {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        state: AgentState,
        message: Option<String>,
        seq: Option<u64>,
        session_ref: Option<crate::agent_resume::AgentSessionRef>,
    },
    /// Agent session identity was reported without state authority.
    AgentSessionReported {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        seq: Option<u64>,
        session_ref: Option<crate::agent_resume::AgentSessionRef>,
        session_start_source: Option<String>,
    },
    /// A reporter supplied the command that resumes its own session.
    AgentResumeReported {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        seq: Option<u64>,
        argv: Vec<String>,
    },
    /// A pane held by a self-reported agent is back at its idle shell.
    ReportedAgentShellReturned {
        pane_id: PaneId,
        observed_at: std::time::Instant,
    },
    /// Display-only agent metadata was reported for a pane.
    HookMetadataReported {
        pane_id: PaneId,
        source: String,
        agent_label: Option<String>,
        applies_to_source: Option<String>,
        title: Option<String>,
        display_agent: Option<String>,
        state_labels: std::collections::HashMap<String, String>,
        clear_title: bool,
        clear_display_agent: bool,
        clear_state_labels: bool,
        seq: Option<u64>,
        ttl: Option<std::time::Duration>,
    },
    /// Hook authority was explicitly cleared for a pane.
    HookAuthorityCleared {
        pane_id: PaneId,
        source: Option<String>,
        seq: Option<u64>,
    },
    /// The current detected agent gracefully released this pane back to the shell.
    HookAgentReleased {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        known_agent: Option<Agent>,
        seq: Option<u64>,
    },
    /// A new version is available through the active installation manager.
    UpdateReady {
        version: String,
        install_command: String,
    },
    /// Remote agent detection manifest update check finished.
    AgentDetectionManifestsUpdated {
        updated: Vec<crate::detect::manifest_update::ManifestUpdateCommit>,
        activated: Vec<crate::detect::Agent>,
        status: crate::detect::manifest_update::ManifestUpdateStatus,
    },
    /// A pane child emitted one or more executable BEL characters.
    /// The host-facing process forwards them to its outer terminal.
    TerminalBell { pane_id: PaneId, count: u16 },
    /// A pane child emitted a valid OSC 52 clipboard write. The main loop
    /// re-emits it through mercury's own clipboard writer.
    ClipboardWrite { content: Vec<u8> },
    /// A pane child reported its shell current directory through terminal
    /// metadata such as OSC 7.
    TerminalCwdReported {
        pane_id: PaneId,
        cwd: std::path::PathBuf,
    },
    /// Background git status refresh completed for workspaces.
    GitStatusRefreshed {
        results: Vec<WorkspaceGitStatus>,
        cache_updates: Vec<(std::path::PathBuf, GitStatusCacheEntry)>,
    },
    /// Background usage refresh completed for tracked Claude/Codex sessions
    /// (`src/app/usage.rs`, `docs/dev/usage-ui-design.md` D1/D3).
    UsageRefreshed {
        sessions: Vec<TerminalUsageUpdate>,
        limits: Vec<mercury_usage::ProviderLimits>,
    },
    /// The bounded startup scan of agent-owned JSONL completed.
    UsageHistoryScanned {
        sessions: Vec<mercury_usage::history_scan::HistoricalSession>,
    },
    /// Read-only Codex app-server usage poll completed in a background thread.
    CodexUsageRefreshed {
        identity: Option<String>,
        limits: Option<mercury_usage::ProviderLimits>,
        costs: Vec<(String, f64)>,
        error: Option<String>,
    },
    ClaudeAccountUsageRefreshed {
        limits: Option<mercury_usage::ProviderLimits>,
        identity: Option<String>,
        error: Option<String>,
    },
    /// An on-demand Codex account-limit read for queued route previews ended.
    RouteLimitsRefreshed {
        limits: Option<mercury_usage::ProviderLimits>,
    },
    /// The cost ledger thread (`src/app/usage_ledger.rs`, D9) replayed past
    /// entries at startup or appended a new delta from a statusline report.
    UsageCostRecorded {
        entries: Vec<mercury_usage::CostLedgerEntry>,
    },
    /// A scheduled command finished off-thread; the app persists its bounded
    /// output and final ledger status on the server-owner thread.
    CronCompleted {
        run: Box<mercury_cron::scheduler::ReservedRun>,
        result: Result<mercury_cron::executor::CommandOutput, String>,
    },
    /// A reversible Learn edit changed the route table in config.toml.
    LearnRouteChanged,
    /// A reversible Learn edit or undo changed one allowlisted Cron job.
    LearnCronChanged,
    /// A configured tab bar status command finished.
    TabBarCommandFinished {
        generation: u64,
        segment_index: usize,
        result: Result<Option<String>, String>,
    },
    /// A plugin action or event command finished.
    PluginCommandFinished {
        log_id: String,
        finished_unix_ms: u64,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        error: Option<String>,
    },
    /// Background `git worktree add` completed.
    WorktreeAddFinished(Box<WorktreeAddResult>),
    /// Background `git worktree remove` completed.
    WorktreeRemoveFinished(Box<WorktreeRemoveResult>),
    /// Background worktree discovery completed for an API list/open request.
    WorktreeReadFinished(Box<WorktreeReadResult>),
}
