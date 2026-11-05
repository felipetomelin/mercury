//! Single-owner scheduler state. Reserving in SQLite and advancing jobs.toml
//! both happen before a `ReservedRun` is returned for dispatch.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use crate::executor::ObservedUsage;
use crate::storage::{IncidentUpdate, JobsFile, SqliteLedger};
use crate::{CatchUp, Job, Schedule};

const MAX_ACTIVE_RUNS: u32 = 4;

#[derive(Debug, Clone)]
pub struct ReservedRun {
    pub id: String,
    pub job: Job,
    pub occurrence_at: i64,
}

pub struct Scheduler {
    jobs_path: PathBuf,
    jobs: JobsFile,
    ledger: SqliteLedger,
    _lock: File,
}

impl Scheduler {
    pub fn open(
        jobs_path: &Path,
        ledger_path: &Path,
        lock_path: &Path,
        now: OffsetDateTime,
    ) -> Result<Self, String> {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create scheduler directory: {error}"))?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(lock_path)
            .map_err(|error| format!("open scheduler lock: {error}"))?;
        lock.try_lock()
            .map_err(|error| format!("another scheduler owns this cron directory: {error}"))?;
        let jobs = {
            let _guard = crate::jobs_file_lock()?;
            JobsFile::load(jobs_path)?
        };
        let ledger = SqliteLedger::open(ledger_path)?;
        ledger.interrupt_unfinished(now.unix_timestamp())?;
        let mut scheduler = Self {
            jobs_path: jobs_path.to_owned(),
            jobs,
            ledger,
            _lock: lock,
        };
        scheduler.reconcile_repeat_counts()?;
        Ok(scheduler)
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs.jobs
    }

    pub fn ledger(&self) -> &SqliteLedger {
        &self.ledger
    }

    pub fn next_run_at(&self) -> Option<i64> {
        self.jobs
            .jobs
            .iter()
            .filter(|job| job.enabled)
            .filter_map(|job| job.next_run_at)
            .min()
    }

    fn refresh_jobs_from_disk(&mut self) -> Result<(), String> {
        self.jobs = JobsFile::load(&self.jobs_path)?;
        Ok(())
    }

