//! Codex rollout reader (`~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl`).
//!
//! Relevant lines:
//! - `session_meta` → `payload.id` (thread/session id), `payload.cwd`.
//! - `turn_context` → `payload.model`.
//! - `event_msg` with `payload.type == "token_count"` → cumulative
//!   `info.total_token_usage`, `info.last_token_usage`, `info.model_context_window`,
//!   and account `rate_limits` (`primary`/`secondary`/`credits`, `null` on plans
//!   that don't report them).
//!
//! Per-request events are derived from deltas of the cumulative total, which
//! works on every rollout version (newer `token_usage_record` lines are not needed).
//! Only documented JSONL is read; Codex's internal SQLite files are never touched.

use serde_json::Value;
use time::OffsetDateTime;

use crate::model::{
    ContextUsage, Credits, Provider, ProviderLimit, ProviderLimits, SessionUsage, TokenCounts,
    UsageEvent,
};

fn u64_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Codex counts cached tokens inside `input_tokens`; normalize to uncached input.
fn codex_tokens(usage: &Value) -> TokenCounts {
    let input = u64_field(usage, "input_tokens");
    let cache_read = u64_field(usage, "cached_input_tokens");
    TokenCounts {
        input: input.saturating_sub(cache_read),
        cache_read,
        cache_write: u64_field(usage, "cache_write_input_tokens"),
        output: u64_field(usage, "output_tokens"),
        reasoning: u64_field(usage, "reasoning_output_tokens"),
    }
}

fn parse_window(
    key: &str,
    value: &Value,
    observed_at: Option<OffsetDateTime>,
) -> Option<ProviderLimit> {
    if !value.is_object() {
        return None;
    }
    let used_percent = value.get("used_percent").and_then(Value::as_f64)?;
    let resets_at = value
        .get("resets_at")
        .and_then(crate::parse_reset_value)
        .or_else(|| {
            let seconds = value.get("resets_in_seconds").and_then(Value::as_i64)?;
            observed_at.map(|at| at + time::Duration::seconds(seconds))
        });
    Some(ProviderLimit {
        key: key.to_owned(),
        used_percent,
        window_minutes: value.get("window_minutes").and_then(Value::as_u64),
        resets_at,
        used_usd: None,
        limit_usd: None,
        period: None,
    })
}

fn parse_rate_limits(value: &Value, observed_at: Option<OffsetDateTime>) -> Option<ProviderLimits> {
    if !value.is_object() {
        return None;
    }
    let limits = ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| parse_window(key, value.get(key)?, observed_at))
        .collect();
    let credits = value
        .get("credits")
        .filter(|credits| credits.is_object())
        .map(|credits| Credits {
            has_credits: credits
                .get("has_credits")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            unlimited: credits
                .get("unlimited")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            balance: match credits.get("balance") {
                Some(Value::String(text)) => Some(text.clone()),
                Some(Value::Number(number)) => Some(number.to_string()),
                _ => None,
            },
        });
    Some(ProviderLimits {
        provider: Provider::Codex,
        limits,
        credits,
        plan_type: str_field(value, "plan_type"),
        limit_reached_type: str_field(value, "rate_limit_reached_type"),
        observed_at,
    })
}

/// Accumulates usage of one rollout file.
#[derive(Debug, Clone, Default)]
pub struct CodexRollout {
    session_id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    last_total: Option<TokenCounts>,
    last_request: Option<Value>,
    context_window: Option<u64>,
    events: Vec<UsageEvent>,
    limits: Option<ProviderLimits>,
    updated_at: Option<OffsetDateTime>,
    skipped_lines: usize,
}

