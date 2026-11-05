//! Apply and undo exact allowlisted changes with a durable pre-write intent.
//! The filesystem has no portable compare-and-swap rename: a concurrent
//! external writer between the last check and rename remains a residual risk.

use crate::policy::{Policy, PolicyError, Proposal};
use crate::storage::{ChangeRecord, ChangeStatus, Storage, StorageError};
#[cfg(unix)]
use std::sync::Mutex;

/// The server owns one Learn ledger, but review and user undo run on separate
/// threads. Serialize their filesystem/ledger transitions within the process.
#[cfg(unix)]
static OPERATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum ChangeError {
    Policy(PolicyError),
    Storage(StorageError),
    Io(std::io::Error),
    Conflict,
    UnsafeTarget,
    TooLarge,
    NoChange,
    Missing,
    InvalidState,
    Uncertain,
    UnsupportedPlatform,
}

impl From<PolicyError> for ChangeError {
    fn from(value: PolicyError) -> Self {
        Self::Policy(value)
    }
}

impl From<StorageError> for ChangeError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<std::io::Error> for ChangeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Changes {
    storage: Storage,
}

/// Content shown to the reviewer and its trusted, same-read precondition.
/// Keep this value outside model output and pass `before_hash` to `apply`.
#[derive(Debug, Clone)]
pub struct ReviewSnapshot {
    pub target_id: String,
    pub content: String,
    pub before_hash: String,
    pub max_bytes: usize,
}

#[cfg(unix)]
pub fn review_snapshots(
    policy: &Policy,
    enabled: bool,
) -> Result<Vec<ReviewSnapshot>, ChangeError> {
    use crate::policy::{PolicyError, TargetKind};

    if !enabled {
        return Err(ChangeError::Policy(PolicyError::Disabled));
    }
    policy
        .targets()
        .iter()
        .map(|target| {
            if target.max_bytes > 64 * 1024 {
                return Err(ChangeError::TooLarge);
            }
            let snapshot = read_current(&target.path, target.max_bytes)?;
            let before_hash = crate::storage::hash(&snapshot.bytes);
            let content =
                String::from_utf8(snapshot.bytes).map_err(|_| ChangeError::InvalidState)?;
            let content = match target.kind {
                TargetKind::ConfigSection => {
                    crate::policy::route_section(&content).map_err(ChangeError::Policy)?
                }
                TargetKind::CronJob => crate::policy::cron_job_snapshot(&content, &target.id)
                    .map_err(ChangeError::Policy)?,
                _ => content,
            };
            Ok(ReviewSnapshot {
                target_id: target.id.clone(),
                content,
                before_hash,
                max_bytes: target.max_bytes,
            })
        })
        .collect()
}

#[cfg(unix)]
pub(crate) fn review_snapshot_for(
    policy: &Policy,
    enabled: bool,
    target_id: &str,
) -> Result<ReviewSnapshot, ChangeError> {
    use crate::policy::{PolicyError, TargetKind};

    if !enabled {
        return Err(ChangeError::Policy(PolicyError::Disabled));
    }
    let target = policy
        .targets()
        .iter()
        .find(|target| target.id == target_id)
        .ok_or(ChangeError::Policy(PolicyError::UnknownTarget))?;
    let snapshot = read_current(&target.path, target.max_bytes)?;
    let before_hash = crate::storage::hash(&snapshot.bytes);
    let content = String::from_utf8(snapshot.bytes).map_err(|_| ChangeError::InvalidState)?;
    let content = match target.kind {
        TargetKind::ConfigSection => {
            crate::policy::route_section(&content).map_err(ChangeError::Policy)?
        }
        TargetKind::CronJob => {
            crate::policy::cron_job_snapshot(&content, &target.id).map_err(ChangeError::Policy)?
        }
        _ => content,
    };
    Ok(ReviewSnapshot {
        target_id: target.id.clone(),
        content,
        before_hash,
        max_bytes: target.max_bytes,
    })
}

#[cfg(not(unix))]
pub fn review_snapshots(
    _policy: &Policy,
    _enabled: bool,
) -> Result<Vec<ReviewSnapshot>, ChangeError> {
    Err(ChangeError::UnsupportedPlatform)
}

