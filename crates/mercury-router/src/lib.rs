//! Pure route selection. Side effects (pane creation and prompt submission)
//! belong to the Mercury server, never to this crate.

use mercury_usage::{Provider, ProviderLimits};
use regex::Regex;
use serde::Deserialize;
use time::{Duration, OffsetDateTime};

const MAX_PATTERN_BYTES: usize = 1024;
const MAX_RULES: usize = 128;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRule {
    pub pattern: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub project: Option<String>,
    pub provider: Provider,
    pub model: Option<String>,
    pub effort: Option<String>,
    #[serde(default)]
    pub fallback: Vec<Provider>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RouteConfig {
    pub enabled: bool,
    pub default_provider: Provider,
    pub headroom_threshold_percent: f64,
    pub limit_ttl_seconds: i64,
    #[serde(rename = "rule")]
    pub rules: Vec<RouteRule>,
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: Provider::Claude,
            headroom_threshold_percent: 90.0,
            limit_ttl_seconds: 60,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Headroom {
    Available,
    Exceeded,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    pub provider: Provider,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub matched_rule: Option<usize>,
    pub headroom: Headroom,
    pub fallback_from: Option<Provider>,
}

pub struct RouteRequest<'a> {
    pub prompt: &'a str,
    pub tags: &'a [&'a str],
    pub project: Option<&'a str>,
}

impl RouteConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.rules.len() > MAX_RULES {
            return Err("too many route rules".into());
        }
        if !self.headroom_threshold_percent.is_finite()
            || !(0.0..=100.0).contains(&self.headroom_threshold_percent)
        {
            return Err("route headroom threshold must be between 0 and 100".into());
        }
        if !(1..=3600).contains(&self.limit_ttl_seconds) {
            return Err("route limit TTL must be between 1 and 3600 seconds".into());
        }
        for (index, rule) in self.rules.iter().enumerate() {
            if let Some(pattern) = &rule.pattern {
                if pattern.len() > MAX_PATTERN_BYTES {
                    return Err(format!("route rule {index} pattern is too long"));
                }
                Regex::new(pattern)
                    .map_err(|_| format!("route rule {index} has invalid pattern"))?;
            }
            if rule.tags.len() > 32
                || rule
                    .tags
                    .iter()
                    .any(|tag| tag.is_empty() || tag.len() > 128)
            {
                return Err(format!("route rule {index} has invalid tags"));
            }
            if rule
                .project
                .as_ref()
                .is_some_and(|project| project.is_empty() || project.len() > 512)
            {
                return Err(format!("route rule {index} has an invalid project"));
            }
            if rule.fallback.contains(&rule.provider) {
                return Err(format!(
                    "route rule {index} repeats its primary as fallback"
                ));
            }
            if rule.fallback.len() > 1
                || rule.model.as_deref().is_some_and(invalid_launch_option)
                || rule.effort.as_deref().is_some_and(invalid_launch_option)
            {
                return Err(format!("route rule {index} has invalid agent options"));
            }
        }
        Ok(())
    }

    pub fn decide(
        &self,
        request: &RouteRequest<'_>,
        limits: &[ProviderLimits],
        now: OffsetDateTime,
    ) -> Result<RouteDecision, String> {
        self.decide_excluding(request, limits, now, &[])
    }

    pub fn decide_excluding(
        &self,
        request: &RouteRequest<'_>,
        limits: &[ProviderLimits],
        now: OffsetDateTime,
        excluded: &[Provider],
    ) -> Result<RouteDecision, String> {
        self.validate()?;
        if !self.enabled {
            return Err("routing is disabled".into());
        }
        let matched = self.rules.iter().enumerate().find(|(_, rule)| {
            rule.pattern.as_ref().is_none_or(|pattern| {
                Regex::new(pattern).is_ok_and(|regex| regex.is_match(request.prompt))
            }) && rule
                .tags
                .iter()
                .all(|tag| request.tags.contains(&tag.as_str()))
                && rule
                    .project
                    .as_deref()
                    .is_none_or(|project| request.project == Some(project))
        });
        let primary = matched
            .as_ref()
            .map_or(self.default_provider, |(_, rule)| rule.provider);
        let fallback = matched
            .as_ref()
            .map_or(&[][..], |(_, rule)| rule.fallback.as_slice());
        let candidates = std::iter::once(primary).chain(fallback.iter().copied());
        for provider in candidates {
            if excluded.contains(&provider) {
                continue;
            }
            let headroom = provider_headroom(
                provider,
                limits,
                now,
                self.limit_ttl_seconds,
                self.headroom_threshold_percent,
            );
            if headroom != Headroom::Exceeded {
                return Ok(RouteDecision {
                    provider,
                    model: (provider == primary)
                        .then(|| matched.as_ref().and_then(|(_, rule)| rule.model.clone()))
                        .flatten(),
                    effort: (provider == primary)
                        .then(|| matched.as_ref().and_then(|(_, rule)| rule.effort.clone()))
                        .flatten(),
                    matched_rule: matched.as_ref().map(|(index, _)| *index),
                    headroom,
                    fallback_from: (provider != primary).then_some(primary),
                });
            }
        }
        Err("no route provider has available headroom or remains after exclusion".into())
    }
}

