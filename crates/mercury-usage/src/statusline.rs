//! Claude Code statusline payload (the JSON Claude Code pipes into the
//! configured `statusLine` command on every refresh).
//!
//! This is the only exact source for context percentage, session cost and the
//! Pro/Max hourly windows (`rate_limits`, absent on other plans). Every field is
//! optional here: the payload grows between Claude Code versions.

use serde_json::Value;
use time::OffsetDateTime;

use crate::model::{
    ContextUsage, Provider, ProviderLimit, ProviderLimits, SessionUsage, TokenCounts,
};

#[derive(Debug, Clone, PartialEq)]
pub struct StatuslineSnapshot {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub model_id: Option<String>,
    pub model_display_name: Option<String>,
    pub cost_usd: Option<f64>,
    pub context: Option<ContextUsage>,
    /// `rate_limits.five_hour` / `seven_day` / `spend_limit`; empty when not reported.
    pub limits: Vec<ProviderLimit>,
    pub cache_hit_ratio: Option<f64>,
    pub observed_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatuslineError {
    Json(String),
    MissingSessionId,
}

impl std::fmt::Display for StatuslineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(message) => write!(f, "invalid statusline JSON: {message}"),
            Self::MissingSessionId => f.write_str("statusline JSON has no session_id"),
        }
    }
}

impl std::error::Error for StatuslineError {}

fn str_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn f64_at(value: &Value, path: &[&str]) -> Option<f64> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_f64()
}

