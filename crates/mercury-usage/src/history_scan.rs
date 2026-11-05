//! Bounded startup scan of agent-owned JSONL for budget history. This module
//! never writes to Claude/Codex storage and must run off the render thread.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use time::OffsetDateTime;

use crate::{ClaudeTranscript, CodexRollout, Provider, UsageEvent};

#[derive(Debug, Clone)]
pub struct HistoricalSession {
    pub provider: Provider,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    pub events: Vec<UsageEvent>,
}

fn recent_jsonl(path: &Path, modified_since: SystemTime) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "jsonl")
        && fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified >= modified_since)
}

fn child_dirs(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

fn jsonl_files(path: &Path, modified_since: SystemTime) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| recent_jsonl(path, modified_since))
        .collect()
}

fn scan_claude(path: &Path, cutoff: OffsetDateTime) -> io::Result<Option<HistoricalSession>> {
    let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Ok(None);
    };
    if !crate::locate::is_safe_session_id(session_id) {
        return Ok(None);
    }
    let mut main = ClaudeTranscript::new();
    let mut cwd = None;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if cwd.is_none() && line.contains("\"cwd\"") {
            cwd = serde_json::from_str::<Value>(&line)
                .ok()
                .and_then(|value| value.get("cwd")?.as_str().map(PathBuf::from))
                .filter(|path| path.is_absolute());
        }
        main.ingest_line(&line);
    }
    let mut events: Vec<_> = main
        .events()
        .iter()
        .filter(|event| event.at >= cutoff)
        .cloned()
        .collect();
    for subagent in crate::locate::claude_subagent_transcripts(path) {
        let mut transcript = ClaudeTranscript::new();
        let Ok(file) = File::open(subagent) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { break };
            transcript.ingest_line(&line);
        }
        events.extend(
            transcript
                .events()
                .iter()
                .filter(|event| event.at >= cutoff)
                .cloned(),
        );
    }
    for event in &mut events {
        event.session_id = session_id.to_owned();
    }
    if events.is_empty() {
        return Ok(None);
    }
    Ok(Some(HistoricalSession {
        provider: Provider::Claude,
        session_id: session_id.to_owned(),
        cwd,
        events,
    }))
}

fn scan_codex(path: &Path, cutoff: OffsetDateTime) -> io::Result<Option<HistoricalSession>> {
    let mut rollout = CodexRollout::new();
    for line in BufReader::new(File::open(path)?).lines() {
        rollout.ingest_line(&line?);
    }
    let Some(session_id) = rollout.session_id() else {
        return Ok(None);
    };
    if !crate::locate::is_safe_session_id(session_id) {
        return Ok(None);
    }
    let events: Vec<_> = rollout
        .events()
        .iter()
        .filter(|event| event.at >= cutoff)
        .cloned()
        .collect();
    if events.is_empty() {
        return Ok(None);
    }
    Ok(Some(HistoricalSession {
        provider: Provider::Codex,
        session_id: session_id.to_owned(),
        cwd: rollout
            .cwd()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
        events,
    }))
}

/// Scans files modified within the supplied horizon and keeps only events in
/// that horizon. Duplicate files for one provider/session choose the newest
/// modified file, avoiding double-counting restored copies of a rollout.
pub fn scan(
    claude_home: Option<&Path>,
    codex_home: Option<&Path>,
    horizon: Duration,
) -> Vec<HistoricalSession> {
    let modified_since = SystemTime::now()
        .checked_sub(horizon)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let cutoff = OffsetDateTime::now_utc() - time::Duration::seconds(horizon.as_secs() as i64);
    let mut paths = Vec::new();
    if let Some(home) = claude_home {
        for project in child_dirs(&home.join("projects")) {
            paths.extend(
                jsonl_files(&project, modified_since)
                    .into_iter()
                    .map(|path| (Provider::Claude, path)),
            );
        }
    }
    if let Some(home) = codex_home {
        for year in child_dirs(&home.join("sessions")) {
            for month in child_dirs(&year) {
                for day in child_dirs(&month) {
                    paths.extend(
                        jsonl_files(&day, modified_since)
                            .into_iter()
                            .map(|path| (Provider::Codex, path)),
                    );
                }
            }
        }
    }
    paths.sort_by_key(|(_, path)| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    let mut by_session = HashMap::new();
    for (provider, path) in paths {
        let parsed = match provider {
            Provider::Claude => scan_claude(&path, cutoff),
            Provider::Codex => scan_codex(&path, cutoff),
        };
        if let Ok(Some(session)) = parsed {
            by_session.insert((session.provider, session.session_id.clone()), session);
        }
    }
    by_session.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_recent_claude_and_codex_without_old_events_or_invalid_files() {
        let root = std::env::temp_dir().join(format!(
            "mercury-usage-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let claude = root.join("claude");
        let codex = root.join("codex");
        let project = claude.join("projects").join("-tmp-project");
        let day = codex.join("sessions/2026/10/06");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&day).unwrap();
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        let old = (OffsetDateTime::now_utc() - time::Duration::days(40))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        let session_id = "11111111-2222-3333-4444-555555555551";
        let claude_path = project.join(format!("{session_id}.jsonl"));
        let claude_line = |at: &str, request: &str| {
            format!("{{\"type\":\"assistant\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{session_id}\",\"timestamp\":\"{at}\",\"requestId\":\"{request}\",\"message\":{{\"id\":\"{request}\",\"model\":\"claude-sonnet\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":2}}}}}}\n")
        };
        fs::write(
            &claude_path,
            format!("{}{}", claude_line(&old, "old"), claude_line(&now, "new")),
        )
        .unwrap();
        let subagents = project.join(session_id).join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(subagents.join("child.jsonl"), claude_line(&now, "child")).unwrap();
        fs::write(project.join("invalid.jsonl"), "not json\n").unwrap();
        let codex_id = "11111111-2222-3333-4444-555555555552";
        let rollout = format!("{{\"timestamp\":\"{now}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{codex_id}\",\"cwd\":\"/tmp/project\"}}}}\n{{\"timestamp\":\"{now}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":30,\"output_tokens\":3}}}}}}}}\n");
        fs::write(day.join(format!("rollout-{codex_id}.jsonl")), rollout).unwrap();

        let mut sessions = scan(
            Some(&claude),
            Some(&codex),
            Duration::from_secs(35 * 24 * 60 * 60),
        );
        sessions.sort_by_key(|session| session.provider);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].provider, Provider::Claude);
        assert_eq!(sessions[0].events.len(), 2);
        assert_eq!(sessions[0].cwd.as_deref(), Some(Path::new("/tmp/project")));
        assert_eq!(sessions[1].provider, Provider::Codex);
        assert_eq!(sessions[1].events.len(), 1);
        assert_eq!(sessions[1].events[0].tokens.input, 30);
        fs::remove_dir_all(root).unwrap();
    }
}
