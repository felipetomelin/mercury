#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mercury_learn::pipeline::{review_session, PipelineError, SessionReview};
use mercury_learn::policy::{Policy, TargetKind, TargetPolicy};
use mercury_learn::reviewer::{ClaudeReviewer, ReviewerConfig};
use mercury_learn::storage::Storage;
use mercury_learn::transcript::Provider;
use mercury_learn::{ChangeError, Changes};
use serde_json::json;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    target: PathBuf,
    transcript: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-pipeline-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("AGENTS.md");
        fs::write(&target, b"Before").unwrap();
        let transcript = root.join("session-1.jsonl");
        let events = [
            json!({"type":"user","sessionId":"session-1","uuid":"u1","message":{"role":"user","content":"Remember the confirmed preference"}}),
            json!({"type":"assistant","sessionId":"session-1","requestId":"r1","message":{"role":"assistant","id":"a1","content":[{"type":"text","text":"I will remember it"}]}}),
        ];
        let contents = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&transcript, format!("{contents}\n")).unwrap();
        let script = root.join("fake-claude");
        Self {
            root,
            target,
            transcript,
            script,
        }
    }

    fn policy(&self) -> Policy {
        Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: self.target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 4096,
        }])
        .unwrap()
    }

    fn changes(&self) -> Changes {
        Changes::new(Storage::open(&self.root.join("learn")).unwrap())
    }

    fn session(&self, milestone_turns: Option<usize>) -> SessionReview<'_> {
        SessionReview {
            provider: Provider::Claude,
            session_id: "session-1",
            transcript_path: &self.transcript,
            milestone_turns,
        }
    }

    fn reviewer(&self, before_result: &str) -> ClaudeReviewer {
        let response = json!({
            "type":"result", "subtype":"success", "is_error":false,
            "total_cost_usd":0.01,
            "structured_output":{"proposals":[{
                "target_id":"instructions", "content":"After",
                "reason":"Remember the preference", "evidence":"The user requested it"
            }]}
        });
        let quoted_response = response.to_string().replace('\'', "'\\''");
        let body = format!("#!/bin/sh\n{before_result}\nprintf '%s\\n' '{quoted_response}'\n");
        fs::write(&self.script, body).unwrap();
        fs::set_permissions(&self.script, fs::Permissions::from_mode(0o700)).unwrap();
        ClaudeReviewer::new(ReviewerConfig {
            executable: self.script.clone(),
            workdir: self.root.clone(),
            model: "haiku".into(),
            max_budget_usd: 0.1,
            timeout: Duration::from_secs(2),
            max_input_bytes: 128 * 1024,
            max_stdout_bytes: 8 * 1024,
            max_stderr_bytes: 1024,
        })
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn completed_fixture_session_applies_and_undoes_exact_original_bytes() {
    let fixture = Fixture::new();
    let mut changes = fixture.changes();
    let outcome = review_session(
        fixture.session(None),
        &fixture.policy(),
        || true,
        &fixture.reviewer(":"),
        &mut changes,
    )
    .await
    .unwrap();

    assert_eq!(outcome.applied_ids.len(), 1);
    assert_eq!(outcome.observed_cost_usd, 0.01);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"After");
    changes.undo(&outcome.applied_ids[0]).unwrap();
    assert_eq!(fs::read(&fixture.target).unwrap(), b"Before");
}

#[tokio::test]
async fn disabled_or_wrong_turn_count_never_invokes_reviewer_or_writes() {
    let fixture = Fixture::new();
    let marker = fixture.root.join("called");
    let script = format!("touch '{}'", marker.display());
    let reviewer = fixture.reviewer(&script);
    let mut changes = fixture.changes();

    assert!(matches!(
        review_session(
            fixture.session(None),
            &fixture.policy(),
            || false,
            &reviewer,
            &mut changes
        )
        .await,
        Err(PipelineError::Disabled)
    ));
    assert!(matches!(
        review_session(
            fixture.session(Some(2)),
            &fixture.policy(),
            || true,
            &reviewer,
            &mut changes
        )
        .await,
        Err(PipelineError::InvalidMilestone)
    ));
    assert!(!marker.exists());
    assert_eq!(fs::read(&fixture.target).unwrap(), b"Before");
}

#[tokio::test]
async fn human_edit_during_review_is_preserved_as_conflict() {
    let fixture = Fixture::new();
    let script = format!("printf 'Human edit' > '{}'", fixture.target.display());
    let reviewer = fixture.reviewer(&script);
    let mut changes = fixture.changes();

    let result = review_session(
        fixture.session(None),
        &fixture.policy(),
        || true,
        &reviewer,
        &mut changes,
    )
    .await;

    assert!(matches!(
        result,
        Err(PipelineError::Apply {
            applied: 0,
            error: ChangeError::Conflict
        })
    ));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"Human edit");
    assert_eq!(
        fs::read_dir(fixture.root.join("learn/backups"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn disabling_learn_during_review_prevents_application() {
    let fixture = Fixture::new();
    let disabled = fixture.root.join("disabled");
    let script = format!("touch '{}'", disabled.display());
    let reviewer = fixture.reviewer(&script);
    let mut changes = fixture.changes();

    let result = review_session(
        fixture.session(None),
        &fixture.policy(),
        || !disabled.exists(),
        &reviewer,
        &mut changes,
    )
    .await;

    assert!(matches!(result, Err(PipelineError::Disabled)));
    assert_eq!(fs::read(&fixture.target).unwrap(), b"Before");
    assert_eq!(
        fs::read_dir(fixture.root.join("learn/backups"))
            .unwrap()
            .count(),
        0
    );
}