    /// Reconcile a reversible Learn edit or undo before deciding the next run.
    pub fn reload_jobs(&mut self) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()
    }

    pub fn add(&mut self, job: Job) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        job.validate()?;
        if self.jobs.jobs.iter().any(|existing| existing.id == job.id) {
            return Err("duplicate job id".into());
        }
        let mut updated = self.jobs.clone();
        updated.jobs.push(job);
        updated.save_atomic(&self.jobs_path)?;
        self.jobs = updated;
        Ok(())
    }

    pub fn edit(&mut self, mut job: Job) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        job.validate()?;
        if self.ledger.active_count(&job.id)? > 0 {
            return Err("cannot edit a job with active runs".into());
        }
        let mut updated = self.jobs.clone();
        let current = updated
            .jobs
            .iter_mut()
            .find(|current| current.id == job.id)
            .ok_or("job not found")?;
        if let Some(repeat) = &mut job.repeat {
            repeat.completed = self.ledger.completed_count(&job.id)?.min(repeat.times);
            if repeat.completed >= repeat.times {
                job.enabled = false;
            }
        }
        *current = job;
        updated.save_atomic(&self.jobs_path)?;
        self.jobs = updated;
        Ok(())
    }

    pub fn pause(&mut self, job_id: &str, paused: bool) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        let mut updated = self.jobs.clone();
        let job = updated
            .jobs
            .iter_mut()
            .find(|job| job.id == job_id)
            .ok_or("job not found")?;
        if !paused && job.next_run_at.is_none() {
            return Err("job has no next occurrence".into());
        }
        job.enabled = !paused;
        updated.save_atomic(&self.jobs_path)?;
        self.jobs = updated;
        Ok(())
    }

    pub fn remove(&mut self, job_id: &str) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        let mut updated = self.jobs.clone();
        updated.jobs.retain(|job| job.id != job_id);
        if updated.jobs.len() == self.jobs.jobs.len() {
            return Err("job not found".into());
        }
        updated.save_atomic(&self.jobs_path)?;
        self.jobs = updated;
        Ok(())
    }

    /// A manual run does not modify or reserve the next scheduled occurrence.
    pub fn run_now(&mut self, job_id: &str, now: OffsetDateTime) -> Result<ReservedRun, String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        if self.ledger.active_total()? >= MAX_ACTIVE_RUNS {
            return Err("cron concurrency limit reached".into());
        }
        let job = self
            .jobs
            .jobs
            .iter()
            .find(|job| job.id == job_id)
            .ok_or("job not found")?
            .clone();
        if let Some(repeat) = &job.repeat {
            let active = self.ledger.active_count(job_id)?;
            if repeat.completed.saturating_add(active) >= repeat.times {
                return Err("job reached repeat limit or all remaining slots are running".into());
            }
        }
        let occurrence_at = now.unix_timestamp();
        let id = self.ledger.reserve_manual(job_id, occurrence_at)?;
        Ok(ReservedRun {
            id,
            job,
            occurrence_at,
        })
    }

    pub fn mark_running(&self, run_id: &str, now: OffsetDateTime) -> Result<(), String> {
        self.ledger.mark_running(run_id, now.unix_timestamp())
    }

    pub fn finish(
        &mut self,
        run: &ReservedRun,
        now: OffsetDateTime,
        success: bool,
        output_path: Option<&Path>,
        error_class: Option<&str>,
        learn_usage: Option<(&str, ObservedUsage, Option<&str>)>,
    ) -> Result<Option<IncidentUpdate>, String> {
        self.ledger.finish(
            &run.id,
            now.unix_timestamp(),
            success,
            output_path,
            error_class,
            learn_usage,
        )?;
        let incident = if success {
            self.ledger
                .resolve_incidents(&run.job.id, now.unix_timestamp())?;
            None
        } else {
            Some(self.ledger.record_failure(
                &run.job.id,
                error_class.unwrap_or("execution_failed"),
                now.unix_timestamp(),
            )?)
        };
        self.reconcile_repeat_counts()?;
        Ok(incident)
    }

    fn reconcile_repeat_counts(&mut self) -> Result<(), String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        let mut updated = self.jobs.clone();
        for job in &mut updated.jobs {
            if let Some(repeat) = job.repeat.as_mut() {
                let count = self.ledger.completed_count(&job.id)?.min(repeat.times);
                repeat.completed = count;
                if count >= repeat.times {
                    job.enabled = false;
                }
            }
        }
        if updated.jobs != self.jobs.jobs {
            updated.save_atomic(&self.jobs_path)?;
            self.jobs = updated;
        }
        Ok(())
    }

    /// Applies restart catch-up policy. `RunOnce` reserves at most one missed
    /// occurrence; `Skip` advances without dispatch. Any interrupted run's
    /// unique reservation prevents re-dispatch of the same occurrence.
    pub fn recover_on_startup(&mut self, now: OffsetDateTime) -> Result<Vec<ReservedRun>, String> {
        self.take_due_internal(now, true)
    }

    pub fn take_due(&mut self, now: OffsetDateTime) -> Result<Vec<ReservedRun>, String> {
        self.take_due_internal(now, false)
    }

    fn take_due_internal(
        &mut self,
        now: OffsetDateTime,
        startup: bool,
    ) -> Result<Vec<ReservedRun>, String> {
        let _guard = crate::jobs_file_lock()?;
        self.refresh_jobs_from_disk()?;
        let mut updated = self.jobs.clone();
        let mut reserved = Vec::new();
        let mut changed = false;
        let mut available = MAX_ACTIVE_RUNS.saturating_sub(self.ledger.active_total()?);
        for job in &mut updated.jobs {
            let Some(due) = job
                .next_run_at
                .filter(|due| job.enabled && *due <= now.unix_timestamp())
            else {
                continue;
            };
            let schedule = job.parsed_schedule()?;
            if available == 0 && (!startup || job.catch_up == CatchUp::RunOnce) {
                continue;
            }
            let at_limit = if let Some(repeat) = &job.repeat {
                repeat
                    .completed
                    .saturating_add(self.ledger.active_count(&job.id)?)
                    >= repeat.times
            } else {
                false
            };
            let should_reserve = !at_limit && (!startup || job.catch_up == CatchUp::RunOnce);
            let id = if should_reserve {
                self.ledger.reserve(&job.id, due)?
            } else {
                None
            };
            job.next_run_at = advance_past(&schedule, due, now.unix_timestamp());
            if job.next_run_at.is_none() {
                job.enabled = false;
            }
            if let Some(id) = id {
                available = available.saturating_sub(1);
                reserved.push(ReservedRun {
                    id,
                    job: job.clone(),
                    occurrence_at: due,
                });
            }
            changed = true;
        }
        if changed {
            updated.save_atomic(&self.jobs_path)?;
            self.jobs = updated;
        }
        Ok(reserved)
    }
}

