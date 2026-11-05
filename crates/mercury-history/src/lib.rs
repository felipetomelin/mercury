//! Private, local search index for Claude Code and Codex conversations.

mod parse;

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub use parse::Provider;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    UnsafePath,
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub provider: Provider,
    pub session_id: String,
    pub cwd: Option<String>,
    pub timestamp: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeTarget {
    pub provider: Provider,
    pub session_id: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Default)]
pub struct SearchFilter<'a> {
    pub query: &'a str,
    pub provider: Option<Provider>,
    pub cwd: Option<&'a str>,
    pub from: Option<&'a str>,
    pub until: Option<&'a str>,
    pub limit: usize,
}

pub struct HistoryIndex {
    db: Connection,
}

impl HistoryIndex {
    /// The caller chooses the private index location. Existing DBs with broad
    /// permissions are rejected instead of silently exposing transcript text.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let parent = path.parent().ok_or(Error::UnsafePath)?;
        if !parent.exists() {
            fs::create_dir_all(parent)?;
            make_private_directory(parent)?;
        }
        secure_directory(parent)?;
        let existing_db = match fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(Error::Io(error)),
        };
        if existing_db {
            secure_file(path)?;
        }
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let shm = PathBuf::from(format!("{}-shm", path.display()));
        for sidecar in [&wal, &shm] {
            match fs::symlink_metadata(sidecar) {
                Ok(_) => secure_file(sidecar)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS sessions (
                path TEXT PRIMARY KEY, provider TEXT NOT NULL, session_id TEXT NOT NULL,
                cwd TEXT, timestamp TEXT NOT NULL, size INTEGER NOT NULL, modified_ns INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS sessions_identity_newest
                ON sessions(provider, session_id, modified_ns DESC, path DESC);
            CREATE VIRTUAL TABLE IF NOT EXISTS messages USING fts5(path UNINDEXED, body);")?;
        if !existing_db {
            make_private_file(path)?;
        }
        secure_file(path)?;
        if wal.exists() {
            if existing_db {
                secure_file(&wal)?;
            } else {
                make_private_file(&wal)?;
            }
        }
        if shm.exists() {
            if existing_db {
                secure_file(&shm)?;
            } else {
                make_private_file(&shm)?;
            }
        }
        Ok(Self { db })
    }

    /// Scan every regular transcript in the configured homes. No age cutoff.
    /// Files that cannot be read are left at their previous good index version.
    pub fn sync(&mut self, claude_home: &Path, codex_home: &Path) -> Result<(), Error> {
        let mut seen = HashSet::new();
        for path in discover(claude_home.join("projects"), 2, false)? {
            self.index_file(&path, Provider::Claude)?;
            seen.insert(path.to_string_lossy().into_owned());
        }
        for path in discover(codex_home.join("sessions"), 4, true)? {
            self.index_file(&path, Provider::Codex)?;
            seen.insert(path.to_string_lossy().into_owned());
        }
        // If a home is temporarily unavailable, retain its prior results.
        for (home, provider) in [
            (claude_home, Provider::Claude),
            (codex_home, Provider::Codex),
        ] {
            if !home.exists() {
                continue;
            }
            let mut stmt = self
                .db
                .prepare("SELECT path FROM sessions WHERE provider=?1")?;
            let paths = stmt
                .query_map([provider.as_str()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for path in paths {
                if Path::new(&path).starts_with(home) && !seen.contains(&path) {
                    let tx = self.db.transaction()?;
                    tx.execute("DELETE FROM messages WHERE path=?1", [&path])?;
                    tx.execute("DELETE FROM sessions WHERE path=?1", [&path])?;
                    tx.commit()?;
                }
            }
        }
        Ok(())
    }

    fn index_file(&mut self, path: &Path, provider: Provider) -> Result<(), Error> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() {
            return Ok(());
        }
        let size = metadata.len();
        if size > 128 * 1024 * 1024 {
            return Ok(());
        }
        let size = size as i64;
        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |v| v.as_nanos() as i64);
        let path_text = path.to_string_lossy().into_owned();
        let old: Option<(i64, i64)> = self
            .db
            .query_row(
                "SELECT size, modified_ns FROM sessions WHERE path=?1",
                [&path_text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if old == Some((size, modified_ns)) {
            return Ok(());
        }
        let parsed = match parse::read(path, provider) {
            Ok(value) => value,
            Err(_) => return Ok(()), // active/unreadable transcript: retain last good index
        };
        let Some(parsed) = parsed else {
            return Ok(());
        };
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM messages WHERE path=?1", [&path_text])?;
        tx.execute("DELETE FROM sessions WHERE path=?1", [&path_text])?;
        tx.execute(
            "INSERT INTO sessions(path, provider, session_id, cwd, timestamp, size, modified_ns)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                path_text,
                provider.as_str(),
                parsed.session_id,
                parsed.cwd,
                parsed.timestamp,
                size,
                modified_ns
            ],
        )?;
        for body in parsed.messages {
            tx.execute(
                "INSERT INTO messages(path, body) VALUES (?1, ?2)",
                params![path_text, body],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn search(&self, filter: SearchFilter<'_>) -> Result<Vec<SearchHit>, Error> {
        let limit = filter.limit.clamp(1, 100) as i64;
        let query = fts_query(filter.query);
        let mut hits = Vec::new();
        if let Some(query) = query {
            let mut statement = self.db.prepare(
                "SELECT s.provider, s.session_id, s.cwd, s.timestamp,
                snippet(messages, 1, '', '', ' … ', 18)
                FROM messages JOIN sessions s ON s.path=messages.path
                WHERE messages MATCH ?1 AND (?2 IS NULL OR s.provider=?2)
                AND (?3 IS NULL OR s.cwd=?3) AND (?4 IS NULL OR substr(s.timestamp, 1, 10)>=?4)
                AND (?5 IS NULL OR substr(s.timestamp, 1, 10)<=?5)
                AND s.path = (SELECT newest.path FROM sessions newest
                    WHERE newest.provider=s.provider AND newest.session_id=s.session_id
                    ORDER BY newest.modified_ns DESC, newest.path DESC LIMIT 1)
                ORDER BY bm25(messages), s.timestamp DESC LIMIT ?6",
            )?;
            let rows = statement.query_map(
                params![
                    query,
                    filter.provider.map(Provider::as_str),
                    filter.cwd,
                    filter.from,
                    filter.until,
                    limit
                ],
                read_hit,
            )?;
            for row in rows {
                hits.push(row?);
            }
        } else {
            let mut statement = self.db.prepare(
                "SELECT s.provider, s.session_id, s.cwd, s.timestamp, ''
                FROM sessions s WHERE (?1 IS NULL OR s.provider=?1)
                AND (?2 IS NULL OR s.cwd=?2) AND (?3 IS NULL OR substr(s.timestamp, 1, 10)>=?3)
                AND (?4 IS NULL OR substr(s.timestamp, 1, 10)<=?4)
                AND s.path = (SELECT newest.path FROM sessions newest
                    WHERE newest.provider=s.provider AND newest.session_id=s.session_id
                    ORDER BY newest.modified_ns DESC, newest.path DESC LIMIT 1)
                ORDER BY s.timestamp DESC LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![
                    filter.provider.map(Provider::as_str),
                    filter.cwd,
                    filter.from,
                    filter.until,
                    limit
                ],
                read_hit,
            )?;
            for row in rows {
                hits.push(row?);
            }
        }
        for hit in &mut hits {
            hit.snippet = hit
                .snippet
                .chars()
                .take(240)
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect();
        }
        Ok(hits)
    }

    /// Revalidate source and project just before a native resume. Missing or
    /// untrusted paths never produce a runnable target.
    pub fn resolve_resume(
        &self,
        provider: Provider,
        session_id: &str,
    ) -> Result<Option<ResumeTarget>, Error> {
        if !parse::safe_id(session_id) {
            return Ok(None);
        }
        let candidate: Option<(String, Option<String>)> = self
            .db
            .query_row(
                "SELECT path, cwd FROM sessions WHERE provider=?1 AND session_id=?2 ORDER BY modified_ns DESC, path DESC LIMIT 1",
                params![provider.as_str(), session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((source, Some(cwd))) = candidate else {
            return Ok(None);
        };
        let source = PathBuf::from(source);
        let cwd = PathBuf::from(cwd);
        if !fs::symlink_metadata(&source).is_ok_and(|meta| meta.file_type().is_file())
            || !cwd.is_absolute()
            || !cwd.is_dir()
        {
            return Ok(None);
        }
        Ok(Some(ResumeTarget {
            provider,
            session_id: session_id.to_owned(),
            cwd,
        }))
    }
}

fn read_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    let provider: String = row.get(0)?;
    Ok(SearchHit {
        provider: if provider == "claude" {
            Provider::Claude
        } else {
            Provider::Codex
        },
        session_id: row.get(1)?,
        cwd: row.get(2)?,
        timestamp: row.get(3)?,
        snippet: row.get(4)?,
    })
}

fn fts_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .take(16)
        .filter_map(|word| {
            let escaped = word.replace('"', "\"\"");
            (!escaped.is_empty()).then(|| format!("\"{escaped}\""))
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

fn discover(root: PathBuf, depth: usize, codex: bool) -> Result<Vec<PathBuf>, Error> {
    let mut result = Vec::new();
    let mut stack = vec![(root, depth)];
    while let Some((dir, remaining)) = stack.pop() {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(Error::Io(error)),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && remaining > 1 {
                stack.push((path, remaining - 1));
            } else if kind.is_file() && remaining == 1 {
                let valid = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.ends_with(".jsonl") && (!codex || name.starts_with("rollout-"))
                    });
                if valid {
                    result.push(path);
                }
            }
        }
    }
    result.sort();
    Ok(result)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(Error::UnsafePath);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::UnsafePath);
    }
    Ok(())
}

#[cfg(unix)]
fn make_private_directory(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_private_directory(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(not(unix))]
fn secure_directory(path: &Path) -> Result<(), Error> {
    if !path.is_dir() {
        return Err(Error::UnsafePath);
    }
    Ok(())
}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(Error::UnsafePath);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::UnsafePath);
    }
    Ok(())
}

#[cfg(unix)]
fn make_private_file(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(Error::UnsafePath);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_private_file(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(not(unix))]
fn secure_file(path: &Path) -> Result<(), Error> {
    if !path.is_file() {
        return Err(Error::UnsafePath);
    }
    Ok(())
}
