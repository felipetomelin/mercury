//! Budgets: a limit over a window, applied to a scope (PLAN.md 3.1).

use std::collections::HashSet;

use serde::Deserialize;
use time::{OffsetDateTime, UtcOffset};

use crate::model::{Provider, ProviderLimits, TokenCounts};
use crate::window::{CalendarPeriod, CalendarWindow, UsageWindow, WindowBounds};

/// Where a budget applies. Matching is done against [`UsageRecord`] attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Provider(Provider),
    Workspace(String),
    /// An agent name (e.g. `reviewer`) or a pane id.
    Agent(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Input,
    Output,
    CacheRead,
    CacheWrite,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Limit {
    Usd(f64),
    /// Counted over the selected token kinds.
    Tokens {
        max: u64,
        count: Vec<TokenKind>,
    },
    /// Only meaningful for provider windows.
    Percent(f64),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnExceed {
    #[default]
    Notify,
    Route,
    Pause,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub name: String,
    pub scope: Scope,
    pub window: UsageWindow,
    pub limit: Limit,
    /// Percentages that trigger a warning, ascending.
    pub warn_at: Vec<u8>,
    pub on_exceed: OnExceed,
}

/// One attributed unit of usage. Token records come from transcripts/rollouts;
/// cost records come from deltas of agent-reported session cost (statusline),
/// since transcripts carry no prices.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub at: OffsetDateTime,
    pub provider: Provider,
    pub session_id: String,
    pub workspace: Option<String>,
    pub agent: Option<String>,
    pub pane: Option<String>,
    pub tokens: TokenCounts,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetLevel {
    Ok,
    Warn,
    Exceeded,
    /// The source doesn't report this window (e.g. Codex business plans).
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetStatus {
    pub name: String,
    pub used: f64,
    pub limit: f64,
    /// `used / limit`; `None` when unavailable.
    pub fraction: Option<f64>,
    pub level: BudgetLevel,
    /// Highest `warn_at` threshold reached, for alert de-duplication.
    pub warn_threshold: Option<u8>,
    pub bounds: Option<WindowBounds>,
    pub resets_at: Option<OffsetDateTime>,
    /// USD budget with in-scope token usage from sessions that never reported cost.
    pub partial: bool,
    /// Linear burn-rate forecast of when the limit is hit inside this window.
    pub forecast_exhausted_at: Option<OffsetDateTime>,
}

impl Scope {
    pub fn matches(&self, record: &UsageRecord) -> bool {
        match self {
            Self::Global => true,
            Self::Provider(provider) => record.provider == *provider,
            Self::Workspace(workspace) => record.workspace.as_deref() == Some(workspace.as_str()),
            Self::Agent(agent) => {
                record.agent.as_deref() == Some(agent.as_str())
                    || record.pane.as_deref() == Some(agent.as_str())
                    || record.session_id == *agent
            }
        }
    }

    fn provider(&self) -> Option<Provider> {
        match self {
            Self::Provider(provider) => Some(*provider),
            _ => None,
        }
    }
}

fn count_tokens(tokens: &TokenCounts, kinds: &[TokenKind]) -> u64 {
    kinds.iter().fold(0u64, |sum, kind| {
        sum.saturating_add(match kind {
            TokenKind::Input => tokens.input,
            TokenKind::Output => tokens.output,
            TokenKind::CacheRead => tokens.cache_read,
            TokenKind::CacheWrite => tokens.cache_write,
        })
    })
}

impl Budget {
    fn level(&self, fraction: f64) -> (BudgetLevel, Option<u8>) {
        let percent = fraction * 100.0;
        let threshold = self
            .warn_at
            .iter()
            .copied()
            .filter(|threshold| percent >= f64::from(*threshold))
            .max();
        let level = if percent >= 100.0 {
            BudgetLevel::Exceeded
        } else if threshold.is_some() {
            BudgetLevel::Warn
        } else {
            BudgetLevel::Ok
        };
        (level, threshold)
    }

    fn unavailable(&self, limit: f64) -> BudgetStatus {
        BudgetStatus {
            name: self.name.clone(),
            used: 0.0,
            limit,
            fraction: None,
            level: BudgetLevel::Unavailable,
            warn_threshold: None,
            bounds: None,
            resets_at: None,
            partial: false,
            forecast_exhausted_at: None,
        }
    }

    /// Evaluates the budget at `now`. `limits` are the latest provider reports.
    pub fn evaluate(
        &self,
        records: &[UsageRecord],
        limits: &[ProviderLimits],
        now: OffsetDateTime,
    ) -> BudgetStatus {
        if let UsageWindow::Provider { key } = &self.window {
            return self.evaluate_provider(key, limits);
        }
        let limit = match &self.limit {
            Limit::Usd(max) => *max,
            Limit::Tokens { max, .. } => *max as f64,
            Limit::Percent(max) => return self.unavailable(*max),
        };
        let Some(bounds) = self.window.bounds(now) else {
            return self.unavailable(limit);
        };
        let rolling = matches!(self.window, UsageWindow::Rolling { .. });
        let in_scope = records.iter().filter(|record| self.scope.matches(record));
        let mut used = 0.0;
        let mut cost_sessions = HashSet::new();
        let mut token_sessions = HashSet::new();
        for record in in_scope {
            if record.cost_usd.is_some() {
                cost_sessions.insert(record.session_id.as_str());
            }
            // A rolling window ends at `now` and includes it; calendar ends are exclusive.
            let inside = bounds.contains(record.at) || (rolling && record.at == bounds.end);
            if !inside {
                continue;
            }
            match &self.limit {
                Limit::Usd(_) => {
                    used += record.cost_usd.unwrap_or(0.0);
                    if !record.tokens.is_zero() {
                        token_sessions.insert(record.session_id.as_str());
                    }
                }
                Limit::Tokens { count, .. } => {
                    used += count_tokens(&record.tokens, count) as f64;
                }
                Limit::Percent(_) => {}
            }
        }
        let partial = matches!(self.limit, Limit::Usd(_))
            && token_sessions
                .iter()
                .any(|session| !cost_sessions.contains(session));
        let fraction = if limit > 0.0 { used / limit } else { 0.0 };
        let (level, warn_threshold) = self.level(fraction);
        let calendar = matches!(self.window, UsageWindow::Calendar(_));
        BudgetStatus {
            name: self.name.clone(),
            used,
            limit,
            fraction: Some(fraction),
            level,
            warn_threshold,
            bounds: Some(bounds),
            resets_at: calendar.then_some(bounds.end),
            partial,
            forecast_exhausted_at: if calendar {
                forecast(used, limit, bounds, now)
            } else {
                None
            },
        }
    }

    fn evaluate_provider(&self, key: &str, limits: &[ProviderLimits]) -> BudgetStatus {
        let cap = match self.limit {
            Limit::Percent(max) => max,
            _ => 100.0,
        };
        let reported = limits
            .iter()
            .filter(|limits| {
                self.scope
                    .provider()
                    .is_none_or(|provider| provider == limits.provider)
            })
            .find_map(|limits| limits.get(key));
        let Some(reported) = reported else {
            return self.unavailable(cap);
        };
        let fraction = if cap > 0.0 {
            reported.used_percent / cap
        } else {
            0.0
        };
        let (level, warn_threshold) = self.level(fraction);
        BudgetStatus {
            name: self.name.clone(),
            used: reported.used_percent,
            limit: cap,
            fraction: Some(fraction),
            level,
            warn_threshold,
            bounds: None,
            resets_at: reported.resets_at,
            partial: false,
            forecast_exhausted_at: None,
        }
    }
}

fn forecast(
    used: f64,
    limit: f64,
    bounds: WindowBounds,
    now: OffsetDateTime,
) -> Option<OffsetDateTime> {
    if used <= 0.0 || used >= limit {
        return None;
    }
    let elapsed = (now - bounds.start).as_seconds_f64();
    if elapsed <= 0.0 {
        return None;
    }
    // remaining / (used / elapsed), arranged to stay exact for round numbers.
    let remaining_seconds = (limit - used) * elapsed / used;
    let window_left = (bounds.end - now).as_seconds_f64();
    if !remaining_seconds.is_finite() || remaining_seconds >= window_left {
        return None;
    }
    Some(now + time::Duration::seconds_f64(remaining_seconds))
}

// ---------------------------------------------------------------------------
// Config (`[[usage.budget]]` in config.toml)
// ---------------------------------------------------------------------------

/// Raw `[[usage.budget]]` entry. Convert with `Budget::try_from`.
///
/// ```toml
/// [[usage.budget]]
/// name = "claude-monthly"
/// scope = { provider = "claude" }
/// window = { kind = "calendar", period = "monthly", reset_day = 1, utc_offset = "-03:00" }
/// limit = { usd = 1000 }
/// warn_at = [70, 90]
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct BudgetConfig {
    pub name: String,
    #[serde(default)]
    pub scope: ScopeConfig,
    pub window: WindowConfig,
    /// Optional for provider windows (defaults to 100%).
    #[serde(default)]
    pub limit: LimitConfig,
    #[serde(default = "default_warn_at")]
    pub warn_at: Vec<u8>,
    #[serde(default)]
    pub on_exceed: OnExceed,
}

fn default_warn_at() -> Vec<u8> {
    vec![70, 90]
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScopeConfig {
    pub provider: Option<Provider>,
    pub workspace: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WindowConfig {
    Rolling {
        hours: Option<u32>,
        minutes: Option<u32>,
    },
    Calendar {
        period: CalendarPeriodConfig,
        #[serde(default = "default_reset_day")]
        reset_day: u8,
        #[serde(default)]
        reset_hour: u8,
        /// `"+HH:MM"` / `"-HH:MM"`; defaults to UTC.
        utc_offset: Option<String>,
        /// Reserved: named zones are rejected with a hint to use `utc_offset`.
        tz: Option<String>,
    },
    Provider {
        key: String,
    },
}

fn default_reset_day() -> u8 {
    1
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarPeriodConfig {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LimitConfig {
    pub usd: Option<f64>,
    pub tokens: Option<u64>,
    pub percent: Option<f64>,
    /// Token kinds counted by `tokens`; all kinds when omitted.
    pub count: Option<Vec<TokenKind>>,
}

pub fn parse_utc_offset(value: &str) -> Result<UtcOffset, String> {
    let invalid = || format!("invalid utc_offset `{value}`, expected +HH:MM or -HH:MM");
    if value.eq_ignore_ascii_case("utc") || value == "Z" {
        return Ok(UtcOffset::UTC);
    }
    let (sign, rest) = match value.as_bytes().first() {
        Some(b'+') => (1i8, &value[1..]),
        Some(b'-') => (-1i8, &value[1..]),
        _ => return Err(invalid()),
    };
    let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
    let hours: i8 = hours.parse().map_err(|_| invalid())?;
    let minutes: i8 = minutes.parse().map_err(|_| invalid())?;
    if !(0..=14).contains(&hours) || !(0..60).contains(&minutes) {
        return Err(invalid());
    }
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).map_err(|_| invalid())
}

impl TryFrom<BudgetConfig> for Budget {
    type Error = String;

    fn try_from(config: BudgetConfig) -> Result<Self, String> {
        let name = config.name;
        let scope = match (
            config.scope.provider,
            config.scope.workspace,
            config.scope.agent,
        ) {
            (None, None, None) => Scope::Global,
            (Some(provider), None, None) => Scope::Provider(provider),
            (None, Some(workspace), None) => Scope::Workspace(workspace),
            (None, None, Some(agent)) => Scope::Agent(agent),
            _ => {
                return Err(format!(
                    "budget `{name}`: scope takes exactly one of provider/workspace/agent"
                ))
            }
        };
        let window = match config.window {
            WindowConfig::Rolling { hours, minutes } => {
                let total = hours
                    .unwrap_or(0)
                    .saturating_mul(60)
                    .saturating_add(minutes.unwrap_or(0));
                if total == 0 {
                    return Err(format!(
                        "budget `{name}`: rolling window needs hours or minutes"
                    ));
                }
                UsageWindow::Rolling { minutes: total }
            }
            WindowConfig::Calendar {
                period,
                reset_day,
                reset_hour,
                utc_offset,
                tz,
            } => {
                if let Some(tz) = tz {
                    return Err(format!(
                        "budget `{name}`: named time zone `{tz}` is not supported, use utc_offset (e.g. \"-03:00\")"
                    ));
                }
                let offset = match utc_offset {
                    Some(offset) => parse_utc_offset(&offset)
                        .map_err(|err| format!("budget `{name}`: {err}"))?,
                    None => UtcOffset::UTC,
                };
                let period = match period {
                    CalendarPeriodConfig::Daily => CalendarPeriod::Daily,
                    CalendarPeriodConfig::Weekly => CalendarPeriod::Weekly,
                    CalendarPeriodConfig::Monthly => CalendarPeriod::Monthly,
                };
                UsageWindow::Calendar(
                    CalendarWindow::new(period, reset_day, reset_hour, offset)
                        .map_err(|err| format!("budget `{name}`: {err}"))?,
                )
            }
            WindowConfig::Provider { key } => UsageWindow::Provider { key },
        };
        let provider_window = matches!(window, UsageWindow::Provider { .. });
        let limit = match (config.limit.usd, config.limit.tokens, config.limit.percent) {
            (Some(usd), None, None) if !provider_window => Limit::Usd(usd),
            (None, Some(max), None) if !provider_window => Limit::Tokens {
                max,
                count: config.limit.count.unwrap_or_else(|| {
                    vec![
                        TokenKind::Input,
                        TokenKind::Output,
                        TokenKind::CacheRead,
                        TokenKind::CacheWrite,
                    ]
                }),
            },
            (None, None, Some(percent)) if provider_window => Limit::Percent(percent),
            (None, None, None) if provider_window => Limit::Percent(100.0),
            _ if provider_window => {
                return Err(format!(
                    "budget `{name}`: provider windows only take a percent limit"
                ));
            }
            _ => {
                return Err(format!(
                    "budget `{name}`: limit takes exactly one of usd/tokens"
                ))
            }
        };
        let mut warn_at = config.warn_at;
        warn_at.sort_unstable();
        warn_at.dedup();
        Ok(Self {
            name,
            scope,
            window,
            limit,
            warn_at,
            on_exceed: config.on_exceed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderLimit;

    fn ts(value: &str) -> OffsetDateTime {
        crate::parse_timestamp(value).unwrap()
    }

    fn record(
        at: &str,
        provider: Provider,
        session: &str,
        tokens: u64,
        cost: Option<f64>,
    ) -> UsageRecord {
        UsageRecord {
            at: ts(at),
            provider,
            session_id: session.into(),
            workspace: Some("mercury".into()),
            agent: (session == "rev").then(|| "reviewer".into()),
            pane: None,
            tokens: TokenCounts {
                input: tokens,
                ..TokenCounts::default()
            },
            cost_usd: cost,
        }
    }

    #[derive(Deserialize)]
    struct Usage {
        budget: Vec<BudgetConfig>,
    }

    fn budgets(toml_text: &str) -> Vec<Budget> {
        toml::from_str::<Usage>(toml_text)
            .unwrap()
            .budget
            .into_iter()
            .map(|config| Budget::try_from(config).unwrap())
            .collect()
    }

    const PLAN_EXAMPLE: &str = r#"
[[budget]]
name = "claude-mensal"
scope = { provider = "claude" }
window = { kind = "calendar", period = "monthly", reset_day = 1, utc_offset = "-03:00" }
limit = { usd = 1000 }
warn_at = [90, 70]

[[budget]]
name = "reviewer-3h"
scope = { agent = "reviewer" }
window = { kind = "rolling", hours = 3 }
limit = { tokens = 2_000_000 }
on_exceed = "route"

[[budget]]
name = "claude-5h"
scope = { provider = "claude" }
window = { kind = "provider", key = "five_hour" }
"#;

    #[test]
    fn parses_plan_config_example() {
        let parsed = budgets(PLAN_EXAMPLE);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].scope, Scope::Provider(Provider::Claude));
        assert_eq!(parsed[0].limit, Limit::Usd(1000.0));
        assert_eq!(parsed[0].warn_at, vec![70, 90]);
        assert!(
            matches!(parsed[0].window, UsageWindow::Calendar(calendar) if calendar.utc_offset.whole_hours() == -3)
        );
        assert_eq!(parsed[1].window, UsageWindow::Rolling { minutes: 180 });
        assert_eq!(parsed[1].on_exceed, OnExceed::Route);
        assert_eq!(parsed[2].limit, Limit::Percent(100.0));
    }

    #[test]
    fn rejects_ambiguous_or_unsupported_config() {
        for (text, needle) in [
            (
                r#"[[budget]]
name = "x"
scope = { provider = "claude", agent = "a" }
window = { kind = "rolling", hours = 1 }
limit = { usd = 1 }"#,
                "exactly one of provider",
            ),
            (
                r#"[[budget]]
name = "x"
window = { kind = "calendar", period = "monthly", tz = "America/Sao_Paulo" }
limit = { usd = 1 }"#,
                "utc_offset",
            ),
            (
                r#"[[budget]]
name = "x"
window = { kind = "provider", key = "five_hour" }
limit = { usd = 1 }"#,
                "percent",
            ),
            (
                r#"[[budget]]
name = "x"
window = { kind = "rolling" }
limit = { usd = 1 }"#,
                "hours or minutes",
            ),
        ] {
            let config = toml::from_str::<Usage>(text).unwrap().budget.remove(0);
            let err = Budget::try_from(config).unwrap_err();
            assert!(err.contains(needle), "{err}");
        }
    }

    #[test]
    fn monthly_usd_budget_sums_cost_in_window_and_forecasts() {
        let budget = budgets(PLAN_EXAMPLE).remove(0);
        let records = vec![
            // Previous month: excluded.
            record(
                "2026-09-30T12:00:00Z",
                Provider::Claude,
                "a",
                0,
                Some(500.0),
            ),
            record(
                "2026-10-02T12:00:00Z",
                Provider::Claude,
                "a",
                0,
                Some(100.0),
            ),
            record(
                "2026-10-04T12:00:00Z",
                Provider::Claude,
                "b",
                0,
                Some(150.0),
            ),
            // Other provider: excluded.
            record("2026-10-04T12:00:00Z", Provider::Codex, "c", 0, Some(999.0)),
        ];
        let now = ts("2026-10-06T03:00:00Z"); // 5 days into the BRT month.
        let status = budget.evaluate(&records, &[], now);
        assert_eq!(status.used, 250.0);
        assert_eq!(status.level, BudgetLevel::Ok);
        assert!(!status.partial);
        assert_eq!(status.resets_at, Some(ts("2026-11-01T03:00:00Z")));
        // 250 USD in 5 days -> 50/day -> 750 left = 15 more days.
        assert_eq!(
            status.forecast_exhausted_at,
            Some(ts("2026-10-21T03:00:00Z"))
        );
    }

    #[test]
    fn usd_budget_is_partial_when_a_session_has_no_cost_source() {
        let budget = budgets(PLAN_EXAMPLE).remove(0);
        let records = vec![
            record("2026-10-02T12:00:00Z", Provider::Claude, "a", 500, None),
            record("2026-10-02T12:00:01Z", Provider::Claude, "a", 0, Some(1.0)),
            record("2026-10-03T12:00:00Z", Provider::Claude, "old", 900, None),
        ];
        let status = budget.evaluate(&records, &[], ts("2026-10-05T12:00:00Z"));
        assert!(status.partial);
        assert_eq!(status.used, 1.0);
    }

    #[test]
    fn agent_token_budget_warns_and_exceeds() {
        let budget = budgets(PLAN_EXAMPLE).remove(1);
        let now = ts("2026-10-05T12:00:00Z");
        let mut records = vec![
            record(
                "2026-10-05T08:00:00Z",
                Provider::Claude,
                "rev",
                5_000_000,
                None,
            ), // outside 3h
            record(
                "2026-10-05T10:00:00Z",
                Provider::Claude,
                "rev",
                1_500_000,
                None,
            ),
            record(
                "2026-10-05T11:00:00Z",
                Provider::Claude,
                "other",
                1_500_000,
                None,
            ), // other agent
        ];
        let status = budget.evaluate(&records, &[], now);
        assert_eq!(status.used, 1_500_000.0);
        assert_eq!(status.level, BudgetLevel::Warn);
        assert_eq!(status.warn_threshold, Some(70));
        assert_eq!(status.resets_at, None);

        records.push(record(
            "2026-10-05T12:00:00Z",
            Provider::Claude,
            "rev",
            600_000,
            None,
        ));
        let status = budget.evaluate(&records, &[], now);
        assert_eq!(status.level, BudgetLevel::Exceeded);
        assert_eq!(status.warn_threshold, Some(90));
    }

    #[test]
    fn provider_window_uses_reported_percent_or_is_unavailable() {
        let budget = budgets(PLAN_EXAMPLE).remove(2);
        let now = ts("2026-10-05T12:00:00Z");
        let status = budget.evaluate(&[], &[], now);
        assert_eq!(status.level, BudgetLevel::Unavailable);
        assert_eq!(status.fraction, None);

        let limits = ProviderLimits {
            provider: Provider::Claude,
            limits: vec![ProviderLimit {
                key: "five_hour".into(),
                used_percent: 75.0,
                window_minutes: Some(300),
                resets_at: Some(ts("2026-10-05T14:00:00Z")),
                used_usd: None,
                limit_usd: None,
                period: None,
            }],
            credits: None,
            plan_type: None,
            limit_reached_type: None,
            observed_at: Some(now),
        };
        let mut codex = limits.clone();
        codex.provider = Provider::Codex;
        codex.limits[0].used_percent = 99.0;
        let status = budget.evaluate(&[], &[codex, limits], now);
        assert_eq!(status.used, 75.0);
        assert_eq!(status.level, BudgetLevel::Warn);
        assert_eq!(status.resets_at, Some(ts("2026-10-05T14:00:00Z")));
    }

    #[test]
    fn parses_utc_offsets() {
        assert_eq!(parse_utc_offset("-03:00").unwrap().whole_minutes(), -180);
        assert_eq!(parse_utc_offset("+05:30").unwrap().whole_minutes(), 330);
        assert_eq!(parse_utc_offset("UTC").unwrap(), UtcOffset::UTC);
        assert!(parse_utc_offset("03:00").is_err());
        assert!(parse_utc_offset("+25:00").is_err());
    }
}