impl CodexRollout {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one JSONL line. Returns `true` when usage, model or limits changed.
    pub fn ingest_line(&mut self, line: &str) -> bool {
        let interesting = line.contains("\"session_meta\"")
            || line.contains("\"turn_context\"")
            || line.contains("\"token_count\"");
        if !interesting {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            self.skipped_lines += 1;
            return false;
        };
        let at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(crate::parse_timestamp);
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                let changed = self.session_id.is_none();
                self.session_id = self.session_id.take().or_else(|| str_field(payload, "id"));
                self.cwd = self.cwd.take().or_else(|| str_field(payload, "cwd"));
                changed
            }
            Some("turn_context") => {
                let model = str_field(payload, "model");
                let changed = model.is_some() && model != self.model;
                if model.is_some() {
                    self.model = model;
                }
                changed
            }
            Some("event_msg")
                if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
            {
                self.ingest_token_count(payload, at)
            }
            _ => false,
        }
    }

    fn ingest_token_count(&mut self, payload: &Value, at: Option<OffsetDateTime>) -> bool {
        let mut changed = false;
        if let Some(limits) = payload
            .get("rate_limits")
            .and_then(|limits| parse_rate_limits(limits, at))
        {
            changed |= self.limits.as_ref() != Some(&limits);
            self.limits = Some(limits);
        }
        let Some(info) = payload.get("info").filter(|info| info.is_object()) else {
            return changed;
        };
        if let Some(window) = info.get("model_context_window").and_then(Value::as_u64) {
            self.context_window = Some(window);
        }
        if let Some(last) = info.get("last_token_usage").filter(|last| last.is_object()) {
            self.last_request = Some(last.clone());
        }
        let Some(total) = info
            .get("total_token_usage")
            .filter(|total| total.is_object())
            .map(codex_tokens)
        else {
            return changed;
        };
        let delta = match self.last_total {
            Some(previous) => total.checked_delta(&previous).unwrap_or(total),
            None => total,
        };
        self.last_total = Some(total);
        if let Some(at) = at {
            self.updated_at = self.updated_at.max(Some(at));
            if !delta.is_zero() {
                self.events.push(UsageEvent {
                    provider: Provider::Codex,
                    session_id: self.session_id.clone().unwrap_or_default(),
                    at,
                    model: self.model.clone(),
                    tokens: delta,
                });
                changed = true;
            }
        }
        changed
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn events(&self) -> &[UsageEvent] {
        &self.events
    }

    /// Latest account-level limits seen in this rollout.
    pub fn limits(&self) -> Option<&ProviderLimits> {
        self.limits.as_ref()
    }

    pub fn skipped_lines(&self) -> usize {
        self.skipped_lines
    }

    /// Cumulative usage and context (`last_token_usage.total_tokens /
    /// model_context_window`, the same numbers the Codex TUI uses).
    pub fn session_usage(&self) -> Option<SessionUsage> {
        let totals = self.last_total?;
        let context = self.last_request.as_ref().map(|last| ContextUsage {
            used_tokens: last
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| {
                    let tokens = codex_tokens(last);
                    tokens.total()
                }),
            window_tokens: self.context_window,
            exact: true,
        });
        Some(SessionUsage {
            provider: Provider::Codex,
            session_id: self.session_id.clone().unwrap_or_default(),
            model: self.model.clone(),
            totals,
            context,
            cost_usd: None,
            updated_at: self.updated_at,
        })
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn parse_rollout(text: &str) -> CodexRollout {
    let mut rollout = CodexRollout::new();
    for line in text.lines() {
        rollout.ingest_line(line);
    }
    rollout
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/codex-rollout.jsonl");

    #[test]
    fn reads_session_model_and_cumulative_usage() {
        let rollout = parse_rollout(FIXTURE);
        assert_eq!(
            rollout.session_id(),
            Some("01a0ed25-0000-7000-8000-000000000001")
        );
        assert_eq!(rollout.cwd(), Some("/home/user/project"));
        let usage = rollout.session_usage().unwrap();
        assert_eq!(usage.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(
            usage.totals,
            TokenCounts {
                input: 30_000 - 20_000,
                cache_read: 20_000,
                cache_write: 0,
                output: 800,
                reasoning: 300,
            }
        );
        let context = usage.context.unwrap();
        assert_eq!(context.used_tokens, 11_163);
        assert_eq!(context.window_tokens, Some(258_400));
        assert!(context.exact);
    }

    #[test]
    fn derives_per_request_events_from_cumulative_deltas() {
        let rollout = parse_rollout(FIXTURE);
        let events = rollout.events();
        // Three token_count lines, but the repeated total produces no event and
        // the info:null line only carries limits.
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].tokens.input, 19_337 - 9_984);
        assert_eq!(events[1].tokens.output, 800 - 296);
        assert_eq!(events[1].model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(
            events.iter().map(|event| event.tokens).sum::<TokenCounts>(),
            rollout.session_usage().unwrap().totals
        );
    }

    #[test]
    fn null_rate_limits_are_reported_as_absent_with_credits() {
        let rollout = parse_rollout(FIXTURE);
        let limits = rollout.limits().unwrap();
        assert!(limits.limits.is_empty());
        assert_eq!(limits.plan_type.as_deref(), Some("business"));
        assert_eq!(
            limits.credits,
            Some(Credits {
                has_credits: true,
                unlimited: false,
                balance: None,
            })
        );
    }

    #[test]
    fn parses_reported_rate_limit_windows() {
        let line = r#"{"timestamp":"2026-10-05T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":42.5,"window_minutes":300,"resets_at":1791216000},"secondary":{"used_percent":10.0,"window_minutes":10080,"resets_in_seconds":3600},"credits":null,"plan_type":"pro"}}}"#;
        let rollout = parse_rollout(line);
        let limits = rollout.limits().unwrap();
        let primary = limits.get("primary").unwrap();
        assert_eq!(primary.used_percent, 42.5);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(
            primary.resets_at.map(|at| at.unix_timestamp()),
            Some(1_791_216_000)
        );
        let secondary = limits.get("secondary").unwrap();
        assert_eq!(
            secondary.resets_at,
            crate::parse_timestamp("2026-10-05T13:00:00Z")
        );
        assert_eq!(limits.credits, None);
        assert!(rollout.session_usage().is_none());
    }

    #[test]
    fn counter_reset_is_treated_as_fresh_delta() {
        let mut rollout = parse_rollout(FIXTURE);
        rollout.ingest_line(r#"{"timestamp":"2026-10-05T13:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10,"total_tokens":110},"model_context_window":258400}}}"#);
        let last = rollout.events().last().unwrap();
        assert_eq!(last.tokens.input, 100);
        assert_eq!(last.tokens.output, 10);
    }
}
