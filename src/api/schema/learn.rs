use serde::{Deserialize, Serialize};

/// Local Claude SessionEnd hook signal. This is not a public authorization to
/// review arbitrary paths: the server checks pane ownership and session ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LearnClaudeSessionEndParams {
    pub pane_id: String,
    pub session_id: String,
    pub transcript_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LearnListParams {
    /// Defaults to 30; server refuses values outside 1–100.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LearnChangeTarget {
    pub change_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LearnUndoDateParams {
    /// UTC calendar date in YYYY-MM-DD form.
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnChangeInfo {
    pub id: String,
    pub target_id: String,
    pub status: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnChangeDetail {
    pub change: LearnChangeInfo,
    pub before_hash: String,
    pub after_hash: String,
    /// The model's reason is deliberately omitted from the public payload.
    pub reason_redacted: bool,
    /// `None` means an older server did not report backup verification.
    #[serde(default)]
    pub backup_available: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnDiffSummary {
    pub before_lines: usize,
    pub after_lines: usize,
    pub unchanged_prefix_lines: usize,
    pub unchanged_suffix_lines: usize,
    pub removed_lines: usize,
    pub added_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnDiffInfo {
    pub change: LearnChangeInfo,
    /// None means the after snapshot does not exist (older ledger item).
    pub summary: Option<LearnDiffSummary>,
    pub content_redacted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnUndoOutcomeKind {
    Undone,
    Conflict,
    Skipped,
    Failed,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnUndoOutcome {
    pub change_id: String,
    pub outcome: LearnUndoOutcomeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnChangeCounts {
    pub prepared: u64,
    pub applied: u64,
    pub undo_prepared: u64,
    pub conflict: u64,
    pub failed: u64,
    pub undone: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnTriggerCounts {
    pub claimed: u64,
    pub succeeded: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnTriggerInfo {
    /// Stable truncated digest of the private key; never a raw session ID.
    pub fingerprint: String,
    pub provider: String,
    pub kind: String,
    pub status: String,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnStatusInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<LearnRuntimeInfo>,
    pub ledger_present: bool,
    pub changes: LearnChangeCounts,
    pub triggers: LearnTriggerCounts,
    /// Newest entries only; empty when the ledger is absent.
    #[serde(default)]
    pub recent_triggers: Vec<LearnTriggerInfo>,
    /// Bounded, provider-observed headless runs; these are not an efficacy comparison.
    #[serde(default)]
    pub recent_metrics: Vec<LearnObservedMetricInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnRuntimeInfo {
    pub enabled: bool,
    pub external_allowed: bool,
    pub worker_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnObservedMetricInfo {
    pub task_key: String,
    pub provider: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microusd: Option<u64>,
    pub outcome: Option<String>,
    pub observed_at_ms: i64,
    /// Descriptive cohorts for one explicitly linked job/target, never causality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<LearnComparisonInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LearnComparisonInfo {
    pub baseline_runs: u64,
    pub after_runs: u64,
    pub baseline_average_tokens: u64,
    pub after_average_tokens: u64,
    pub baseline_succeeded: u64,
    pub after_succeeded: u64,
}
