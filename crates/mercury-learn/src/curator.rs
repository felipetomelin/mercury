//! Deterministic curation plans for exact allowlisted skills and memories.
//! This module proposes changes only. The existing writer owns backup, ledger,
//! conflict detection, application, and undo for every step.

use crate::changes::{review_snapshot_for, ChangeError, Changes};
use crate::policy::{Policy, Proposal, TargetKind, TargetPolicy};
const ARCHIVE_MARKER: &str = "<!-- Archived by Mercury Learn -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationKind {
    Merge,
}

#[derive(Debug, Clone)]
pub struct PlannedChange {
    pub proposal: Proposal,
    /// From the same trusted read as the content used to build the proposal.
    pub expected_before_hash: String,
}

#[derive(Debug, Clone)]
pub struct CurationPlan {
    pub kind: CurationKind,
    /// For merge: destination first, then source archive. A caller must
    /// inspect and recover a partial apply instead of claiming atomicity.
    pub steps: Vec<PlannedChange>,
}

#[derive(Debug)]
pub struct CurationApplyFailure {
    /// Already committed steps remain in the private ledger and require an
    /// explicit undo; a multi-step plan is not a filesystem transaction.
    pub applied_ids: Vec<String>,
    pub error: ChangeError,
}

#[derive(Debug)]
pub enum CuratorError {
    Change(ChangeError),
    InvalidSource,
    UnsupportedTarget,
    SameTarget,
    CrossDirectory,
    IncompatibleSkill,
    AlreadyArchived,
    TooLarge,
}

impl From<ChangeError> for CuratorError {
    fn from(value: ChangeError) -> Self {
        Self::Change(value)
    }
}

fn target<'a>(policy: &'a Policy, id: &str) -> Result<&'a TargetPolicy, CuratorError> {
    let target = policy
        .targets()
        .iter()
        .find(|target| target.id == id)
        .ok_or(CuratorError::UnsupportedTarget)?;
    if !matches!(target.kind, TargetKind::Skill | TargetKind::Memory) {
        return Err(CuratorError::UnsupportedTarget);
    }
    Ok(target)
}

fn valid_source(source: &str) -> bool {
    !source.is_empty()
        && source.len() <= 256
        && source
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}

fn archive_stub(kind: TargetKind, target_id: &str) -> String {
    match kind {
        TargetKind::Skill => {
            let mut name = format!("archived-{}", target_id.replace('_', "-"));
            name.truncate(64);
            format!(
                "---\nname: {name}\ndescription: Archived by Mercury Learn; do not use.\n---\n\n{ARCHIVE_MARKER}\n# Archived skill\nOriginal instructions are in the private Learn backup. Use `mercury learn list` and `mercury learn undo` to restore.\n"
            )
        }
        TargetKind::Memory => format!(
            "{ARCHIVE_MARKER}\n# Archived memory\nOriginal content is in the private Learn backup. Use `mercury learn list` and `mercury learn undo` to restore.\n"
        ),
        _ => unreachable!("curator only accepts skills or memories"),
    }
}