impl Changes {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// `expected_before_hash` must be computed from the trusted file snapshot
    /// supplied to the reviewer, never from the reviewer's proposed response.
    /// A later human edit is a conflict before any backup or write is made.
    #[cfg(unix)]
    pub fn apply(
        &mut self,
        policy: &Policy,
        enabled: bool,
        proposal: Proposal,
        expected_before_hash: &str,
    ) -> Result<ChangeRecord, ChangeError> {
        use crate::storage::PrepareChange;

        let _operation = OPERATION_LOCK.lock().map_err(|_| ChangeError::Uncertain)?;
        let _jobs = mercury_cron::jobs_file_lock().map_err(|_| ChangeError::Uncertain)?;
        let proposal = policy.validate(enabled, proposal)?;
        self.reconcile_unlocked()?;
        let before = read_current(&proposal.path, proposal.max_bytes)?;
        if crate::storage::hash(&before.bytes) != expected_before_hash {
            return Err(ChangeError::Conflict);
        }
        if before.bytes == proposal.content {
            return Err(ChangeError::NoChange);
        }
        let prepared = self.storage.prepare(PrepareChange {
            target_id: &proposal.target_id,
            target_path: &proposal.path,
            source: &proposal.source,
            reason: &proposal.reason,
            before_bytes: &before.bytes,
            after_bytes: &proposal.content,
        })?;

        match replace_checked(
            &proposal.path,
            &before,
            &proposal.content,
            proposal.max_bytes,
        ) {
            Ok(()) => {
                self.storage.transition(
                    &prepared.id,
                    ChangeStatus::Prepared,
                    ChangeStatus::Applied,
                )?;
                self.storage.get(&prepared.id)?.ok_or(ChangeError::Missing)
            }
            Err(ReplaceFailure::Before(error)) => {
                let status = if matches!(error, ChangeError::Conflict) {
                    ChangeStatus::Conflict
                } else {
                    ChangeStatus::Failed
                };
                self.storage
                    .transition(&prepared.id, ChangeStatus::Prepared, status)?;
                Err(error)
            }
            Err(ReplaceFailure::After) => Err(ChangeError::Uncertain),
        }
    }

    #[cfg(not(unix))]
    pub fn apply(
        &mut self,
        _policy: &Policy,
        _enabled: bool,
        _proposal: Proposal,
        _expected_before_hash: &str,
    ) -> Result<ChangeRecord, ChangeError> {
        Err(ChangeError::UnsupportedPlatform)
    }

    #[cfg(unix)]
    pub fn undo(&mut self, id: &str) -> Result<ChangeRecord, ChangeError> {
        let _operation = OPERATION_LOCK.lock().map_err(|_| ChangeError::Uncertain)?;
        let _jobs = mercury_cron::jobs_file_lock().map_err(|_| ChangeError::Uncertain)?;
        if let Some(record) = self.storage.get(id)? {
            if record
                .target_path
                .file_name()
                .is_some_and(|name| name == "jobs.toml")
                && crate::policy::cron_job_has_active_run(&record.target_path, &record.target_id)
                    .map_err(ChangeError::Policy)?
            {
                return Err(ChangeError::Conflict);
            }
        }
        self.undo_after_temp(id, || {})
    }

    #[cfg(unix)]
    fn undo_after_temp(
        &mut self,
        id: &str,
        after_temp: impl FnOnce(),
    ) -> Result<ChangeRecord, ChangeError> {
        use crate::policy::MAX_FILE_BYTES;

        self.reconcile_unlocked()?;
        let record = self.storage.get(id)?.ok_or(ChangeError::Missing)?;
        if !matches!(
            record.status,
            ChangeStatus::Applied | ChangeStatus::Conflict
        ) {
            return Err(ChangeError::InvalidState);
        }
        let backup = self.storage.read_backup(&record)?;
        let current = match read_current(&record.target_path, MAX_FILE_BYTES) {
            Ok(current) if crate::storage::hash(&current.bytes) == record.after_hash => current,
            _ => {
                if record.status == ChangeStatus::Applied {
                    self.storage
                        .transition(id, ChangeStatus::Applied, ChangeStatus::Conflict)?;
                }
                return Err(ChangeError::Conflict);
            }
        };
        self.storage
            .transition(id, record.status, ChangeStatus::UndoPrepared)?;
        match replace_checked_after(
            &record.target_path,
            &current,
            &backup,
            MAX_FILE_BYTES,
            after_temp,
            || Ok(()),
        ) {
            Ok(()) => {
                self.storage
                    .transition(id, ChangeStatus::UndoPrepared, ChangeStatus::Undone)?;
                self.storage.get(id)?.ok_or(ChangeError::Missing)
            }
            Err(ReplaceFailure::Before(error)) => {
                let status = if matches!(error, ChangeError::Conflict) {
                    ChangeStatus::Conflict
                } else {
                    ChangeStatus::Applied
                };
                self.storage
                    .transition(id, ChangeStatus::UndoPrepared, status)?;
                Err(error)
            }
            Err(ReplaceFailure::After) => Err(ChangeError::Uncertain),
        }
    }

