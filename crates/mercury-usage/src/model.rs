//! Provider-neutral usage data.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// Token counts normalized across providers.
///
/// `input` is *uncached* input: Claude reports it that way already, Codex
/// reports cached tokens as a subset of `input_tokens`, so the Codex reader
/// subtracts them. `reasoning` is informational and already included in `output`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    pub reasoning: u64,
}

impl TokenCounts {
    /// Every billed token: input + cache read + cache write + output.
    pub fn total(&self) -> u64 {
        self.prompt().saturating_add(self.output)
    }

    /// Tokens sent to the model in a request (what occupies the context window).
    pub fn prompt(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// Field-wise `self - earlier`; `None` if any counter went backwards
    /// (counter reset), so callers can treat `self` as a fresh delta instead.
    pub fn checked_delta(&self, earlier: &Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_sub(earlier.input)?,
            cache_read: self.cache_read.checked_sub(earlier.cache_read)?,
            cache_write: self.cache_write.checked_sub(earlier.cache_write)?,
            output: self.output.checked_sub(earlier.output)?,
            reasoning: self.reasoning.checked_sub(earlier.reasoning)?,
        })
    }
}

impl std::ops::Add for TokenCounts {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}

impl std::ops::AddAssign for TokenCounts {
    fn add_assign(&mut self, rhs: Self) {
        self.input = self.input.saturating_add(rhs.input);
        self.cache_read = self.cache_read.saturating_add(rhs.cache_read);
        self.cache_write = self.cache_write.saturating_add(rhs.cache_write);
        self.output = self.output.saturating_add(rhs.output);
        self.reasoning = self.reasoning.saturating_add(rhs.reasoning);
    }
}

impl std::iter::Sum for TokenCounts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |acc, item| acc + item)
    }
}

/// One model request (Claude) or one token-count delta (Codex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEvent {
    pub provider: Provider,
    pub session_id: String,
    pub at: OffsetDateTime,
    pub model: Option<String>,
    pub tokens: TokenCounts,
}

/// How full an agent's context window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used_tokens: u64,
    /// Model context size; `None` when the source does not say and it can't be inferred.
    pub window_tokens: Option<u64>,
    /// `true` when reported by the agent itself (statusline / Codex), `false` for
    /// estimates derived from transcripts.
    pub exact: bool,
}

impl ContextUsage {
    /// Used fraction in `0.0..`, may exceed 1.0 if the estimate overshoots.
    pub fn fraction(&self) -> Option<f64> {
        let window = self.window_tokens.filter(|window| *window > 0)?;
        Some(self.used_tokens as f64 / window as f64)
    }

    /// Used share in permille (0..=1000, clamped), handy for integer wire formats.
    pub fn permille(&self) -> Option<u16> {
        self.fraction()
            .map(|fraction| (fraction * 1000.0).round().clamp(0.0, 1000.0) as u16)
    }
}

/// Everything the per-pane gauge needs for one agent session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUsage {
    pub provider: Provider,
    pub session_id: String,
    pub model: Option<String>,
    pub totals: TokenCounts,
    pub context: Option<ContextUsage>,
    /// Session cost as reported by the agent; `None` when no cost source exists.
    pub cost_usd: Option<f64>,
    /// Timestamp of the newest data point that fed this value.
    pub updated_at: Option<OffsetDateTime>,
}

/// One provider-reported window (Claude `five_hour`/`seven_day`/`spend_limit`,
/// Codex `primary`/`secondary`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderLimit {
    pub key: String,
    pub used_percent: f64,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<OffsetDateTime>,
    pub used_usd: Option<f64>,
    pub limit_usd: Option<f64>,
    /// `daily` / `weekly` / `monthly` for spend limits.
    pub period: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credits {
    pub has_credits: bool,
    pub unlimited: bool,
    /// Kept as text: providers have used both numbers and strings here.
    pub balance: Option<String>,
}

/// Snapshot of what a provider reported about account-level limits.
/// An empty `limits` list means "not reported" and must be shown as n/a.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderLimits {
    pub provider: Provider,
    pub limits: Vec<ProviderLimit>,
    pub credits: Option<Credits>,
    pub plan_type: Option<String>,
    pub limit_reached_type: Option<String>,
    pub observed_at: Option<OffsetDateTime>,
}

impl ProviderLimits {
    pub fn get(&self, key: &str) -> Option<&ProviderLimit> {
        self.limits.iter().find(|limit| limit.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_totals_and_deltas() {
        let earlier = TokenCounts {
            input: 10,
            cache_read: 100,
            cache_write: 5,
            output: 7,
            reasoning: 2,
        };
        let later = TokenCounts {
            input: 15,
            cache_read: 150,
            cache_write: 5,
            output: 20,
            reasoning: 4,
        };
        assert_eq!(later.prompt(), 170);
        assert_eq!(later.total(), 190);
        assert_eq!(
            later.checked_delta(&earlier),
            Some(TokenCounts {
                input: 5,
                cache_read: 50,
                cache_write: 0,
                output: 13,
                reasoning: 2,
            })
        );
        assert_eq!(earlier.checked_delta(&later), None);
        assert_eq!([earlier, later].into_iter().sum::<TokenCounts>().input, 25);
    }

    #[test]
    fn context_permille_clamps() {
        let context = ContextUsage {
            used_tokens: 50_000,
            window_tokens: Some(200_000),
            exact: true,
        };
        assert_eq!(context.permille(), Some(250));
        let over = ContextUsage {
            used_tokens: 300_000,
            ..context
        };
        assert_eq!(over.permille(), Some(1000));
        let unknown = ContextUsage {
            window_tokens: None,
            ..context
        };
        assert_eq!(unknown.permille(), None);
    }
}
