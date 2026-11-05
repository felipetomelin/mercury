use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mercury_learn::storage::{
    transcript_fingerprint, ChangeStatus, PrepareChange, Storage, StorageError, TriggerStatus,
};
use mercury_learn::transcript::Provider;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-storage-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("AGENTS.md");
        Self { root, target }
    }

    fn data(&self) -> PathBuf {
        self.root.join("learn")
    }

    fn change<'a>(&'a self, before: &'a [u8], after: &'a [u8]) -> PrepareChange<'a> {
        PrepareChange {
            target_id: "instructions",
            target_path: &self.target,
            source: "codex/session-1",
            reason: "record a durable lesson",
            before_bytes: before,
            after_bytes: after,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn backup_and_prepared_ledger_survive_restart() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage
        .prepare(fixture.change(b"original bytes", b"new bytes"))
        .unwrap();
    assert_eq!(prepared.status, ChangeStatus::Prepared);
    assert_eq!(storage.read_backup(&prepared).unwrap(), b"original bytes");
    assert_eq!(
        storage.read_after_snapshot(&prepared).unwrap(),
        b"new bytes"
    );
    drop(storage);

    let storage = Storage::open(&fixture.data()).unwrap();
    let restored = storage.get(&prepared.id).unwrap().unwrap();
    assert_eq!(restored.target_id, "instructions");
    assert_eq!(restored.status, ChangeStatus::Prepared);
    assert_eq!(storage.read_backup(&restored).unwrap(), b"original bytes");
    assert_eq!(
        storage.read_after_snapshot(&restored).unwrap(),
        b"new bytes"
    );
}

#[test]
fn list_recent_and_date_range_are_bounded_and_complete() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let first = storage.prepare(fixture.change(b"one", b"two")).unwrap();
    let second = storage.prepare(fixture.change(b"two", b"three")).unwrap();

    let recent = storage.list_recent(1).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, second.id);
    assert_eq!(storage.list_recent(2).unwrap().len(), 2);
    assert!(matches!(
        storage.list_recent(0),
        Err(StorageError::InvalidInput)
    ));
    assert!(matches!(
        storage.list_recent(101),
        Err(StorageError::InvalidInput)
    ));

    let start = first.created_at_ms.min(second.created_at_ms);
    let end = first.created_at_ms.max(second.created_at_ms) + 1;
    let all = storage.list_created_between(start, end, 2).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, second.id);
    assert!(all.iter().any(|record| record.id == first.id));
    assert!(all.iter().any(|record| record.id == second.id));
    assert!(matches!(
        storage.list_created_between(start, end, 1),
        Err(StorageError::TooLarge)
    ));
    assert!(matches!(
        storage.list_created_between(end, start, 2),
        Err(StorageError::InvalidInput)
    ));
    assert!(storage
        .list_created_between(end, end + 1, 2)
        .unwrap()
        .is_empty());
}

#[test]
fn status_counts_entire_ledger_including_triggers() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    assert_eq!(storage.status().unwrap().changes.applied, 0);
    let applied = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    let failed = storage.prepare(fixture.change(b"old2", b"new2")).unwrap();
    storage
        .transition(&applied.id, ChangeStatus::Prepared, ChangeStatus::Applied)
        .unwrap();
    storage
        .transition(&failed.id, ChangeStatus::Prepared, ChangeStatus::Failed)
        .unwrap();
    storage.claim_trigger("codex:session-1:end").unwrap();
    storage.claim_trigger("codex:session-2:end").unwrap();
    storage
        .finish_trigger("codex:session-1:end", TriggerStatus::Succeeded)
        .unwrap();

    let status = storage.status().unwrap();
    assert_eq!(status.changes.applied, 1);
    assert_eq!(status.changes.failed, 1);
    assert_eq!(status.changes.prepared, 0);
    assert_eq!(status.triggers.claimed, 1);
    assert_eq!(status.triggers.succeeded, 1);
    assert_eq!(status.triggers.failed, 0);
}

