#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mercury_learn::policy::{Policy, PolicyError, Proposal, TargetKind, TargetPolicy};
use mercury_learn::storage::{ChangeStatus, PrepareChange, Storage};
use mercury_learn::{review_snapshots, ChangeError, Changes};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn expected_hash(&self) -> String {
        mercury_learn::storage::hash(b"original bytes")
    }

    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-change-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("AGENTS.md");
        fs::write(&target, b"original bytes").unwrap();
        Self { root, target }
    }

    fn data(&self) -> PathBuf {
        self.root.join("learn")
    }

    fn policy(&self) -> Policy {
        Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: self.target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap()
    }

    fn proposal(&self) -> Proposal {
        Proposal {
            target_id: "instructions".into(),
            content: "new bytes".into(),
            reason: "lesson from test session".into(),
            source: "codex/session-1".into(),
        }
    }

    fn changes(&self) -> Changes {
        Changes::new(Storage::open(&self.data()).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn apply_then_undo_restores_exact_bytes_across_restart() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fs::set_permissions(&fixture.target, fs::Permissions::from_mode(0o640)).unwrap();
    let mut changes = fixture.changes();
    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();
    assert_eq!(applied.status, ChangeStatus::Applied);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"new bytes");
    assert_eq!(
        fs::metadata(&fixture.target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    drop(changes);

    let mut changes = fixture.changes();
    let undone = changes.undo(&applied.id).unwrap();
    assert_eq!(undone.status, ChangeStatus::Undone);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
    assert_eq!(
        fs::metadata(&fixture.target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn disabled_or_unknown_target_never_writes_or_creates_backup() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    assert!(matches!(
        changes.apply(
            &fixture.policy(),
            false,
            fixture.proposal(),
            &fixture.expected_hash()
        ),
        Err(ChangeError::Policy(PolicyError::Disabled))
    ));
    let mut proposal = fixture.proposal();
    proposal.target_id = "../../AGENTS.md".into();
    assert!(matches!(
        changes.apply(&fixture.policy(), true, proposal, &fixture.expected_hash()),
        Err(ChangeError::Policy(PolicyError::UnknownTarget))
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn human_edit_after_apply_is_preserved_on_undo_conflict() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();
    fs::write(&fixture.target, b"human edit").unwrap();

    assert!(matches!(
        changes.undo(&applied.id),
        Err(ChangeError::Conflict)
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"human edit");
    assert_eq!(
        changes.storage().get(&applied.id).unwrap().unwrap().status,
        ChangeStatus::Conflict
    );
}

#[test]
fn conflict_can_be_undone_after_applied_bytes_are_restored() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();
    fs::write(&fixture.target, b"human edit").unwrap();
    assert!(matches!(
        changes.undo(&applied.id),
        Err(ChangeError::Conflict)
    ));
    fs::write(&fixture.target, b"new bytes").unwrap();

    let undone = changes.undo(&applied.id).unwrap();
    assert_eq!(undone.status, ChangeStatus::Undone);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
}

#[test]
fn no_change_does_not_create_backup_or_ledger_item() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let mut proposal = fixture.proposal();
    proposal.content = "original bytes".into();

    assert!(matches!(
        changes.apply(&fixture.policy(), true, proposal, &fixture.expected_hash()),
        Err(ChangeError::NoChange)
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn human_edit_after_review_is_preserved_without_backup_or_ledger_item() {
    let fixture = Fixture::new();
    let snapshots = review_snapshots(&fixture.policy(), true).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].target_id, "instructions");
    assert_eq!(snapshots[0].content, "original bytes");
    let reviewed_hash = &snapshots[0].before_hash;
    fs::write(&fixture.target, b"human edit after review").unwrap();
    let mut changes = fixture.changes();

    assert!(matches!(
        changes.apply(&fixture.policy(), true, fixture.proposal(), reviewed_hash),
        Err(ChangeError::Conflict)
    ));
    assert_eq!(
        fs::read(&fixture.target).unwrap(),
        b"human edit after review"
    );
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
    let count: i64 = rusqlite::Connection::open(fixture.data().join("ledger.sqlite"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM changes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn review_snapshot_rejects_disabled_or_non_utf8_target() {
    let fixture = Fixture::new();
    assert!(matches!(
        review_snapshots(&fixture.policy(), false),
        Err(ChangeError::Policy(PolicyError::Disabled))
    ));
    fs::write(&fixture.target, [0xff, 0xfe]).unwrap();
    assert!(matches!(
        review_snapshots(&fixture.policy(), true),
        Err(ChangeError::InvalidState)
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), [0xff, 0xfe]);
}

#[test]
fn missing_backup_preserves_applied_bytes() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();
    fs::remove_file(fixture.data().join("backups").join(&applied.backup_name)).unwrap();

    assert!(matches!(
        changes.undo(&applied.id),
        Err(ChangeError::Storage(_))
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"new bytes");
    assert_eq!(
        changes.storage().get(&applied.id).unwrap().unwrap().status,
        ChangeStatus::Applied
    );
}

#[test]
fn pending_apply_reconciles_old_or_new_bytes_without_writing() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let first = storage
        .prepare(PrepareChange {
            target_id: "instructions",
            target_path: &fixture.target,
            source: "codex/session-1",
            reason: "test interrupted apply",
            before_bytes: b"original bytes",
            after_bytes: b"new bytes",
        })
        .unwrap();
    drop(storage);
    let mut changes = fixture.changes();
    assert_eq!(changes.reconcile().unwrap(), 1);
    assert_eq!(
        changes.storage().get(&first.id).unwrap().unwrap().status,
        ChangeStatus::Failed
    );
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");

    let second = Storage::open(&fixture.data())
        .unwrap()
        .prepare(PrepareChange {
            target_id: "instructions",
            target_path: &fixture.target,
            source: "codex/session-1",
            reason: "test interrupted apply after rename",
            before_bytes: b"original bytes",
            after_bytes: b"new bytes",
        })
        .unwrap();
    fs::write(&fixture.target, b"new bytes").unwrap();
    assert_eq!(changes.reconcile().unwrap(), 1);
    assert_eq!(
        changes.storage().get(&second.id).unwrap().unwrap().status,
        ChangeStatus::Applied
    );
}

#[test]
fn next_apply_reconciles_prior_pending_intent_before_new_backup() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let interrupted = storage
        .prepare(PrepareChange {
            target_id: "instructions",
            target_path: &fixture.target,
            source: "codex/session-1",
            reason: "interrupted before rename",
            before_bytes: b"original bytes",
            after_bytes: b"new bytes",
        })
        .unwrap();
    drop(storage);
    let mut changes = fixture.changes();

    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();

    assert_eq!(applied.status, ChangeStatus::Applied);
    assert_eq!(
        changes
            .storage()
            .get(&interrupted.id)
            .unwrap()
            .unwrap()
            .status,
        ChangeStatus::Failed
    );
    assert_eq!(fs::read(&fixture.target).unwrap(), b"new bytes");
}

#[test]
fn pending_undo_reconciles_restored_bytes() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let applied = changes
        .apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash(),
        )
        .unwrap();
    drop(changes);
    let mut storage = Storage::open(&fixture.data()).unwrap();
    storage
        .transition(
            &applied.id,
            ChangeStatus::Applied,
            ChangeStatus::UndoPrepared,
        )
        .unwrap();
    drop(storage);
    fs::write(&fixture.target, b"original bytes").unwrap();

    let mut changes = fixture.changes();
    assert_eq!(changes.reconcile().unwrap(), 1);
    assert_eq!(
        changes.storage().get(&applied.id).unwrap().unwrap().status,
        ChangeStatus::Undone
    );
}

#[test]
fn symlinked_target_is_rejected_before_backup() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let original = fixture.root.join("original.md");
    fs::rename(&fixture.target, &original).unwrap();
    symlink(&original, &fixture.target).unwrap();
    let mut changes = fixture.changes();

    assert!(matches!(
        changes.apply(
            &fixture.policy(),
            true,
            fixture.proposal(),
            &fixture.expected_hash()
        ),
        Err(ChangeError::Policy(PolicyError::UnsafeFile))
    ));
    assert_eq!(fs::read(&original).unwrap(), b"original bytes");
    assert_eq!(
        fs::read_dir(fixture.data().join("backups"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn backup_creation_failure_preserves_target() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let backups = fixture.data().join("backups");
    fs::set_permissions(&backups, fs::Permissions::from_mode(0o500)).unwrap();

    let result = changes.apply(
        &fixture.policy(),
        true,
        fixture.proposal(),
        &fixture.expected_hash(),
    );

    fs::set_permissions(&backups, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(ChangeError::Storage(_))));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
    assert_eq!(fs::read_dir(backups).unwrap().count(), 0);
}

#[test]
fn target_directory_write_failure_preserves_target_and_records_failure() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o500)).unwrap();

    let result = changes.apply(
        &fixture.policy(),
        true,
        fixture.proposal(),
        &fixture.expected_hash(),
    );

    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(ChangeError::Io(_))));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original bytes");
    let backup_count = fs::read_dir(fixture.data().join("backups"))
        .unwrap()
        .count();
    assert_eq!(backup_count, 1);
    assert_eq!(changes.reconcile().unwrap(), 0);
}
