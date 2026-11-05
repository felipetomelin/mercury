#![cfg(unix)]
use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::atomic::AtomicUsize};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    config: Config,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-auto-worker-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut config = Config::default()
            .prepare_at(&root.join("knowledge"))
            .unwrap();
        config.automation.poll_secs = 1;
        config.automation.debounce_secs = 1;
        config.automation.retry_secs = 1;
        let path = root.join("session-1.jsonl");
        let lines = [
            json!({"type":"user","sessionId":"session-1","uuid":"u1","cwd":"/fixture/project","message":{"role":"user","content":"Remember: use Queue for retries."}}),
            json!({"type":"assistant","sessionId":"session-1","requestId":"r1","message":{"role":"assistant","id":"a1","content":[{"type":"text","text":"Use Queue for retries."}]}}),
        ];
        fs::write(
            &path,
            lines.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        Self { root, config, path }
    }
    fn index(&self) -> Index {
        Index::open(&self.root.join("knowledge")).unwrap()
    }
    fn entry(&self) -> Capture {
        Capture::new(
            "claude",
            "session-1",
            self.path.clone(),
            mercury_learn::storage::transcript_fingerprint(&self.path).unwrap(),
        )
        .unwrap()
    }
    fn session(&self) -> crate::app::learn::ReviewSession {
        crate::app::learn::ReviewTrigger::session_end(
            mercury_learn::transcript::Provider::Claude,
            "session-1",
            &self.path,
        )
        .unwrap()
        .session()
        .unwrap()
        .clone()
    }
    fn native_stub(&mut self) {
        let path = self.root.join("fake-claude");
        fs::write(&path,r##"#!/bin/sh
if [ "$1" = --version ]; then printf 'Claude fixture 1\n'; exit 0; fi
IFS= read -r input
case "$input" in
  *'"task":"propose '*) output='{"markdown":"# A reusable lesson\nUse [[Queue]] for retries."}' ;;
  *'"task":"extract '*) output='{"keywords":[],"entities":[],"relations":[]}' ;;
  *) output='{"insufficient":true,"claims":[]}' ;;
