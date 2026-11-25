//! Human-readable session names, independent of transcript modification times.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::{parse::safe_id, Error, HistoryIndex, Provider};

#[derive(Debug, Clone)]
pub struct SessionTitle {
    pub provider: Provider,
    pub session_id: String,
    pub title: String,
}

pub(crate) fn clean_title(value: &str) -> Option<String> {
    let text: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(1024)
        .collect();
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect();
    (!text.is_empty()).then_some(text)
}

pub(crate) fn migrate(db: &mut Connection) -> Result<(), Error> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let has_titles: bool = tx.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('sessions') WHERE name='title'",
        [],
        |row| row.get(0),
    )?;
    if !has_titles {
        tx.execute_batch(
            "ALTER TABLE sessions ADD COLUMN title TEXT;
            ALTER TABLE sessions ADD COLUMN first_prompt TEXT;
            UPDATE sessions SET modified_ns=-1;",
        )?;
    }
    let has_classification: bool = tx.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('sessions') WHERE name='is_subagent'",
        [],
        |row| row.get(0),
    )?;
    if !has_classification {
        tx.execute_batch(
            "ALTER TABLE sessions ADD COLUMN is_subagent INTEGER NOT NULL DEFAULT 0;
            UPDATE sessions SET modified_ns=-1;",
        )?;
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_titles (
        provider TEXT NOT NULL, session_id TEXT NOT NULL, source TEXT NOT NULL,
        title TEXT NOT NULL, PRIMARY KEY(provider, session_id, source)
    );",
    )?;
    tx.commit()?;
    Ok(())
}

impl HistoryIndex {
    /// Persist user-assigned names even after their Mercury pane is closed.
    pub fn record_titles(&mut self, titles: &[SessionTitle]) -> Result<(), Error> {
        self.record_titles_from(titles, "mercury")
    }

    fn record_titles_from(&mut self, titles: &[SessionTitle], source: &str) -> Result<(), Error> {
        let tx = self.db.transaction()?;
        for entry in titles {
            if !safe_id(&entry.session_id) {
                continue;
            }
            let Some(title) = clean_title(&entry.title) else {
                continue;
            };
            tx.execute(
                "INSERT INTO session_titles(provider, session_id, source, title)
                VALUES (?1, ?2, ?3, ?4) ON CONFLICT(provider, session_id, source)
                DO UPDATE SET title=excluded.title WHERE title<>excluded.title",
                params![entry.provider.as_str(), entry.session_id, source, title],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn sync_provider_titles(
        &mut self,
        claude_home: &Path,
        codex_home: &Path,
    ) -> Result<(), Error> {
        let mut titles = Vec::new();
        if let Some(file) = metadata_file(&codex_home.join("session_index.jsonl")) {
            // Read the append-only name index in order: the latest name wins.
            let mut reader = BufReader::new(file.take(64 * 1024 * 1024));
            let mut line = Vec::new();
            while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                if line.len() <= 64 * 1024 {
                    if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                        push_title(
                            &mut titles,
                            Provider::Codex,
                            &value["id"],
                            &value["thread_name"],
                        );
                    }
                }
                line.clear();
            }
        }
        if let Ok(projects) = fs::read_dir(claude_home.join("projects")) {
            for project in projects.flatten() {
                if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let Some(file) = metadata_file(&project.path().join("sessions-index.json")) else {
                    continue;
                };
                let Ok(value) = serde_json::from_reader::<_, Value>(file.take(16 * 1024 * 1024))
                else {
                    continue;
                };
                if let Some(entries) = value["entries"].as_array() {
                    for entry in entries {
                        let title = entry
                            .get("customTitle")
                            .filter(|value| value.as_str().is_some_and(|s| !s.trim().is_empty()))
                            .or_else(|| entry.get("summary"));
                        if let Some(title) = title {
                            push_title(&mut titles, Provider::Claude, &entry["sessionId"], title);
                        }
                    }
                }
            }
        }
        self.record_titles_from(&titles, "provider")
    }
}

fn metadata_file(path: &Path) -> Option<File> {
    fs::symlink_metadata(path)
        .ok()?
        .file_type()
        .is_file()
        .then(|| File::open(path).ok())
        .flatten()
}

fn push_title(titles: &mut Vec<SessionTitle>, provider: Provider, id: &Value, title: &Value) {
    if let (Some(id), Some(title)) = (id.as_str(), title.as_str().and_then(clean_title)) {
        if safe_id(id) {
            titles.push(SessionTitle {
                provider,
                session_id: id.to_owned(),
                title,
            });
        }
    }
}
