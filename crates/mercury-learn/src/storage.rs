//! Private durable ledger and exact-byte pre-write backups. No target writes
//! occur here; the apply/undo layer owns filesystem replacement and recovery.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::transcript::Provider;

const MAX_BACKUP_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Prepared,
    Applied,
    UndoPrepared,
    Conflict,
    Failed,
    Undone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerStatus {
    Claimed,
    Succeeded,
    Failed,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChangeCounts {
    pub prepared: u64,
    pub applied: u64,
    pub undo_prepared: u64,
    pub conflict: u64,
    pub failed: u64,
    pub undone: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TriggerCounts {
    pub claimed: u64,
    pub succeeded: u64,
    pub failed: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LedgerStatus {
    pub changes: ChangeCounts,
    pub triggers: TriggerCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedSession {
    pub provider: Provider,
    pub session_id: String,
    pub transcript_path: PathBuf,
    pub completed_at_ms: i64,
    /// Missing on pre-fingerprint rows; those rows cannot be scheduled.
    pub fingerprint: Option<String>,
}

/// Opaque file identity for the completed-session index. A symlink, directory,
/// replacement, append, or metadata change makes the prior entry ineligible.
pub fn transcript_fingerprint(path: &Path) -> Result<String, StorageError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(StorageError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StorageError::Corrupt)?;
        Ok(format!("{}:{}", metadata.len(), modified.as_nanos()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRecord {
    pub key: String,
    pub status: TriggerStatus,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

impl TriggerStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "claimed" => Ok(Self::Claimed),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(StorageError::Corrupt),
        }
    }
}

impl ChangeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Applied => "applied",
            Self::UndoPrepared => "undo_prepared",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
            Self::Undone => "undone",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "applied" => Ok(Self::Applied),
            "undo_prepared" => Ok(Self::UndoPrepared),
            "conflict" => Ok(Self::Conflict),
            "failed" => Ok(Self::Failed),
            "undone" => Ok(Self::Undone),
            _ => Err(StorageError::Corrupt),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangeRecord {
    pub id: String,
    pub target_id: String,
    pub target_path: PathBuf,
    pub source: String,
    pub reason: String,
    pub before_hash: String,
    pub after_hash: String,
    pub backup_name: String,
    pub status: ChangeStatus,
    pub created_at_ms: i64,
}

struct RawChangeRecord {
    id: String,
    target_id: String,
    target_path: String,
    source: String,
    reason: String,
    before_hash: String,
    after_hash: String,
    backup_name: String,
    status: String,
    created_at_ms: i64,
}

pub struct PrepareChange<'a> {
    pub target_id: &'a str,
    pub target_path: &'a Path,
    pub source: &'a str,
    pub reason: &'a str,
    pub before_bytes: &'a [u8],
    pub after_bytes: &'a [u8],
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    UnsafePath,
    InvalidInput,
    TooLarge,
    Corrupt,
    Missing,
    InvalidTransition,
}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

pub struct Storage {
    root: PathBuf,
    pub(crate) connection: Connection,
}

impl Storage {
    pub fn open(root: &Path) -> Result<Self, StorageError> {
        if !root.is_absolute() {
            return Err(StorageError::UnsafePath);
        }
        create_private_dir(root)?;
        create_private_dir(&root.join("backups"))?;
        create_private_dir(&root.join("snapshots"))?;
        let database = root.join("ledger.sqlite");
        ensure_private_database(&database)?;
        let connection = Connection::open(&database)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=4).contains(&version) {
            return Err(StorageError::Corrupt);
        }
        if version == 1 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE changes RENAME TO changes_v1;
                 CREATE TABLE changes (
                    id TEXT PRIMARY KEY,
                    target_id TEXT NOT NULL,
                    target_path TEXT NOT NULL,
                    source TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    before_hash TEXT NOT NULL,
                    after_hash TEXT NOT NULL,
                    backup_name TEXT NOT NULL UNIQUE,
                    status TEXT NOT NULL CHECK(status IN ('prepared','applied','undo_prepared','conflict','failed','undone')),
                    created_at_ms INTEGER NOT NULL
                 );
                 INSERT INTO changes SELECT * FROM changes_v1 ORDER BY rowid;
                 DROP TABLE changes_v1;
                 CREATE INDEX changes_created_at ON changes(created_at_ms);
                 CREATE INDEX changes_status ON changes(status);
                 PRAGMA user_version=2;
                 COMMIT;",
            )?;
        } else {
            connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS changes (
                id TEXT PRIMARY KEY,
                target_id TEXT NOT NULL,
                target_path TEXT NOT NULL,
                source TEXT NOT NULL,
                reason TEXT NOT NULL,
                before_hash TEXT NOT NULL,
                after_hash TEXT NOT NULL,
                backup_name TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL CHECK(status IN ('prepared','applied','undo_prepared','conflict','failed','undone')),
                created_at_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS changes_created_at ON changes(created_at_ms);
            CREATE INDEX IF NOT EXISTS changes_status ON changes(status);",
            )?;
            if version == 0 {
                connection.pragma_update(None, "user_version", 2)?;
            }
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS review_triggers (
                key TEXT PRIMARY KEY,
                status TEXT NOT NULL CHECK(status IN ('claimed','succeeded','failed')),
                created_at_ms INTEGER NOT NULL,
                finished_at_ms INTEGER
            );
            CREATE INDEX IF NOT EXISTS review_triggers_created_at
                ON review_triggers(created_at_ms);",
        )?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS completed_sessions (
                provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
                session_id TEXT NOT NULL,
                transcript_path TEXT NOT NULL,
                completed_at_ms INTEGER NOT NULL,
                fingerprint TEXT,
                PRIMARY KEY(provider,session_id)
            );
            CREATE INDEX IF NOT EXISTS completed_sessions_recent
                ON completed_sessions(completed_at_ms DESC);",
        )?;
        let has_fingerprint = connection
            .prepare("PRAGMA table_info(completed_sessions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|column| column.is_ok_and(|name| name == "fingerprint"));
        if !has_fingerprint {
            connection
                .execute_batch("ALTER TABLE completed_sessions ADD COLUMN fingerprint TEXT")?;
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_metrics (
                id INTEGER PRIMARY KEY,
                observation_id TEXT NOT NULL,
                task_key TEXT NOT NULL,
                provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
                phase TEXT NOT NULL CHECK(phase IN ('observed','baseline','after')),
                input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0),
                output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
                cost_microusd INTEGER CHECK(cost_microusd >= 0),
                outcome TEXT CHECK(outcome IN ('succeeded','failed')),
                observed_at_ms INTEGER NOT NULL,
                UNIQUE(provider,observation_id)
            );
            CREATE INDEX IF NOT EXISTS task_metrics_comparison
                ON task_metrics(task_key,provider,phase);",
        )?;
        if version < 3 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE task_metrics RENAME TO task_metrics_v2;
                 CREATE TABLE task_metrics (
                    id INTEGER PRIMARY KEY,
                    observation_id TEXT NOT NULL,
                    task_key TEXT NOT NULL,
                    provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
                    phase TEXT NOT NULL CHECK(phase IN ('observed','baseline','after')),
                    input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0),
                    output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
                    cost_microusd INTEGER CHECK(cost_microusd >= 0),
                    outcome TEXT CHECK(outcome IN ('succeeded','failed')),
                    observed_at_ms INTEGER NOT NULL,
                    UNIQUE(provider,observation_id)
                 );
                 INSERT INTO task_metrics SELECT * FROM task_metrics_v2 ORDER BY id;
                 DROP TABLE task_metrics_v2;
                 CREATE INDEX task_metrics_comparison ON task_metrics(task_key,provider,phase);
                 PRAGMA user_version=3;
                 COMMIT;",
            )?;
        }
        if version < 4 {
            let has_applied_at = connection
                .prepare("PRAGMA table_info(changes)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .any(|column| column.is_ok_and(|name| name == "applied_at_ms"));
            if !has_applied_at {
                connection.execute_batch("ALTER TABLE changes ADD COLUMN applied_at_ms INTEGER")?;
            }
            connection.pragma_update(None, "user_version", 4)?;
        }
        sync_directory(root)?;
        if let Some(parent) = root.parent() {
            sync_directory(parent)?;
        }
        Ok(Self {
            root: root.to_path_buf(),
            connection,
        })
    }

    pub fn prepare(&mut self, change: PrepareChange<'_>) -> Result<ChangeRecord, StorageError> {
        if change.target_id.is_empty()
            || change.target_id.len() > 64
            || !change.target_path.is_absolute()
            || change
                .target_path
                .to_str()
                .is_none_or(|path| path.len() > 4096)
            || change.source.is_empty()
            || change.source.len() > 256
            || change.reason.is_empty()
            || change.reason.len() > 1024
        {
            return Err(StorageError::InvalidInput);
        }
        if change.before_bytes.len() > MAX_BACKUP_BYTES
            || change.after_bytes.len() > MAX_BACKUP_BYTES
        {
            return Err(StorageError::TooLarge);
        }
        let created_at_ms = now_ms()?;
        let id = Uuid::new_v4().to_string();
        let backup_name = format!("{id}.bak");
        let backups = self.root.join("backups");
        let backup = backups.join(&backup_name);
        let snapshots = self.root.join("snapshots");
        let after_snapshot = snapshots.join(format!("{id}.after"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&backup)?;
        let backup_result = (|| -> Result<(), StorageError> {
            file.write_all(change.before_bytes)?;
            file.sync_all()?;
            sync_directory(&backups)?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = backup_result {
            let _ = fs::remove_file(&backup);
            let _ = sync_directory(&backups);
            return Err(error);
        }
        let after_result = (|| -> Result<(), StorageError> {
            let mut after_file = options.open(&after_snapshot)?;
            after_file.write_all(change.after_bytes)?;
            after_file.sync_all()?;
            sync_directory(&snapshots)?;
            Ok(())
        })();
        if let Err(error) = after_result {
            let _ = fs::remove_file(&after_snapshot);
            let _ = fs::remove_file(&backup);
            let _ = sync_directory(&snapshots);
            let _ = sync_directory(&backups);
            return Err(error);
        }

        let record = ChangeRecord {
            id,
            target_id: change.target_id.to_owned(),
            target_path: change.target_path.to_path_buf(),
            source: change.source.to_owned(),
            reason: change.reason.to_owned(),
            before_hash: hash(change.before_bytes),
            after_hash: hash(change.after_bytes),
            backup_name,
            status: ChangeStatus::Prepared,
            created_at_ms,
        };
        if let Err(error) = self.connection.execute(
            "INSERT INTO changes (id,target_id,target_path,source,reason,before_hash,after_hash,backup_name,status,created_at_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                record.id,
                record.target_id,
                record.target_path.to_string_lossy(),
                record.source,
                record.reason,
                record.before_hash,
                record.after_hash,
                record.backup_name,
                record.status.as_str(),
                record.created_at_ms,
            ],
        ) {
            let _ = fs::remove_file(&after_snapshot);
            let _ = fs::remove_file(&backup);
            let _ = sync_directory(&snapshots);
            let _ = sync_directory(&backups);
            return Err(error.into());
        }
        sync_directory(&self.root)?;
        Ok(record)
    }

    /// Durable at-most-once dispatch claim. An interrupted claim remains
    /// claimed after restart; it is not automatically replayed.
    pub fn claim_trigger(&mut self, key: &str) -> Result<bool, StorageError> {
        if !valid_trigger_key(key) {
            return Err(StorageError::InvalidInput);
        }
        let changed = self.connection.execute(
            "INSERT OR IGNORE INTO review_triggers (key,status,created_at_ms) VALUES (?1,'claimed',?2)",
            params![key, now_ms()?],
        )?;
        Ok(changed == 1)
    }

    /// Keep metadata only for bounded, trusted completed-session candidates.
    /// The caller must have validated the transcript; consumers revalidate it.
    pub fn record_completed_session(
        &mut self,
        provider: Provider,
        session_id: &str,
        transcript_path: &Path,
        expected_fingerprint: &str,
    ) -> Result<(), StorageError> {
        if session_id.is_empty()
            || session_id.len() > 128
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !transcript_path.is_absolute()
            || transcript_path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(StorageError::InvalidInput);
        }
        let path = transcript_path.to_str().ok_or(StorageError::InvalidInput)?;
        if path.len() > 4096 || expected_fingerprint.is_empty() || expected_fingerprint.len() > 256
        {
            return Err(StorageError::InvalidInput);
        }
        if transcript_fingerprint(transcript_path)? != expected_fingerprint {
            return Err(StorageError::InvalidTransition);
        }
        let provider = match provider {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT OR REPLACE INTO completed_sessions
                (provider,session_id,transcript_path,completed_at_ms,fingerprint)
             VALUES (?1,?2,?3,?4,?5)",
            params![provider, session_id, path, now_ms()?, expected_fingerprint],
        )?;
        transaction.execute(
            "DELETE FROM completed_sessions WHERE rowid NOT IN (
                SELECT rowid FROM completed_sessions
                ORDER BY completed_at_ms DESC,rowid DESC LIMIT 64
            )",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recent_completed_sessions(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletedSession>, StorageError> {
        if !(1..=64).contains(&limit) {
            return Err(StorageError::InvalidInput);
        }
        let mut query = self.connection.prepare(
            "SELECT provider,session_id,transcript_path,completed_at_ms,fingerprint
             FROM completed_sessions ORDER BY completed_at_ms DESC,rowid DESC LIMIT ?1",
        )?;
        let rows = query.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        let sessions = rows
            .map(|row| {
                let (provider, session_id, path, completed_at_ms, fingerprint) = row?;
                let provider = match provider.as_str() {
                    "claude" => Provider::Claude,
                    "codex" => Provider::Codex,
                    _ => return Err(StorageError::Corrupt),
                };
                Ok(CompletedSession {
                    provider,
                    session_id,
                    transcript_path: PathBuf::from(path),
                    completed_at_ms,
                    fingerprint,
                })
            })
            .collect();
        sessions
    }

    pub fn finish_trigger(&mut self, key: &str, status: TriggerStatus) -> Result<(), StorageError> {
        if !valid_trigger_key(key) || status == TriggerStatus::Claimed {
            return Err(StorageError::InvalidInput);
        }
        let changed = self.connection.execute(
            "UPDATE review_triggers SET status=?1,finished_at_ms=?2 WHERE key=?3 AND status='claimed'",
            params![status.as_str(), now_ms()?, key],
        )?;
        if changed != 1 {
            return Err(StorageError::InvalidTransition);
        }
        Ok(())
    }

    pub fn trigger_status(&self, key: &str) -> Result<Option<TriggerStatus>, StorageError> {
        if !valid_trigger_key(key) {
            return Err(StorageError::InvalidInput);
        }
        self.connection
            .query_row(
                "SELECT status FROM review_triggers WHERE key=?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|status| TriggerStatus::parse(&status))
            .transpose()
    }

    pub fn recent_triggers(&self, limit: usize) -> Result<Vec<TriggerRecord>, StorageError> {
        if !(1..=16).contains(&limit) {
            return Err(StorageError::InvalidInput);
        }
        let mut query = self.connection.prepare(
            "SELECT key,status,created_at_ms,finished_at_ms FROM review_triggers
             ORDER BY created_at_ms DESC,rowid DESC LIMIT ?1",
        )?;
        let rows = query.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (key, status, created_at_ms, finished_at_ms) = row?;
            if !valid_trigger_key(&key) {
                return Err(StorageError::Corrupt);
            }
            Ok(TriggerRecord {
                key,
                status: TriggerStatus::parse(&status)?,
                created_at_ms,
                finished_at_ms,
            })
        })
        .collect()
    }

    pub fn get(&self, id: &str) -> Result<Option<ChangeRecord>, StorageError> {
        if Uuid::parse_str(id).is_err() {
            return Err(StorageError::InvalidInput);
        }
        let row: Option<RawChangeRecord> = self
            .connection
            .query_row(
                "SELECT id,target_id,target_path,source,reason,before_hash,after_hash,backup_name,status,created_at_ms FROM changes WHERE id=?1",
                [id],
                |row| {
                    Ok(RawChangeRecord {
                        id: row.get(0)?,
                        target_id: row.get(1)?,
                        target_path: row.get(2)?,
                        source: row.get(3)?,
                        reason: row.get(4)?,
                        before_hash: row.get(5)?,
                        after_hash: row.get(6)?,
                        backup_name: row.get(7)?,
                        status: row.get(8)?,
                        created_at_ms: row.get(9)?,
                    })
                },
            )
            .optional()?;
        row.map(|row| {
            let status = ChangeStatus::parse(&row.status)?;
            Ok(ChangeRecord {
                id: row.id,
                target_id: row.target_id,
                target_path: PathBuf::from(row.target_path),
                source: row.source,
                reason: row.reason,
                before_hash: row.before_hash,
                after_hash: row.after_hash,
                backup_name: row.backup_name,
                status,
                created_at_ms: row.created_at_ms,
            })
        })
        .transpose()
    }

    /// Aggregate the entire ledger in SQLite; never infer status from a
    /// bounded recent-items page.
    pub fn status(&self) -> Result<LedgerStatus, StorageError> {
        let mut result = LedgerStatus::default();
        let mut changes = self
            .connection
            .prepare("SELECT status,COUNT(*) FROM changes GROUP BY status")?;
        for row in changes.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (status, count) = row?;
            let count = u64::try_from(count).map_err(|_| StorageError::Corrupt)?;
            match ChangeStatus::parse(&status)? {
                ChangeStatus::Prepared => result.changes.prepared = count,
                ChangeStatus::Applied => result.changes.applied = count,
                ChangeStatus::UndoPrepared => result.changes.undo_prepared = count,
                ChangeStatus::Conflict => result.changes.conflict = count,
                ChangeStatus::Failed => result.changes.failed = count,
                ChangeStatus::Undone => result.changes.undone = count,
            }
        }
        let mut triggers = self
            .connection
            .prepare("SELECT status,COUNT(*) FROM review_triggers GROUP BY status")?;
        for row in triggers.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (status, count) = row?;
            let count = u64::try_from(count).map_err(|_| StorageError::Corrupt)?;
            match TriggerStatus::parse(&status)? {
                TriggerStatus::Claimed => result.triggers.claimed = count,
                TriggerStatus::Succeeded => result.triggers.succeeded = count,
                TriggerStatus::Failed => result.triggers.failed = count,
            }
        }
        Ok(result)
    }

    /// Newest changes first. The caller receives no more than 100 records.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ChangeRecord>, StorageError> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidInput);
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM changes ORDER BY rowid DESC LIMIT ?1")?;
        let ids = statement
            .query_map([limit as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.get(&id)?.ok_or(StorageError::Corrupt))
            .collect()
    }

    /// Complete UTC half-open date range for a bounded batch undo. Refuse a
    /// partial result so a date command never silently skips older items.
    pub fn list_created_between(
        &self,
        start_ms: i64,
        end_ms: i64,
        max_items: usize,
    ) -> Result<Vec<ChangeRecord>, StorageError> {
        if start_ms < 0 || end_ms <= start_ms || !(1..=1000).contains(&max_items) {
            return Err(StorageError::InvalidInput);
        }
        let mut statement = self.connection.prepare(
            "SELECT id FROM changes WHERE created_at_ms >= ?1 AND created_at_ms < ?2 \
             ORDER BY rowid DESC LIMIT ?3",
        )?;
        let ids = statement
            .query_map(params![start_ms, end_ms, (max_items + 1) as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() > max_items {
            return Err(StorageError::TooLarge);
        }
        ids.into_iter()
            .map(|id| self.get(&id)?.ok_or(StorageError::Corrupt))
            .collect()
    }

    pub fn read_backup(&self, record: &ChangeRecord) -> Result<Vec<u8>, StorageError> {
        if record.backup_name != format!("{}.bak", record.id)
            || Uuid::parse_str(&record.id).is_err()
        {
            return Err(StorageError::UnsafePath);
        }
        let path = self.root.join("backups").join(&record.backup_name);
        read_private_file(&path, &record.before_hash)
    }

    /// A missing after snapshot is expected for ledger items written by older
    /// Mercury versions. Callers must report that diff as unavailable.
    pub fn read_after_snapshot(&self, record: &ChangeRecord) -> Result<Vec<u8>, StorageError> {
        if Uuid::parse_str(&record.id).is_err() {
            return Err(StorageError::UnsafePath);
        }
        let path = self
            .root
            .join("snapshots")
            .join(format!("{}.after", record.id));
        read_private_file(&path, &record.after_hash)
    }

    pub fn pending(&self) -> Result<Vec<ChangeRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id FROM changes WHERE status IN ('prepared','undo_prepared') ORDER BY created_at_ms,id",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.get(&id)?.ok_or(StorageError::Corrupt))
            .collect()
    }

    pub fn transition(
        &mut self,
        id: &str,
        from: ChangeStatus,
        to: ChangeStatus,
    ) -> Result<(), StorageError> {
        if Uuid::parse_str(id).is_err()
            || !matches!(
                (from, to),
                (ChangeStatus::Prepared, ChangeStatus::Applied)
                    | (ChangeStatus::Prepared, ChangeStatus::Conflict)
                    | (ChangeStatus::Prepared, ChangeStatus::Failed)
                    | (ChangeStatus::Applied, ChangeStatus::UndoPrepared)
                    | (ChangeStatus::Applied, ChangeStatus::Conflict)
                    | (ChangeStatus::UndoPrepared, ChangeStatus::Undone)
                    | (ChangeStatus::UndoPrepared, ChangeStatus::Applied)
                    | (ChangeStatus::UndoPrepared, ChangeStatus::Conflict)
                    | (ChangeStatus::Conflict, ChangeStatus::UndoPrepared)
            )
        {
            return Err(StorageError::InvalidTransition);
        }
        let applied_at_ms = if from == ChangeStatus::Prepared && to == ChangeStatus::Applied {
            Some(now_ms()?)
        } else {
            None
        };
        let changed = self.connection.execute(
            "UPDATE changes SET status=?1,
                applied_at_ms=COALESCE(?4,applied_at_ms)
             WHERE id=?2 AND status=?3",
            params![to.as_str(), id, from.as_str(), applied_at_ms],
        )?;
        if changed != 1 {
            return Err(StorageError::Missing);
        }
        Ok(())
    }
}