    #[cfg(not(unix))]
    pub fn undo(&mut self, _id: &str) -> Result<ChangeRecord, ChangeError> {
        Err(ChangeError::UnsupportedPlatform)
    }

    /// Reconciles only interrupted operations; it never writes a target file.
    #[cfg(unix)]
    pub fn reconcile(&mut self) -> Result<usize, ChangeError> {
        let _operation = OPERATION_LOCK.lock().map_err(|_| ChangeError::Uncertain)?;
        let _jobs = mercury_cron::jobs_file_lock().map_err(|_| ChangeError::Uncertain)?;
        self.reconcile_unlocked()
    }

    #[cfg(unix)]
    fn reconcile_unlocked(&mut self) -> Result<usize, ChangeError> {
        use crate::policy::MAX_FILE_BYTES;

        let pending = self.storage.pending()?;
        let count = pending.len();
        for record in pending {
            let after = self.storage.read_backup(&record).ok().and_then(|_| {
                read_current(&record.target_path, MAX_FILE_BYTES)
                    .ok()
                    .map(|snapshot| crate::storage::hash(&snapshot.bytes))
            });
            let new_status = match record.status {
                ChangeStatus::Prepared => match after.as_deref() {
                    Some(hash) if hash == record.after_hash => ChangeStatus::Applied,
                    Some(hash) if hash == record.before_hash => ChangeStatus::Failed,
                    _ => ChangeStatus::Conflict,
                },
                ChangeStatus::UndoPrepared => match after.as_deref() {
                    Some(hash) if hash == record.before_hash => ChangeStatus::Undone,
                    Some(hash) if hash == record.after_hash => ChangeStatus::Applied,
                    _ => ChangeStatus::Conflict,
                },
                _ => continue,
            };
            self.storage
                .transition(&record.id, record.status, new_status)?;
        }
        Ok(count)
    }