fn advance_past(schedule: &Schedule, due: i64, now: i64) -> Option<i64> {
    match schedule {
        Schedule::Once(_) => None,
        Schedule::Every { seconds } => {
            let interval = i64::try_from(*seconds).ok()?;
            let steps = now
                .saturating_sub(due)
                .div_euclid(interval)
                .checked_add(1)?;
            due.checked_add(interval.checked_mul(steps)?)
        }
        Schedule::Cron(cron) => cron
            .next_after(OffsetDateTime::from_unix_timestamp(now).ok()?)
            .map(|at| at.unix_timestamp()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobTarget, Repeat};
    use time::UtcOffset;

    fn temp_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("mercury-cron-scheduler-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn open(dir: &Path, now: OffsetDateTime) -> Scheduler {
        Scheduler::open(
            &dir.join("jobs.toml"),
            &dir.join("executions.db"),
            &dir.join("scheduler.lock"),
            now,
        )
        .unwrap()
    }

    fn script_job(now: OffsetDateTime, expression: &str) -> Job {
        Job::new(
            "check".into(),
            expression,
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "check".into(),
                args: Vec::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn reserves_then_advances_before_dispatch_and_survives_restart() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        let job = script_job(now, "every 1h");
        let job_id = job.id.clone();
        scheduler.add(job).unwrap();
        let due = now + time::Duration::hours(1);
        let reserved = scheduler.take_due(due).unwrap();
        assert_eq!(reserved.len(), 1);
        assert_eq!(reserved[0].occurrence_at, due.unix_timestamp());
        assert_eq!(
            JobsFile::load(&dir.join("jobs.toml")).unwrap().jobs[0].next_run_at,
            Some((due + time::Duration::hours(1)).unix_timestamp())
        );
        assert_eq!(scheduler.take_due(due).unwrap().len(), 0);
        drop(scheduler);
        let mut reopened = open(&dir, due + time::Duration::minutes(1));
        assert!(reopened
            .recover_on_startup(due + time::Duration::minutes(1))
            .unwrap()
            .is_empty());
        assert_eq!(
            reopened
                .ledger
                .find(&job_id, due.unix_timestamp())
                .unwrap()
                .unwrap()
                .status,
            "interrupted"
        );
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_catch_up_skips_or_runs_once_without_burst() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        let skip = script_job(now, "every 1h");
        let mut once = script_job(now, "every 1h");
        once.catch_up = CatchUp::RunOnce;
        scheduler.add(skip).unwrap();
        scheduler.add(once).unwrap();
        drop(scheduler);
        let later = now + time::Duration::hours(5);
        let mut reopened = open(&dir, later);
        let reserved = reopened.recover_on_startup(later).unwrap();
        assert_eq!(reserved.len(), 1);
        assert_eq!(
            reopened
                .jobs()
                .iter()
                .filter_map(|job| job.next_run_at)
                .collect::<Vec<_>>(),
            vec![(now + time::Duration::hours(6)).unix_timestamp(); 2]
        );
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manual_run_does_not_consume_scheduled_occurrence() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        let job = script_job(now, "every 1h");
        let job_id = job.id.clone();
        scheduler.add(job).unwrap();
        let due = now + time::Duration::hours(1);
        let manual = scheduler.run_now(&job_id, due).unwrap();
        let scheduled = scheduler.take_due(due).unwrap();
        assert_eq!(scheduled.len(), 1);
        assert_ne!(manual.id, scheduled[0].id);
        assert_eq!(scheduler.ledger.list_runs(&job_id, 10).unwrap().len(), 2);
        drop(scheduler);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeat_limit_is_reconciled_from_ledger_after_completion() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        let mut job = script_job(now, "every 1h");
        job.repeat = Some(Repeat {
            times: 1,
            completed: 0,
        });
        let job_id = job.id.clone();
        scheduler.add(job).unwrap();
        let due = now + time::Duration::hours(1);
        let run = scheduler.take_due(due).unwrap().remove(0);
        scheduler.mark_running(&run.id, due).unwrap();
        scheduler.finish(&run, due, true, None, None, None).unwrap();
        assert!(!scheduler.jobs()[0].enabled);
        assert_eq!(scheduler.jobs()[0].repeat.as_ref().unwrap().completed, 1);
        assert!(scheduler.run_now(&job_id, due).is_err());
        drop(scheduler);
        let reopened = open(&dir, due);
        assert!(!reopened.jobs()[0].enabled);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeat_limit_counts_in_flight_runs_before_dispatch() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        let mut job = script_job(now, "every 1h");
        job.repeat = Some(Repeat {
            times: 1,
            completed: 0,
        });
        let job_id = job.id.clone();
        scheduler.add(job).unwrap();
        let due = now + time::Duration::hours(1);
        assert_eq!(scheduler.take_due(due).unwrap().len(), 1);
        assert!(scheduler.run_now(&job_id, due).is_err());
        assert!(scheduler
            .take_due(due + time::Duration::hours(1))
            .unwrap()
            .is_empty());
        drop(scheduler);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dispatch_is_bounded_and_preserves_overflow_occurrences() {
        let dir = temp_dir();
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let mut scheduler = open(&dir, now);
        for _ in 0..5 {
            scheduler.add(script_job(now, "every 1h")).unwrap();
        }
        let due = now + time::Duration::hours(1);
        let runs = scheduler.take_due(due).unwrap();
        assert_eq!(runs.len(), 4);
        assert_eq!(scheduler.ledger.active_total().unwrap(), 4);
        assert_eq!(
            scheduler
                .jobs()
                .iter()
                .filter(|job| job.next_run_at == Some(due.unix_timestamp()))
                .count(),
            1
        );
        assert!(scheduler.take_due(due).unwrap().is_empty());
        scheduler
            .finish(&runs[0], due, true, None, None, None)
            .unwrap();
        assert_eq!(scheduler.take_due(due).unwrap().len(), 1);
        drop(scheduler);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