esac
printf '{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01,"structured_output":%s}\n' "$output"
"##).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        self.config.allow_external = true;
        self.config.allowed_providers = vec!["claude".into()];
        self.config.executables.insert("claude".into(), path);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Fake {
    calls: AtomicUsize,
}
impl Model for Fake {
    fn fingerprint(&self) -> String {
        "capture-fixture-v1".into()
    }
    fn embed(&self, _: &str) -> Result<Vec<f32>> {
        Ok(vec![1., 0.])
    }
    fn structured(&self, task: &str, _: &Value, _: &Value) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if task.starts_with("propose") {
            Ok(json!({"markdown":"Use [[Queue]] for retries."}))
        } else if task.starts_with("extract") {
            Ok(json!({"keywords":[],"entities":[],"relations":[]}))
        } else {
            Ok(json!({"insufficient":true,"claims":[]}))
        }
    }
}
fn wait(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "condition did not become true");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn capture_to_note_to_index_to_graph_and_replay_is_idempotent() {
    let f = Fixture::new();
    let mut index = f.index();
    let entry = f.entry();
    index.enqueue_capture(&entry).unwrap();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    let cancel = AtomicBool::new(false);
    process_captures(
        &mut index,
        &f.config,
        std::slice::from_ref(&entry),
        &fake,
        &cancel,
    )
    .unwrap();
    assert_eq!(index.status().unwrap().notes, 1);
    let proposal = index.proposals().unwrap().pop().unwrap();
    assert!(proposal.markdown.contains("status: proposed"));
    index
        .build_graph_verified(&fake, &cancel, &f.config)
        .unwrap();
    assert!(index.status().unwrap().model.is_some());
    let count = fake.calls.load(Ordering::Relaxed);
    drop(index);
    let mut index = f.index();
    let pending = index.pending_captures(mercury_knowledge::now()).unwrap();
    process_captures(&mut index, &f.config, &pending, &fake, &cancel).unwrap();
    assert_eq!(fake.calls.load(Ordering::Relaxed), count);
    assert_eq!(index.proposals().unwrap().len(), 1);
}
#[test]
fn restart_after_proposal_persistence_publishes_without_another_model_call() {
    let f = Fixture::new();
    let index = f.index();
    let entry = f.entry();
    index.enqueue_capture(&entry).unwrap();
    let source = format!(
        "auto:observed:{}:{}:{}",
        entry.provider, entry.session_id, entry.fingerprint
    );
    index
        .propose_automatic(&source, "Lesson", "project", "Use Queue.")
        .unwrap();
    drop(index);
    let mut index = f.index();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    process_captures(
        &mut index,
        &f.config,
        &[entry],
        &fake,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
    assert_eq!(index.status().unwrap().notes, 1);
}
#[test]
fn changed_transcript_or_human_edited_note_is_never_overwritten() {
    let f = Fixture::new();
    let mut index = f.index();
    let entry = f.entry();
    index.enqueue_capture(&entry).unwrap();
    fs::write(&f.path, "changed").unwrap();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    process_captures(
        &mut index,
        &f.config,
        std::slice::from_ref(&entry),
        &fake,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        index.capture_record(&entry.id).unwrap().unwrap().state,
        "failed"
    );
    assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
}
#[test]
fn cancellation_before_capture_leaves_queue_for_restart() {
    let f = Fixture::new();
    let mut index = f.index();
    let entry = f.entry();
    index.enqueue_capture(&entry).unwrap();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    assert!(matches!(
        process_captures(
            &mut index,
            &f.config,
            std::slice::from_ref(&entry),
            &fake,
            &AtomicBool::new(true)
        ),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        index.capture_record(&entry.id).unwrap().unwrap().state,
        "queued"
    );
    assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
}
#[test]
fn worker_indexes_changes_and_deletions_locally_without_cli() {
    let f = Fixture::new();
    let mut worker = AutomationWorker::start_at(
        f.root.join("knowledge"),
        f.config.clone(),
        Default::default(),
        vec![],
    )
    .unwrap();
    let path = f.config.vault.join("Note.md");
    fs::write(&path, "# Note\nfirst-token").unwrap();
    wait(|| f.index().status().unwrap().notes == 1);
    fs::write(&path, "# Note\nsecond-token").unwrap();
    wait(|| {
        !f.index()
            .search("second-token", &Default::default(), 5)
            .unwrap()
            .is_empty()
    });
    fs::remove_file(path).unwrap();
    wait(|| f.index().status().unwrap().notes == 0);
    assert_eq!(
        f.index().automation_status().unwrap()["reserved_calls_today"],
        0
    );
    assert!(worker.stop(Duration::from_secs(2)));
}
#[test]
fn real_worker_with_stub_cli_completes_capture_and_graph_once() {
    let mut f = Fixture::new();
    f.native_stub();
    fs::write(f.config.vault.join("Queue.md"), "# Queue\nRetries.").unwrap();
    let mut worker = AutomationWorker::start_at(
        f.root.join("knowledge"),
        f.config.clone(),
        Default::default(),
        vec![],
    )
    .unwrap();
    assert!(worker.submit(f.session()));
    wait(|| f.index().status().unwrap().notes == 2 && f.index().status().unwrap().model.is_some());
    let first = f.index().automation_status().unwrap()["reserved_calls_today"].clone();
    assert!(worker.stop(Duration::from_secs(2)));
    let mut worker = AutomationWorker::start_at(
        f.root.join("knowledge"),
        f.config.clone(),
        Default::default(),
        vec![],
    )
    .unwrap();
    assert!(worker.submit(f.session()));
    wait(|| f.index().automation_status().unwrap()["runtime"]["state"] == "watching");
    assert!(worker.stop(Duration::from_secs(2)));
    assert_eq!(f.index().proposals().unwrap().len(), 1);
    assert_eq!(
        f.index().automation_status().unwrap()["reserved_calls_today"],
        first
    );
}
#[test]
fn lease_prevents_two_generations_writing_at_once() {
    let f = Fixture::new();
    let _index = f.index();
    let held = lease(&f.root.join("knowledge")).unwrap();
    assert!(matches!(
        lease(&f.root.join("knowledge")),
        Err(Error::Unavailable)
    ));
    drop(held);
    assert!(lease(&f.root.join("knowledge")).is_ok());
}

#[test]
#[ignore = "explicit synthetic live-model smoke; set MERCURY_KNOWLEDGE_LIVE_SMOKE=1"]
fn live_automation_smoke_with_synthetic_session_only() {
    assert_eq!(
        std::env::var("MERCURY_KNOWLEDGE_LIVE_SMOKE").as_deref(),
        Ok("1")
    );
    let mut f = Fixture::new();
    f.config.allow_external = true;
    // The available authenticated provider in this environment; no Claude login required.
    f.config.allowed_providers = vec!["codex".into()];
    f.config.timeout_secs = 90;
    f.config.automation.daily_model_calls = 32;
    fs::write(f.config.vault.join("Alpha.md"),"# Synthetic Alpha\nAlpha depends on Beta. Beta provides queue replay for Alpha. ADR-17 selected Beta because replay is required.\n").unwrap();
    fs::write(f.config.vault.join("Beta.md"),"# Synthetic Beta\nBeta provides durable queue persistence. These are fictional services for a test.\n").unwrap();
    let mut worker = AutomationWorker::start_at(
        f.root.join("knowledge"),
        f.config.clone(),
        Default::default(),
        vec![],
    )
    .unwrap();
    assert!(worker.submit(f.session()));
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let index = f.index();
        let status = index.status().unwrap();
        if status.notes == 3 && status.model.is_some() {
            assert_eq!(index.proposals().unwrap().len(), 1);
            assert!(index.proposals().unwrap()[0]
                .markdown
                .contains("status: proposed"));
            eprintln!(
                "LIVE_AUTOMATION_PASS notes={} explicit_relations={} entities={} inferred_relations={} summaries={} status={}",
                status.notes,
                status.relations,
                status.entities,
                status.inferred_relations,
                status.summaries,
                index.automation_status().unwrap()
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "live automation deadline: {}",
            index.automation_status().unwrap()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(worker.stop(Duration::from_secs(3)));
}