fn parse_limit(key: &str, value: &Value) -> Option<ProviderLimit> {
    let used_percent = value
        .get("used_percentage")
        .or_else(|| value.get("used_percent"))
        .and_then(Value::as_f64)?;
    Some(ProviderLimit {
        key: key.to_owned(),
        used_percent,
        window_minutes: match key {
            "five_hour" => Some(300),
            "seven_day" => Some(7 * 24 * 60),
            _ => None,
        },
        resets_at: value.get("resets_at").and_then(crate::parse_reset_value),
        used_usd: value.get("used_usd").and_then(Value::as_f64),
        limit_usd: value.get("limit_usd").and_then(Value::as_f64),
        period: value
            .get("period")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn parse_context(value: &Value) -> Option<ContextUsage> {
    let window = value.get("context_window")?;
    let size = window.get("context_window_size").and_then(Value::as_u64);
    let from_current = window
        .get("current_usage")
        .filter(|usage| usage.is_object())
        .map(|usage| {
            let field = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            TokenCounts {
                input: field("input_tokens"),
                cache_read: field("cache_read_input_tokens"),
                cache_write: field("cache_creation_input_tokens"),
                output: 0,
                reasoning: 0,
            }
            .prompt()
        })
        .filter(|tokens| *tokens > 0);
    let from_percentage = window
        .get("used_percentage")
        .and_then(Value::as_f64)
        .zip(size)
        .map(|(percent, size)| (percent / 100.0 * size as f64).round() as u64);
    let used_tokens = from_current.or(from_percentage)?;
    Some(ContextUsage {
        used_tokens,
        window_tokens: size,
        exact: true,
    })
}

impl StatuslineSnapshot {
    pub fn parse(json: &str, observed_at: OffsetDateTime) -> Result<Self, StatuslineError> {
        let value: Value =
            serde_json::from_str(json).map_err(|err| StatuslineError::Json(err.to_string()))?;
        Self::from_value(&value, observed_at)
    }

    /// Same as [`Self::parse`] for an already-decoded payload (the
    /// `usage.report_statusline` API method carries it as a JSON object).
    pub fn from_value(value: &Value, observed_at: OffsetDateTime) -> Result<Self, StatuslineError> {
        if !value.is_object() {
            return Err(StatuslineError::Json("payload is not a JSON object".into()));
        }
        let session_id = str_at(value, &["session_id"]).ok_or(StatuslineError::MissingSessionId)?;
        let limits = value
            .get("rate_limits")
            .filter(|limits| limits.is_object())
            .map(|limits| {
                ["five_hour", "seven_day", "spend_limit"]
                    .into_iter()
                    .filter_map(|key| parse_limit(key, limits.get(key)?))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            session_id,
            transcript_path: str_at(value, &["transcript_path"]),
            cwd: str_at(value, &["cwd"]).or_else(|| str_at(value, &["workspace", "current_dir"])),
            model_id: str_at(value, &["model", "id"]),
            model_display_name: str_at(value, &["model", "display_name"]),
            cost_usd: f64_at(value, &["cost", "total_cost_usd"]),
            context: parse_context(value),
            limits,
            cache_hit_ratio: f64_at(value, &["prompt_cache", "hit_ratio"]),
            observed_at,
        })
    }

    /// Account-level windows, or `None` when the plan doesn't report any
    /// (the UI shows n/a, never a made-up number).
    pub fn provider_limits(&self) -> Option<ProviderLimits> {
        (!self.limits.is_empty()).then(|| ProviderLimits {
            provider: Provider::Claude,
            limits: self.limits.clone(),
            credits: None,
            plan_type: None,
            limit_reached_type: None,
            observed_at: Some(self.observed_at),
        })
    }

    /// Overlays the exact values onto transcript-derived usage of the same session.
    pub fn apply_to(&self, usage: &mut SessionUsage) {
        if usage.session_id != self.session_id {
            return;
        }
        if let Some(context) = self.context {
            usage.context = Some(context);
        }
        if self.cost_usd.is_some() {
            usage.cost_usd = self.cost_usd;
        }
        if let Some(model) = self.model_id.clone() {
            usage.model = Some(model);
        }
        usage.updated_at = usage.updated_at.max(Some(self.observed_at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/claude-statusline.json");

    fn now() -> OffsetDateTime {
        crate::parse_timestamp("2026-10-05T15:00:00Z").unwrap()
    }

    #[test]
    fn parses_enterprise_payload_without_rate_limits() {
        let snapshot = StatuslineSnapshot::parse(FIXTURE, now()).unwrap();
        assert_eq!(snapshot.session_id, "11111111-2222-3333-4444-555555555555");
        assert_eq!(snapshot.model_id.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(snapshot.model_display_name.as_deref(), Some("Opus 5.5"));
        assert_eq!(snapshot.cost_usd, Some(3.4127));
        assert_eq!(
            snapshot.context,
            Some(ContextUsage {
                used_tokens: 3 + 1_200 + 84_000,
                window_tokens: Some(200_000),
                exact: true,
            })
        );
        assert!(snapshot.limits.is_empty());
        assert!(snapshot.provider_limits().is_none());
        assert_eq!(snapshot.cache_hit_ratio, Some(0.97));
    }

    #[test]
    fn parses_rate_limits_when_present() {
        let json = r#"{"session_id":"s","rate_limits":{"five_hour":{"used_percentage":61.0,"resets_at":"2026-10-05T17:00:00Z"},"seven_day":{"used_percentage":12.5,"resets_at":1791800000},"spend_limit":{"used_percentage":40.0,"resets_at":"2026-11-01T00:00:00Z","used_usd":400.0,"limit_usd":1000.0,"period":"monthly"}},"context_window":{"context_window_size":1000000,"used_percentage":25.0}}"#;
        let snapshot = StatuslineSnapshot::parse(json, now()).unwrap();
        let limits = snapshot.provider_limits().unwrap();
        assert_eq!(limits.limits.len(), 3);
        let five = limits.get("five_hour").unwrap();
        assert_eq!(five.used_percent, 61.0);
        assert_eq!(five.window_minutes, Some(300));
        assert_eq!(
            five.resets_at,
            crate::parse_timestamp("2026-10-05T17:00:00Z")
        );
        let spend = limits.get("spend_limit").unwrap();
        assert_eq!(spend.limit_usd, Some(1000.0));
        assert_eq!(spend.period.as_deref(), Some("monthly"));
        // Without current_usage the percentage is used.
        assert_eq!(snapshot.context.unwrap().used_tokens, 250_000);
    }

    #[test]
    fn overlays_exact_values_on_transcript_usage() {
        let snapshot = StatuslineSnapshot::parse(FIXTURE, now()).unwrap();
        let mut usage = SessionUsage {
            provider: Provider::Claude,
            session_id: snapshot.session_id.clone(),
            model: Some("claude-sonnet-5".into()),
            totals: TokenCounts::default(),
            context: None,
            cost_usd: None,
            updated_at: None,
        };
        snapshot.apply_to(&mut usage);
        assert_eq!(usage.cost_usd, Some(3.4127));
        assert_eq!(usage.model.as_deref(), Some("claude-opus-5-5"));
        assert!(usage.context.unwrap().exact);
        assert_eq!(usage.updated_at, Some(now()));

        let mut other = SessionUsage {
            session_id: "different".into(),
            ..usage.clone()
        };
        other.cost_usd = None;
        snapshot.apply_to(&mut other);
        assert_eq!(other.cost_usd, None);
    }

    #[test]
    fn rejects_payload_without_session() {
        assert_eq!(
            StatuslineSnapshot::parse("{}", now()),
            Err(StatuslineError::MissingSessionId)
        );
        assert!(matches!(
            StatuslineSnapshot::parse("not json", now()),
            Err(StatuslineError::Json(_))
        ));
    }
}
