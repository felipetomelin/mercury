#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mercury_learn::reviewer::{ClaudeReviewer, ReviewError, ReviewTarget, ReviewerConfig};
use mercury_learn::transcript::{Message, Provider, Role, Transcript};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new(body: &str) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-reviewer-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let script = root.join("fake-claude");
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, script }
    }

    fn reviewer(&self) -> ClaudeReviewer {
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

fn transcript(text: &str) -> Transcript {
    Transcript {
        provider: Provider::Codex,
        session_id: "session-1".into(),
        cwd: None,
        messages: vec![Message {
            role: Role::User,
            text: text.into(),
        }],
        truncated: false,
        user_turns: Some(1),
    }
}

fn targets() -> Vec<ReviewTarget> {
    vec![ReviewTarget {
        id: "instructions".into(),
        current_content: "Before".into(),
        max_bytes: 4096,
    }]
}

fn response(content: &str, cost: f64) -> String {
    serde_json::json!({
        "type":"result", "subtype":"success", "is_error":false,
        "total_cost_usd":cost,
        "structured_output":{"proposals":[{
            "target_id":"instructions", "content":content,
            "reason":"Remember the confirmed preference",
            "evidence":"The user requested it"
        }]}
    })
    .to_string()
}

fn printing_script(json: &str) -> String {
    format!("printf '%s\\n' '{}'", json.replace('\'', "'\\''"))
}

#[tokio::test]
async fn reviewer_returns_structured_proposal_with_trusted_source() {
    let fixture = Fixture::new(&format!(
        "printf '%s\\n' \"$@\" > args\nIFS= read -r input\nprintf '%s' \"$input\" > input\n{}",
        printing_script(&response("After", 0.03))
    ));
    let result = fixture
        .reviewer()
        .review(&transcript("Please remember this"), &targets())
        .await
        .unwrap();

    assert_eq!(result.proposals.len(), 1);
    assert_eq!(result.proposals[0].source, "codex/session-1");
    assert_eq!(result.proposals[0].content, "After");
    assert_eq!(result.observed_cost_usd, Some(0.03));
    let args = fs::read_to_string(fixture.root.join("args")).unwrap();
    assert!(args.contains("--restricted\n"));
    assert!(args.contains("--strict-mcp-config\n"));
    assert!(args.contains("--json-schema\n"));
    assert!(!args.contains("Please remember this"));
    let input = fs::read_to_string(fixture.root.join("input")).unwrap();
    assert!(input.contains("Please remember this"));
    assert!(input.contains("instructions"));
    assert!(!input.contains(fixture.root.to_str().unwrap()));
}

#[tokio::test]
async fn reviewer_rejects_response_over_reported_budget() {
    let fixture = Fixture::new(&printing_script(&response("After", 0.11)));
    let error = fixture
        .reviewer()
        .review(&transcript("Hello"), &targets())
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewError::OverBudget));
}

#[tokio::test]
async fn reviewer_rejects_unknown_target_and_sensitive_input() {
    let invalid = response("After", 0.01).replace("instructions", "arbitrary-file");
    let fixture = Fixture::new(&printing_script(&invalid));
    let error = fixture
        .reviewer()
        .review(&transcript("Hello"), &targets())
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewError::InvalidResponse));

    let error = fixture
        .reviewer()
        .review(&transcript("sk-ant-fake"), &targets())
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewError::SensitiveInput));
}

#[tokio::test]
async fn reviewer_bounds_stdout_and_timeout() {
    let loud = Fixture::new("head -c 10000 /dev/zero");
    let error = loud
        .reviewer()
        .review(&transcript("Hello"), &targets())
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewError::OutputTooLarge));

    let slow = Fixture::new("sleep 5");
    let error = slow
        .reviewer()
        .review(&transcript("Hello"), &targets())
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewError::Timeout));
}

#[test]
fn reviewer_refuses_symlinked_workdir() {
    let fixture = Fixture::new(":");
    let link = fixture.root.with_extension("link");
    std::os::unix::fs::symlink(&fixture.root, &link).unwrap();
    let result = ClaudeReviewer::new(ReviewerConfig {
        executable: fixture.script.clone(),
        workdir: link.clone(),
        model: "haiku".into(),
        max_budget_usd: 0.1,
        timeout: Duration::from_secs(2),
        max_input_bytes: 1024,
        max_stdout_bytes: 1024,
        max_stderr_bytes: 1024,
    });
    assert!(matches!(result, Err(ReviewError::InvalidConfig)));
    fs::remove_file(link).unwrap();
}
