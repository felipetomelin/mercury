//! `[usage]` config (Fase 2 passo 5, D7 in `docs/dev/usage-ui-design.md`):
//! poll cadence, the context-fill alert threshold (`compact_at`), and
//! `[[usage.budget]]` entries whose shape is `mercury_usage::BudgetConfig`
//! verbatim — this module only wires that crate's config type into Mercury's
//! own config file, it never redefines the budget schema.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Default global context-fill percentage that triggers the "contexto alto"
/// toast (`src/client/shell/endpoint_agent_state.rs`). Was a hardcoded
/// constant (`CONTEXT_ALERT_PERMILLE`) until this config landed.
const DEFAULT_COMPACT_AT: u8 = 80;
const DEFAULT_POLL_ACTIVE_MS: u64 = 1000;
const DEFAULT_POLL_IDLE_MS: u64 = 10_000;

fn deserialize_usage_agents<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, UsageAgentConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let agents = BTreeMap::<String, UsageAgentConfig>::deserialize(deserializer)?;
    for id in agents.keys() {
        if crate::detect::parse_canonical_agent_label(id).is_none() {
            return Err(serde::de::Error::custom(format!(
                "unknown canonical agent id `{id}` in usage.agents"
            )));
        }
    }
    Ok(agents)
}

/// `[usage]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UsageConfig {
    pub enabled: bool,
    /// Poll cadence (ms) while a tracked pane's agent is actively working.
    pub poll_active_ms: u64,
    /// Poll cadence (ms) while a tracked pane's agent is idle/blocked/done.
    pub poll_idle_ms: u64,
    /// Global context-fill percentage (0..=100) that triggers the "contexto
    /// alto" toast. Overridable per agent, see `agents`.
    pub compact_at: u8,
    /// `[[usage.budget]]`; each entry is `mercury_usage::BudgetConfig`.
    #[serde(rename = "budget")]
    pub budgets: Vec<mercury_usage::BudgetConfig>,
    /// `[usage.agents.<canonical-agent-id>]`, keyed the same way as
    /// `ui.sidebar.agents.rows_by_agent` (`crate::detect::parse_canonical_agent_label`).
    #[serde(default, deserialize_with = "deserialize_usage_agents")]
    pub agents: BTreeMap<String, UsageAgentConfig>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_active_ms: DEFAULT_POLL_ACTIVE_MS,
            poll_idle_ms: DEFAULT_POLL_IDLE_MS,
            compact_at: DEFAULT_COMPACT_AT,
            budgets: Vec::new(),
            agents: BTreeMap::new(),
        }
    }
}

/// `[usage.agents.<id>]`.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct UsageAgentConfig {
    pub compact_at: Option<u8>,
}

impl UsageConfig {
    /// Context-fill alert threshold in permille (0..=1000) for `agent`
    /// (a canonical agent id, e.g. `"claude"`), falling back to the global
    /// `compact_at` when the agent has no override or isn't known.
    pub fn compact_at_permille_for(&self, agent: Option<&str>) -> u16 {
        let percent = agent
            .and_then(|agent| self.agents.get(agent))
            .and_then(|config| config.compact_at)
            .unwrap_or(self.compact_at);
        u16::from(percent.min(100)) * 10
    }

    /// Resolves every `[[usage.budget]]` entry into a `mercury_usage::Budget`,
    /// skipping (and reporting) entries that fail validation (ambiguous
    /// scope, unsupported `tz`, etc. — see `mercury_usage::budget::Budget::try_from`)
    /// instead of rejecting the whole `[usage]` section.
    pub fn resolved_budgets(&self) -> (Vec<mercury_usage::Budget>, Vec<String>) {
        let mut budgets = Vec::with_capacity(self.budgets.len());
        let mut diagnostics = Vec::new();
        for budget in &self.budgets {
            match mercury_usage::Budget::try_from(budget.clone()) {
                Ok(budget) => budgets.push(budget),
                Err(err) => diagnostics.push(format!("usage.budget: {err}; skipping")),
            }
        }
        (budgets, diagnostics)
    }

    pub fn poll_active(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.poll_active_ms.max(1))
    }

    pub fn poll_idle(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.poll_idle_ms.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_from(text: &str) -> UsageConfig {
        #[derive(Deserialize)]
        struct Wrapper {
            usage: UsageConfig,
        }
        toml::from_str::<Wrapper>(text).unwrap().usage
    }

    #[test]
    fn defaults_match_design_doc() {
        let config = UsageConfig::default();
        assert!(config.enabled);
        assert_eq!(config.poll_active_ms, 1000);
        assert_eq!(config.poll_idle_ms, 10_000);
        assert_eq!(config.compact_at, 80);
        assert!(config.budgets.is_empty());
    }

    #[test]
    fn parses_full_plan_example() {
        let config = usage_from(
            r#"
[usage]
enabled = true
poll_active_ms = 500
poll_idle_ms = 5000
compact_at = 75

[[usage.budget]]
name = "claude-mensal"
scope = { provider = "claude" }
window = { kind = "calendar", period = "monthly", reset_day = 1, utc_offset = "-03:00" }
limit = { usd = 1000 }
warn_at = [70, 90]

[[usage.budget]]
name = "reviewer-3h"
scope = { agent = "reviewer" }
window = { kind = "rolling", hours = 3 }
limit = { tokens = 2000000 }
on_exceed = "route"

[usage.agents.claude]
compact_at = 90
"#,
        );
        assert_eq!(config.poll_active_ms, 500);
        assert_eq!(config.budgets.len(), 2);
        assert_eq!(
            config
                .agents
                .get("claude")
                .and_then(|agent| agent.compact_at),
            Some(90)
        );
        let (budgets, diagnostics) = config.resolved_budgets();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(budgets.len(), 2);
        assert_eq!(config.compact_at_permille_for(Some("claude")), 900);
        assert_eq!(config.compact_at_permille_for(Some("codex")), 750);
        assert_eq!(config.compact_at_permille_for(None), 750);
    }

    #[test]
    fn rejects_unknown_agent_id() {
        let text = r#"
[usage.agents.not-a-real-agent]
compact_at = 50
"#;
        #[derive(Debug, Deserialize)]
        struct Wrapper {
            #[allow(dead_code)]
            usage: UsageConfig,
        }
        let err = toml::from_str::<Wrapper>(text).unwrap_err();
        assert!(err.to_string().contains("unknown canonical agent id"));
    }

    #[test]
    fn resolved_budgets_skips_invalid_entries_but_keeps_the_rest() {
        let config = usage_from(
            r#"
[[usage.budget]]
name = "bad"
scope = { provider = "claude", agent = "x" }
window = { kind = "rolling", hours = 1 }
limit = { usd = 1 }

[[usage.budget]]
name = "good"
window = { kind = "rolling", hours = 1 }
limit = { usd = 1 }
"#,
        );
        let (budgets, diagnostics) = config.resolved_budgets();
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].name, "good");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("bad") || diagnostics[0].contains("exactly one of"));
    }

    #[test]
    fn tz_without_utc_offset_is_rejected_at_resolution() {
        let config = usage_from(
            r#"
[[usage.budget]]
name = "x"
window = { kind = "calendar", period = "monthly", tz = "America/Sao_Paulo" }
limit = { usd = 1 }
"#,
        );
        let (budgets, diagnostics) = config.resolved_budgets();
        assert!(budgets.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("utc_offset"));
    }
}
