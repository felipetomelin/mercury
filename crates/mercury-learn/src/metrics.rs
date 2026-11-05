//! Bounded, observed task measurements. These numbers describe samples; they
//! do not assert that a Learn change caused an improvement.

use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::storage::{Storage, StorageError};
use crate::transcript::Provider;

const MAX_SAMPLES: i64 = 4096;
const MAX_TOKENS_PER_SAMPLE: u64 = 10_000_000;
const MAX_COST_MICROUSD_PER_SAMPLE: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricPhase {
    Observed,
    Baseline,
    After,
}

impl MetricPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Baseline => "baseline",
            Self::After => "after",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    Succeeded,
    Failed,
}

impl TaskOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TaskObservation<'a> {
    /// Stable ID of this observation for bounded replay deduplication.
    pub observation_id: &'a str,
    /// Stable opaque identity supplied by a trusted caller, never task text.
    pub task_key: &'a str,
    pub provider: Provider,
    pub phase: MetricPhase,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `None` means unobserved, not zero cost.
    pub cost_microusd: Option<u64>,
    /// `None` means the task outcome was not observed.
    pub outcome: Option<TaskOutcome>,
    /// Time of the observed task event, not the time of a delayed replay.
    pub observed_at_ms: i64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ObservedAggregate {
    pub samples: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Present only if cost was observed for every sample.
    pub cost_microusd: Option<u64>,
    pub succeeded: u64,
    pub failed: u64,
    pub unknown_outcomes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonState {
    InsufficientData,
    ComparableObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskComparison {
    /// Unclassified run history; never used to claim a before/after comparison.
    pub observed: ObservedAggregate,
    pub baseline: ObservedAggregate,
    pub after: ObservedAggregate,
    pub state: ComparisonState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentObservedMetric {
    pub task_key: String,
    pub provider: Provider,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microusd: Option<u64>,
    pub outcome: Option<TaskOutcome>,
    pub observed_at_ms: i64,
}

fn valid_task_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 160
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

/// A short, non-secret binding to the exact allowlisted path at run completion.
/// The digest is only an identity guard, not an authorization token.
pub fn target_path_fingerprint(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    let digest = Sha256::digest(text.as_bytes());
    Some(format!(
        "{:016x}",
        u64::from_be_bytes(digest[..8].try_into().ok()?)
    ))
}

fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
    }
}

impl Storage {
    /// Newest unclassified samples only. No prompt, path, or raw provider
    /// envelope is returned; the opaque task key is bounded at insertion.
    pub fn recent_observed_metrics(
        &self,
        limit: usize,
    ) -> Result<Vec<RecentObservedMetric>, StorageError> {
        if !(1..=16).contains(&limit) {
            return Err(StorageError::InvalidInput);
        }
        let mut query = self.connection.prepare(
            "SELECT task_key,provider,input_tokens,output_tokens,cost_microusd,outcome,observed_at_ms
             FROM task_metrics WHERE phase='observed'
             ORDER BY observed_at_ms DESC,id DESC LIMIT ?1",
        )?;
        let rows = query.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (task_key, provider, input, output, cost, outcome, observed_at_ms) = row?;
            let provider = match provider.as_str() {
                "claude" => Provider::Claude,
                "codex" => Provider::Codex,
                _ => return Err(StorageError::Corrupt),
            };
            let outcome = match outcome.as_deref() {
                Some("succeeded") => Some(TaskOutcome::Succeeded),
                Some("failed") => Some(TaskOutcome::Failed),
                None => None,
                _ => return Err(StorageError::Corrupt),
            };
            if !valid_task_key(&task_key)
                || input < 0
                || output < 0
                || cost.is_some_and(|value| value < 0)
                || observed_at_ms < 0
            {
                return Err(StorageError::Corrupt);
            }
            Ok(RecentObservedMetric {
                task_key,
                provider,
                input_tokens: input as u64,
                output_tokens: output as u64,
                cost_microusd: cost.map(|value| value as u64),
                outcome,
                observed_at_ms,
            })
        })
        .collect()
    }

    /// Called only with provider-reported usage and an observed task outcome.
    /// The database is private; no prompt or task description is persisted.
    pub fn record_task_observation(
        &mut self,
        observation: TaskObservation<'_>,
    ) -> Result<bool, StorageError> {
        if !valid_task_key(observation.observation_id)
            || !valid_task_key(observation.task_key)
            || observation.input_tokens > MAX_TOKENS_PER_SAMPLE
            || observation.output_tokens > MAX_TOKENS_PER_SAMPLE
            || observation
                .cost_microusd
                .is_some_and(|cost| cost > MAX_COST_MICROUSD_PER_SAMPLE)
            || observation.observed_at_ms < 0
        {
            return Err(StorageError::InvalidInput);
        }
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "INSERT INTO task_metrics
                (observation_id,task_key,provider,phase,input_tokens,output_tokens,cost_microusd,outcome,observed_at_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(provider,observation_id) DO NOTHING",
            params![
                observation.observation_id,
                observation.task_key,
                provider_name(observation.provider),
                observation.phase.as_str(),
                observation.input_tokens as i64,
                observation.output_tokens as i64,
                observation.cost_microusd.map(|value| value as i64),
                observation.outcome.map(TaskOutcome::as_str),
                observation.observed_at_ms,
            ],
        )?;
        if changed == 0 {
            let existing = transaction
                .query_row(
                    "SELECT task_key,phase,input_tokens,output_tokens,cost_microusd,outcome,observed_at_ms
                     FROM task_metrics WHERE provider=?1 AND observation_id=?2",
                    params![
                        provider_name(observation.provider),
                        observation.observation_id
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, i64>(6)?,
                        ))
                    },
                )
                .optional()?;
            let expected = (
                observation.task_key,
                observation.phase.as_str(),
                observation.input_tokens as i64,
                observation.output_tokens as i64,
                observation.cost_microusd.map(|value| value as i64),
                observation.outcome.map(TaskOutcome::as_str),
                observation.observed_at_ms,
            );
            if existing.as_ref().is_none_or(|row| {
                (
                    row.0.as_str(),
                    row.1.as_str(),
                    row.2,
                    row.3,
                    row.4,
                    row.5.as_deref(),
                    row.6,
                ) != expected
            }) {
                return Err(StorageError::InvalidInput);
            }
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM task_metrics WHERE id NOT IN
                (SELECT id FROM task_metrics ORDER BY id DESC LIMIT ?1)",
            [MAX_SAMPLES],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Same task identity and provider are required for both cohorts. Even a
    /// comparable result is descriptive rather than a causal quality claim.
    pub fn task_comparison(
        &self,
        task_key: &str,
        provider: Provider,
    ) -> Result<TaskComparison, StorageError> {
        if !valid_task_key(task_key) {
            return Err(StorageError::InvalidInput);
        }
        let mut result = TaskComparison {
            observed: ObservedAggregate::default(),
            baseline: ObservedAggregate::default(),
            after: ObservedAggregate::default(),
            state: ComparisonState::InsufficientData,
        };
        let mut query = self.connection.prepare(
            "SELECT phase,COUNT(*),SUM(input_tokens),SUM(output_tokens),
                    SUM(cost_microusd),COUNT(cost_microusd),
                    SUM(CASE WHEN outcome='succeeded' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN outcome='failed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN outcome IS NULL THEN 1 ELSE 0 END)
             FROM task_metrics WHERE task_key=?1 AND provider=?2 GROUP BY phase",
        )?;
        let rows = query.query_map(params![task_key, provider_name(provider)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        for row in rows {
            let (phase, count, input, output, cost, cost_count, succeeded, failed, unknown) = row?;
            let aggregate = ObservedAggregate {
                samples: count as u64,
                input_tokens: input as u64,
                output_tokens: output as u64,
                cost_microusd: (cost_count == count).then_some(cost.unwrap_or(0) as u64),
                succeeded: succeeded as u64,
                failed: failed as u64,
                unknown_outcomes: unknown as u64,
            };
            match phase.as_str() {
                "observed" => result.observed = aggregate,
                "baseline" => result.baseline = aggregate,
                "after" => result.after = aggregate,
                _ => return Err(StorageError::Corrupt),
            }
        }
        if result.baseline.samples == 0 && result.after.samples == 0 {
            self.classify_associated_observations(task_key, provider, &mut result)?;
        }
        if result.baseline.samples > 0
            && result.after.samples > 0
            && result.baseline.unknown_outcomes == 0
            && result.after.unknown_outcomes == 0
        {
            result.state = ComparisonState::ComparableObserved;
        }
        Ok(result)
    }

    fn classify_associated_observations(
        &self,
        task_key: &str,
        provider: Provider,
        result: &mut TaskComparison,
    ) -> Result<(), StorageError> {
        let Some((_, binding)) = task_key
            .strip_prefix("cron:")
            .and_then(|key| key.split_once(":target:"))
        else {
            return Ok(());
        };
        let Some((target_id, path_fingerprint)) = binding.rsplit_once(':') else {
            // Pre-binding samples stay observed, never retroactively classified.
            return Ok(());
        };
        if target_id.is_empty()
            || path_fingerprint.len() != 16
            || !path_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Ok(());
        }
        let mut changes = self.connection.prepare(
            "SELECT id,target_path,after_hash,created_at_ms,applied_at_ms
             FROM changes WHERE target_id=?1 AND status='applied' LIMIT 2",
        )?;
        let matches = changes
            .query_map([target_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let [(change_id, path, expected_hash, prepared_at, Some(applied_at))] = matches.as_slice()
        else {
            return Ok(());
        };
        if applied_at < prepared_at || *prepared_at < 0 {
            return Ok(());
        }
        let path = Path::new(path);
        if target_path_fingerprint(path).as_deref() != Some(path_fingerprint)
            || !self.target_still_matches(change_id, path, target_id, expected_hash)?
        {
            return Ok(());
        }
        let mut query = self.connection.prepare(
            "SELECT observed_at_ms,input_tokens,output_tokens,cost_microusd,outcome
             FROM task_metrics WHERE task_key=?1 AND provider=?2 AND phase='observed'",
        )?;
        let rows = query.query_map(params![task_key, provider_name(provider)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (at, input, output, cost, outcome) = row?;
            let cohort = if at < *prepared_at {
                &mut result.baseline
            } else if at > *applied_at {
                &mut result.after
            } else {
                continue;
            };
            let first_sample = cohort.samples == 0;
            cohort.samples += 1;
            cohort.input_tokens += input as u64;
            cohort.output_tokens += output as u64;
            cohort.cost_microusd = match (first_sample, cohort.cost_microusd, cost) {
                (true, _, Some(cost)) => Some(cost as u64),
                (true, _, None) => None,
                (false, None, _) => None,
                (false, Some(total), Some(cost)) => Some(total + cost as u64),
                (false, Some(_), None) => None,
            };
            match outcome.as_deref() {
                Some("succeeded") => cohort.succeeded += 1,
                Some("failed") => cohort.failed += 1,
                None => cohort.unknown_outcomes += 1,
                _ => return Err(StorageError::Corrupt),
            }
        }
        Ok(())
    }

    fn target_still_matches(
        &self,
        change_id: &str,
        path: &Path,
        target_id: &str,
        expected_hash: &str,
    ) -> Result<bool, StorageError> {
        if crate::policy::check_existing_file(path, 1024 * 1024).is_err() {
            return Ok(false);
        }
        let current = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(false),
        };
        if crate::storage::hash(&current) == expected_hash {
            return Ok(true);
        }
        let Some(record) = self.get(change_id)? else {
            return Ok(false);
        };
        let after = match self.read_after_snapshot(&record) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(false),
        };
        match path.file_name().and_then(|name| name.to_str()) {
            Some("config.toml") => {
                let current = std::str::from_utf8(&current).ok();
                let after = std::str::from_utf8(&after).ok();
                Ok(current.zip(after).is_some_and(|(current, after)| {
                    crate::policy::route_section(current)
                        .ok()
                        .zip(crate::policy::route_section(after).ok())
                        .is_some_and(|(current, after)| current == after)
                }))
            }
            Some("jobs.toml") => {
                let current = std::str::from_utf8(&current)
                    .ok()
                    .and_then(|text| toml::from_str::<mercury_cron::storage::JobsFile>(text).ok());
                let after = std::str::from_utf8(&after)
                    .ok()
                    .and_then(|text| toml::from_str::<mercury_cron::storage::JobsFile>(text).ok());
                let same = current.zip(after).and_then(|(current, after)| {
                    let current = current.jobs.into_iter().find(|job| job.id == target_id)?;
                    let after = after.jobs.into_iter().find(|job| job.id == target_id)?;
                    Some(stable_job(current) == stable_job(after))
                });
                Ok(same.unwrap_or(false))
            }
            _ => Ok(false),
        }
    }
}

fn stable_job(mut job: mercury_cron::Job) -> mercury_cron::Job {
    job.next_run_at = None;
    if let Some(repeat) = &mut job.repeat {
        repeat.completed = 0;
    }
    job
}
