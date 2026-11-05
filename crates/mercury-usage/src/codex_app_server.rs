//! Read-only app-server usage responses. The transport belongs to Mercury's
//! server; this module only maps Codex's camelCase JSON into neutral types.

use serde_json::Value;
use time::OffsetDateTime;

use crate::model::{Credits, Provider, ProviderLimit, ProviderLimits};

fn window(key: &str, value: &Value) -> Option<ProviderLimit> {
    let used_percent = value.get("usedPercent")?.as_f64()?;
    if !used_percent.is_finite() || used_percent < 0.0 {
        return None;
    }
    Some(ProviderLimit {
        key: key.into(),
        used_percent,
        window_minutes: value.get("windowDurationMins").and_then(Value::as_u64),
        resets_at: value.get("resetsAt").and_then(crate::parse_reset_value),
        used_usd: None,
        limit_usd: None,
        period: None,
    })
}

/// Parse `account/rateLimits/read.result`. Null windows remain absent. The
/// `individualLimit` amount has no declared currency in the protocol and is
/// deliberately excluded from USD budgets.
pub fn account_limits(result: &Value, observed_at: OffsetDateTime) -> Option<ProviderLimits> {
    let limits = result
        .get("rateLimitsByLimitId")
        .and_then(|buckets| buckets.get("codex"))
        .or_else(|| result.get("rateLimits"))?
        .as_object()?;
    let mut windows: Vec<_> = ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| window(key, limits.get(key)?))
        .collect();
    if let Some(individual) = limits.get("individualLimit") {
        if let Some(remaining) = individual
            .get("remainingPercent")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
        {
            windows.push(ProviderLimit {
                key: "individual".into(),
                used_percent: 100.0 - remaining,
                window_minutes: None,
                resets_at: individual
                    .get("resetsAt")
                    .and_then(crate::parse_reset_value),
                used_usd: None,
                limit_usd: None,
                period: None,
            });
        }
    }
    let credits = limits
        .get("credits")
        .and_then(Value::as_object)
        .map(|credits| Credits {
            has_credits: credits
                .get("hasCredits")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            unlimited: credits
                .get("unlimited")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            balance: credits.get("balance").and_then(|balance| match balance {
                Value::String(text) => Some(text.clone()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            }),
        });
    Some(ProviderLimits {
        provider: Provider::Codex,
        limits: windows,
        credits,
        plan_type: limits
            .get("planType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        limit_reached_type: limits
            .get("rateLimitReachedType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        observed_at: Some(observed_at),
    })
}

/// Parse `account/usage/read.result` for a requested thread. `None` means no
/// price was reported, not zero cost. Thread identity is checked before use.
pub fn thread_total_usd(result: &Value, requested_thread_id: &str) -> Option<f64> {
    let usage = result.get("threadUsage")?.as_object()?;
    if usage.get("threadId")?.as_str()? != requested_thread_id {
        return None;
    }
    let micros = usage.get("estimatedUsageUsdMicros")?.as_i64()?;
    (micros >= 0).then_some(micros as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_individual_quota_is_remaining_share_and_never_money() {
        for (remaining, used) in [(23, 77.0), (0, 100.0), (100, 0.0)] {
            let value = serde_json::json!({"rateLimitsByLimitId":{"codex":{
                "primary":null,"secondary":null,"planType":"business",
                "individualLimit":{"remainingPercent":remaining,"resetsAt":1793491200,"limit":"25000","used":"19354.7"}
            }}});
            let snapshot = account_limits(&value, OffsetDateTime::UNIX_EPOCH).unwrap();
            assert_eq!(snapshot.limits.len(), 1);
            let quota = &snapshot.limits[0];
            assert_eq!(quota.used_percent, used);
            assert_eq!(quota.resets_at.unwrap().unix_timestamp(), 1793491200);
            assert!(quota.used_usd.is_none() && quota.limit_usd.is_none());
        }
    }

    #[test]
    fn malformed_individual_quota_is_not_fabricated() {
        for remaining in [
            serde_json::json!(-1),
            serde_json::json!(101),
            serde_json::json!("23"),
            serde_json::Value::Null,
        ] {
            let snapshot=account_limits(&serde_json::json!({"rateLimits":{"individualLimit":{"remainingPercent":remaining}}}),OffsetDateTime::UNIX_EPOCH).unwrap();
            assert!(snapshot.limits.is_empty());
        }
    }

    #[test]
    fn business_snapshot_keeps_absent_windows_and_reported_credits() {
        let result = serde_json::json!({
            "rateLimits": {"primary":null,"secondary":null,"credits":{"hasCredits":true,"unlimited":false,"balance":null},"planType":"business","individualLimit":{"limit":"25000","used":"1094"}}
        });
        let limits = account_limits(&result, OffsetDateTime::UNIX_EPOCH).unwrap();
        assert!(limits.limits.is_empty());
        assert_eq!(limits.plan_type.as_deref(), Some("business"));
        assert!(limits.credits.unwrap().has_credits);
    }

    #[test]
    fn windows_use_reported_percentage_and_reset() {
        let result = serde_json::json!({"rateLimits": {"primary":{"usedPercent":42,"windowDurationMins":300,"resetsAt":1791300000},"secondary":null}});
        let limits = account_limits(&result, OffsetDateTime::UNIX_EPOCH).unwrap();
        assert_eq!(limits.limits.len(), 1);
        assert_eq!(limits.limits[0].used_percent, 42.0);
        assert_eq!(limits.limits[0].window_minutes, Some(300));
        assert_eq!(
            limits.limits[0].resets_at.unwrap().unix_timestamp(),
            1791300000
        );
    }

    #[test]
    fn thread_cost_requires_matching_id_and_non_null_usd() {
        let result = serde_json::json!({"threadUsage":{"threadId":"thread-1","estimatedUsageUsdMicros":42934}});
        assert_eq!(thread_total_usd(&result, "thread-1"), Some(0.042934));
        assert_eq!(thread_total_usd(&result, "thread-2"), None);
        assert_eq!(
            thread_total_usd(&serde_json::json!({"threadUsage":null}), "thread-1"),
            None
        );
        assert_eq!(
            thread_total_usd(
                &serde_json::json!({"threadUsage":{"threadId":"thread-1","estimatedUsageUsdMicros":null}}),
                "thread-1"
            ),
            None
        );
    }
}
