use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mercury_learn::transcript::{read, Limits, Provider, Role, TranscriptError};
use serde_json::{json, Value};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    file: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-learn-transcript-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let file = root.join(name);
        Self { root, file }
    }

    fn write(&self, events: &[Value]) {
        let mut content = Vec::new();
        for event in events {
            content.extend(serde_json::to_vec(event).unwrap());
            content.push(b'\n');
        }
        fs::write(&self.file, content).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn codex_meta(id: &str) -> Value {
    json!({"type":"session_meta","payload":{"session_id":id,"cwd":"/project"}})
}

fn codex_user(id: &str, text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","id":id,"role":"user","content":[{"type":"input_text","text":text}]}})
}

#[test]
fn claude_reader_keeps_roles_dedupes_streaming_and_excludes_tools() {
    let fixture = Fixture::new("session-1.jsonl");
    fixture.write(&[
        json!({"type":"user","sessionId":"session-1","uuid":"u1","cwd":"/project","message":{"role":"user","content":"Please remember cache expiry"}}),
        json!({"type":"assistant","sessionId":"session-1","requestId":"r1","message":{"role":"assistant","id":"a1","content":[{"type":"text","text":"draft"},{"type":"thinking","thinking":"secret reasoning"}]}}),
        json!({"type":"assistant","sessionId":"session-1","requestId":"r1","message":{"role":"assistant","id":"a1","content":[{"type":"text","text":"final answer"},{"type":"tool_use","name":"Bash","input":"secret tool input"}]}}),
        json!({"type":"user","sessionId":"session-1","uuid":"tool","message":{"role":"user","content":[{"type":"tool_result","content":"secret tool result"}]}}),
        json!({"type":"user","sessionId":"session-1","isSidechain":true,"uuid":"side","message":{"role":"user","content":"secret sidechain"}}),
        json!({"type":"user","sessionId":"session-1","uuid":"u2","message":{"role":"user","content":"Also track retries"}}),
    ]);

    let result = read(
        &fixture.file,
        Provider::Claude,
        "session-1",
        Limits::default(),
    )
    .unwrap();

    assert_eq!(result.cwd, Some(PathBuf::from("/project")));
    assert_eq!(result.messages.len(), 3);
    assert_eq!(result.messages[0].role, Role::User);
    assert_eq!(result.messages[1].role, Role::Assistant);
    assert_eq!(result.messages[1].text, "final answer");
    assert_eq!(result.user_turns, Some(2));
    assert!(!result.truncated);
    let visible = result
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<String>();
    assert!(!visible.contains("secret"));
}

#[test]
fn codex_reader_requires_session_meta_and_excludes_telemetry_reasoning_and_tools() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[
        codex_meta("thread-1"),
        codex_user("u1", "Fix cache expiry"),
        json!({"type":"response_item","payload":{"type":"reasoning","text":"hidden reasoning"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","output":"hidden tool output"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"a1","content":[{"type":"output_text","text":"Done"},{"type":"reasoning","text":"hidden"}]}}),
        json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10}}}}),
    ]);

    let result = read(
        &fixture.file,
        Provider::Codex,
        "thread-1",
        Limits::default(),
    )
    .unwrap();

    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].text, "Fix cache expiry");
    assert_eq!(result.messages[1].text, "Done");
    assert_eq!(result.user_turns, Some(1));
    assert_eq!(result.cwd, Some(PathBuf::from("/project")));
}

#[test]
fn mismatch_or_missing_identity_is_rejected() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[codex_meta("thread-1"), codex_user("u1", "hello")]);
    assert!(matches!(
        read(
            &fixture.file,
            Provider::Codex,
            "thread-2",
            Limits::default()
        ),
        Err(TranscriptError::InvalidSession)
    ));
    fixture.write(&[codex_user("u1", "hello")]);
    assert!(matches!(
        read(
            &fixture.file,
            Provider::Codex,
            "thread-1",
            Limits::default()
        ),
        Err(TranscriptError::MissingIdentity)
    ));
    let claude = Fixture::new("session-1.jsonl");
    claude.write(&[json!({"type":"user","uuid":"u1","message":{"role":"user","content":"hello"}})]);
    assert!(matches!(
        read(
            &claude.file,
            Provider::Claude,
            "session-1",
            Limits::default()
        ),
        Err(TranscriptError::MissingIdentity)
    ));
}

