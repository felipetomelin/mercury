use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};

use mercury_learn::metrics::{ComparisonState, MetricPhase, TaskObservation, TaskOutcome};
#[cfg(unix)]
use mercury_learn::policy::{Policy, Proposal, TargetKind, TargetPolicy};
use mercury_learn::storage::{Storage, StorageError};
use mercury_learn::transcript::Provider;
#[cfg(unix)]
use mercury_learn::{review_snapshots, Changes};
use rusqlite::params;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-metrics-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn data(&self) -> PathBuf {
        self.0.join("learn")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn sample<'a>(task_key: &'a str, phase: MetricPhase) -> TaskObservation<'a> {
    TaskObservation {
        observation_id: match phase {
            MetricPhase::Observed => "observed-1",
            MetricPhase::Baseline => "baseline-1",
            MetricPhase::After => "after-1",
        },
        task_key,
        provider: Provider::Claude,
        phase,
        input_tokens: 100,
        output_tokens: 20,
        cost_microusd: Some(400),
        outcome: Some(TaskOutcome::Succeeded),
        observed_at_ms: 1_000,
    }
}

#[test]
fn comparable_outcomes_require_same_task_provider_and_known_outcomes() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let empty = storage.task_comparison("task-1", Provider::Claude).unwrap();
    assert_eq!(empty.state, ComparisonState::InsufficientData);
    assert_eq!(empty.baseline.samples, 0);

    storage
        .record_task_observation(sample("task-1", MetricPhase::Baseline))
        .unwrap();
    assert_eq!(
        storage
            .task_comparison("task-1", Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
    let mut after = sample("task-1", MetricPhase::After);
    after.input_tokens = 80;
    after.output_tokens = 10;
    after.cost_microusd = None;
    after.outcome = Some(TaskOutcome::Failed);
    storage.record_task_observation(after).unwrap();
    drop(storage);

    let storage = Storage::open(&fixture.data()).unwrap();
    let compared = storage.task_comparison("task-1", Provider::Claude).unwrap();
    assert_eq!(compared.state, ComparisonState::ComparableObserved);
    assert_eq!(compared.baseline.input_tokens, 100);
    assert_eq!(compared.after.input_tokens, 80);
    assert_eq!(compared.after.failed, 1);
    assert_eq!(compared.after.cost_microusd, None);
    assert_eq!(
        storage
            .task_comparison("task-1", Provider::Codex)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
    assert_eq!(
        storage
            .task_comparison("task-2", Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
}

#[test]
fn unclassified_observed_runs_survive_restart_without_implying_improvement() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let observation = sample("cron:job-1", MetricPhase::Observed);
    assert!(storage.record_task_observation(observation).unwrap());
    drop(storage);

    let mut reopened = Storage::open(&fixture.data()).unwrap();
    assert!(!reopened.record_task_observation(observation).unwrap());
    let comparison = reopened
        .task_comparison("cron:job-1", Provider::Claude)
        .unwrap();
    assert_eq!(comparison.observed.samples, 1);
    assert_eq!(comparison.observed.input_tokens, 100);
    assert_eq!(comparison.baseline.samples, 0);
    assert_eq!(comparison.after.samples, 0);
    assert_eq!(comparison.state, ComparisonState::InsufficientData);
}

#[cfg(unix)]
#[test]
fn explicit_job_target_uses_only_verified_applied_change_as_boundary() {
    let fixture = Fixture::new();
    let target = fixture.0.join("AGENTS.md");
    fs::write(&target, b"Before\n").unwrap();
    let policy = Policy::new(vec![TargetPolicy {
        id: "project".into(),
        path: target.clone(),
        kind: TargetKind::Instructions,
        max_bytes: 4096,
    }])
    .unwrap();
    let key = format!(
        "cron:123e4567-e89b-12d3-a456-426614174000:target:project:{}",
        mercury_learn::metrics::target_path_fingerprint(&target).unwrap()
    );
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut before = sample(&key, MetricPhase::Observed);
    before.observation_id = "before-run";
    before.observed_at_ms = now_ms() - 1000;
    storage.record_task_observation(before).unwrap();
    drop(storage);

    let snapshot = review_snapshots(&policy, true).unwrap().remove(0);
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    let change = changes
        .apply(
            &policy,
            true,
            Proposal {
                target_id: "project".into(),
                content: "After\n".into(),
                reason: "Confirmed preference".into(),
                source: "claude:session-1".into(),
            },
            &snapshot.before_hash,
        )
        .unwrap();
    let mut after = sample(&key, MetricPhase::Observed);
    after.observation_id = "after-run";
    after.observed_at_ms = now_ms() + 1000;
    after.input_tokens = 50;
    drop(changes);
    let mut storage = Storage::open(&fixture.data()).unwrap();
    storage.record_task_observation(after).unwrap();
    let compared = storage.task_comparison(&key, Provider::Claude).unwrap();
    assert_eq!(compared.state, ComparisonState::ComparableObserved);
    assert_eq!(compared.baseline.input_tokens, 100);
    assert_eq!(compared.after.input_tokens, 50);
    drop(storage);

    fs::write(&target, b"human edit\n").unwrap();
    let storage = Storage::open(&fixture.data()).unwrap();
    assert_eq!(
        storage
            .task_comparison(&key, Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
    drop(storage);
    fs::write(&target, b"After\n").unwrap();
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    changes.undo(&change.id).unwrap();
    assert_eq!(
        changes
            .storage()
            .task_comparison(&key, Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
}

#[cfg(unix)]
#[test]
fn cron_target_comparison_ignores_run_cursor_but_not_job_definition_changes() {
    let fixture = Fixture::new();
    let jobs = fixture.0.join("jobs.toml");
    let now = time::OffsetDateTime::now_utc();
    let mut scheduler = mercury_cron::scheduler::Scheduler::open(
        &jobs,
        &fixture.0.join("executions.db"),
        &fixture.0.join("scheduler.lock"),
        now,
    )
    .unwrap();
    let job = mercury_cron::Job::new(
        "fixture".into(),
        "every 1h",
        time::UtcOffset::UTC,
        now,
        mercury_cron::JobTarget::Script {
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        },
    )
    .unwrap();
    let job_id = job.id.clone();
    scheduler.add(job).unwrap();
    let key = format!(
        "cron:{job_id}:target:{job_id}:{}",
        mercury_learn::metrics::target_path_fingerprint(&jobs).unwrap()
    );
    let policy = Policy::new(vec![TargetPolicy {
        id: job_id.clone(),
        path: jobs,
        kind: TargetKind::CronJob,
        max_bytes: 4096,
    }])
    .unwrap();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut before = sample(&key, MetricPhase::Observed);
    before.observation_id = "before-cron";
    before.observed_at_ms = now_ms() - 1000;
    storage.record_task_observation(before).unwrap();
    drop(storage);
    let snapshot = review_snapshots(&policy, true).unwrap().remove(0);
    let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
    changes
        .apply(
            &policy,
            true,
            Proposal {
                target_id: job_id.clone(),
                content: r#"{"timeout_seconds":60}"#.into(),
                reason: "bound execution".into(),
                source: "claude:session-1".into(),
            },
            &snapshot.before_hash,
        )
        .unwrap();
    drop(changes);
    scheduler.reload_jobs().unwrap();
    let mut updated = scheduler.jobs()[0].clone();
    updated.next_run_at = updated.next_run_at.map(|at| at + 3600);
    scheduler.edit(updated.clone()).unwrap();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut after = sample(&key, MetricPhase::Observed);
    after.observation_id = "after-cron";
    after.observed_at_ms = now_ms() + 1000;
    storage.record_task_observation(after).unwrap();
    assert_eq!(
        storage
            .task_comparison(&key, Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::ComparableObserved
    );
    drop(storage);
    updated.target = mercury_cron::JobTarget::Script {
        program: "/usr/bin/false".into(),
        args: Vec::new(),
    };
    scheduler.edit(updated).unwrap();
    assert_eq!(
        Storage::open(&fixture.data())
            .unwrap()
            .task_comparison(&key, Provider::Claude)
            .unwrap()
            .state,
        ComparisonState::InsufficientData
    );
}

#[test]
fn recent_observed_metrics_are_bounded_ordered_and_exclude_other_phases() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    storage
        .record_task_observation(sample("cron:old", MetricPhase::Baseline))
        .unwrap();
    for index in 0..3 {
        let mut observation = sample("cron:job", MetricPhase::Observed);
        observation.observation_id = match index {
            0 => "run-0",
            1 => "run-1",
            _ => "run-2",
        };
        observation.input_tokens = 10 + index;
        storage.record_task_observation(observation).unwrap();
    }
    let recent = storage.recent_observed_metrics(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].input_tokens, 12);
    assert_eq!(recent[1].input_tokens, 11);
    assert!(recent.iter().all(|row| row.task_key == "cron:job"));
    assert!(matches!(
        storage.recent_observed_metrics(17),
        Err(StorageError::InvalidInput)
    ));
}

#[test]
fn version_two_metric_rows_survive_observed_phase_migration() {
    let fixture = Fixture::new();
    drop(Storage::open(&fixture.data()).unwrap());
    let connection = rusqlite::Connection::open(fixture.data().join("ledger.sqlite")).unwrap();
    connection
        .execute_batch(
            "DROP TABLE task_metrics;
             CREATE TABLE task_metrics (
                id INTEGER PRIMARY KEY,
                observation_id TEXT NOT NULL,
                task_key TEXT NOT NULL,
                provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
                phase TEXT NOT NULL CHECK(phase IN ('baseline','after')),
                input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0),
                output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
                cost_microusd INTEGER CHECK(cost_microusd >= 0),
                outcome TEXT CHECK(outcome IN ('succeeded','failed')),
                observed_at_ms INTEGER NOT NULL,
                UNIQUE(provider,observation_id)
             );
             INSERT INTO task_metrics
                (observation_id,task_key,provider,phase,input_tokens,output_tokens,cost_microusd,outcome,observed_at_ms)
             VALUES ('old-1','cron:job-1','claude','baseline',50,10,250,'succeeded',1);
             PRAGMA user_version=2;",
        )
        .unwrap();
    drop(connection);

    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut observed = sample("cron:job-1", MetricPhase::Observed);
    observed.observation_id = "new-1";
    assert!(storage.record_task_observation(observed).unwrap());
    let comparison = storage
        .task_comparison("cron:job-1", Provider::Claude)
        .unwrap();
    assert_eq!(comparison.baseline.samples, 1);
    assert_eq!(comparison.baseline.cost_microusd, Some(250));
    assert_eq!(comparison.observed.samples, 1);
    assert_eq!(comparison.state, ComparisonState::InsufficientData);
}

#[test]
fn unknown_outcome_and_partial_cost_are_not_imputed() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    storage
        .record_task_observation(sample("task-1", MetricPhase::Baseline))
        .unwrap();
    let mut unknown = sample("task-1", MetricPhase::After);
    unknown.outcome = None;
    unknown.cost_microusd = None;
    storage.record_task_observation(unknown).unwrap();
    let mut observed = sample("task-1", MetricPhase::After);
    observed.observation_id = "after-2";
    observed.cost_microusd = Some(0);
    storage.record_task_observation(observed).unwrap();

    let compared = storage.task_comparison("task-1", Provider::Claude).unwrap();
    assert_eq!(compared.state, ComparisonState::InsufficientData);
    assert_eq!(compared.after.samples, 2);
    assert_eq!(compared.after.unknown_outcomes, 1);
    assert_eq!(compared.after.cost_microusd, None);
}

#[test]
fn repeated_observation_is_idempotent_and_conflicting_replay_is_rejected() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let first = sample("task-1", MetricPhase::Baseline);
    assert!(storage.record_task_observation(first).unwrap());
    drop(storage);

    let mut storage = Storage::open(&fixture.data()).unwrap();
    assert!(!storage.record_task_observation(first).unwrap());
    let mut conflict = first;
    conflict.input_tokens = 101;
    assert!(matches!(
        storage.record_task_observation(conflict),
        Err(StorageError::InvalidInput)
    ));
    let mut changed_time = first;
    changed_time.observed_at_ms += 1;
    assert!(matches!(
        storage.record_task_observation(changed_time),
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(
        storage
            .task_comparison("task-1", Provider::Claude)
            .unwrap()
            .baseline
            .samples,
        1
    );
}

#[test]
fn invalid_samples_are_rejected_without_leaking_task_text() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    for key in ["", "../escape", "task with text", "a/b"] {
        assert!(matches!(
            storage.record_task_observation(sample(key, MetricPhase::Baseline)),
            Err(StorageError::InvalidInput)
        ));
    }
    let mut oversized = sample("task-1", MetricPhase::Baseline);
    oversized.input_tokens = 10_000_001;
    assert!(matches!(
        storage.record_task_observation(oversized),
        Err(StorageError::InvalidInput)
    ));
    let mut invalid_time = sample("task-1", MetricPhase::Baseline);
    invalid_time.observed_at_ms = -1;
    assert!(matches!(
        storage.record_task_observation(invalid_time),
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(
        storage
            .task_comparison("task-1", Provider::Claude)
            .unwrap()
            .baseline
            .samples,
        0
    );
}

#[test]
fn oldest_samples_are_pruned_at_the_private_ledger_limit() {
    let fixture = Fixture::new();
    let mut storage = Storage::open(&fixture.data()).unwrap();
    let mut direct = rusqlite::Connection::open(fixture.data().join("ledger.sqlite")).unwrap();
    let transaction = direct.transaction().unwrap();
    for index in 0..4096 {
        transaction
            .execute(
                "INSERT INTO task_metrics
                    (observation_id,task_key,provider,phase,input_tokens,output_tokens,cost_microusd,outcome,observed_at_ms)
                 VALUES (?1,'old-task','claude','baseline',1,0,0,'succeeded',?2)",
                params![format!("old-{index}"), index],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    storage
        .record_task_observation(sample("new-task", MetricPhase::After))
        .unwrap();
    let count: i64 = direct
        .query_row("SELECT COUNT(*) FROM task_metrics", [], |row| row.get(0))
        .unwrap();
    let oldest: i64 = direct
        .query_row("SELECT MIN(id) FROM task_metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 4096);
    assert_eq!(oldest, 2);
    assert_eq!(
        storage
            .task_comparison("old-task", Provider::Claude)
            .unwrap()
            .baseline
            .samples,
        4095
    );
}
