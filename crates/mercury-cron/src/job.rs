use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};

use crate::Schedule;

fn default_timeout() -> u64 {
    30 * 60
}

fn default_offset() -> String {
    "+00:00".into()
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CatchUp {
    #[default]
    Skip,
    RunOnce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Repeat {
    pub times: u32,
    #[serde(default)]
    pub completed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobTarget {
    Headless {
        provider: String,
        prompt: String,
        #[serde(default)]
        prompt_files: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        #[serde(default)]
        skills: Vec<String>,
    },
    Pane {
        pane_id: String,
        prompt: String,
    },
    Script {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Delivery {
    Local {
        #[serde(default)]
        notify: bool,
    },
    Pane {
        pane_id: String,
    },
    Teams {
        webhook_env: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    /// Canonical expression: RFC3339 timestamp, `every <duration>`, or five
    /// cron fields. Bare relative durations are resolved before persistence.
    pub schedule: String,
    #[serde(default = "default_offset")]
    pub utc_offset: String,
    /// Unix seconds. Advanced durably before dispatch.
    pub next_run_at: Option<i64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub catch_up: CatchUp,
    pub repeat: Option<Repeat>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    pub workdir: Option<PathBuf>,
    pub target: JobTarget,
    #[serde(default)]
    pub deliver: Vec<Delivery>,
}

fn default_enabled() -> bool {
    true
}

fn env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn parse_utc_offset(value: &str) -> Result<UtcOffset, String> {
    let text = value.as_bytes();
    if text.len() != 6
        || !matches!(text[0], b'+' | b'-')
        || text[3] != b':'
        || !text[1..3].iter().all(u8::is_ascii_digit)
        || !text[4..6].iter().all(u8::is_ascii_digit)
    {
        return Err("utc_offset must look like +HH:MM or -HH:MM".into());
    }
    let hours = value[1..3]
        .parse::<i8>()
        .map_err(|_| "invalid utc_offset hours")?;
    let minutes = value[4..6]
        .parse::<i8>()
        .map_err(|_| "invalid utc_offset minutes")?;
    let sign = if text[0] == b'-' { -1 } else { 1 };
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).map_err(|error| error.to_string())
}

impl Job {
    pub fn new(
        name: String,
        expression: &str,
        offset: UtcOffset,
        now: OffsetDateTime,
        target: JobTarget,
    ) -> Result<Self, String> {
        let schedule = Schedule::parse(expression, now, offset)?;
        let next = schedule
            .next_after(now)
            .ok_or_else(|| "schedule has no future occurrence".to_owned())?;
        let schedule = match schedule {
            Schedule::Once(at) => at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
            _ => expression.trim().to_owned(),
        };
        let offset_seconds = offset.whole_seconds();
        let absolute = offset_seconds.unsigned_abs();
        let job = Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            schedule,
            utc_offset: format!(
                "{}{:02}:{:02}",
                if offset_seconds < 0 { '-' } else { '+' },
                absolute / 3600,
                (absolute % 3600) / 60
            ),
            next_run_at: Some(next.unix_timestamp()),
            enabled: true,
            catch_up: CatchUp::Skip,
            repeat: None,
            timeout_seconds: default_timeout(),
            workdir: None,
            target,
            deliver: vec![Delivery::Local { notify: true }],
        };
        job.validate()?;
        Ok(job)
    }

    pub fn offset(&self) -> Result<UtcOffset, String> {
        parse_utc_offset(&self.utc_offset)
    }

    pub fn parsed_schedule(&self) -> Result<Schedule, String> {
        let parsed = Schedule::parse(&self.schedule, OffsetDateTime::UNIX_EPOCH, self.offset()?)?;
        if matches!(parsed, Schedule::Once(_)) && !self.schedule.contains('T') {
            return Err("persisted one-shot schedule must be RFC3339, not relative".into());
        }
        Ok(parsed)
    }

    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.id).is_err() {
            return Err("job id must be a UUID".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 120 {
            return Err("job name must contain 1–120 characters".into());
        }
        self.parsed_schedule()?;
        if self.enabled && self.next_run_at.is_none() {
            return Err("enabled job needs next_run_at".into());
        }
        if self.timeout_seconds == 0 || self.timeout_seconds > 24 * 60 * 60 {
            return Err("timeout_seconds must be 1..=86400".into());
        }
        if self
            .workdir
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("workdir must be absolute".into());
        }
        if let Some(repeat) = &self.repeat {
            if repeat.times == 0 || repeat.completed > repeat.times {
                return Err("repeat.times must be positive and completed <= times".into());
            }
        }
        match &self.target {
            JobTarget::Headless {
                provider,
                prompt,
                prompt_files,
                ..
            } => {
                if !matches!(provider.as_str(), "claude" | "codex") || prompt.trim().is_empty() {
                    return Err(
                        "headless target needs provider claude/codex and nonempty prompt".into(),
                    );
                }
                if prompt_files.iter().any(|path| !path.is_absolute()) {
                    return Err("prompt_files must use absolute paths".into());
                }
            }
            JobTarget::Pane { pane_id, prompt } => {
                if pane_id.is_empty() || prompt.trim().is_empty() {
                    return Err("pane target needs pane_id and nonempty prompt".into());
                }
            }
            JobTarget::Script { program, .. } if program.trim().is_empty() => {
                return Err("script target needs program".into());
            }
            JobTarget::Script { .. } => {}
        }
        for delivery in &self.deliver {
            match delivery {
                Delivery::Local { .. } => {}
                Delivery::Pane { pane_id } if !pane_id.is_empty() => {}
                Delivery::Teams { webhook_env } if env_name(webhook_env) => {}
                _ => return Err("invalid delivery destination or environment variable name".into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_job_is_persisted_as_absolute_one_shot() {
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let job = Job::new(
            "backup".into(),
            "30m",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "backup".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        assert!(job.schedule.contains('T'));
        assert_eq!(job.next_run_at, Some(now.unix_timestamp() + 1800));
        assert_eq!(
            job.parsed_schedule()
                .unwrap()
                .next_after(now)
                .unwrap()
                .unix_timestamp(),
            now.unix_timestamp() + 1800
        );
    }

    #[test]
    fn rejects_secret_literal_and_ambiguous_targets() {
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut job = Job::new(
            "notify".into(),
            "every 1h",
            UtcOffset::UTC,
            now,
            JobTarget::Headless {
                provider: "codex".into(),
                prompt: "check".into(),
                prompt_files: Vec::new(),
                model: None,
                effort: None,
                skills: Vec::new(),
            },
        )
        .unwrap();
        job.deliver = vec![Delivery::Teams {
            webhook_env: "https://example.com/secret".into(),
        }];
        assert!(job.validate().is_err());
        job.deliver = vec![Delivery::Teams {
            webhook_env: "TEAMS_WEBHOOK_URL".into(),
        }];
        assert!(job.validate().is_ok());
        job.target = JobTarget::Pane {
            pane_id: "".into(),
            prompt: "check".into(),
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn email_is_not_a_supported_delivery_kind() {
        let former_email = serde_json::json!({
            "kind":"email", "smtp_url_env":"SMTP_URL", "username_env":"SMTP_USER",
            "password_env":"SMTP_PASS", "from":"x@example.invalid", "to":["y@example.invalid"]
        });
        assert!(serde_json::from_value::<Delivery>(former_email).is_err());
    }
}
