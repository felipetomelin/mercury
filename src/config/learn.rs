//! Explicit, default-off configuration for automatic learning. Parsing is
//! intentionally strict because this section can authorize future writes.

use std::{collections::BTreeMap, path::PathBuf};

use mercury_learn::policy::{Policy, TargetKind, TargetPolicy};
use serde::{de, Deserialize, Deserializer};

#[derive(Debug, Clone, PartialEq)]
pub struct LearnConfig {
    pub enabled: bool,
    pub review_every_turns: u32,
    pub weekly: bool,
    pub model: String,
    pub max_budget_usd: f64,
    pub timeout_secs: u64,
    pub targets: Vec<LearnTargetConfig>,
    /// Explicit Cron job UUID -> allowlisted Learn target ID association.
    pub job_targets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnTargetConfig {
    pub id: String,
    pub path: PathBuf,
    pub kind: LearnTargetKind,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LearnTargetKind {
    Instructions,
    Skill,
    Memory,
    ConfigSection,
    CronJob,
}

impl From<LearnTargetKind> for TargetKind {
    fn from(value: LearnTargetKind) -> Self {
        match value {
            LearnTargetKind::Instructions => Self::Instructions,
            LearnTargetKind::Skill => Self::Skill,
            LearnTargetKind::Memory => Self::Memory,
            LearnTargetKind::ConfigSection => Self::ConfigSection,
            LearnTargetKind::CronJob => Self::CronJob,
        }
    }
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            review_every_turns: 12,
            weekly: true,
            model: "haiku".into(),
            max_budget_usd: 0.05,
            timeout_secs: 45,
            targets: Vec::new(),
            job_targets: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawLearnConfig {
    enabled: bool,
    review_every_turns: u32,
    weekly: bool,
    model: String,
    max_budget_usd: f64,
    timeout_secs: u64,
    #[serde(rename = "target")]
    targets: Vec<LearnTargetConfig>,
    job_targets: BTreeMap<String, String>,
}

impl Default for RawLearnConfig {
    fn default() -> Self {
        let config = LearnConfig::default();
        Self {
            enabled: config.enabled,
            review_every_turns: config.review_every_turns,
            weekly: config.weekly,
            model: config.model,
            max_budget_usd: config.max_budget_usd,
            timeout_secs: config.timeout_secs,
            targets: config.targets,
            job_targets: config.job_targets,
        }
    }
}

impl<'de> Deserialize<'de> for LearnConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawLearnConfig::deserialize(deserializer)?;
        let config = Self {
            enabled: raw.enabled,
            review_every_turns: raw.review_every_turns,
            weekly: raw.weekly,
            model: raw.model,
            max_budget_usd: raw.max_budget_usd,
            timeout_secs: raw.timeout_secs,
            targets: raw.targets,
            job_targets: raw.job_targets,
        };
        config.validate().map_err(de::Error::custom)?;
        Ok(config)
    }
}

impl LearnConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=1000).contains(&self.review_every_turns) {
            return Err("learn.review_every_turns must be 1..=1000");
        }
        if self.model.is_empty()
            || self.model.len() > 80
            || !self
                .model
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("learn.model must be a safe model identifier");
        }
        if !self.max_budget_usd.is_finite() || !(0.001..=5.0).contains(&self.max_budget_usd) {
            return Err("learn.max_budget_usd must be 0.001..=5");
        }
        if !(1..=300).contains(&self.timeout_secs) {
            return Err("learn.timeout_secs must be 1..=300");
        }
        if self.targets.len() > 16 || (self.enabled && self.targets.is_empty()) {
            return Err("learn needs 1..=16 explicit targets when enabled");
        }
        if self
            .targets
            .iter()
            .any(|target| target.max_bytes == 0 || target.max_bytes > 64 * 1024)
        {
            return Err("learn.target max_bytes must be 1..=65536");
        }
        if self.job_targets.len() > 128
            || self.job_targets.iter().any(|(job_id, target_id)| {
                !valid_job_id(job_id) || !self.targets.iter().any(|target| target.id == *target_id)
            })
        {
            return Err("learn.job_targets must map job UUIDs to configured target IDs");
        }
        Policy::new(self.target_policies())
            .map(|_| ())
            .map_err(|_| "learn.target has invalid or duplicate ID, path, or kind")
    }

    pub fn target_policies(&self) -> Vec<TargetPolicy> {
        self.targets
            .iter()
            .map(|target| TargetPolicy {
                id: target.id.clone(),
                path: target.path.clone(),
                kind: target.kind.into(),
                max_bytes: target.max_bytes,
            })
            .collect()
    }
}

fn valid_job_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<LearnConfig, toml::de::Error> {
        #[derive(Deserialize)]
        struct Wrapper {
            learn: LearnConfig,
        }
        toml::from_str::<Wrapper>(text).map(|wrapper| wrapper.learn)
    }

    #[test]
    fn default_is_off_without_targets() {
        let config = parse("[learn]").unwrap();
        assert!(!config.enabled);
        assert!(config.targets.is_empty());
        assert_eq!(config.review_every_turns, 12);
    }

    #[test]
    fn explicit_target_and_opt_in_parse() {
        let config = parse("[learn]\nenabled = true\nreview_every_turns = 4\n[[learn.target]]\nid = 'project'\npath = '/tmp/AGENTS.md'\nkind = 'instructions'\nmax_bytes = 4096").unwrap();
        assert!(config.enabled);
        assert_eq!(config.target_policies().len(), 1);
        assert_eq!(config.review_every_turns, 4);
    }

    #[test]
    fn job_target_association_requires_an_allowlisted_target() {
        let good = "[learn]\n[[learn.target]]\nid = 'project'\npath = '/tmp/AGENTS.md'\nkind = 'instructions'\nmax_bytes = 4096\n[learn.job_targets]\n'123e4567-e89b-12d3-a456-426614174000' = 'project'";
        let config = parse(good).unwrap();
        assert_eq!(config.job_targets.len(), 1);
        assert!(parse(&good.replace("174000' = 'project'", "174000' = 'missing'")).is_err());
    }

    #[test]
    fn malformed_or_unsupported_target_fails_closed() {
        for text in [
            "[learn]\nenabled = true",
            "[learn]\nreview_every_turns = 0",
            "[learn]\nmodel = '../escape'",
            "[learn]\nmax_budget_usd = 10",
            "[learn]\ntimeout_secs = 0",
            "[learn]\nunknown = true",
            "[learn]\n[[learn.target]]\nid = 'x'\npath = '../AGENTS.md'\nkind = 'instructions'\nmax_bytes = 4096",
            "[learn]\n[[learn.target]]\nid = 'x'\npath = '/tmp/wrong.toml'\nkind = 'config-section'\nmax_bytes = 4096",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert!(parse("[learn]\n[[learn.target]]\nid = 'route'\npath = '/tmp/config.toml'\nkind = 'config-section'\nmax_bytes = 4096").is_ok());
        assert!(parse("[learn]\n[[learn.target]]\nid = '123e4567-e89b-12d3-a456-426614174000'\npath = '/tmp/jobs.toml'\nkind = 'cron-job'\nmax_bytes = 4096").is_ok());
    }
}
