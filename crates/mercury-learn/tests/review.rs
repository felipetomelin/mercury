#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use mercury_learn::pipeline::{review_session, SessionReview};
use mercury_learn::policy::{Policy, Proposal, TargetKind, TargetPolicy};
use mercury_learn::reviewer::{ClaudeReviewer, ReviewerConfig};
use mercury_learn::storage::{Storage, TriggerStatus};
use mercury_learn::transcript::Provider;
use mercury_learn::{review_snapshots, Changes};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn one_claim_survives_restart_and_one_review_remains_reversible() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("mercury-learn-review-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

    let target = root.join("AGENTS.md");
    fs::write(&target, b"Before\n").unwrap();
    let transcript = root.join("session-1.jsonl");
    fs::write(
        &transcript,
        format!(
            "{}\n{}\n",
            json!({"type":"user","sessionId":"session-1","uuid":"u1","message":{"role":"user","content":"Remember this preference"}}),
            json!({"type":"assistant","sessionId":"session-1","requestId":"r1","message":{"role":"assistant","id":"a1","content":[{"type":"text","text":"Understood"}]}}),
        ),
    )
    .unwrap();
    let policy = Policy::new(vec![TargetPolicy {
        id: "instructions".into(),
        path: target.clone(),
        kind: TargetKind::Instructions,
        max_bytes: 4096,
    }])
    .unwrap();

    let invocations = root.join("invocations");
    let script = root.join("fake-claude");
    let response = json!({
        "type":"result", "subtype":"success", "is_error":false,
        "total_cost_usd":0.01,
        "structured_output":{"proposals":[{
            "target_id":"instructions", "content":"After\n",
            "reason":"Confirmed preference", "evidence":"User requested it"
        }]}
    });
    let quoted_response = response.to_string().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nprintf '%s\\n' '{quoted_response}'\n",
            invocations.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let reviewer = ClaudeReviewer::new(ReviewerConfig {
        executable: script,
        workdir: root.clone(),
        model: "haiku".into(),
        max_budget_usd: 0.1,
        timeout: Duration::from_secs(2),
        max_input_bytes: 128 * 1024,
        max_stdout_bytes: 8 * 1024,
        max_stderr_bytes: 1024,
    })
    .unwrap();
    let ledger = root.join("learn");
    let key = "claude:session-1:end";
    let mut storage = Storage::open(&ledger).unwrap();
    assert!(storage.claim_trigger(key).unwrap());
    let mut changes = Changes::new(storage);
    let result = review_session(
        SessionReview {
            provider: Provider::Claude,
            session_id: "session-1",
            transcript_path: &transcript,
            milestone_turns: None,
        },
        &policy,
        || true,
        &reviewer,
        &mut changes,
    )
    .await
    .unwrap();
    assert_eq!(result.applied_ids.len(), 1);
    assert_eq!(fs::read(&target).unwrap(), b"After\n");
    drop(changes);

    let mut reopened = Storage::open(&ledger).unwrap();
    reopened
        .finish_trigger(key, TriggerStatus::Succeeded)
        .unwrap();
    assert!(!reopened.claim_trigger(key).unwrap());
    assert_eq!(
        reopened.trigger_status(key).unwrap(),
        Some(TriggerStatus::Succeeded)
    );
    assert_eq!(fs::read(&invocations).unwrap(), b"x");
    let mut changes = Changes::new(reopened);
    changes.undo(&result.applied_ids[0]).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"Before\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn route_section_change_is_reversible_without_touching_unrelated_config() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("mercury-learn-route-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.join("config.toml");
    let original = b"# untouched\n[theme]\nname = 'test'\n[route]\nenabled = false\n";
    fs::write(&config, original).unwrap();
    let policy = Policy::new(vec![TargetPolicy {
        id: "routing".into(),
        path: config.clone(),
        kind: TargetKind::ConfigSection,
        max_bytes: 4096,
    }])
    .unwrap();
    let snapshot = review_snapshots(&policy, true).unwrap().remove(0);
    assert_eq!(snapshot.content, "[route]\nenabled = false\n");
    let mut changes = Changes::new(Storage::open(&root.join("learn")).unwrap());
    let record = changes
        .apply(
            &policy,
            true,
            Proposal {
                target_id: "routing".into(),
                content: "[route]\nenabled = true\n".into(),
                reason: "Prefer route selection".into(),
                source: "claude:session-1".into(),
            },
            &snapshot.before_hash,
        )
        .unwrap();
    let changed = fs::read_to_string(&config).unwrap();
    assert!(changed.starts_with("# untouched\n[theme]\nname = 'test'\n"));
    assert!(changed.contains("[route]\nenabled = true"));
    changes.undo(&record.id).unwrap();
    assert_eq!(fs::read(&config).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cron_job_patch_is_reversible_and_scheduler_reloads_it() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("mercury-learn-cron-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let jobs = root.join("jobs.toml");
    let now = time::OffsetDateTime::now_utc();
    let mut scheduler = mercury_cron::scheduler::Scheduler::open(
        &jobs,
        &root.join("executions.db"),
        &root.join("scheduler.lock"),
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
    let original = fs::read(&jobs).unwrap();
    let policy = Policy::new(vec![TargetPolicy {
        id: job_id.clone(),
        path: jobs.clone(),
        kind: TargetKind::CronJob,
        max_bytes: 4096,
    }])
    .unwrap();
    let snapshot = review_snapshots(&policy, true).unwrap().remove(0);
    assert!(!snapshot.content.contains("/usr/bin/true"));
    let mut changes = Changes::new(Storage::open(&root.join("learn")).unwrap());
    let proposal = Proposal {
        target_id: job_id.clone(),
        content: r#"{"enabled":false,"timeout_seconds":60}"#.into(),
        reason: "avoid wasted runs".into(),
        source: "claude:session-1".into(),
    };
    let active = scheduler.run_now(&job_id, now).unwrap();
    assert!(changes
        .apply(&policy, true, proposal.clone(), &snapshot.before_hash)
        .is_err());
    assert_eq!(fs::read(&jobs).unwrap(), original);
    scheduler
        .finish(&active, now, true, None, None, None)
        .unwrap();
    let record = changes
        .apply(&policy, true, proposal, &snapshot.before_hash)
        .unwrap();
    // A scheduler mutation must first ingest Learn's file change, not save a
    // stale in-memory copy over it.
    scheduler.pause(&job_id, true).unwrap();
    scheduler.reload_jobs().unwrap();
    assert!(!scheduler.jobs()[0].enabled);
    assert_eq!(scheduler.jobs()[0].timeout_seconds, 60);
    let active = scheduler.run_now(&job_id, now).unwrap();
    assert!(changes.undo(&record.id).is_err());
    scheduler
        .finish(&active, now, true, None, None, None)
        .unwrap();
    changes.undo(&record.id).unwrap();
    scheduler.reload_jobs().unwrap();
    assert!(scheduler.jobs()[0].enabled);
    assert_eq!(fs::read(&jobs).unwrap(), original);
    drop(scheduler);
    fs::remove_dir_all(root).unwrap();
}
