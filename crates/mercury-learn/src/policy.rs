//! Deterministic authorization boundary between model proposals and files.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

pub(crate) const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Instructions,
    Skill,
    Memory,
    /// Config edits need a section-aware merger; whole-file replacement is forbidden.
    ConfigSection,
    /// Exact existing Cron job, addressed by UUID and rendered as a safe patch.
    CronJob,
}

#[derive(Debug, Clone)]
pub struct TargetPolicy {
    pub id: String,
    pub path: PathBuf,
    pub kind: TargetKind,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub target_id: String,
    pub content: String,
    pub reason: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProposal {
    pub(crate) target_id: String,
    pub(crate) path: PathBuf,
    pub(crate) content: Vec<u8>,
    pub(crate) reason: String,
    pub(crate) source: String,
    pub(crate) max_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    Disabled,
    InvalidPolicy,
    DuplicateTarget,
    UnknownTarget,
    UnsupportedTarget,
    InvalidProposal,
    InvalidPath,
    UnsafeFile,
    TooLarge,
    Io,
}

pub struct Policy {
    targets: Vec<TargetPolicy>,
}

impl Policy {
    pub fn new(targets: Vec<TargetPolicy>) -> Result<Self, PolicyError> {
        let mut ids = HashSet::new();
        for target in &targets {
            if !valid_id(&target.id)
                || target.max_bytes == 0
                || target.max_bytes > MAX_FILE_BYTES
                || !valid_path(&target.path)
                || !valid_kind_path(target.kind, &target.path)
                || (target.kind == TargetKind::CronJob
                    && uuid::Uuid::parse_str(&target.id).is_err())
            {
                return Err(PolicyError::InvalidPolicy);
            }
            if !ids.insert(target.id.as_str()) {
                return Err(PolicyError::DuplicateTarget);
            }
        }
        Ok(Self { targets })
    }

    /// Only validates. The separate writer must recheck identity/hash just
    /// before replacement, because filesystem state can change after this call.
    pub fn validate(
        &self,
        enabled: bool,
        proposal: Proposal,
    ) -> Result<ValidatedProposal, PolicyError> {
        if !enabled {
            return Err(PolicyError::Disabled);
        }
        let target = self
            .targets
            .iter()
            .find(|target| target.id == proposal.target_id)
            .ok_or(PolicyError::UnknownTarget)?;
        if proposal.content.len() > target.max_bytes {
            return Err(PolicyError::TooLarge);
        }
        if proposal.reason.trim().is_empty()
            || proposal.reason.len() > 1024
            || proposal.source.trim().is_empty()
            || proposal.source.len() > 256
        {
            return Err(PolicyError::InvalidProposal);
        }
        check_existing_file(&target.path, target.max_bytes)?;
        let content = match target.kind {
            TargetKind::ConfigSection => {
                let current = fs::read_to_string(&target.path).map_err(|_| PolicyError::Io)?;
                merge_route_section(&current, &proposal.content, target.max_bytes)?.into_bytes()
            }
            TargetKind::CronJob => merge_cron_job(
                &target.path,
                &target.id,
                &proposal.content,
                target.max_bytes,
            )?
            .into_bytes(),
            _ => proposal.content.into_bytes(),
        };
        Ok(ValidatedProposal {
            target_id: proposal.target_id,
            path: target.path.clone(),
            content,
            reason: proposal.reason,
            source: proposal.source,
            max_bytes: target.max_bytes,
        })
    }

    pub(crate) fn targets(&self) -> &[TargetPolicy] {
        &self.targets
    }
}

/// The model may replace only the route table, never the surrounding config.
pub(crate) fn merge_route_section(
    current: &str,
    proposed: &str,
    max_bytes: usize,
) -> Result<String, PolicyError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OnlyRoute {
        route: mercury_router::RouteConfig,
    }