pub fn provider_headroom(
    provider: Provider,
    limits: &[ProviderLimits],
    now: OffsetDateTime,
    ttl_seconds: i64,
    threshold_percent: f64,
) -> Headroom {
    let Some(snapshot) = limits.iter().find(|snapshot| snapshot.provider == provider) else {
        return Headroom::Unknown;
    };
    let Some(observed_at) = snapshot.observed_at else {
        return Headroom::Unknown;
    };
    let age = now - observed_at;
    if age < Duration::ZERO || age > Duration::seconds(ttl_seconds) {
        return Headroom::Unknown;
    }
    if snapshot
        .limits
        .iter()
        .any(|limit| limit.resets_at.is_some_and(|reset| reset <= now))
    {
        return Headroom::Unknown;
    }
    if snapshot.limit_reached_type.is_some() {
        return Headroom::Exceeded;
    }
    let mut reported = false;
    for limit in &snapshot.limits {
        if !limit.used_percent.is_finite() || limit.used_percent < 0.0 {
            continue;
        }
        reported = true;
        if limit.used_percent >= threshold_percent {
            return Headroom::Exceeded;
        }
    }
    if reported {
        Headroom::Available
    } else {
        Headroom::Unknown
    }
}

fn invalid_launch_option(value: &str) -> bool {
    value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(provider: Provider) -> RouteRule {
        RouteRule {
            pattern: None,
            tags: Vec::new(),
            project: None,
            provider,
            model: None,
            effort: None,
            fallback: Vec::new(),
        }
    }

    fn snapshot(provider: Provider, used_percent: f64, at: OffsetDateTime) -> ProviderLimits {
        ProviderLimits {
            provider,
            limits: vec![mercury_usage::ProviderLimit {
                key: "primary".into(),
                used_percent,
                window_minutes: Some(300),
                resets_at: None,
                used_usd: None,
                limit_usd: None,
                period: None,
            }],
            credits: None,
            plan_type: None,
            limit_reached_type: None,
            observed_at: Some(at),
        }
    }

    #[test]
    fn first_matching_rule_wins_and_filters_are_conjunctive() {
        let mut config = RouteConfig {
            enabled: true,
            ..RouteConfig::default()
        };
        let mut first = rule(Provider::Codex);
        first.pattern = Some("(?i)review".into());
        first.tags = vec!["security".into()];
        first.project = Some("checkout".into());
        first.model = Some("codex-model".into());
        config.rules = vec![first, rule(Provider::Claude)];
        let request = RouteRequest {
            prompt: "Review this patch",
            tags: &["security"],
            project: Some("checkout"),
        };
        let decision = config
            .decide(&request, &[], OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(decision.provider, Provider::Codex);
        assert_eq!(decision.matched_rule, Some(0));
        assert_eq!(decision.model.as_deref(), Some("codex-model"));
        assert_eq!(decision.headroom, Headroom::Unknown);
        let other = RouteRequest {
            tags: &[],
            ..request
        };
        assert_eq!(
            config
                .decide(&other, &[], OffsetDateTime::UNIX_EPOCH)
                .unwrap()
                .matched_rule,
            Some(1)
        );
    }

    #[test]
    fn exceeded_primary_uses_fallback_but_stale_data_does_not_fake_headroom() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::hours(2);
        let mut primary = rule(Provider::Claude);
        primary.fallback.push(Provider::Codex);
        let config = RouteConfig {
            enabled: true,
            rules: vec![primary],
            ..RouteConfig::default()
        };
        let request = RouteRequest {
            prompt: "task",
            tags: &[],
            project: None,
        };
        let limits = [
            snapshot(Provider::Claude, 95.0, now),
            snapshot(Provider::Codex, 20.0, now),
        ];
        let decision = config.decide(&request, &limits, now).unwrap();
        assert_eq!(decision.provider, Provider::Codex);
        assert_eq!(decision.fallback_from, Some(Provider::Claude));
        let stale = config
            .decide(&request, &limits, now + Duration::minutes(2))
            .unwrap();
        assert_eq!(stale.provider, Provider::Claude);
        assert_eq!(stale.headroom, Headroom::Unknown);
    }

    #[test]
    fn invalid_rule_and_unknown_limits_are_explicit() {
        let mut config = RouteConfig {
            enabled: true,
            ..RouteConfig::default()
        };
        let mut invalid = rule(Provider::Claude);
        invalid.pattern = Some("[".into());
        config.rules.push(invalid);
        assert!(config.validate().unwrap_err().contains("invalid pattern"));
        config.rules.clear();
        let request = RouteRequest {
            prompt: "task",
            tags: &[],
            project: None,
        };
        let decision = config
            .decide(&request, &[], OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(decision.headroom, Headroom::Unknown);
    }

    #[test]
    fn launch_options_and_provider_values_are_bounded() {
        let mut config = RouteConfig {
            enabled: true,
            ..RouteConfig::default()
        };
        let mut invalid = rule(Provider::Claude);
        invalid.model = Some("model\nunsafe".into());
        config.rules.push(invalid);
        assert!(config
            .validate()
            .unwrap_err()
            .contains("invalid agent options"));

        config.rules[0].model = None;
        config.rules[0].fallback = vec![Provider::Codex, Provider::Codex];
        assert!(config
            .validate()
            .unwrap_err()
            .contains("invalid agent options"));

        let now = OffsetDateTime::UNIX_EPOCH;
        let mut invalid_limit = snapshot(Provider::Claude, -1.0, now);
        assert_eq!(
            provider_headroom(Provider::Claude, &[invalid_limit.clone()], now, 60, 90.0),
            Headroom::Unknown
        );
        invalid_limit.limits[0].used_percent = 95.0;
        assert_eq!(
            provider_headroom(Provider::Claude, &[invalid_limit], now, 60, 90.0),
            Headroom::Exceeded
        );
    }

    #[test]
    fn excluded_primary_uses_fallback_and_elapsed_reset_is_unknown() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::hours(2);
        let mut primary = rule(Provider::Claude);
        primary.fallback.push(Provider::Codex);
        let config = RouteConfig {
            enabled: true,
            rules: vec![primary],
            ..RouteConfig::default()
        };
        let request = RouteRequest {
            prompt: "task",
            tags: &[],
            project: None,
        };
        let decision = config
            .decide_excluding(&request, &[], now, &[Provider::Claude])
            .unwrap();
        assert_eq!(decision.provider, Provider::Codex);
        assert_eq!(decision.fallback_from, Some(Provider::Claude));
        assert!(config
            .decide_excluding(&request, &[], now, &[Provider::Claude, Provider::Codex])
            .is_err());

        let mut elapsed = snapshot(Provider::Claude, 95.0, now);
        elapsed.limits[0].resets_at = Some(now - Duration::seconds(1));
        assert_eq!(
            provider_headroom(Provider::Claude, &[elapsed], now, 60, 90.0),
            Headroom::Unknown
        );
    }
}
