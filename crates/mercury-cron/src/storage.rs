//! Durable jobs and SQLite run ledger. The scheduler must reserve a run here,
//! advance and save `jobs.toml`, and only then dispatch external effects.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::executor::ObservedUsage;
use crate::Job;

const JOBS_VERSION: u32 = 1;
const DB_VERSION: i32 = 5;

fn valid_learn_target_id(id: &str) -> bool {
    fn valid_base(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 64
            && id.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'-' | b'_'))
            })
    }
    match id.split_once(':') {
        Some((base, fingerprint)) => {
            valid_base(base)
                && fingerprint.len() == 16
                && fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }
        None => valid_base(id), // old outbox rows remain unclassified
    }
}
const MAX_PENDING_LEARN_USAGE: i64 = 4096;
const MAX_JOBS_BYTES: u64 = 4 * 1024 * 1024;
const MAX_JOBS: usize = 1024;

/// Read-only active-run guard for a Learn patch or undo while the scheduler's
/// in-process jobs lock is held. Never creates a ledger as a side effect.
pub fn job_has_active_run(path: &Path, job_id: &str) -> Result<bool, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspect run ledger: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("run ledger must be a regular file".into());
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("open run ledger read-only: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM runs WHERE job_id=?1 AND status IN ('reserved','running')",
            [job_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("query active runs: {error}"))?;
    Ok(count > 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobsFile {
    pub version: u32,
    #[serde(default)]
    pub jobs: Vec<Job>,
}

impl Default for JobsFile {
    fn default() -> Self {
        Self {
            version: JOBS_VERSION,
            jobs: Vec::new(),
        }
    }
}

impl JobsFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(format!("read jobs: {error}")),
        };
        let mut content = String::new();
        file.take(MAX_JOBS_BYTES + 1)
            .read_to_string(&mut content)
            .map_err(|error| format!("read jobs: {error}"))?;
        if content.len() as u64 > MAX_JOBS_BYTES {
            return Err("jobs file exceeds 4 MiB".into());
        }
        let mut value: toml::Value =
            toml::from_str(&content).map_err(|error| format!("parse jobs: {error}"))?;
        // Existing jobs may have an email destination from an earlier build.
        // Drop only that destination while loading so all other jobs continue
        // to run and no SMTP path can be revived by persisted configuration.
        if let Some(jobs) = value.get_mut("jobs").and_then(toml::Value::as_array_mut) {
            for job in jobs {
                if let Some(deliveries) = job.get_mut("deliver").and_then(toml::Value::as_array_mut)
                {
                    deliveries.retain(|delivery| {
                        delivery.get("kind").and_then(toml::Value::as_str) != Some("email")
                    });
                }
            }
        }
        let file: Self = value
            .try_into()
            .map_err(|error| format!("parse jobs: {error}"))?;
        file.validate()?;
        Ok(file)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != JOBS_VERSION {
            return Err(format!("unsupported jobs version {}", self.version));
        }
        if self.jobs.len() > MAX_JOBS {
            return Err("jobs file exceeds 1024 jobs".into());
        }
        let mut ids = std::collections::HashSet::new();
        for job in &self.jobs {
            job.validate()
                .map_err(|error| format!("job {}: {error}", job.id))?;
            if !ids.insert(job.id.as_str()) {
                return Err(format!("duplicate job id {}", job.id));
            }
        }
        Ok(())
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path.parent().ok_or("jobs path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| format!("create jobs directory: {error}"))?;
        let content =
            toml::to_string_pretty(self).map_err(|error| format!("serialize jobs: {error}"))?;
        if content.len() as u64 > MAX_JOBS_BYTES {
            return Err("jobs file exceeds 4 MiB".into());
        }
        let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        let outcome = (|| -> Result<(), String> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| format!("open temporary jobs: {error}"))?;
            file.write_all(content.as_bytes())
                .map_err(|error| format!("write temporary jobs: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync temporary jobs: {error}"))?;
            fs::rename(&temporary, path).map_err(|error| format!("replace jobs: {error}"))?;
            #[cfg(unix)]
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| format!("sync jobs directory: {error}"))?;
            Ok(())
        })();
        if outcome.is_err() {
            let _ = fs::remove_file(temporary);
        }
        outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub job_id: String,
    pub occurrence_at: i64,
    pub trigger: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output_path: Option<PathBuf>,
    pub error_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLearnUsage {
    pub run_id: String,
    pub job_id: String,
    /// Target ID plus path fingerprint frozen at completion; old/unassociated
    /// runs remain None or carry a legacy unbound ID.
    pub learn_target_id: Option<String>,
    pub provider: String,
    pub usage: ObservedUsage,
    pub succeeded: bool,
    /// Persisted run completion time, converted from seconds.
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentRow {
    pub id: String,
    pub job_id: String,
    pub fingerprint: String,
    pub failure_streak: u32,
    pub first_at: i64,
    pub last_at: i64,
    pub last_notified_at: Option<i64>,
    pub acknowledged_at: Option<i64>,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct IncidentUpdate {
    pub incident: IncidentRow,
    pub should_notify: bool,
}

pub struct SqliteLedger {
    connection: Connection,
}

impl SqliteLedger {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create ledger directory: {error}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(|error| format!("restrict ledger directory: {error}"))?;
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .map_err(|error| format!("create private ledger: {error}"))?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("restrict ledger: {error}"))?;
        }
        let connection = Connection::open(path).map_err(|error| format!("open ledger: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let version: i32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if version > DB_VERSION {
            return Err(format!(
                "ledger schema version {version} is newer than supported {DB_VERSION}"
            ));
        }
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| error.to_string())?;
        if version == 1 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE runs_v2 (
                   id TEXT PRIMARY KEY, job_id TEXT NOT NULL, occurrence_at INTEGER NOT NULL,
                   trigger TEXT NOT NULL DEFAULT 'scheduled', status TEXT NOT NULL,
                   started_at INTEGER, finished_at INTEGER, output_path TEXT, error_class TEXT
                 );
                 INSERT INTO runs_v2(id, job_id, occurrence_at, status, started_at, finished_at, output_path, error_class)
                   SELECT id, job_id, occurrence_at, status, started_at, finished_at, output_path, error_class FROM runs;
                 DROP TABLE runs;
                 ALTER TABLE runs_v2 RENAME TO runs;
                 CREATE UNIQUE INDEX runs_scheduled_occurrence ON runs(job_id, occurrence_at) WHERE trigger = 'scheduled';
                 PRAGMA user_version = 2;
                 COMMIT;"
            ).map_err(|error| format!("migrate ledger: {error}"))?;
        }
        if version == 1 || version == 2 {
            connection
                .execute_batch(
                    "ALTER TABLE incidents ADD COLUMN resolved_at INTEGER;
                 PRAGMA user_version = 3;",
                )
                .map_err(|error| format!("migrate incidents: {error}"))?;
        }
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
               id TEXT PRIMARY KEY,
               job_id TEXT NOT NULL,
               occurrence_at INTEGER NOT NULL,
               trigger TEXT NOT NULL DEFAULT 'scheduled',
               status TEXT NOT NULL,
               started_at INTEGER,
               finished_at INTEGER,
               output_path TEXT,
               error_class TEXT
             );
             CREATE UNIQUE INDEX IF NOT EXISTS runs_scheduled_occurrence ON runs(job_id, occurrence_at) WHERE trigger = 'scheduled';
             CREATE INDEX IF NOT EXISTS runs_by_job ON runs(job_id, occurrence_at DESC);
             CREATE TABLE IF NOT EXISTS incidents (
               id TEXT PRIMARY KEY,
               job_id TEXT NOT NULL,
               fingerprint TEXT NOT NULL,
               failure_streak INTEGER NOT NULL,
               first_at INTEGER NOT NULL,
               last_at INTEGER NOT NULL,
               last_notified_at INTEGER,
               acknowledged_at INTEGER,
               resolved_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS incidents_by_job ON incidents(job_id, last_at DESC);
             CREATE TABLE IF NOT EXISTS learn_usage_outbox (
               run_id TEXT PRIMARY KEY,
               job_id TEXT NOT NULL,
               learn_target_id TEXT,
               provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
               input_tokens INTEGER NOT NULL CHECK(input_tokens BETWEEN 0 AND 10000000),
               output_tokens INTEGER NOT NULL CHECK(output_tokens BETWEEN 0 AND 10000000),
               cost_microusd INTEGER CHECK(cost_microusd BETWEEN 0 AND 1000000000),
               succeeded INTEGER NOT NULL CHECK(succeeded IN (0,1))
             );",
            )
            .map_err(|error| format!("initialize ledger: {error}"))?;
        if version == 4 {
            connection
                .execute_batch("ALTER TABLE learn_usage_outbox ADD COLUMN learn_target_id TEXT;")
                .map_err(|error| format!("migrate Learn usage association: {error}"))?;
        }
        connection
            .pragma_update(None, "user_version", DB_VERSION)
            .map_err(|error| error.to_string())?;
        Ok(Self { connection })
    }

    /// Reserves a unique job occurrence. `None` means it was already reserved
    /// by this or another server. Callers must save the advanced jobs file
    /// before dispatching the returned run id.
    pub fn reserve(&self, job_id: &str, occurrence_at: i64) -> Result<Option<String>, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO runs(id, job_id, occurrence_at, status) VALUES (?1, ?2, ?3, 'reserved')",
            params![id, job_id, occurrence_at],
        ).map_err(|error| error.to_string())?;
        Ok((inserted == 1).then_some(id))
    }

    pub fn reserve_manual(&self, job_id: &str, requested_at: i64) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO runs(id, job_id, occurrence_at, trigger, status) VALUES (?1, ?2, ?3, 'manual', 'reserved')",
            params![id, job_id, requested_at],
        ).map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn mark_running(&self, run_id: &str, started_at: i64) -> Result<(), String> {
        let changed = self.connection.execute(
            "UPDATE runs SET status = 'running', started_at = ?2 WHERE id = ?1 AND status = 'reserved'",
            params![run_id, started_at],
        ).map_err(|error| error.to_string())?;
        if changed != 1 {
            return Err("run is not reserved".into());
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        run_id: &str,
        finished_at: i64,
        success: bool,
        output_path: Option<&Path>,
        error_class: Option<&str>,
        learn_usage: Option<(&str, ObservedUsage, Option<&str>)>,
    ) -> Result<(), String> {
        if error_class.is_some_and(|class| {
            class.len() > 64
                || class.is_empty()
                || !class
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        }) {
            return Err("error_class must be a short machine-readable code".into());
        }
        if learn_usage.is_some_and(|(provider, usage, target_id)| {
            !matches!(provider, "claude" | "codex")
                || usage.input_tokens > 10_000_000
                || usage.output_tokens > 10_000_000
                || usage.cost_microusd.is_some_and(|cost| cost > 1_000_000_000)
                || target_id.is_some_and(|id| !valid_learn_target_id(id))
        }) {
            return Err("invalid provider-reported usage".into());
        }
        let status = if success { "success" } else { "failure" };
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let changed = transaction.execute(
            "UPDATE runs SET status = ?2, finished_at = ?3, output_path = ?4, error_class = ?5 WHERE id = ?1 AND status IN ('reserved', 'running')",
            params![run_id, status, finished_at, output_path.map(|path| path.to_string_lossy().into_owned()), error_class],
        ).map_err(|error| error.to_string())?;
        if changed != 1 {
            return Err("run is not active".into());
        }
        if let Some((provider, usage, target_id)) = learn_usage {
            let job_id: String = transaction
                .query_row("SELECT job_id FROM runs WHERE id=?1", [run_id], |row| {
                    row.get(0)
                })
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO learn_usage_outbox
                    (run_id,job_id,learn_target_id,provider,input_tokens,output_tokens,cost_microusd,succeeded)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        run_id,
                        job_id,
                        target_id,
                        provider,
                        usage.input_tokens as i64,
                        usage.output_tokens as i64,
                        usage.cost_microusd.map(|value| value as i64),
                        i64::from(success)
                    ],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "DELETE FROM learn_usage_outbox WHERE run_id NOT IN
                    (SELECT run_id FROM learn_usage_outbox ORDER BY rowid DESC LIMIT ?1)",
                    [MAX_PENDING_LEARN_USAGE],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn pending_learn_usage(&self, limit: usize) -> Result<Vec<PendingLearnUsage>, String> {
        if !(1..=64).contains(&limit) {
            return Err("invalid Learn usage limit".into());
        }
        let mut query = self
            .connection
            .prepare(
                "SELECT o.run_id,o.job_id,o.learn_target_id,o.provider,o.input_tokens,o.output_tokens,
                    o.cost_microusd,o.succeeded,r.finished_at
             FROM learn_usage_outbox o LEFT JOIN runs r ON r.id=o.run_id
             ORDER BY o.rowid LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = query
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        rows.map(|row| {
            let (
                run_id,
                job_id,
                learn_target_id,
                provider,
                input,
                output,
                cost,
                succeeded,
                finished_at,
            ) = row.map_err(|error| error.to_string())?;
            if !matches!(provider.as_str(), "claude" | "codex")
                || input < 0
                || output < 0
                || cost.is_some_and(|value| value < 0)
                || !matches!(succeeded, 0 | 1)
                || learn_target_id
                    .as_deref()
                    .is_some_and(|id| !valid_learn_target_id(id))
                || finished_at.is_none_or(|value| value < 0)
            {
                return Err("invalid pending Learn usage".into());
            }
            let observed_at_ms = finished_at
                .and_then(|value| value.checked_mul(1000))
                .ok_or("invalid pending Learn completion time")?;
            Ok(PendingLearnUsage {
                run_id,
                job_id,
                learn_target_id,
                provider,
                usage: ObservedUsage {
                    input_tokens: input as u64,
                    output_tokens: output as u64,
                    cost_microusd: cost.map(|value| value as u64),
                },
                succeeded: succeeded == 1,
                observed_at_ms,
            })
        })
        .collect()
    }

    pub fn ack_learn_usage(&self, run_id: &str) -> Result<bool, String> {
        self.connection
            .execute("DELETE FROM learn_usage_outbox WHERE run_id=?1", [run_id])
            .map(|changed| changed == 1)
            .map_err(|error| error.to_string())
    }

    pub fn find(&self, job_id: &str, occurrence_at: i64) -> Result<Option<RunRow>, String> {
        self.connection.query_row(
            "SELECT id, job_id, occurrence_at, trigger, status, started_at, finished_at, output_path, error_class FROM runs WHERE job_id = ?1 AND occurrence_at = ?2 AND trigger = 'scheduled'",
            params![job_id, occurrence_at],
            |row| Ok(RunRow {
                id: row.get(0)?, job_id: row.get(1)?, occurrence_at: row.get(2)?, trigger: row.get(3)?, status: row.get(4)?,
                started_at: row.get(5)?, finished_at: row.get(6)?,
                output_path: row.get::<_, Option<String>>(7)?.map(PathBuf::from), error_class: row.get(8)?,
            }),
        ).optional().map_err(|error| error.to_string())
    }

    pub fn interrupt_unfinished(&self, at: i64) -> Result<usize, String> {
        self.connection
            .execute(
                "UPDATE runs SET status = 'interrupted', finished_at = ?1, error_class = 'server_restart' WHERE status IN ('reserved', 'running')",
                params![at],
            )
            .map_err(|error| error.to_string())
    }

    pub fn completed_count(&self, job_id: &str) -> Result<u32, String> {
        self.connection
            .query_row(
                "SELECT count(*) FROM runs WHERE job_id = ?1 AND status IN ('success', 'failure')",
                params![job_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }

    pub fn active_count(&self, job_id: &str) -> Result<u32, String> {
        self.connection
            .query_row(
                "SELECT count(*) FROM runs WHERE job_id = ?1 AND status IN ('reserved', 'running')",
                params![job_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }

    pub fn active_total(&self) -> Result<u32, String> {
        self.connection
            .query_row(
                "SELECT count(*) FROM runs WHERE status IN ('reserved', 'running')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }

    pub fn record_failure(
        &self,
        job_id: &str,
        fingerprint: &str,
        at: i64,
    ) -> Result<IncidentUpdate, String> {
        if fingerprint.is_empty()
            || fingerprint.len() > 64
            || !fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err("incident fingerprint must be a short machine-readable code".into());
        }
        let existing: Option<(String, u32, i64, Option<i64>)> = self.connection.query_row(
            "SELECT id, failure_streak, first_at, last_notified_at FROM incidents WHERE job_id = ?1 AND fingerprint = ?2 AND acknowledged_at IS NULL AND resolved_at IS NULL ORDER BY last_at DESC LIMIT 1",
            params![job_id, fingerprint],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional().map_err(|error| error.to_string())?;
        let (id, failure_streak, first_at, previous_notice) = if let Some((
            id,
            streak,
            first,
            notice,
        )) = existing
        {
            let next = streak.saturating_add(1);
            self.connection
                .execute(
                    "UPDATE incidents SET failure_streak = ?2, last_at = ?3 WHERE id = ?1",
                    params![id, next, at],
                )
                .map_err(|error| error.to_string())?;
            (id, next, first, notice)
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            self.connection.execute(
                "INSERT INTO incidents(id, job_id, fingerprint, failure_streak, first_at, last_at) VALUES (?1, ?2, ?3, 1, ?4, ?4)",
                params![id, job_id, fingerprint, at],
            ).map_err(|error| error.to_string())?;
            (id, 1, at, None)
        };
        let should_notify = previous_notice.is_none_or(|last| at.saturating_sub(last) >= 6 * 3600);
        let last_notified_at = if should_notify {
            self.connection
                .execute(
                    "UPDATE incidents SET last_notified_at = ?2 WHERE id = ?1",
                    params![id, at],
                )
                .map_err(|error| error.to_string())?;
            Some(at)
        } else {
            previous_notice
        };
        Ok(IncidentUpdate {
            incident: IncidentRow {
                id,
                job_id: job_id.to_owned(),
                fingerprint: fingerprint.to_owned(),
                failure_streak,
                first_at,
                last_at: at,
                last_notified_at,
                acknowledged_at: None,
                resolved_at: None,
            },
            should_notify,
        })
    }

    pub fn resolve_incidents(&self, job_id: &str, at: i64) -> Result<usize, String> {
        self.connection.execute(
            "UPDATE incidents SET resolved_at = ?2 WHERE job_id = ?1 AND resolved_at IS NULL AND acknowledged_at IS NULL",
            params![job_id, at],
        ).map_err(|error| error.to_string())
    }

    pub fn acknowledge_incident(&self, id: &str, at: i64) -> Result<bool, String> {
        self.connection.execute(
            "UPDATE incidents SET acknowledged_at = ?2 WHERE id = ?1 AND acknowledged_at IS NULL",
            params![id, at],
        ).map(|changed| changed == 1).map_err(|error| error.to_string())
    }

    pub fn list_incidents(&self, job_id: &str, limit: usize) -> Result<Vec<IncidentRow>, String> {
        let mut query = self.connection.prepare(
            "SELECT id, job_id, fingerprint, failure_streak, first_at, last_at, last_notified_at, acknowledged_at, resolved_at FROM incidents WHERE job_id = ?1 ORDER BY last_at DESC LIMIT ?2"
        ).map_err(|error| error.to_string())?;
        let rows = query
            .query_map(params![job_id, limit.min(1000) as i64], |row| {
                Ok(IncidentRow {
                    id: row.get(0)?,
                    job_id: row.get(1)?,
                    fingerprint: row.get(2)?,
                    failure_streak: row.get(3)?,
                    first_at: row.get(4)?,
                    last_at: row.get(5)?,
                    last_notified_at: row.get(6)?,
                    acknowledged_at: row.get(7)?,
                    resolved_at: row.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn list_runs(&self, job_id: &str, limit: usize) -> Result<Vec<RunRow>, String> {
        let mut query = self.connection.prepare(
            "SELECT id, job_id, occurrence_at, trigger, status, started_at, finished_at, output_path, error_class FROM runs WHERE job_id = ?1 ORDER BY occurrence_at DESC, rowid DESC LIMIT ?2"
        ).map_err(|error| error.to_string())?;
        let rows = query
            .query_map(params![job_id, limit.min(1000) as i64], |row| {
                Ok(RunRow {
                    id: row.get(0)?,
                    job_id: row.get(1)?,
                    occurrence_at: row.get(2)?,
                    trigger: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    output_path: row.get::<_, Option<String>>(7)?.map(PathBuf::from),
                    error_class: row.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<RunRow>, String> {
        self.connection.query_row(
            "SELECT id, job_id, occurrence_at, trigger, status, started_at, finished_at, output_path, error_class FROM runs WHERE id = ?1",
            params![run_id],
            |row| Ok(RunRow {
                id: row.get(0)?,
                job_id: row.get(1)?,
                occurrence_at: row.get(2)?,
                trigger: row.get(3)?,
                status: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                output_path: row.get::<_, Option<String>>(7)?.map(PathBuf::from),
                error_class: row.get(8)?,
            }),
        ).optional().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JobTarget;
    use time::{OffsetDateTime, UtcOffset};

    fn temp_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("mercury-cron-storage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn jobs_file_round_trips_and_rejects_duplicate_ids() {
        let dir = temp_dir();
        let path = dir.join("jobs.toml");
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let job = Job::new(
            "backup".into(),
            "every 2h",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "backup".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        let file = JobsFile {
            version: 1,
            jobs: vec![job.clone()],
        };
        file.save_atomic(&path).unwrap();
        assert_eq!(JobsFile::load(&path).unwrap().jobs, vec![job.clone()]);
        let duplicate = JobsFile {
            version: 1,
            jobs: vec![job.clone(), job],
        };
        assert!(duplicate.save_atomic(&path).is_err());
        assert_eq!(JobsFile::load(&path).unwrap().jobs.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_email_destination_is_ignored_without_losing_other_jobs() {
        let dir = temp_dir();
        let path = dir.join("jobs.toml");
        let now = OffsetDateTime::from_unix_timestamp(1_791_273_600).unwrap();
        let job = Job::new(
            "existing".into(),
            "every 2h",
            UtcOffset::UTC,
            now,
            JobTarget::Script {
                program: "echo".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        JobsFile {
            version: 1,
            jobs: vec![job.clone()],
        }
        .save_atomic(&path)
        .unwrap();
        let mut content = fs::read_to_string(&path).unwrap();
        content.push_str("\n[[jobs.deliver]]\nkind = 'email'\nsmtp_url_env = 'OLD_SMTP'\nusername_env = 'OLD_USER'\npassword_env = 'OLD_PASS'\nfrom = 'x@example.invalid'\nto = ['y@example.invalid']\n");
        fs::write(&path, content).unwrap();

        let loaded = JobsFile::load(&path).unwrap();
        assert_eq!(loaded.jobs, vec![job]);
        loaded.save_atomic(&path).unwrap();
        assert!(!fs::read_to_string(&path)
            .unwrap()
            .contains("kind = \"email\""));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sqlite_reservation_is_unique_across_reopen_and_run_is_durable() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let occurrence = 1_791_273_600;
        let id = {
            let ledger = SqliteLedger::open(&path).unwrap();
            let id = ledger.reserve("job-1", occurrence).unwrap().unwrap();
            assert_eq!(ledger.reserve("job-1", occurrence).unwrap(), None);
            ledger.mark_running(&id, occurrence).unwrap();
            id
        };
        {
            let mut ledger = SqliteLedger::open(&path).unwrap();
            assert_eq!(ledger.reserve("job-1", occurrence).unwrap(), None);
            assert_eq!(
                ledger.find("job-1", occurrence).unwrap().unwrap().status,
                "running"
            );
            assert_eq!(ledger.get_run(&id).unwrap().unwrap().id, id);
            ledger
                .finish(&id, occurrence + 5, true, None, None, None)
                .unwrap();
            assert_eq!(
                ledger.find("job-1", occurrence).unwrap().unwrap().status,
                "success"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn completion_and_learn_usage_outbox_commit_together_across_restart() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let usage = ObservedUsage {
            input_tokens: 120,
            output_tokens: 30,
            cost_microusd: Some(125),
        };
        let run_id = {
            let mut ledger = SqliteLedger::open(&path).unwrap();
            let run_id = ledger.reserve_manual("job-1", 100).unwrap();
            assert!(ledger
                .finish(
                    &run_id,
                    101,
                    true,
                    None,
                    None,
                    Some(("claude", usage, Some("project:0123456789abcdef")))
                )
                .is_ok());
            run_id
        };
        let ledger = SqliteLedger::open(&path).unwrap();
        let pending = ledger.pending_learn_usage(16).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].run_id, run_id);
        assert_eq!(pending[0].job_id, "job-1");
        assert_eq!(
            pending[0].learn_target_id.as_deref(),
            Some("project:0123456789abcdef")
        );
        assert_eq!(pending[0].usage, usage);
        assert!(pending[0].succeeded);
        assert_eq!(pending[0].observed_at_ms, 101_000);
        assert!(ledger.ack_learn_usage(&run_id).unwrap());
        assert!(!ledger.ack_learn_usage(&run_id).unwrap());
        assert!(ledger.pending_learn_usage(16).unwrap().is_empty());
        drop(ledger);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pending_learn_usage_rejects_completion_timestamp_overflow() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let mut ledger = SqliteLedger::open(&path).unwrap();
        let run_id = ledger.reserve_manual("job-1", 100).unwrap();
        ledger
            .finish(
                &run_id,
                i64::MAX,
                true,
                None,
                None,
                Some((
                    "claude",
                    ObservedUsage {
                        input_tokens: 1,
                        output_tokens: 1,
                        cost_microusd: None,
                    },
                    None,
                )),
            )
            .unwrap();
        assert!(ledger.pending_learn_usage(16).is_err());
        drop(ledger);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_usage_rolls_back_run_completion() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let mut ledger = SqliteLedger::open(&path).unwrap();
        let run_id = ledger.reserve_manual("job-1", 100).unwrap();
        let invalid = ObservedUsage {
            input_tokens: 10_000_001,
            output_tokens: 0,
            cost_microusd: None,
        };
        assert!(ledger
            .finish(
                &run_id,
                101,
                true,
                None,
                None,
                Some(("claude", invalid, None))
            )
            .is_err());
        assert_eq!(ledger.get_run(&run_id).unwrap().unwrap().status, "reserved");
        assert!(ledger.pending_learn_usage(16).unwrap().is_empty());
        drop(ledger);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_three_ledger_gains_outbox_without_losing_runs() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let ledger = SqliteLedger::open(&path).unwrap();
        let run_id = ledger.reserve_manual("job-1", 100).unwrap();
        drop(ledger);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("DROP TABLE learn_usage_outbox; PRAGMA user_version=3;")
            .unwrap();
        drop(connection);
        let mut reopened = SqliteLedger::open(&path).unwrap();
        assert_eq!(
            reopened.get_run(&run_id).unwrap().unwrap().status,
            "reserved"
        );
        assert!(reopened.pending_learn_usage(16).unwrap().is_empty());
        reopened
            .finish(
                &run_id,
                101,
                true,
                None,
                None,
                Some((
                    "codex",
                    ObservedUsage {
                        input_tokens: 1,
                        output_tokens: 2,
                        cost_microusd: None,
                    },
                    None,
                )),
            )
            .unwrap();
        assert_eq!(reopened.pending_learn_usage(16).unwrap().len(), 1);
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_four_outbox_gains_nullable_target_without_reassigning_history() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let mut ledger = SqliteLedger::open(&path).unwrap();
        let run_id = ledger.reserve_manual("job-1", 100).unwrap();
        ledger.finish(&run_id, 101, true, None, None, None).unwrap();
        drop(ledger);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE learn_usage_outbox;
                 CREATE TABLE learn_usage_outbox (
                   run_id TEXT PRIMARY KEY, job_id TEXT NOT NULL, provider TEXT NOT NULL,
                   input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
                   cost_microusd INTEGER, succeeded INTEGER NOT NULL
                 );
                 PRAGMA user_version=4;",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learn_usage_outbox
                 (run_id,job_id,provider,input_tokens,output_tokens,cost_microusd,succeeded)
                 VALUES (?1,'job-1','codex',1,2,NULL,1)",
                [&run_id],
            )
            .unwrap();
        drop(connection);
        let reopened = SqliteLedger::open(&path).unwrap();
        let pending = reopened.pending_learn_usage(16).unwrap();
        assert_eq!(pending[0].run_id, run_id);
        assert_eq!(pending[0].learn_target_id, None);
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn two_connections_cannot_reserve_the_same_occurrence() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let first = SqliteLedger::open(&path).unwrap();
        let second = SqliteLedger::open(&path).unwrap();
        assert!(first.reserve("job-1", 1_791_273_600).unwrap().is_some());
        assert_eq!(second.reserve("job-1", 1_791_273_600).unwrap(), None);
        drop(first);
        drop(second);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_ledger_schema_is_not_modified() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", DB_VERSION + 1)
            .unwrap();
        drop(connection);
        assert!(SqliteLedger::open(&path).is_err());
        let connection = Connection::open(&path).unwrap();
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        drop(connection);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn incident_groups_failures_and_rate_limits_reminders() {
        let dir = temp_dir();
        let ledger = SqliteLedger::open(&dir.join("executions.db")).unwrap();
        let first = ledger.record_failure("job-1", "timeout", 100).unwrap();
        assert!(first.should_notify);
        let second = ledger.record_failure("job-1", "timeout", 101).unwrap();
        assert!(!second.should_notify);
        assert_eq!(second.incident.failure_streak, 2);
        assert_eq!(first.incident.id, second.incident.id);
        let reminder = ledger
            .record_failure("job-1", "timeout", 100 + 6 * 3600)
            .unwrap();
        assert!(reminder.should_notify);
        ledger
            .resolve_incidents("job-1", 100 + 6 * 3600 + 1)
            .unwrap();
        let reopened = ledger
            .record_failure("job-1", "timeout", 100 + 6 * 3600 + 2)
            .unwrap();
        assert!(reopened.should_notify);
        assert_ne!(reopened.incident.id, first.incident.id);
        assert!(ledger
            .acknowledge_incident(&reopened.incident.id, 100 + 6 * 3600 + 3)
            .unwrap());
        assert!(!ledger
            .acknowledge_incident(&reopened.incident.id, 100 + 6 * 3600 + 4)
            .unwrap());
        drop(ledger);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn v1_ledger_migrates_without_losing_reserved_runs() {
        let dir = temp_dir();
        let path = dir.join("executions.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE runs(id TEXT PRIMARY KEY, job_id TEXT NOT NULL, occurrence_at INTEGER NOT NULL,
                status TEXT NOT NULL, started_at INTEGER, finished_at INTEGER, output_path TEXT,
                error_class TEXT, UNIQUE(job_id, occurrence_at));
             CREATE TABLE incidents(id TEXT PRIMARY KEY, job_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
                failure_streak INTEGER NOT NULL, first_at INTEGER NOT NULL, last_at INTEGER NOT NULL,
                last_notified_at INTEGER, acknowledged_at INTEGER);
             INSERT INTO runs(id, job_id, occurrence_at, status) VALUES ('old-run', 'job-1', 100, 'reserved');
             PRAGMA user_version = 1;"
        ).unwrap();
        drop(connection);
        let ledger = SqliteLedger::open(&path).unwrap();
        assert_eq!(ledger.find("job-1", 100).unwrap().unwrap().id, "old-run");
        assert!(ledger.reserve("job-1", 100).unwrap().is_none());
        assert!(ledger.reserve_manual("job-1", 100).is_ok());
        drop(ledger);
        fs::remove_dir_all(dir).unwrap();
    }
}
