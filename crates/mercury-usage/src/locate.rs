//! Finding usage files for a session id reported by the agent hooks.
//!
//! Mercury's hooks (`pane.report_agent_session`) give us the session id and
//! the pane's cwd; the transcript/rollout path is derived here.

use std::path::{Path, PathBuf};

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn default_claude_home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".claude")))
}

/// `$CODEX_HOME`, else `~/.codex`.
pub fn default_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".codex")))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Session ids end up in file names; refuse anything that could escape a directory.
pub fn is_safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Claude Code's project directory name: every non-alphanumeric character of
/// the absolute cwd becomes `-` (`/Users/me/my.app` → `-Users-me-my-app`).
pub fn claude_project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// `<claude_home>/projects/<slug>/<session>.jsonl`, falling back to a scan of
/// all project directories (the cwd may have changed since the session began).
pub fn claude_transcript_path(
    claude_home: &Path,
    cwd: Option<&Path>,
    session_id: &str,
) -> Option<PathBuf> {
    if !is_safe_session_id(session_id) {
        return None;
    }
    let projects = claude_home.join("projects");
    let file_name = format!("{session_id}.jsonl");
    if let Some(cwd) = cwd {
        let direct = projects.join(claude_project_slug(cwd)).join(&file_name);
        if direct.is_file() {
            return Some(direct);
        }
    }
    std::fs::read_dir(&projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// Subagent transcripts of a session: `<dir>/<session>/subagents/*.jsonl`.
pub fn claude_subagent_transcripts(transcript: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(stem)) = (transcript.parent(), transcript.file_stem()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent.join(stem).join("subagents")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    paths.sort();
    paths
}

fn sorted_dirs_desc(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort_by(|left, right| right.cmp(left));
    dirs
}

/// Finds `<codex_home>/sessions/YYYY/MM/DD/rollout-*-<session>.jsonl`, newest
/// day first, stopping after `max_days` day directories.
pub fn codex_rollout_path(codex_home: &Path, session_id: &str, max_days: usize) -> Option<PathBuf> {
    if !is_safe_session_id(session_id) {
        return None;
    }
    let suffix = format!("-{session_id}.jsonl");
    let mut days_seen = 0;
    for year in sorted_dirs_desc(&codex_home.join("sessions")) {
        for month in sorted_dirs_desc(&year) {
            for day in sorted_dirs_desc(&month) {
                if days_seen >= max_days {
                    return None;
                }
                days_seen += 1;
                let Ok(entries) = std::fs::read_dir(&day) else {
                    continue;
                };
                let found = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                name.starts_with("rollout-") && name.ends_with(&suffix)
                            })
                    });
                if found.is_some() {
                    return found;
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = format!(
                "mercury-usage-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_nanos())
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"{}\n").unwrap();
    }

    #[test]
    fn slug_matches_claude_project_dirs() {
        assert_eq!(
            claude_project_slug(Path::new("/Users/me/Documents/REPOS/MercuryPlugin")),
            "-Users-me-Documents-REPOS-MercuryPlugin"
        );
        assert_eq!(
            claude_project_slug(Path::new("/home/me/my.app_x")),
            "-home-me-my-app-x"
        );
    }

    #[test]
    fn rejects_unsafe_session_ids() {
        assert!(is_safe_session_id("2dee4965-803b-40ae-83c3-808851af59ee"));
        for bad in ["", "../etc", "a/b", "a.b", "x\0"] {
            assert!(!is_safe_session_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn finds_claude_transcript_directly_or_by_scan() {
        let home = TempDir::new("claude");
        let id = "11111111-2222-3333-4444-555555555555";
        let moved = home.0.join("projects/-old-cwd").join(format!("{id}.jsonl"));
        touch(&moved);
        assert_eq!(
            claude_transcript_path(&home.0, Some(Path::new("/new/cwd")), id),
            Some(moved.clone())
        );
        let direct = home.0.join("projects/-new-cwd").join(format!("{id}.jsonl"));
        touch(&direct);
        assert_eq!(
            claude_transcript_path(&home.0, Some(Path::new("/new/cwd")), id),
            Some(direct.clone())
        );
        assert_eq!(claude_transcript_path(&home.0, None, "missing"), None);

        let sub = home
            .0
            .join("projects/-new-cwd")
            .join(id)
            .join("subagents/agent-a1.jsonl");
        touch(&sub);
        touch(&sub.with_file_name("notes.txt"));
        assert_eq!(claude_subagent_transcripts(&direct), vec![sub]);
    }

    #[test]
    fn finds_newest_codex_rollout() {
        let home = TempDir::new("codex");
        let id = "01a0ed25-1a03-7a90-bc6c-904119f57526";
        let older = home
            .0
            .join("sessions/2026/09/28")
            .join(format!("rollout-2026-09-28T10-00-00-{id}.jsonl"));
        let other = home
            .0
            .join("sessions/2026/10/01/rollout-2026-10-01T10-00-00-other.jsonl");
        touch(&older);
        touch(&other);
        assert_eq!(codex_rollout_path(&home.0, id, 30), Some(older));
        assert_eq!(codex_rollout_path(&home.0, id, 1), None);
        assert_eq!(codex_rollout_path(&home.0, "../x", 30), None);
    }
}
