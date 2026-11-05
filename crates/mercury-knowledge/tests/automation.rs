#![cfg(unix)]
use mercury_knowledge::{
    automation::{BudgetedModel, Capture},
    backend::Model,
    *,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    index: Index,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-auto-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let index = Index::open(&root.join("data")).unwrap();
        let config = Config::default().prepare_at(&root.join("data")).unwrap();
        Self {
            root,
            index,
            config,
        }
    }
    fn capture(&self) -> Capture {
        Capture::new(
            "codex",
            "session-1",
            self.root.join("session.jsonl"),
            "a".repeat(64),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Fake {
    calls: AtomicUsize,
    fail: bool,
}
impl Model for Fake {
    fn fingerprint(&self) -> String {
        "auto-test-v1".into()
    }
    fn embed(&self, _: &str) -> Result<Vec<f32>> {
        Ok(vec![1., 0.])
    }
    fn structured(&self, _: &str, _: &Value, _: &Value) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            Err(Error::Unavailable)
        } else {
            Ok(json!({}))
        }
    }
}

#[test]
fn defaults_enable_local_scope_without_external_processing() {
    let f = Fixture::new();
    assert!(f.config.enabled);
    assert!(f.config.automation.enabled);
    assert!(!f.config.allow_external);
    assert!(f.config.vault.is_dir());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert!(backend::NativeModel::new(
        &f.config,
        &f.root.join("model"),
        &Default::default(),
        &[],
        None,
        &cancel
    )
    .is_err());
    assert!(!f.root.join("model").exists());
}
#[test]
fn durable_capture_deduplicates_across_reopen() {
    let f = Fixture::new();
    let c = f.capture();
    assert!(f.index.enqueue_capture(&c).unwrap());
    let reopened = Index::open(&f.root.join("data")).unwrap();
    assert!(!reopened.enqueue_capture(&c).unwrap());
    assert_eq!(reopened.pending_captures(now()).unwrap().len(), 1);
    assert_eq!(
        reopened.capture_record(&c.id).unwrap().unwrap().fingerprint,
        c.fingerprint
    );
}
#[test]
fn retry_deadline_and_completed_capture_survive_restart() {
    let f = Fixture::new();
    let mut c = f.capture();
    c.next_at = now() + 3600;
    f.index.enqueue_capture(&c).unwrap();
    assert!(f.index.pending_captures(now()).unwrap().is_empty());
    c.state = "done".into();
    f.index.save_capture(&c).unwrap();
    let reopened = Index::open(&f.root.join("data")).unwrap();
    assert!(reopened.pending_captures(now() + 7200).unwrap().is_empty());
    assert!(!reopened.enqueue_capture(&c).unwrap());
}
#[test]
fn invalid_capture_identity_is_rejected() {
    let f = Fixture::new();
    for (provider, id, path, fp) in [
        ("unknown", "session-1", f.root.join("x"), "a".repeat(64)),
        ("codex", "../escape", f.root.join("x"), "a".repeat(64)),
        (
            "codex",
            "session-1",
            PathBuf::from("relative"),
            "a".repeat(64),
        ),
        ("codex", "session-1", f.root.join("x"), "bad".into()),
    ] {
        assert!(Capture::new(provider, id, path, fp).is_err());
    }
}
#[test]
fn model_budget_is_durable_and_resets_on_next_utc_day() {
    let f = Fixture::new();
    let timestamp = 10 * 86400;
    f.index.reserve_automatic_call(timestamp, 4).unwrap();
    let reopened = Index::open(&f.root.join("data")).unwrap();
    reopened.reserve_automatic_call(timestamp, 4).unwrap();
    assert!(f.index.reserve_automatic_call(timestamp, 4).is_err());
    f.index
        .reserve_automatic_call(timestamp + 86400, 4)
        .unwrap();
}
#[test]
fn failed_model_calls_consume_budget_but_local_vectors_do_not() {
    let f = Fixture::new();
    let fake = Fake {
        calls: AtomicUsize::new(0),
        fail: true,
    };
    let model = BudgetedModel {
        model: &fake,
        index: &f.index,
        daily_limit: 2,
    };
    model.embed("note").unwrap();
    assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
    assert!(model.structured("extract", &json!({}), &json!({})).is_err());
    assert!(model.structured("extract", &json!({}), &json!({})).is_err());
    assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn recognizable_secret_blocks_automatic_model_input_without_spending_budget() {
    let f = Fixture::new();
    let fake = Fake {
        calls: AtomicUsize::new(0),
        fail: false,
    };
    let model = BudgetedModel {
        model: &fake,
        index: &f.index,
        daily_limit: 2,
    };
    assert!(model
        .structured(
            "extract",
            &json!({"text":"authorization: bearer fixture-only"}),
            &json!({})
        )
        .is_err());
    assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        f.index.automation_status().unwrap()["reserved_calls_today"],
        0
    );
}
#[test]
fn automatic_publication_stays_proposed_and_has_conflict_aware_undo() {
    let f = Fixture::new();
    let p = f
        .index
        .propose_automatic(
            "auto:observed:fixture",
            "Lesson",
            "project",
            "Use [[Queue]] for retries.",
        )
        .unwrap();
    assert!(p.markdown.contains("status: proposed"));
    let accepted = f.index.review(&f.config, &p.id, "accept").unwrap();
    assert_eq!(accepted.state, "accepted");
    let path = f.config.vault.join("Mercury").join(format!("{}.md", p.id));
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("status: proposed"));
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    assert_eq!(
        f.index
            .proposal_by_source("auto:observed:fixture")
            .unwrap()
            .unwrap()
            .id,
        p.id
    );
    fs::write(&path, "human edit").unwrap();
    assert!(matches!(
        f.index.review(&f.config, &p.id, "undo"),
        Err(Error::Conflict)
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "human edit");
}
#[test]
fn automatic_note_can_be_undone_without_touching_other_notes() {
    let f = Fixture::new();
    fs::write(f.config.vault.join("Human.md"), "keep").unwrap();
    let p = f
        .index
        .propose_automatic("auto:observed:fixture", "Lesson", "project", "Lesson.")
        .unwrap();
    f.index.review(&f.config, &p.id, "accept").unwrap();
    f.index.review(&f.config, &p.id, "undo").unwrap();
    assert!(!f
        .config
        .vault
        .join("Mercury")
        .join(format!("{}.md", p.id))
        .exists());
    assert_eq!(
        fs::read_to_string(f.config.vault.join("Human.md")).unwrap(),
        "keep"
    );
}
#[test]
fn explicit_rebuild_retries_failed_work_preserving_its_proposal() {
    let f = Fixture::new();
    let mut c = f.capture();
    f.index.enqueue_capture(&c).unwrap();
    c.state = "failed".into();
    c.attempts = 5;
    c.proposal_id = Some("b".repeat(64));
    f.index.save_capture(&c).unwrap();
    f.index.retry_automation().unwrap();
    let requeued = f.index.pending_captures(now()).unwrap().pop().unwrap();
    assert_eq!(requeued.proposal_id, c.proposal_id);
    assert_eq!(requeued.attempts, 0);
}
#[test]
fn status_excludes_transcript_paths_and_session_identifiers() {
    let f = Fixture::new();
    let mut c = f.capture();
    f.index.enqueue_capture(&c).unwrap();
    c.state = "failed".into();
    c.error = Some("knowledge model unavailable".into());
    f.index.save_capture(&c).unwrap();
    let status = f.index.automation_status().unwrap().to_string();
    assert!(!status.contains(f.root.to_str().unwrap()));
    assert!(!status.contains("session-1"));
    assert!(status.contains("knowledge model unavailable"));
}
