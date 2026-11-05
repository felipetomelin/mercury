use mercury_history::{HistoryIndex, Provider, SearchFilter};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct Temp(PathBuf);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

impl Temp {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        loop {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mercury-history-{}-{suffix}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create history test directory: {error}"),
            }
        }
    }
    fn file(&self, path: &str, content: &str) -> PathBuf {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, content).unwrap();
        target
    }
    fn db(&self) -> PathBuf {
        self.0.join("private/index.sqlite")
    }
    fn claude(&self) -> PathBuf {
        self.0.join("claude")
    }
    fn codex(&self) -> PathBuf {
        self.0.join("codex")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn search(index: &HistoryIndex, query: &str) -> Vec<mercury_history::SearchHit> {
    index
        .search(SearchFilter {
            query,
            limit: 20,
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn indexes_both_providers_and_filters_without_tool_text() {
    let temp = Temp::new();
    let claude = concat!(
        "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2020-01-01T00:00:00Z\",\"cwd\":\"/a/project\",\"message\":{\"role\":\"user\",\"content\":\"rare-platypus request\"}}\n",
        "{\"type\":\"assistant\",\"requestId\":\"r1\",\"timestamp\":\"2020-01-01T00:01:00Z\",\"message\":{\"id\":\"m1\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"rare-platypus answer\"}]}}\n",
        "{\"type\":\"assistant\",\"requestId\":\"r1\",\"timestamp\":\"2020-01-01T00:01:00Z\",\"message\":{\"id\":\"m1\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"rare-platypus answer\"}]}}\n",
        "{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"content\":\"secret-tool-payload\"}]}}\n"
    );
    let codex = concat!(
        "{\"timestamp\":\"2020-01-02T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"session_id\":\"codex-1\",\"cwd\":\"/b/project\"}}\n",
        "{\"timestamp\":\"2020-01-02T00:00:30Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"codex-user-question\"}]}}\n",
        "{\"timestamp\":\"2020-01-02T00:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"rare-platypus codex\"}]}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"output\":\"secret-tool-payload\"}}\n"
    );
    temp.file("claude/projects/-a-project/claude-1.jsonl", claude);
    temp.file(
        "codex/sessions/2020/01/02/rollout-2020-codex-1.jsonl",
        codex,
    );
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    let hits = search(&index, "rare-platypus");
    assert_eq!(hits.len(), 3); // user + deduplicated assistant + Codex
    assert_eq!(search(&index, "codex-user-question").len(), 1);
    assert!(search(&index, "secret-tool-payload").is_empty());
    let filtered = index
        .search(SearchFilter {
            query: "rare-platypus",
            provider: Some(Provider::Codex),
            cwd: Some("/b/project"),
            from: Some("2020-01-02"),
            limit: 20,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].session_id, "codex-1");
    assert!(index
        .search(SearchFilter {
            query: "rare-platypus",
            cwd: Some("/other/project"),
            limit: 20,
            ..Default::default()
        })
        .unwrap()
        .is_empty());
}

#[test]
fn updates_and_removes_transcripts_and_handles_quoted_query() {
    let temp = Temp::new();
    let file = temp.file("claude/projects/-p/session-1.jsonl", "{\"type\":\"user\",\"uuid\":\"a\",\"message\":{\"role\":\"user\",\"content\":\"firstword\"}}\n");
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert_eq!(search(&index, "firstword").len(), 1);
    assert!(search(&index, "firstword\" OR secondword").is_empty());
    fs::write(&file, "{\"type\":\"user\",\"uuid\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"secondword\"}}\n{partial").unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert!(search(&index, "firstword").is_empty());
    assert_eq!(search(&index, "secondword").len(), 1);
    fs::remove_file(&file).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert!(search(&index, "secondword").is_empty());
}

#[test]
fn latest_streaming_assistant_message_replaces_partial_copy() {
    let temp = Temp::new();
    temp.file("claude/projects/-p/session-2.jsonl", concat!(
        "{\"type\":\"assistant\",\"requestId\":\"r\",\"message\":{\"id\":\"m\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"partialword\"}]}}\n",
        "{\"type\":\"assistant\",\"requestId\":\"r\",\"message\":{\"id\":\"m\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"finalword\"}]}}\n"
    ));
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert!(search(&index, "partialword").is_empty());
    assert_eq!(search(&index, "finalword").len(), 1);
}

#[test]
fn duplicate_session_files_search_only_the_newest_source() {
    let temp = Temp::new();
    temp.file("claude/projects/a/session-dup.jsonl", "{\"type\":\"user\",\"uuid\":\"a\",\"message\":{\"role\":\"user\",\"content\":\"oldercopyword\"}}\n");
    temp.file("claude/projects/z/session-dup.jsonl", "{\"type\":\"user\",\"uuid\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"newercopyword\"}}\n");
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert!(search(&index, "oldercopyword").is_empty());
    assert_eq!(search(&index, "newercopyword").len(), 1);
    assert_eq!(search(&index, "").len(), 1);
}

#[test]
fn until_date_includes_messages_through_the_end_of_that_day() {
    let temp = Temp::new();
    temp.file(
        "claude/projects/-p/on-day.jsonl",
        "{\"type\":\"user\",\"uuid\":\"a\",\"timestamp\":\"2024-07-02T23:59:59Z\",\"message\":{\"role\":\"user\",\"content\":\"date-boundary\"}}\n",
    );
    temp.file(
        "claude/projects/-p/next-day.jsonl",
        "{\"type\":\"user\",\"uuid\":\"b\",\"timestamp\":\"2024-07-03T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"date-boundary\"}}\n",
    );
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();

    for query in ["date-boundary", ""] {
        let hits = index
            .search(SearchFilter {
                query,
                from: Some("2024-07-02"),
                until: Some("2024-07-02"),
                limit: 20,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1, "query={query:?}");
        assert_eq!(hits[0].session_id, "on-day");
    }
}

#[test]
#[ignore = "manual performance check reads local transcripts into a private temporary index"]
fn benchmark_initial_sync_of_local_history() {
    let claude = PathBuf::from(
        std::env::var_os("MERCURY_HISTORY_BENCH_CLAUDE_HOME")
            .expect("set MERCURY_HISTORY_BENCH_CLAUDE_HOME explicitly"),
    );
    let codex = PathBuf::from(
        std::env::var_os("MERCURY_HISTORY_BENCH_CODEX_HOME")
            .expect("set MERCURY_HISTORY_BENCH_CODEX_HOME explicitly"),
    );
    let temp = Temp::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    let start = std::time::Instant::now();
    index.sync(&claude, &codex).unwrap();
    eprintln!("initial history sync: {:.2?}", start.elapsed());
    let warm_start = std::time::Instant::now();
    index.sync(&claude, &codex).unwrap();
    eprintln!("unchanged history sync: {:.2?}", warm_start.elapsed());
    assert!(temp.db().exists());
}

#[test]
fn resume_requires_existing_source_and_project_and_safe_id() {
    let temp = Temp::new();
    let project = temp.0.join("project");
    fs::create_dir(&project).unwrap();
    let transcript = temp.file(
        "claude/projects/-p/session-3.jsonl",
        &format!("{{\"type\":\"user\",\"cwd\":\"{}\",\"message\":{{\"role\":\"user\",\"content\":\"resume me\"}}}}\n", project.display()),
    );
    let mut index = HistoryIndex::open(&temp.db()).unwrap();
    index.sync(&temp.claude(), &temp.codex()).unwrap();
    assert_eq!(
        index
            .resolve_resume(Provider::Claude, "session-3")
            .unwrap()
            .unwrap()
            .cwd,
        project
    );
    assert!(index
        .resolve_resume(Provider::Claude, "../session-3")
        .unwrap()
        .is_none());
    fs::remove_file(transcript).unwrap();
    assert!(index
        .resolve_resume(Provider::Claude, "session-3")
        .unwrap()
        .is_none());
}

#[cfg(unix)]
#[test]
fn rejects_public_existing_database_directory() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let dir = temp.0.join("public");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(HistoryIndex::open(&dir.join("index.sqlite")).is_err());
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn rejects_database_symlink_even_when_broken() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = Temp::new();
    let private = temp.0.join("private");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(temp.0.join("missing"), private.join("index.sqlite")).unwrap();
    assert!(HistoryIndex::open(&private.join("index.sqlite")).is_err());
}