#[test]
fn large_file_reads_bounded_tail_and_marks_turn_count_unknown() {
    let fixture = Fixture::new("rollout-test.jsonl");
    let mut events = vec![codex_meta("thread-1")];
    for index in 0..50 {
        events.push(json!({"type":"event_msg","payload":{"type":"noise","index":index,"padding":"abcdefghijklmnop"}}));
    }
    events.push(codex_user("u-last", "last user request"));
    events.push(json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"last answer"}]}}));
    fixture.write(&events);
    let limits = Limits {
        tail_bytes: 512,
        ..Limits::default()
    };

    let result = read(&fixture.file, Provider::Codex, "thread-1", limits).unwrap();

    assert!(result.truncated);
    assert_eq!(result.user_turns, None);
    assert_eq!(result.messages[0].text, "last user request");
}

#[test]
fn message_count_and_text_limits_drop_oldest_and_report_truncation() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[
        codex_meta("thread-1"),
        codex_user("u1", "first"),
        codex_user("u2", "second"),
        codex_user("u3", "third"),
    ]);
    let limits = Limits {
        messages: 2,
        ..Limits::default()
    };

    let result = read(&fixture.file, Provider::Codex, "thread-1", limits).unwrap();

    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].text, "second");
    assert_eq!(result.messages[1].text, "third");
    assert!(result.truncated);
    assert_eq!(result.user_turns, None);
}

#[test]
fn duplicate_growth_still_enforces_total_text_limit() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[
        codex_meta("thread-1"),
        codex_user("u1", "one"),
        codex_user("u2", "two"),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"a1","content":[{"type":"output_text","text":"x"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"a1","content":[{"type":"output_text","text":"1234567890123456"}]}}),
    ]);
    let limits = Limits {
        message_bytes: 16,
        total_text_bytes: 20,
        ..Limits::default()
    };

    let result = read(&fixture.file, Provider::Codex, "thread-1", limits).unwrap();

    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].text, "two");
    assert_eq!(result.messages[1].text, "1234567890123456");
    assert!(result.messages.iter().map(|m| m.text.len()).sum::<usize>() <= 20);
    assert!(result.truncated);
}

#[test]
fn unicode_message_clipping_is_safe_and_marks_turn_count_unknown() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[codex_meta("thread-1"), codex_user("u1", "ááááá")]);
    let limits = Limits {
        message_bytes: 7,
        ..Limits::default()
    };

    let result = read(&fixture.file, Provider::Codex, "thread-1", limits).unwrap();

    assert_eq!(result.messages[0].text, "ááá");
    assert!(result.truncated);
    assert_eq!(result.user_turns, None);
}

#[test]
fn control_characters_are_removed_before_reviewer_snapshot() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[
        codex_meta("thread-1"),
        codex_user("u1", "safe\u{0000}\u{001b}[31m text"),
    ]);

    let result = read(
        &fixture.file,
        Provider::Codex,
        "thread-1",
        Limits::default(),
    )
    .unwrap();

    assert_eq!(result.messages[0].text, "safe[31m text");
    assert!(result.truncated);
}

#[test]
fn oversized_jsonl_line_is_skipped_without_unbounded_message() {
    let fixture = Fixture::new("rollout-test.jsonl");
    fixture.write(&[
        codex_meta("thread-1"),
        codex_user("huge", &"x".repeat(300 * 1024)),
        codex_user("u2", "safe final request"),
    ]);

    let result = read(
        &fixture.file,
        Provider::Codex,
        "thread-1",
        Limits::default(),
    )
    .unwrap();

    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].text, "safe final request");
    assert!(result.truncated);
    assert_eq!(result.user_turns, None);
}

#[test]
fn incomplete_last_line_is_not_reviewed() {
    let fixture = Fixture::new("session-1.jsonl");
    fixture.write(&[json!({"type":"user","sessionId":"session-1","uuid":"u1","message":{"role":"user","content":"hello"}})]);
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&fixture.file)
        .unwrap()
        .write_all(b"{broken")
        .unwrap();

    assert!(matches!(
        read(
            &fixture.file,
            Provider::Claude,
            "session-1",
            Limits::default()
        ),
        Err(TranscriptError::Incomplete)
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_transcript_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("session-1.jsonl");
    fixture.write(&[json!({"type":"user","sessionId":"session-1","uuid":"u1","message":{"role":"user","content":"hello"}})]);
    let link = fixture.root.join("session-2.jsonl");
    symlink(&fixture.file, &link).unwrap();

    assert!(matches!(
        read(&link, Provider::Claude, "session-2", Limits::default()),
        Err(TranscriptError::InvalidPath)
    ));
}
