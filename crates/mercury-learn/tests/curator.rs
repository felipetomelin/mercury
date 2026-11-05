#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mercury_learn::curator::{
    apply_plan, plan_exact_duplicate_cleanup, plan_merge, CurationKind, CuratorError,
};
use mercury_learn::policy::{Policy, TargetKind, TargetPolicy};
use mercury_learn::storage::Storage;
use mercury_learn::{ChangeError, Changes};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-curator-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn data(&self) -> PathBuf {
        self.0.join("learn")
    }

    fn target(&self, id: &str, relative: &str, kind: TargetKind, bytes: &[u8]) -> TargetPolicy {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        TargetPolicy {
            id: id.into(),
            path,
            kind,
            max_bytes: 4096,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn weekly_duplicate_cleanup_applies_only_exact_safe_pairs_and_undoes() {
    let fixture = Fixture::new();
    let skill_bytes = b"---\nname: shared\ndescription: same\n---\nUse this.\n";
    let skill_primary = fixture.target(
        "skill_primary",
        "skills/a/SKILL.md",
        TargetKind::Skill,
        skill_bytes,
    );
    let skill_copy = fixture.target(
        "skill_copy",
        "skills/b/SKILL.md",
        TargetKind::Skill,
        skill_bytes,
    );
    let memory_primary = fixture.target(
        "memory_primary",
        "notes/a.md",
        TargetKind::Memory,
        b"same memory\n",
    );
    let memory_copy = fixture.target(
        "memory_copy",
        "notes/b.md",
        TargetKind::Memory,
        b"same memory\n",
    );
    let other_project = fixture.target(
        "other_project",
        "other/c.md",
        TargetKind::Memory,
        b"same memory\n",
    );
    let distinct = fixture.target("distinct", "notes/d.md", TargetKind::Memory, b"different\n");
    let policy = Policy::new(vec![
        skill_primary.clone(),
        skill_copy.clone(),
        memory_primary.clone(),
        memory_copy.clone(),
        other_project.clone(),
        distinct.clone(),
    ])
    .unwrap();
    let plans = plan_exact_duplicate_cleanup(&policy, true, "curator:weekly:2026-10-07").unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].steps[0].proposal.target_id, "skill_copy");
    assert_eq!(plans[1].steps[0].proposal.target_id, "memory_copy");
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let mut ids = Vec::new();
    for plan in plans {
        ids.extend(apply_plan(&policy, || true, &mut changes, plan).unwrap());
    }
    assert_eq!(ids.len(), 2);
    assert_eq!(fs::read(&skill_primary.path).unwrap(), skill_bytes);
    assert_eq!(fs::read(&memory_primary.path).unwrap(), b"same memory\n");
    assert_eq!(fs::read(&other_project.path).unwrap(), b"same memory\n");
    assert_eq!(fs::read(&distinct.path).unwrap(), b"different\n");
    for id in ids.iter().rev() {
        changes.undo(id).unwrap();
    }
    assert_eq!(fs::read(&skill_copy.path).unwrap(), skill_bytes);
    assert_eq!(fs::read(&memory_copy.path).unwrap(), b"same memory\n");
}

