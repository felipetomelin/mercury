#![cfg(unix)]
use mercury_learn::{
    checkpoints::Journal,
    transcript::{self, Provider},
};
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
struct Fixture {
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("mercury-checkpoint-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("session-1.jsonl");
        fs::write(
            &path,
            format!(
                "{}\n",
                json!({"type":"session_meta","payload":{"id":"session-1","cwd":root}})
            ),
        )
        .unwrap();
        Self { root, path }
    }
    fn journal(&self) -> Journal {
        Journal::open(&self.root.join("learn")).unwrap()
    }
    fn append(&self, v: serde_json::Value) {
        writeln!(
            OpenOptions::new().append(true).open(&self.path).unwrap(),
            "{v}"
        )
        .unwrap();
    }
    fn task(&self, n: usize) {
        self.append(json!({"type":"response_item","payload":{"type":"message","id":format!("u{n}"),"role":"user","content":[{"type":"input_text","text":format!("preference-{n}")}]}}));
        self.append(json!({"type":"response_item","payload":{"type":"message","id":format!("a{n}"),"role":"assistant","content":[{"type":"output_text","text":format!("lesson-{n}")}]}}));
        self.append(json!({"type":"event_msg","payload":{"type":"task_complete"}}));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn native_checkpoints_survive_append_restart_and_independent_consumers() {
    let f = Fixture::new();
    f.journal()
        .register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.task(1);
    let mut j = f.journal();
    j.observe().unwrap();
    let first = j.pending("learn").unwrap().remove(0);
    let original = fs::read(&first.path).unwrap();
    j.finish("learn", &first.id, "no_change").unwrap();
    j.finish("learn", &first.id, "done").unwrap();
    drop(j);
    f.task(2);
    let mut j = f.journal();
    j.observe().unwrap();
    assert_eq!(fs::read(&first.path).unwrap(), original);
    first.verify().unwrap();
    assert_eq!(j.pending("learn").unwrap().len(), 1);
    assert_eq!(j.pending("knowledge").unwrap().len(), 2);
    let second = j.pending("learn").unwrap().remove(0);
    assert!(String::from_utf8(fs::read(&second.path).unwrap())
        .unwrap()
        .contains("lesson-2"));
    assert!(!String::from_utf8(fs::read(&second.path).unwrap())
        .unwrap()
        .contains("lesson-1"));
    assert_eq!(second.turns, 2);
    j.observe().unwrap();
    assert_eq!(j.pending("knowledge").unwrap().len(), 2);
    assert!(j.status().unwrap()["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["learn"] == "no_change"));
}
#[test]
fn long_session_and_oversized_tool_lines_do_not_disable_capture() {
    let f = Fixture::new();
    f.journal()
        .register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.append(json!({"type":"tool_result","text":"x".repeat(5*1024*1024)}));
    for n in 0..50 {
        f.task(n);
    }
    let mut j = f.journal();
    for _ in 0..10 {
        j.observe().unwrap();
    }
    let mut turns = Vec::new();
    loop {
        let pending = j.pending("learn").unwrap();
        if pending.is_empty() {
            break;
        }
        for c in pending {
            c.verify().unwrap();
            let t =
                transcript::read(&c.path, c.provider, &c.session_id, Default::default()).unwrap();
            assert_eq!(
                t.messages
                    .iter()
                    .filter(|m| m.role == mercury_learn::transcript::Role::User)
                    .count(),
                1
            );
            turns.push(c.turns);
            j.finish("learn", &c.id, "done").unwrap();
        }
    }
    assert_eq!(turns, (1..=50).collect::<Vec<_>>());
    assert_eq!(j.status().unwrap()["blocked_sources"], 0);
}
#[test]
fn explicit_claude_checkpoint_is_durable_and_bounded() {
    let f = Fixture::new();
    fs::write(&f.path, "").unwrap();
    let j = f.journal();
    j.register(Provider::Claude, "session-1", &f.path, false)
        .unwrap();
    f.append(json!({"type":"user","sessionId":"session-1","message":{"role":"user","content":"confirmed preference"}}));
    f.append(json!({"type":"assistant","sessionId":"session-1","message":{"role":"assistant","id":"a","content":"recorded preference"}}));
    assert!(j.request(Provider::Claude, "session-1", &f.path).unwrap());
    drop(j);
    let mut j = f.journal();
    j.observe().unwrap();
    assert_eq!(j.pending("knowledge").unwrap().len(), 1);
    j.observe().unwrap();
    assert_eq!(j.pending("knowledge").unwrap().len(), 1);
}
#[test]
fn existing_history_is_not_replayed_and_automated_origins_stay_excluded() {
    let f = Fixture::new();
    f.task(0);
    let mut j = f.journal();
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    j.observe().unwrap();
    assert!(j.pending("learn").unwrap().is_empty());
    j.exclude(Provider::Codex, "session-1").unwrap();
    drop(j);
    f.task(1);
    let mut j = f.journal();
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    j.observe().unwrap();
    assert!(j.pending("learn").unwrap().is_empty());
    assert!(!j.trusted(Provider::Codex, "session-1", &f.path).unwrap());
}
#[test]
fn rewritten_source_and_edited_snapshot_are_conflicts() {
    let f = Fixture::new();
    let mut j = f.journal();
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.task(1);
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    fs::write(&c.path, "human edit").unwrap();
    assert!(c.verify().is_err());
    j.finish("learn", &c.id, "failed").unwrap();
    assert!(j.retry("learn", &c.id, true).is_err());
    fs::write(&f.path, "rewritten\n").unwrap();
    j.observe().unwrap();
    assert_eq!(j.status().unwrap()["blocked_sources"], 1);
}
#[test]
fn partial_line_waits_and_installations_with_same_id_are_isolated() {
    let a = Fixture::new();
    let b = Fixture::new();
    let mut ja = a.journal();
    let mut jb = b.journal();
    ja.register(Provider::Codex, "session-1", &a.path, false)
        .unwrap();
    jb.register(Provider::Codex, "session-1", &b.path, false)
        .unwrap();
    a.task(1);
    write!(
        OpenOptions::new().append(true).open(&a.path).unwrap(),
        "{{\"type\":"
    )
    .unwrap();
    ja.observe().unwrap();
    jb.observe().unwrap();
    assert_eq!(ja.pending("learn").unwrap().len(), 1);
    assert!(jb.pending("learn").unwrap().is_empty());
    writeln!(
        OpenOptions::new().append(true).open(&a.path).unwrap(),
        "\"telemetry\"}}"
    )
    .unwrap();
    ja.observe().unwrap();
    assert_eq!(ja.status().unwrap()["blocked_sources"], 0);
}

#[test]
fn stop_captures_its_recorded_boundary_even_if_next_task_has_already_started() {
    let f = Fixture::new();
    fs::write(&f.path, "").unwrap();
    let j = f.journal();
    j.register(Provider::Claude, "session-1", &f.path, false)
        .unwrap();
    for (role, text) in [
        ("user", "completed-task"),
        ("assistant", "completed-lesson"),
    ] {
        f.append(
            json!({"type":role,"sessionId":"session-1","message":{"role":role,"content":text}}),
        );
    }
    assert!(j.request(Provider::Claude, "session-1", &f.path).unwrap());
    f.append(json!({"type":"user","sessionId":"session-1","message":{"role":"user","content":"NEXT_TASK_MUST_NOT_LEAK"}}));
    let mut j = f.journal();
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    let bytes = fs::read_to_string(c.path).unwrap();
    assert!(bytes.contains("completed-lesson"));
    assert!(!bytes.contains("NEXT_TASK_MUST_NOT_LEAK"));
}

#[test]
fn exclusion_applies_to_each_unconsumed_stage_and_pending_binding_survives_restart() {
    let f = Fixture::new();
    let j = f.journal();
    j.register_pending(Provider::Codex, "session-1", false)
        .unwrap();
    drop(j);
    let mut j = f.journal();
    assert_eq!(j.unresolved().unwrap().len(), 1);
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.task(1);
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    j.finish("learn", &c.id, "done").unwrap();
    j.exclude(Provider::Codex, "session-1").unwrap();
    assert!(j.pending("knowledge").unwrap().is_empty());
    assert_eq!(j.state("learn", &c.id).unwrap().as_deref(), Some("done"));
}

#[test]
fn automation_exclusion_before_binding_and_retry_backoff_are_durable() {
    let f = Fixture::new();
    let j = f.journal();
    j.exclude(Provider::Codex, "automated").unwrap();
    drop(j);
    let j = f.journal();
    j.register_pending(Provider::Codex, "automated", false)
        .unwrap();
    assert!(!j.trusted(Provider::Codex, "automated", &f.path).unwrap());
    j.exclude_next("stable-terminal-uuid", Provider::Claude)
        .unwrap();
    drop(j);
    let mut j = f.journal();
    assert!(j
        .next_excluded("stable-terminal-uuid", Provider::Claude)
        .unwrap());
    j.clear_next("stable-terminal-uuid", Provider::Claude)
        .unwrap();
    assert!(!j
        .next_excluded("stable-terminal-uuid", Provider::Claude)
        .unwrap());
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.task(1);
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    let future = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    j.fail_learn(&c.id, future).unwrap();
    drop(j);
    let j = f.journal();
    assert!(j.pending("learn").unwrap().is_empty());
    assert_eq!(j.pending("knowledge").unwrap().len(), 1);
    for _ in 0..4 {
        j.fail_learn(&c.id, future).unwrap();
    }
    assert_eq!(j.state("learn", &c.id).unwrap().as_deref(), Some("failed"));
    assert_eq!(j.retry("learn", &c.id, false).unwrap()["applied"], false);
    assert!(j.pending("learn").unwrap().is_empty());
    j.retry("learn", &c.id, true).unwrap();
    assert_eq!(j.pending("learn").unwrap().len(), 1);
}

#[test]
fn escaped_snapshot_bytes_do_not_reduce_the_advertised_text_budget() {
    let f = Fixture::new();
    let mut j = f.journal();
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    for role in ["user", "assistant", "user", "assistant"] {
        f.append(json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":if role=="user"{"input_text"}else{"output_text"},"text":"\"".repeat(16*1024)}]}}));
    }
    f.append(json!({"type":"event_msg","payload":{"type":"task_complete"}}));
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    c.verify().unwrap();
    assert!(fs::metadata(c.path).unwrap().len() > 128 * 1024);
    assert_eq!(j.status().unwrap()["blocked_sources"], 0);
}

#[test]
fn a_long_single_task_retains_its_user_and_declares_truncation() {
    let f = Fixture::new();
    let mut j = f.journal();
    j.register(Provider::Codex, "session-1", &f.path, false)
        .unwrap();
    f.append(json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Remember this confirmed preference"}]}}));
    for n in 0..80 {
        f.append(json!({"type":"response_item","payload":{"type":"message","id":format!("a{n}"),"role":"assistant","content":[{"type":"output_text","text":format!("progress {n}")}]}}));
    }
    f.append(json!({"type":"event_msg","payload":{"type":"task_complete"}}));
    j.observe().unwrap();
    let c = j.pending("learn").unwrap().remove(0);
    assert_eq!(c.turns, 1);
    let t = transcript::read(&c.path, c.provider, &c.session_id, Default::default()).unwrap();
    assert!(t.truncated);
    assert_eq!(t.user_turns, None);
    assert!(t
        .messages
        .iter()
        .any(|m| m.text.contains("confirmed preference")));
}