    let proposed_doc = proposed
        .parse::<DocumentMut>()
        .map_err(|_| PolicyError::InvalidProposal)?;
    if proposed_doc.iter().count() != 1 || proposed_doc.get("route").is_none() {
        return Err(PolicyError::InvalidProposal);
    }
    let route: OnlyRoute = toml::from_str(proposed).map_err(|_| PolicyError::InvalidProposal)?;
    route
        .route
        .validate()
        .map_err(|_| PolicyError::InvalidProposal)?;
    let mut current_doc = current
        .parse::<DocumentMut>()
        .map_err(|_| PolicyError::InvalidProposal)?;
    current_doc["route"] = proposed_doc["route"].clone();
    let merged = current_doc.to_string();
    if merged.len() > max_bytes {
        return Err(PolicyError::TooLarge);
    }
    Ok(merged)
}

pub(crate) fn route_section(current: &str) -> Result<String, PolicyError> {
    let document = current
        .parse::<DocumentMut>()
        .map_err(|_| PolicyError::InvalidProposal)?;
    let Some(route) = document.get("route") else {
        return Ok("[route]\n".to_owned());
    };
    if !route.is_table() {
        return Err(PolicyError::InvalidProposal);
    }
    let mut section = DocumentMut::new();
    section["route"] = route.clone();
    Ok(section.to_string())
}

pub(crate) fn cron_job_snapshot(current: &str, job_id: &str) -> Result<String, PolicyError> {
    let file: mercury_cron::storage::JobsFile =
        toml::from_str(current).map_err(|_| PolicyError::InvalidProposal)?;
    file.validate().map_err(|_| PolicyError::InvalidProposal)?;
    let job = file
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .ok_or(PolicyError::UnknownTarget)?;
    serde_json::to_string_pretty(&serde_json::json!({
        "schedule": job.schedule,
        "enabled": job.enabled,
        "timeout_seconds": job.timeout_seconds,
    }))
    .map_err(|_| PolicyError::InvalidProposal)
}

fn merge_cron_job(
    path: &Path,
    job_id: &str,
    patch: &str,
    max_bytes: usize,
) -> Result<String, PolicyError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CronPatch {
        schedule: Option<String>,
        enabled: Option<bool>,
        timeout_seconds: Option<u64>,
    }
    let patch: CronPatch = serde_json::from_str(patch).map_err(|_| PolicyError::InvalidProposal)?;
    if patch.schedule.is_none() && patch.enabled.is_none() && patch.timeout_seconds.is_none() {
        return Err(PolicyError::InvalidProposal);
    }
    if cron_job_has_active_run(path, job_id)? {
        return Err(PolicyError::InvalidProposal);
    }
    let mut file =
        mercury_cron::storage::JobsFile::load(path).map_err(|_| PolicyError::InvalidProposal)?;
    let job = file
        .jobs
        .iter_mut()
        .find(|job| job.id == job_id)
        .ok_or(PolicyError::UnknownTarget)?;
    if let Some(schedule) = patch.schedule {
        let refreshed = mercury_cron::Job::new(
            job.name.clone(),
            &schedule,
            job.offset().map_err(|_| PolicyError::InvalidProposal)?,
            time::OffsetDateTime::now_utc(),
            job.target.clone(),
        )
        .map_err(|_| PolicyError::InvalidProposal)?;
        job.schedule = refreshed.schedule;
        job.next_run_at = refreshed.next_run_at;
    }
    if let Some(enabled) = patch.enabled {
        job.enabled = enabled;
    }
    if let Some(timeout) = patch.timeout_seconds {
        job.timeout_seconds = timeout;
    }
    file.validate().map_err(|_| PolicyError::InvalidProposal)?;
    let merged = toml::to_string_pretty(&file).map_err(|_| PolicyError::InvalidProposal)?;
    if merged.len() > max_bytes {
        return Err(PolicyError::TooLarge);
    }
    Ok(merged)
}

pub(crate) fn cron_job_has_active_run(path: &Path, job_id: &str) -> Result<bool, PolicyError> {
    let ledger = path
        .parent()
        .ok_or(PolicyError::InvalidPath)?
        .join("executions.db");
    mercury_cron::storage::job_has_active_run(&ledger, job_id)
        .map_err(|_| PolicyError::InvalidProposal)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && (byte == b'-' || byte == b'_'))
        })
}

fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|path| path.len() <= 4096)
        && path
            .components()
            .all(|part| !matches!(part, Component::CurDir | Component::ParentDir))
}