/// Exact duplicate skills can be deduplicated. Distinct memory content is
/// combined only within the same directory to avoid cross-project leakage.
#[cfg(unix)]
pub fn plan_merge(
    policy: &Policy,
    enabled: bool,
    source_id: &str,
    destination_id: &str,
    source: &str,
) -> Result<CurationPlan, CuratorError> {
    if !valid_source(source) {
        return Err(CuratorError::InvalidSource);
    }
    if source_id == destination_id {
        return Err(CuratorError::SameTarget);
    }
    let source_target = target(policy, source_id)?;
    let destination_target = target(policy, destination_id)?;
    if source_target.path == destination_target.path {
        return Err(CuratorError::SameTarget);
    }
    if source_target.kind != destination_target.kind {
        return Err(CuratorError::UnsupportedTarget);
    }
    let source_snapshot = review_snapshot_for(policy, enabled, source_id)?;
    let destination_snapshot = review_snapshot_for(policy, enabled, destination_id)?;
    if source_snapshot.content.contains(ARCHIVE_MARKER)
        || destination_snapshot.content.contains(ARCHIVE_MARKER)
    {
        return Err(CuratorError::AlreadyArchived);
    }
    let identical = source_snapshot.content == destination_snapshot.content;
    if source_target.kind == TargetKind::Skill && !identical {
        return Err(CuratorError::IncompatibleSkill);
    }
    if source_target.kind == TargetKind::Memory
        && source_target.path.parent() != destination_target.path.parent()
    {
        return Err(CuratorError::CrossDirectory);
    }
    let mut steps = Vec::with_capacity(2);
    if !identical {
        let mut merged = destination_snapshot.content.clone();
        if !merged.ends_with('\n') {
            merged.push('\n');
        }
        merged.push('\n');
        merged.push_str(&format!("<!-- Mercury Learn merge from {source_id} -->\n"));
        merged.push_str(&source_snapshot.content);
        if merged.len() > destination_target.max_bytes {
            return Err(CuratorError::TooLarge);
        }
        steps.push(PlannedChange {
            proposal: Proposal {
                target_id: destination_id.to_owned(),
                content: merged,
                reason: format!("Combine allowlisted {source_id} into {destination_id}"),
                source: source.to_owned(),
            },
            expected_before_hash: destination_snapshot.before_hash,
        });
    }
    let stub = archive_stub(source_target.kind, source_id);
    if stub.len() > source_target.max_bytes {
        return Err(CuratorError::TooLarge);
    }
    steps.push(PlannedChange {
        proposal: Proposal {
            target_id: source_id.to_owned(),
            content: stub,
            reason: format!("Merge allowlisted {source_id} into {destination_id}"),
            source: source.to_owned(),
        },
        expected_before_hash: source_snapshot.before_hash,
    });
    Ok(CurationPlan {
        kind: CurationKind::Merge,
        steps,
    })
}

/// Conservative weekly automation: only byte-identical allowlisted targets.
/// No-use archival and distinct-content memory merging require separate
/// trusted evidence and are deliberately not inferred here.
#[cfg(unix)]
pub fn plan_exact_duplicate_cleanup(
    policy: &Policy,
    enabled: bool,
    source: &str,
) -> Result<Vec<CurationPlan>, CuratorError> {
    if !enabled {
        return Err(CuratorError::Change(ChangeError::Policy(
            crate::policy::PolicyError::Disabled,
        )));
    }
    if !valid_source(source) {
        return Err(CuratorError::InvalidSource);
    }
    let targets = policy.targets();
    let mut plans = Vec::new();
    for (source_index, candidate) in targets.iter().enumerate().skip(1) {
        if !matches!(candidate.kind, TargetKind::Skill | TargetKind::Memory) {
            continue;
        }
        let Ok(candidate_snapshot) = review_snapshot_for(policy, enabled, &candidate.id) else {
            continue;
        };
        for destination in &targets[..source_index] {
            if destination.kind != candidate.kind
                || (candidate.kind == TargetKind::Memory
                    && candidate.path.parent() != destination.path.parent())
            {
                continue;
            }
            let Ok(destination_snapshot) = review_snapshot_for(policy, enabled, &destination.id)
            else {
                continue;
            };
            if candidate_snapshot.content != destination_snapshot.content {
                continue;
            }
            if let Ok(plan) = plan_merge(policy, enabled, &candidate.id, &destination.id, source) {
                plans.push(plan);
                break;
            }
        }
        if plans.len() == 4 {
            break;
        }
    }
    Ok(plans)
}

#[cfg(unix)]
pub fn apply_plan(
    policy: &Policy,
    is_enabled: impl Fn() -> bool,
    changes: &mut Changes,
    plan: CurationPlan,
) -> Result<Vec<String>, CurationApplyFailure> {
    let mut applied_ids = Vec::with_capacity(plan.steps.len());
    for step in plan.steps {
        match changes.apply(
            policy,
            is_enabled(),
            step.proposal,
            &step.expected_before_hash,
        ) {
            Ok(record) => applied_ids.push(record.id),
            Err(error) => return Err(CurationApplyFailure { applied_ids, error }),
        }
    }
    Ok(applied_ids)
}