fn read_private_file(path: &Path, expected_hash: &str) -> Result<Vec<u8>, StorageError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(StorageError::UnsafePath);
    }
    if metadata.len() > MAX_BACKUP_BYTES as u64 {
        return Err(StorageError::TooLarge);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.file_type().is_file() || opened.len() > MAX_BACKUP_BYTES as u64 {
        return Err(StorageError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(StorageError::UnsafePath);
        }
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BACKUP_BYTES || hash(&bytes) != expected_hash {
        return Err(StorageError::Corrupt);
    }
    Ok(bytes)
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn now_ms() -> Result<i64, StorageError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Corrupt)?
        .as_millis() as i64)
}

fn valid_trigger_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}

/// Create or validate a private directory without following symlinked parents.
pub fn ensure_private_dir(path: &Path) -> Result<(), StorageError> {
    create_private_dir(path)
}

fn create_private_dir(path: &Path) -> Result<(), StorageError> {
    check_directory_chain(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(path)?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(StorageError::UnsafePath);
        }
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)?;
        if !fs::symlink_metadata(path)?.file_type().is_dir() {
            return Err(StorageError::UnsafePath);
        }
    }
    check_directory_chain(path)?;
    Ok(())
}

fn check_directory_chain(path: &Path) -> Result<(), StorageError> {
    if !path.is_absolute() {
        return Err(StorageError::UnsafePath);
    }
    let mut prefix = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            return Err(StorageError::UnsafePath);
        }
        prefix.push(component.as_os_str());
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if !metadata.file_type().is_dir() => return Err(StorageError::UnsafePath),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_private_database(path: &Path) -> Result<(), StorageError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => file.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(StorageError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(StorageError::UnsafePath);
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