#[test]
fn same_directory_memory_merge_is_reversible_for_both_files() {
    let fixture = Fixture::new();
    let original_destination = b"# Destination\nfirst idea\n";
    let original_source = b"# Source\nsecond idea\n";
    let destination = fixture.target(
        "destination",
        "notes/destination.md",
        TargetKind::Memory,
        original_destination,
    );
    let source = fixture.target(
        "source",
        "notes/source.md",
        TargetKind::Memory,
        original_source,
    );
    let policy = Policy::new(vec![destination.clone(), source.clone()]).unwrap();

    let plan = plan_merge(&policy, true, "source", "destination", "curator:weekly").unwrap();
    assert_eq!(plan.kind, CurationKind::Merge);
    assert_eq!(plan.steps.len(), 2);
    assert!(plan.steps[0].proposal.content.contains("first idea"));
    assert!(plan.steps[0].proposal.content.contains("second idea"));
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let applied = plan
        .steps
        .iter()
        .map(|step| {
            changes
                .apply(
                    &policy,
                    true,
                    step.proposal.clone(),
                    &step.expected_before_hash,
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        changes.storage().read_backup(&applied[0]).unwrap(),
        original_destination
    );
    assert_eq!(
        changes.storage().read_backup(&applied[1]).unwrap(),
        original_source
    );
    assert!(fs::read_to_string(&source.path)
        .unwrap()
        .contains("Archived memory"));
    for record in applied.iter().rev() {
        changes.undo(&record.id).unwrap();
    }
    assert_eq!(fs::read(&destination.path).unwrap(), original_destination);
    assert_eq!(fs::read(&source.path).unwrap(), original_source);
}

#[test]
fn duplicate_skill_archives_only_duplicate_and_rejects_distinct_skill_merge() {
    let fixture = Fixture::new();
    let shared = b"---\nname: shared\ndescription: same\n---\nUse same instructions.\n";
    let first = fixture.target("first", "skills/first/SKILL.md", TargetKind::Skill, shared);
    let second = fixture.target(
        "second",
        "skills/second/SKILL.md",
        TargetKind::Skill,
        shared,
    );
    let policy = Policy::new(vec![first.clone(), second.clone()]).unwrap();
    let plan = plan_merge(&policy, true, "second", "first", "curator:weekly").unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].proposal.target_id, "second");
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let applied = changes
        .apply(
            &policy,
            true,
            plan.steps[0].proposal.clone(),
            &plan.steps[0].expected_before_hash,
        )
        .unwrap();
    assert_eq!(fs::read(&first.path).unwrap(), shared);
    changes.undo(&applied.id).unwrap();
    assert_eq!(fs::read(&second.path).unwrap(), shared);

    fs::write(&second.path, b"different skill").unwrap();
    assert!(matches!(
        plan_merge(&policy, true, "second", "first", "curator:weekly"),
        Err(CuratorError::IncompatibleSkill)
    ));
}

#[test]
fn cross_directory_memory_merge_fails_closed() {
    let fixture = Fixture::new();
    let first = fixture.target("first", "one/first.md", TargetKind::Memory, b"one\n");
    let second = fixture.target("second", "two/second.md", TargetKind::Memory, b"two\n");
    let policy = Policy::new(vec![first.clone(), second.clone()]).unwrap();
    assert!(matches!(
        plan_merge(&policy, true, "second", "first", "curator:weekly"),
        Err(CuratorError::CrossDirectory)
    ));
    fs::write(&second.path, b"one\n").unwrap();
    assert!(matches!(
        plan_merge(&policy, true, "second", "first", "curator:weekly"),
        Err(CuratorError::CrossDirectory)
    ));
    assert_eq!(fs::read(&first.path).unwrap(), b"one\n");
    assert_eq!(fs::read(&second.path).unwrap(), b"one\n");
}

#[test]
fn second_merge_step_conflict_leaves_first_step_auditable_and_recoverable() {
    let fixture = Fixture::new();
    let destination = fixture.target(
        "destination",
        "notes/destination.md",
        TargetKind::Memory,
        b"first\n",
    );
    let source = fixture.target("source", "notes/source.md", TargetKind::Memory, b"second\n");
    let policy = Policy::new(vec![destination.clone(), source.clone()]).unwrap();
    let plan = plan_merge(&policy, true, "source", "destination", "curator:weekly").unwrap();
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let first = changes
        .apply(
            &policy,
            true,
            plan.steps[0].proposal.clone(),
            &plan.steps[0].expected_before_hash,
        )
        .unwrap();
    fs::write(&source.path, b"human source edit\n").unwrap();
    assert!(matches!(
        changes.apply(
            &policy,
            true,
            plan.steps[1].proposal.clone(),
            &plan.steps[1].expected_before_hash,
        ),
        Err(ChangeError::Conflict)
    ));
    assert!(fs::read_to_string(&destination.path)
        .unwrap()
        .contains("second"));
    assert_eq!(fs::read(&source.path).unwrap(), b"human source edit\n");
    assert_eq!(
        changes.storage().get(&first.id).unwrap().unwrap().status,
        mercury_learn::storage::ChangeStatus::Applied
    );
    changes.undo(&first.id).unwrap();
    assert_eq!(fs::read(&destination.path).unwrap(), b"first\n");
    assert_eq!(fs::read(&source.path).unwrap(), b"human source edit\n");
}

