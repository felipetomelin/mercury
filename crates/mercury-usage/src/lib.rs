//! Usage readers and budget windows for Mercury.
//!
//! This crate is deliberately free of any Mercury server/UI types: it turns the
//! files and payloads that Claude Code and Codex already write into plain usage
//! data, and evaluates budget windows over that data. The server owns file
//! watching, pane attribution and pushing results to clients.
//!
//! Sources (formats validated against real data, see PLAN.md "Fase 0"):
//! - Claude Code transcripts `~/.claude/projects/<slug>/<sessionId>.jsonl`
//!   ([`claude::ClaudeTranscript`]).
//! - Codex rollouts `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
//!   ([`codex::CodexRollout`]).
//! - Claude Code statusline JSON ([`statusline::StatuslineSnapshot`]), the only
//!   source of exact context percentage, session cost and hourly limits.
//! - Mercury's own append-only cost ledger ([`ledger`]): cost is not
//!   recomputable from transcripts, so live deltas are persisted.
//!
//! There is no price table: cost only ever comes from what the agents report.

pub mod budget;
pub mod claude;
pub mod codex;
pub mod codex_app_server;
pub mod history_scan;
pub mod jsonl;
pub mod ledger;
pub mod locate;
pub mod model;
pub mod statusline;
pub mod window;

pub use budget::{
    Budget, BudgetConfig, BudgetLevel, BudgetStatus, Limit, OnExceed, Scope, TokenKind, UsageRecord,
};
pub use claude::ClaudeTranscript;
pub use codex::CodexRollout;
pub use jsonl::{JsonlTail, TailRead};
pub use ledger::{CostBaselines, CostLedgerEntry, CostObservation};
pub use model::{
    ContextUsage, Credits, Provider, ProviderLimit, ProviderLimits, SessionUsage, TokenCounts,
    UsageEvent,
};
pub use statusline::StatuslineSnapshot;
pub use window::{CalendarPeriod, CalendarWindow, UsageWindow, WindowBounds};

/// Parses an RFC 3339 timestamp as written by both agents (`2026-10-05T14:28:02.578Z`).
pub(crate) fn parse_timestamp(value: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
}

/// Accepts a reset timestamp as unix seconds (number or numeric string) or RFC 3339 string.
pub(crate) fn parse_reset_value(value: &serde_json::Value) -> Option<time::OffsetDateTime> {
    match value {
        serde_json::Value::Number(number) => {
            let seconds = number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))?;
            // Some payloads use milliseconds; anything past year ~5000 in seconds is ms.
            let seconds = if seconds > 100_000_000_000 {
                seconds / 1000
            } else {
                seconds
            };
            time::OffsetDateTime::from_unix_timestamp(seconds).ok()
        }
        serde_json::Value::String(text) => match text.parse::<i64>() {
            Ok(seconds) => time::OffsetDateTime::from_unix_timestamp(seconds).ok(),
            Err(_) => parse_timestamp(text),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_timestamps() {
        let at = parse_timestamp("2026-10-05T14:28:02.578Z").unwrap();
        assert_eq!(at.unix_timestamp(), 1_791_210_482);
        assert!(parse_timestamp("not a date").is_none());
    }

    #[test]
    fn parses_reset_values_in_seconds_millis_and_rfc3339() {
        let expected = 1_791_210_482;
        for value in [
            serde_json::json!(expected),
            serde_json::json!(expected * 1000),
            serde_json::json!(expected.to_string()),
            serde_json::json!("2026-10-05T14:28:02Z"),
        ] {
            assert_eq!(
                parse_reset_value(&value).map(|at| at.unix_timestamp()),
                Some(expected),
                "{value}"
            );
        }
        assert!(parse_reset_value(&serde_json::Value::Null).is_none());
    }
}