#[test]
fn completed_session_metadata_is_bounded_and_survives_restart() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let transcript = fixture.root.join("session.jsonl");
    fs::write(&transcript, b"fixture").unwrap();
    let fingerprint = transcript_fingerprint(&transcript).unwrap();
    assert!(matches!(
        storage.record_completed_session(Provider::Claude, "../bad", &transcript, &fingerprint),
        Err(StorageError::InvalidInput)
    ));
    assert!(matches!(
        storage.record_completed_session(
            Provider::Claude,
            "session-1",
            PathBuf::from("relative").as_path(),
            &fingerprint
        ),
        Err(StorageError::InvalidInput)
    ));
    for index in 0..65 {
        storage
            .record_completed_session(
                Provider::Codex,
                &format!("session-{index}"),
                &transcript,
                &fingerprint,
            )
            .unwrap();
    }
    let recent = storage.recent_completed_sessions(64).unwrap();
    assert_eq!(recent.len(), 64);
    assert_eq!(recent[0].session_id, "session-64");
    assert!(!recent.iter().any(|entry| entry.session_id == "session-0"));
    assert_eq!(recent[0].provider, Provider::Codex);
    assert_eq!(recent[0].transcript_path, transcript);
    assert!(matches!(
        storage.recent_completed_sessions(65),
        Err(StorageError::InvalidInput)
    ));
    storage
        .record_completed_session(Provider::Codex, "session-1", &transcript, &fingerprint)
        .unwrap();
    assert_eq!(
        storage.recent_completed_sessions(1).unwrap()[0].session_id,
        "session-1"
    );
    drop(storage);
    let mut reopened = Storage::open(&fixture.data()).unwrap();
    assert_eq!(reopened.recent_completed_sessions(64).unwrap().len(), 64);
    assert_eq!(
        reopened.recent_completed_sessions(1).unwrap()[0]
            .fingerprint
            .as_deref(),
        Some(fingerprint.as_str())
    );
    fs::write(&transcript, b"changed fixture").unwrap();
    assert!(matches!(
        reopened.record_completed_session(
            Provider::Codex,
            "session-new",
            &transcript,
            &fingerprint
        ),
        Err(StorageError::InvalidTransition)
    ));
}

#[test]
fn legacy_completed_session_rows_migrate_without_becoming_weekly_candidates() {
    let fixture = Fixture::new();
    let storage = Storage::open(&fixture.data()).unwrap();
    drop(storage);
    let database = fixture.data().join("ledger.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "DROP TABLE completed_sessions;
             CREATE TABLE completed_sessions (
               provider TEXT NOT NULL,
               session_id TEXT NOT NULL,
               transcript_path TEXT NOT NULL,
               completed_at_ms INTEGER NOT NULL,
               PRIMARY KEY(provider,session_id)
             );
             INSERT INTO completed_sessions VALUES
               ('claude','session-1','/tmp/old.jsonl',1);",
        )
        .unwrap();
    drop(connection);
    let reopened = Storage::open(&fixture.data()).unwrap();
    let rows = reopened.recent_completed_sessions(1).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fingerprint, None);
}

#[test]
fn transition_requires_expected_previous_state() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage.prepare(fixture.change(b"old", b"new")).unwrap();

    storage
        .transition(&prepared.id, ChangeStatus::Prepared, ChangeStatus::Applied)
        .unwrap();
    assert!(matches!(
        storage.transition(&prepared.id, ChangeStatus::Prepared, ChangeStatus::Failed),
        Err(StorageError::Missing)
    ));
    storage
        .transition(
            &prepared.id,
            ChangeStatus::Applied,
            ChangeStatus::UndoPrepared,
        )
        .unwrap();
    storage
        .transition(
            &prepared.id,
            ChangeStatus::UndoPrepared,
            ChangeStatus::Undone,
        )
        .unwrap();
    assert_eq!(
        storage.get(&prepared.id).unwrap().unwrap().status,
        ChangeStatus::Undone
    );
}

#[test]
fn review_trigger_claim_survives_restart_and_never_dispatches_twice() {
    let fixture = Fixture::new();
    let mut first = Storage::open(&fixture.data()).unwrap();
    assert!(first.claim_trigger("codex:session-1:end").unwrap());
    assert!(!first.claim_trigger("codex:session-1:end").unwrap());
    drop(first);

    let mut second = Storage::open(&fixture.data()).unwrap();
    assert_eq!(
        second.trigger_status("codex:session-1:end").unwrap(),
        Some(TriggerStatus::Claimed)
    );
    assert!(!second.claim_trigger("codex:session-1:end").unwrap());
    second
        .finish_trigger("codex:session-1:end", TriggerStatus::Succeeded)
        .unwrap();
    assert_eq!(
        second.trigger_status("codex:session-1:end").unwrap(),
        Some(TriggerStatus::Succeeded)
    );
    assert!(!second.claim_trigger("codex:session-1:end").unwrap());
}

#[test]
fn review_trigger_rejects_unsafe_keys_and_invalid_transitions() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    assert!(matches!(
        storage.claim_trigger(""),
        Err(StorageError::InvalidInput)
    ));
    assert!(matches!(
        storage.claim_trigger("a/b"),
        Err(StorageError::InvalidInput)
    ));
    assert!(matches!(
        storage.finish_trigger("codex:session-1:end", TriggerStatus::Succeeded),
        Err(StorageError::InvalidTransition)
    ));
    storage.claim_trigger("codex:session-1:end").unwrap();
    storage
        .finish_trigger("codex:session-1:end", TriggerStatus::Failed)
        .unwrap();
    assert!(matches!(
        storage.finish_trigger("codex:session-1:end", TriggerStatus::Succeeded),
        Err(StorageError::InvalidTransition)
    ));
}