    #[cfg(not(unix))]
    pub fn reconcile(&mut self) -> Result<usize, ChangeError> {
        Err(ChangeError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
mod unix {
    use std::fs::{self, File, OpenOptions, Permissions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use crate::policy::check_existing_file;

    use super::ChangeError;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) struct Identity {
        device: u64,
        inode: u64,
    }

    pub(super) struct Snapshot {
        pub(super) bytes: Vec<u8>,
        pub(super) identity: Identity,
        permissions: Permissions,
    }

    pub(super) enum ReplaceFailure {
        Before(ChangeError),
        After,
    }

    struct Temporary {
        path: PathBuf,
        renamed: bool,
    }

    impl Drop for Temporary {
        fn drop(&mut self) {
            if !self.renamed {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    fn identity(metadata: &fs::Metadata) -> Identity {
        Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    pub(super) fn read_current(path: &Path, max_bytes: usize) -> Result<Snapshot, ChangeError> {
        check_existing_file(path, max_bytes).map_err(|_| ChangeError::UnsafeTarget)?;
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() {
            return Err(ChangeError::UnsafeTarget);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(ChangeError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        Read::by_ref(&mut file)
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(ChangeError::TooLarge);
        }
        let live = fs::symlink_metadata(path)?;
        if !live.file_type().is_file() || identity(&live) != identity(&metadata) {
            return Err(ChangeError::Conflict);
        }
        check_existing_file(path, max_bytes).map_err(|_| ChangeError::UnsafeTarget)?;
        Ok(Snapshot {
            bytes,
            identity: identity(&metadata),
            permissions: metadata.permissions(),
        })
    }

    pub(super) fn replace_checked(
        path: &Path,
        expected: &Snapshot,
        content: &[u8],
        max_bytes: usize,
    ) -> Result<(), ReplaceFailure> {
        replace_checked_after(path, expected, content, max_bytes, || {}, || Ok(()))
    }

    pub(super) fn replace_checked_after(
        path: &Path,
        expected: &Snapshot,
        content: &[u8],
        max_bytes: usize,
        after_temp: impl FnOnce(),
        before_rename: impl FnOnce() -> Result<(), ChangeError>,
    ) -> Result<(), ReplaceFailure> {
        let parent = path
            .parent()
            .ok_or(ReplaceFailure::Before(ChangeError::UnsafeTarget))?;
        let temporary = Temporary {
            path: parent.join(format!(".mercury-learn-{}.tmp", Uuid::new_v4())),
            renamed: false,
        };
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&temporary.path)
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;
        file.write_all(content)
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;
        file.sync_all()
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;
        fs::set_permissions(&temporary.path, expected.permissions.clone())
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;
        file.sync_all()
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;

        after_temp();
        let current = read_current(path, max_bytes)
            .map_err(|_| ReplaceFailure::Before(ChangeError::Conflict))?;
        if current.identity != expected.identity || current.bytes != expected.bytes {
            return Err(ReplaceFailure::Before(ChangeError::Conflict));
        }
        before_rename().map_err(ReplaceFailure::Before)?;
        fs::rename(&temporary.path, path)
            .map_err(|error| ReplaceFailure::Before(ChangeError::Io(error)))?;
        let mut temporary = temporary;
        temporary.renamed = true;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| ReplaceFailure::After)?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn edit_after_temp_write_prevents_rename_and_removes_temp() {
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("mercury-learn-recheck-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let target = root.join("AGENTS.md");
            fs::write(&target, b"original").unwrap();
            let expected = read_current(&target, 1024).unwrap();

            let result = replace_checked_after(
                &target,
                &expected,
                b"proposal",
                1024,
                || {
                    fs::write(&target, b"human edit").unwrap();
                },
                || Ok(()),
            );

            assert!(matches!(
                result,
                Err(ReplaceFailure::Before(ChangeError::Conflict))
            ));
            assert_eq!(fs::read(&target).unwrap(), b"human edit");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn replacement_with_same_bytes_but_new_inode_is_a_conflict() {
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("mercury-learn-identity-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let target = root.join("AGENTS.md");
            fs::write(&target, b"original").unwrap();
            let expected = read_current(&target, 1024).unwrap();
            let replacement = root.join("replacement.md");

            let result = replace_checked_after(
                &target,
                &expected,
                b"proposal",
                1024,
                || {
                    fs::write(&replacement, b"original").unwrap();
                    fs::rename(&replacement, &target).unwrap();
                },
                || Ok(()),
            );

            assert!(matches!(
                result,
                Err(ReplaceFailure::Before(ChangeError::Conflict))
            ));
            assert_eq!(fs::read(&target).unwrap(), b"original");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn injected_rename_step_failure_preserves_target_and_cleans_temp() {
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("mercury-learn-rename-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let target = root.join("AGENTS.md");
            fs::write(&target, b"original").unwrap();
            let expected = read_current(&target, 1024).unwrap();

            let result = replace_checked_after(
                &target,
                &expected,
                b"proposal",
                1024,
                || {},
                || {
                    Err(ChangeError::Io(std::io::Error::other(
                        "injected rename fault",
                    )))
                },
            );

            assert!(matches!(
                result,
                Err(ReplaceFailure::Before(ChangeError::Io(_)))
            ));
            assert_eq!(fs::read(&target).unwrap(), b"original");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(unix)]
use unix::{read_current, replace_checked, replace_checked_after, ReplaceFailure};

#[cfg(all(test, unix))]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use crate::policy::{TargetKind, TargetPolicy};

    use super::*;

    #[test]
    fn edit_during_undo_preserves_human_bytes_and_records_conflict() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("mercury-learn-undo-race-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let target = root.join("AGENTS.md");
        fs::write(&target, b"original").unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap();
        let mut changes = Changes::new(Storage::open(&root.join("learn")).unwrap());
        let applied = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "instructions".into(),
                    content: "proposal".into(),
                    reason: "test".into(),
                    source: "codex/session-1".into(),
                },
                &crate::storage::hash(b"original"),
            )
            .unwrap();

        let result = changes.undo_after_temp(&applied.id, || {
            fs::write(&target, b"human edit").unwrap();
        });

        assert!(matches!(result, Err(ChangeError::Conflict)));
        assert_eq!(fs::read(&target).unwrap(), b"human edit");
        assert_eq!(
            changes.storage().get(&applied.id).unwrap().unwrap().status,
            ChangeStatus::Conflict
        );
        drop(changes);
        fs::remove_dir_all(root).unwrap();
    }
}