#[test]
fn ordered_apply_reports_partial_ids_for_explicit_recovery() {
    let fixture = Fixture::new();
    let destination = fixture.target(
        "destination",
        "notes/destination.md",
        TargetKind::Memory,
        b"first\n",
    );
    let source = fixture.target("source", "notes/source.md", TargetKind::Memory, b"second\n");
    let policy = Policy::new(vec![destination.clone(), source.clone()]).unwrap();
    let mut plan = plan_merge(&policy, true, "source", "destination", "curator:weekly").unwrap();
    plan.steps[1].expected_before_hash = "0".repeat(64);
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let failure = apply_plan(&policy, || true, &mut changes, plan).unwrap_err();
    assert!(matches!(failure.error, ChangeError::Conflict));
    assert_eq!(failure.applied_ids.len(), 1);
    assert!(fs::read_to_string(&destination.path)
        .unwrap()
        .contains("second"));
    assert_eq!(fs::read(&source.path).unwrap(), b"second\n");
    changes.undo(&failure.applied_ids[0]).unwrap();
    assert_eq!(fs::read(&destination.path).unwrap(), b"first\n");
}

#[test]
fn disabling_curator_between_merge_steps_preserves_second_target() {
    let fixture = Fixture::new();
    let destination = fixture.target(
        "destination",
        "notes/destination.md",
        TargetKind::Memory,
        b"first\n",
    );
    let source = fixture.target("source", "notes/source.md", TargetKind::Memory, b"second\n");
    let policy = Policy::new(vec![destination.clone(), source.clone()]).unwrap();
    let plan = plan_merge(&policy, true, "source", "destination", "curator:weekly").unwrap();
    let calls = std::cell::Cell::new(0);
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let failure = apply_plan(
        &policy,
        || {
            calls.set(calls.get() + 1);
            calls.get() == 1
        },
        &mut changes,
        plan,
    )
    .unwrap_err();
    assert_eq!(calls.get(), 2);
    assert_eq!(failure.applied_ids.len(), 1);
    assert!(matches!(failure.error, ChangeError::Policy(_)));
    assert_eq!(fs::read(&source.path).unwrap(), b"second\n");
    changes.undo(&failure.applied_ids[0]).unwrap();
    assert_eq!(fs::read(&destination.path).unwrap(), b"first\n");
}

#[test]
fn curator_refuses_instruction_targets_unsafe_source_and_symlinked_memory() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let instructions = fixture.target(
        "instructions",
        "AGENTS.md",
        TargetKind::Instructions,
        b"rules\n",
    );
    let memory = fixture.target("memory", "notes/memory.md", TargetKind::Memory, b"memory\n");
    let other = fixture.target("other", "notes/other.md", TargetKind::Memory, b"other\n");
    let policy = Policy::new(vec![instructions, memory.clone(), other]).unwrap();
    assert!(matches!(
        plan_merge(&policy, true, "instructions", "memory", "curator:weekly"),
        Err(CuratorError::UnsupportedTarget)
    ));
    assert!(matches!(
        plan_merge(&policy, true, "memory", "other", "curator;rm"),
        Err(CuratorError::InvalidSource)
    ));
    let elsewhere = fixture.0.join("elsewhere.md");
    fs::write(&elsewhere, b"outside\n").unwrap();
    fs::remove_file(&memory.path).unwrap();
    symlink(&elsewhere, &memory.path).unwrap();
    assert!(matches!(
        plan_merge(&policy, true, "memory", "other", "curator:weekly"),
        Err(CuratorError::Change(_))
    ));
    assert_eq!(fs::read(&elsewhere).unwrap(), b"outside\n");
}

#[test]
fn two_target_ids_for_one_path_cannot_merge_the_canonical_target() {
    let fixture = Fixture::new();
    let first = fixture.target("first", "notes/shared.md", TargetKind::Memory, b"shared\n");
    let mut alias = first.clone();
    alias.id = "alias".into();
    let policy = Policy::new(vec![first.clone(), alias]).unwrap();

    assert!(matches!(
        plan_merge(&policy, true, "alias", "first", "curator:weekly"),
        Err(CuratorError::SameTarget)
    ));
    assert_eq!(fs::read(&first.path).unwrap(), b"shared\n");
    assert!(!fixture.data().exists());
}