#[test]
fn recent_trigger_audit_is_bounded_and_survives_restart() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    for index in 0..17 {
        storage
            .claim_trigger(&format!("claude:session-{index}:end"))
            .unwrap();
    }
    storage
        .finish_trigger("claude:session-16:end", TriggerStatus::Succeeded)
        .unwrap();
    drop(storage);

    let storage = Storage::open(&fixture.data()).unwrap();
    let triggers = storage.recent_triggers(16).unwrap();
    assert_eq!(triggers.len(), 16);
    assert_eq!(triggers[0].key, "claude:session-16:end");
    assert_eq!(triggers[0].status, TriggerStatus::Succeeded);
    assert!(triggers[0].finished_at_ms.is_some());
    assert!(!triggers
        .iter()
        .any(|item| item.key == "claude:session-0:end"));
    assert!(matches!(
        storage.recent_triggers(17),
        Err(StorageError::InvalidInput)
    ));
}

#[test]
fn modified_backup_fails_integrity_check() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    fs::write(
        fixture.data().join("backups").join(&prepared.backup_name),
        b"bad",
    )
    .unwrap();

    assert!(matches!(
        storage.read_backup(&prepared),
        Err(StorageError::Corrupt)
    ));
}

#[test]
fn modified_after_snapshot_fails_integrity_check_and_missing_is_explicit() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    let after_path = fixture
        .data()
        .join("snapshots")
        .join(format!("{}.after", prepared.id));
    fs::write(&after_path, b"tampered").unwrap();
    assert!(matches!(
        storage.read_after_snapshot(&prepared),
        Err(StorageError::Corrupt)
    ));
    fs::remove_file(after_path).unwrap();
    assert!(matches!(
        storage.read_after_snapshot(&prepared),
        Err(StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
    ));
}

#[test]
fn ledger_insert_failure_cleans_backup_and_preserves_target() {
    let fixture = Fixture::new();
    fs::write(&fixture.target, b"old").unwrap();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let connection = rusqlite::Connection::open(fixture.data().join("ledger.sqlite")).unwrap();
    connection.execute("DROP TABLE changes", []).unwrap();
    drop(connection);

    assert!(matches!(
        storage.prepare(fixture.change(b"old", b"new")),
        Err(StorageError::Sql(_))
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"old");
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_dir(fixture.data().join("snapshots"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn oversized_metadata_is_rejected_before_backup_creation() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut change = fixture.change(b"old", b"new");
    let reason = "x".repeat(1025);
    change.reason = &reason;

    assert!(matches!(
        storage.prepare(change),
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn symlinked_backup_is_never_read() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    let backup = fixture.data().join("backups").join(&prepared.backup_name);
    fs::remove_file(&backup).unwrap();
    symlink(&fixture.target, &backup).unwrap();

    assert!(matches!(
        storage.read_backup(&prepared),
        Err(StorageError::UnsafePath)
    ));
}

#[cfg(unix)]
#[test]
fn storage_rejects_symlinked_root_and_private_paths() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let fixture = Fixture::new();
    let root = fixture.data();
    fs::create_dir(&root).unwrap();
    let linked = fixture.root.join("linked");
    symlink(&root, &linked).unwrap();
    assert!(matches!(
        Storage::open(&linked),
        Err(StorageError::UnsafePath)
    ));
    assert!(matches!(
        Storage::open(&linked.join("nested")),
        Err(StorageError::UnsafePath)
    ));
    assert!(!root.join("nested").exists());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Storage::open(&root),
        Err(StorageError::UnsafePath)
    ));
}

#[cfg(unix)]
#[test]
fn backup_and_database_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let prepared = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    for directory in [
        fixture.data(),
        fixture.data().join("backups"),
        fixture.data().join("snapshots"),
    ] {
        let mode = fs::metadata(directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
    }
    for file in [
        fixture.data().join("ledger.sqlite"),
        fixture.data().join("backups").join(prepared.backup_name),
        fixture
            .data()
            .join("snapshots")
            .join(format!("{}.after", prepared.id)),
    ] {
        let mode = fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
    }
}

#[cfg(unix)]
#[test]
fn existing_version_one_ledger_migrates_before_undo_intent() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fs::create_dir(fixture.data()).unwrap();
    fs::set_permissions(fixture.data(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = fixture.data().join("ledger.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE changes (
                id TEXT PRIMARY KEY, target_id TEXT NOT NULL, target_path TEXT NOT NULL,
                source TEXT NOT NULL, reason TEXT NOT NULL, before_hash TEXT NOT NULL,
                after_hash TEXT NOT NULL, backup_name TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL CHECK(status IN ('prepared','applied','conflict','failed','undone')),
                created_at_ms INTEGER NOT NULL
             );
             CREATE INDEX changes_created_at ON changes(created_at_ms);
             PRAGMA user_version=1;",
        )
        .unwrap();
    drop(connection);
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();

    let mut storage = Storage::open(&fixture.data()).unwrap();
    let record = storage.prepare(fixture.change(b"old", b"new")).unwrap();
    storage
        .transition(&record.id, ChangeStatus::Prepared, ChangeStatus::Applied)
        .unwrap();
    storage
        .transition(
            &record.id,
            ChangeStatus::Applied,
            ChangeStatus::UndoPrepared,
        )
        .unwrap();
    assert_eq!(
        storage.get(&record.id).unwrap().unwrap().status,
        ChangeStatus::UndoPrepared
    );
}