fn valid_kind_path(kind: TargetKind, path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    match kind {
        TargetKind::Instructions => matches!(name, Some("AGENTS.md" | "CLAUDE.md")),
        TargetKind::Skill => name == Some("SKILL.md"),
        TargetKind::Memory => path.extension().is_some_and(|ext| ext == "md"),
        TargetKind::ConfigSection => name == Some("config.toml"),
        TargetKind::CronJob => name == Some("jobs.toml"),
    }
}

pub fn check_existing_file(path: &Path, max_bytes: usize) -> Result<(), PolicyError> {
    if !valid_path(path) {
        return Err(PolicyError::InvalidPath);
    }
    let mut prefix = PathBuf::new();
    let parts: Vec<_> = path.components().collect();
    for (index, part) in parts.iter().enumerate() {
        prefix.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&prefix).map_err(|_| PolicyError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(PolicyError::UnsafeFile);
        }
        if index + 1 == parts.len() {
            if !metadata.file_type().is_file() {
                return Err(PolicyError::UnsafeFile);
            }
            if metadata.len() > max_bytes as u64 {
                return Err(PolicyError::TooLarge);
            }
        } else if !metadata.file_type().is_dir() {
            return Err(PolicyError::UnsafeFile);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        file: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().canonicalize().unwrap();
            let root = base.join(format!(
                "mercury-learn-policy-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let file = root.join("AGENTS.md");
            fs::write(&file, b"original").unwrap();
            Self { root, file }
        }

        fn target(&self) -> TargetPolicy {
            TargetPolicy {
                id: "agent_instructions".into(),
                path: self.file.clone(),
                kind: TargetKind::Instructions,
                max_bytes: 1024,
            }
        }

        fn proposal(&self) -> Proposal {
            Proposal {
                target_id: "agent_instructions".into(),
                content: "new instruction".into(),
                reason: "avoid a repeated error".into(),
                source: "codex/session-1".into(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn valid_exact_target_is_authorized_without_writing() {
        let fixture = Fixture::new();
        let policy = Policy::new(vec![fixture.target()]).unwrap();

        let validated = policy.validate(true, fixture.proposal()).unwrap();

        assert_eq!(validated.path, fixture.file);
        assert_eq!(validated.content, b"new instruction");
        assert_eq!(fs::read(&fixture.file).unwrap(), b"original");
    }

    #[test]
    fn disabled_policy_rejects_a_valid_proposal() {
        let fixture = Fixture::new();
        let policy = Policy::new(vec![fixture.target()]).unwrap();

        let result = policy.validate(false, fixture.proposal());

        assert_eq!(result.unwrap_err(), PolicyError::Disabled);
    }

    #[test]
    fn unknown_target_id_does_not_select_a_path() {
        let fixture = Fixture::new();
        let policy = Policy::new(vec![fixture.target()]).unwrap();
        let mut proposal = fixture.proposal();
        proposal.target_id = "../../AGENTS.md".into();

        let result = policy.validate(true, proposal);

        assert_eq!(result.unwrap_err(), PolicyError::UnknownTarget);
    }

    #[test]
    fn duplicate_or_malformed_allowlist_is_rejected() {
        let fixture = Fixture::new();
        let target = fixture.target();
        assert!(matches!(
            Policy::new(vec![target.clone(), target]),
            Err(PolicyError::DuplicateTarget)
        ));
        let mut target = fixture.target();
        target.path = fixture.root.join("..").join("AGENTS.md");
        assert!(matches!(
            Policy::new(vec![target]),
            Err(PolicyError::InvalidPolicy)
        ));
    }

    #[test]
    fn oversized_new_or_existing_content_is_rejected() {
        let fixture = Fixture::new();
        let mut target = fixture.target();
        target.max_bytes = 8;
        let policy = Policy::new(vec![target]).unwrap();
        assert_eq!(
            policy.validate(true, fixture.proposal()).unwrap_err(),
            PolicyError::TooLarge
        );
        fs::write(&fixture.file, b"original is too big").unwrap();
        let mut proposal = fixture.proposal();
        proposal.content = "short".into();
        assert_eq!(
            policy.validate(true, proposal).unwrap_err(),
            PolicyError::TooLarge
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_rejected_even_when_allowlisted() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let link = fixture.root.join("SKILL.md");
        symlink(&fixture.file, &link).unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "skill".into(),
            path: link,
            kind: TargetKind::Skill,
            max_bytes: 1024,
        }])
        .unwrap();
        let mut proposal = fixture.proposal();
        proposal.target_id = "skill".into();

        assert_eq!(
            policy.validate(true, proposal).unwrap_err(),
            PolicyError::UnsafeFile
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_parent_is_rejected_even_when_target_is_regular() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let linked_parent = fixture.root.join("linked");
        symlink(&fixture.root, &linked_parent).unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: linked_parent.join("AGENTS.md"),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap();
        let mut proposal = fixture.proposal();
        proposal.target_id = "instructions".into();

        assert_eq!(
            policy.validate(true, proposal).unwrap_err(),
            PolicyError::UnsafeFile
        );
    }

    #[test]
    fn config_route_only_merge_preserves_other_sections_and_rejects_extra_tables() {
        let fixture = Fixture::new();
        let config = fixture.root.join("config.toml");
        let original = "# keep me\n[theme]\nname = 'test'\n[route]\nenabled = false\n";
        fs::write(&config, original).unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "routing".into(),
            path: config.clone(),
            kind: TargetKind::ConfigSection,
            max_bytes: 1024,
        }])
        .unwrap();
        let mut proposal = fixture.proposal();
        proposal.target_id = "routing".into();
        proposal.content = "[route]\nenabled = true\n".into();
        let validated = policy.validate(true, proposal.clone()).unwrap();
        let merged = String::from_utf8(validated.content).unwrap();
        assert!(merged.starts_with("# keep me\n[theme]\nname = 'test'\n"));
        assert!(merged.contains("[route]\nenabled = true"));
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        proposal.content = "[route]\nenabled = true\n[learn]\nenabled = true\n".into();
        assert_eq!(
            policy.validate(true, proposal).unwrap_err(),
            PolicyError::InvalidProposal
        );
    }

    #[test]
    fn cron_patch_can_only_change_allowlisted_job_fields() {
        let fixture = Fixture::new();
        let path = fixture.root.join("jobs.toml");
        let now = time::OffsetDateTime::now_utc();
        let job = mercury_cron::Job::new(
            "safe task".into(),
            "every 1h",
            time::UtcOffset::UTC,
            now,
            mercury_cron::JobTarget::Script {
                program: "/usr/bin/true".into(),
                args: Vec::new(),
            },
        )
        .unwrap();
        mercury_cron::storage::JobsFile {
            version: 1,
            jobs: vec![job.clone()],
        }
        .save_atomic(&path)
        .unwrap();
        mercury_cron::storage::SqliteLedger::open(&fixture.root.join("executions.db")).unwrap();
        let original = fs::read(&path).unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: job.id.clone(),
            path: path.clone(),
            kind: TargetKind::CronJob,
            max_bytes: 4096,
        }])
        .unwrap();
        let snapshot = cron_job_snapshot(std::str::from_utf8(&original).unwrap(), &job.id).unwrap();
        assert!(!snapshot.contains("/usr/bin/true"));
        let mut proposal = Proposal {
            target_id: job.id.clone(),
            content: r#"{"enabled":false,"timeout_seconds":90}"#.into(),
            reason: "avoid unnecessary runs".into(),
            source: "claude:session-1".into(),
        };
        let validated = policy.validate(true, proposal.clone()).unwrap();
        let changed: mercury_cron::storage::JobsFile =
            toml::from_str(std::str::from_utf8(&validated.content).unwrap()).unwrap();
        assert!(!changed.jobs[0].enabled);
        assert_eq!(changed.jobs[0].timeout_seconds, 90);
        assert_eq!(changed.jobs[0].target, job.target);
        assert_eq!(fs::read(&path).unwrap(), original);
        proposal.content = r#"{"target":{"kind":"script","program":"/bin/sh"}}"#.into();
        assert_eq!(
            policy.validate(true, proposal).unwrap_err(),
            PolicyError::InvalidProposal
        );
    }
}
