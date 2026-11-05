use crate::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub struct Index {
    pub(crate) db: Connection,
}
#[cfg(not(unix))]
pub(crate) fn private_dir(_path: &Path) -> Result<()> {
    Err(Error::Invalid(
        "private knowledge storage requires Unix permissions",
    ))
}
#[cfg(unix)]
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    crate::ingest::check_components(path)?;
    if !path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
        }
        #[cfg(not(unix))]
        return Err(Error::Invalid(
            "private knowledge storage requires Unix permissions",
        ));
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(Error::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.mode() & 0o077 != 0 || m.uid() != unsafe { libc::geteuid() } {
            return Err(Error::UnsafePath);
        }
    }
    Ok(())
}
pub(crate) fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = o.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

static PUBLISH_NEXT: AtomicUsize = AtomicUsize::new(0);
/// Publish complete bytes without replacing a file created by another editor.
pub(crate) fn publish_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or(Error::UnsafePath)?;
    crate::ingest::check_components(parent)?;
    let staging = parent.join(format!(
        ".mercury-publish-{}-{}.tmp",
        std::process::id(),
        PUBLISH_NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&staging)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(staging.clone());
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // Atomic create-new semantics; rename would overwrite a racing human file.
    fs::hard_link(&staging, path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Conflict
        } else {
            Error::Io(e)
        }
    })?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
impl Index {
    pub fn open(root: &Path) -> Result<Self> {
        private_dir(root)?;
        let path = root.join("index.sqlite");
        if !path.exists() {
            match private_write(&path, b"") {
                Ok(()) => {}
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        for name in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
            let p = root.join(name);
            if let Ok(m) = fs::symlink_metadata(p) {
                if !m.is_file() || m.file_type().is_symlink() {
                    return Err(Error::UnsafePath);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if m.mode() & 0o077 != 0 || m.uid() != unsafe { libc::geteuid() } {
                        return Err(Error::UnsafePath);
                    }
                }
            }
        }
        let db = Connection::open(&path)?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 3 {
            return Err(Error::Invalid("unsupported knowledge schema"));
        }
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        if version < 3 {
            db.execute_batch("BEGIN IMMEDIATE;
   CREATE TABLE IF NOT EXISTS snapshot(id INTEGER PRIMARY KEY CHECK(id=1),body TEXT NOT NULL,updated INTEGER NOT NULL);
   CREATE VIRTUAL TABLE IF NOT EXISTS chunks USING fts5(id UNINDEXED,note_id UNINDEXED,body);
   CREATE TABLE IF NOT EXISTS derived(id INTEGER PRIMARY KEY CHECK(id=1),revision TEXT NOT NULL,fingerprint TEXT NOT NULL,body TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS proposals(id TEXT PRIMARY KEY,body TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS bundles(id TEXT PRIMARY KEY,revision TEXT NOT NULL,body TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,body TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS extraction_cache(chunk_id TEXT NOT NULL, fingerprint TEXT NOT NULL, hash TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(chunk_id,fingerprint));
   PRAGMA user_version=3; COMMIT;")?;
        }
        Ok(Self { db })
    }
    pub fn snapshot(&self) -> Result<Option<Snapshot>> {
        let body: Option<String> = self
            .db
            .query_row("SELECT body FROM snapshot WHERE id=1", [], |r| r.get(0))
            .optional()?;
        body.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn replace(&mut self, s: Snapshot, cancel: &AtomicBool) -> Result<Status> {
        self.replace_with_force(s, cancel, false)
    }
    fn replace_with_force(
        &mut self,
        mut s: Snapshot,
        cancel: &AtomicBool,
        force: bool,
    ) -> Result<Status> {
        let previous = self.snapshot()?;
        if let Some(old) = &previous {
            if old.revision == s.revision && !force {
                return self.status();
            }
            // Preserve inferred IDs across an unambiguous rename; explicit IDs always win.
            let mut mapping = std::collections::BTreeMap::new();
            for note in &s.notes {
                if note.explicit_id {
                    continue;
                }
                let matches: Vec<_> = old.notes.iter().filter(|n| n.hash == note.hash).collect();
                if matches.len() == 1
                    && s.notes.iter().filter(|n| n.hash == note.hash).count() == 1
                    && !s
                        .notes
                        .iter()
                        .any(|n| n.id == matches[0].id && n.path != note.path)
                {
                    mapping.insert(note.id.clone(), matches[0].id.clone());
                }
            }
            for note in &mut s.notes {
                if let Some(id) = mapping.get(&note.id) {
                    let old_id = std::mem::replace(&mut note.id, id.clone());
                    for c in &mut note.chunks {
                        c.id = c.id.replacen(&old_id, id, 1)
                    }
                    for l in &mut note.links {
                        l.evidence = l.evidence.replacen(&old_id, id, 1)
                    }
                }
            }
            crate::ingest::resolve(&mut s);
        }
        let tx = self.db.transaction()?;
        let unchanged: std::collections::BTreeSet<_> = if force {
            std::collections::BTreeSet::new()
        } else {
            s.notes
                .iter()
                .filter(|n| {
                    previous.as_ref().is_some_and(|old| {
                        old.notes
                            .iter()
                            .any(|o| o.id == n.id && o.hash == n.hash && o.project == n.project)
                    })
                })
                .map(|n| n.id.clone())
                .collect()
        };
        if let Some(old) = &previous {
            for note in &old.notes {
                if !unchanged.contains(&note.id) {
                    tx.execute("DELETE FROM chunks WHERE note_id=?1", [&note.id])?;
                    for chunk in &note.chunks {
                        tx.execute(
                            "DELETE FROM extraction_cache WHERE chunk_id=?1",
                            [&chunk.id],
                        )?;
                    }
                }
            }
        }
        for note in s.notes.iter().filter(|n| !unchanged.contains(&n.id)) {
            for chunk in &note.chunks {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                tx.execute(
                    "INSERT INTO chunks(id,note_id,body) VALUES(?1,?2,?3)",
                    params![chunk.id, note.id, chunk.text],
                )?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO snapshot VALUES(1,?1,?2)",
            params![serde_json::to_string(&s)?, now() as i64],
        )?;
        tx.execute("DELETE FROM derived", [])?;
        tx.execute("DELETE FROM bundles", [])?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        tx.commit()?;
        self.status()
    }
    pub fn sync(&mut self, config: &Config, cancel: &AtomicBool) -> Result<Status> {
        self.replace(scan(config, cancel)?, cancel)
    }
    pub fn rebuild(&mut self, config: &Config, cancel: &AtomicBool) -> Result<Status> {
        self.replace_with_force(scan(config, cancel)?, cancel, true)
    }
    pub fn status(&self) -> Result<Status> {
        let s = self.snapshot()?;
        let updated = self
            .db
            .query_row("SELECT updated FROM snapshot WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .unwrap_or(0);
        let derived: Option<(String, String)> = self
            .db
            .query_row("SELECT fingerprint,body FROM derived WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let graph = derived
            .as_ref()
            .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok());
        let count = graph
            .as_ref()
            .and_then(|g| g["summaries"].as_array())
            .map_or(0, Vec::len);
        let entities = graph
            .as_ref()
            .and_then(|g| g["entities"].as_array())
            .map_or(0, Vec::len);
        let inferred_relations = graph
            .as_ref()
            .and_then(|g| g["relations"].as_array())
            .map_or(0, |relations| {
                relations
                    .iter()
                    .filter(|r| r["origin"] == "inferred")
                    .count()
            });
        Ok(Status {
            entities,
            inferred_relations,
            schema_version: 3,
            revision: s.as_ref().map_or(String::new(), |s| s.revision.clone()),
            notes: s.as_ref().map_or(0, |s| s.notes.len()),
            chunks: s
                .as_ref()
                .map_or(0, |s| s.notes.iter().map(|n| n.chunks.len()).sum()),
            relations: s.as_ref().map_or(0, |s| s.relations.len()),
            summaries: count,
            model: derived.map(|d| d.0),
            updated: updated as u64,
        })
    }
    pub fn search(&self, query: &str, filter: &Filter, limit: usize) -> Result<Vec<Hit>> {
        if query.len() > 2048 || limit == 0 || limit > 100 || query.chars().any(char::is_control) {
            return Err(Error::Invalid("invalid knowledge search"));
        }
        let Some(s) = self.snapshot()? else {
            return Ok(vec![]);
        };
        let tokens: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .take(32)
            .map(|t| format!("\"{t}\"*"))
            .collect();
        let mut ids = Vec::new();
        if tokens.is_empty() {
            ids = s
                .notes
                .iter()
                .flat_map(|n| n.chunks.iter().map(|c| (c.id.clone(), 0.0)))
                .collect()
        } else {
            let mut stmt=self.db.prepare("SELECT id,bm25(chunks) FROM chunks WHERE chunks MATCH ?1 ORDER BY bm25(chunks),id LIMIT 10000")?;
            for row in stmt.query_map([tokens.join(" OR ")], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })? {
                ids.push(row?);
            }
        }
        let mut hits = Vec::new();
        for (id, score) in ids {
            if let Some((n, c)) = s
                .notes
                .iter()
                .filter(|n| matches_filter(n, filter, &s))
                .find_map(|n| n.chunks.iter().find(|c| c.id == id).map(|c| (n, c)))
            {
                hits.push(hit(n, c, -score));
                if hits.len() == limit {
                    break;
                }
            }
        }
        Ok(hits)
    }
    pub fn related(&self, id: &str, hops: usize) -> Result<Vec<Relation>> {
        if hops > 2 || id.len() > 512 {
            return Err(Error::Invalid("invalid knowledge graph request"));
        }
        let Some(s) = self.snapshot()? else {
            return Ok(vec![]);
        };
        let roots: Vec<_> = s
            .notes
            .iter()
            .filter(|n| n.id == id || n.path == id || n.title == id)
            .collect();
        if roots.len() != 1 {
            return Err(Error::Invalid("ambiguous or missing note"));
        }
        let mut seen = std::collections::BTreeSet::from([roots[0].id.clone()]);
        let mut out = Vec::new();
        for _ in 0..hops {
            for r in &s.relations {
                if (seen.contains(&r.source) || seen.contains(&r.target))
                    && out.len() < 100
                    && !out.iter().any(|a: &Relation| {
                        a.source == r.source && a.kind == r.kind && a.target == r.target
                    })
                {
                    out.push(r.clone());
                }
            }
            for r in &out {
                if seen.len() < 50 {
                    seen.insert(r.source.clone());
                    seen.insert(r.target.clone());
                }
            }
        }
        Ok(out)
    }
    pub fn save_derived(
        &mut self,
        revision: &str,
        fingerprint: &str,
        value: &serde_json::Value,
    ) -> Result<()> {
        if self.status()?.revision != revision {
            return Err(Error::Conflict);
        }
        self.db.execute(
            "INSERT OR REPLACE INTO derived VALUES(1,?1,?2,?3)",
            params![revision, fingerprint, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
    pub fn derived(&self, fingerprint: &str) -> Result<Option<serde_json::Value>> {
        let body: Option<String> = self
            .db
            .query_row(
                "SELECT body FROM derived WHERE id=1 AND revision=?1 AND fingerprint=?2",
                params![self.status()?.revision, fingerprint],
                |r| r.get(0),
            )
            .optional()?;
        body.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
}
pub(crate) fn matches_filter(n: &Note, f: &Filter, s: &Snapshot) -> bool {
    f.project.as_ref().is_none_or(|v| n.project == *v)
        && f.kind.as_ref().is_none_or(|v| n.kind == *v)
        && f.status.as_ref().is_none_or(|v| n.status == *v)
        && f.service.as_ref().is_none_or(|v| {
            n.title == *v
                || n.id == *v
                || n.links.iter().any(|l| l.target == *v)
                || s.relations
                    .iter()
                    .any(|r| r.source == n.id && r.target == *v)
        })
}
pub(crate) fn hit(n: &Note, c: &Chunk, score: f64) -> Hit {
    Hit {
        chunk_id: c.id.clone(),
        note_id: n.id.clone(),
        path: n.path.clone(),
        heading: c.heading.clone(),
        text: c.text.clone(),
        status: n.status.clone(),
        project: n.project.clone(),
        modified: n.modified,
        score,
    }
}
impl Index {
    pub fn save_job(&self, id: &str, value: &serde_json::Value) -> Result<()> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Invalid("invalid job ID"));
        }
        self.db.execute(
            "INSERT OR REPLACE INTO jobs VALUES(?1,?2)",
            params![id, serde_json::to_string(value)?],
        )?;
        self.db.execute("DELETE FROM jobs WHERE rowid NOT IN (SELECT rowid FROM jobs ORDER BY rowid DESC LIMIT 64)",[])?;
        Ok(())
    }
    pub fn job(&self, id: &str) -> Result<Option<serde_json::Value>> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Invalid("invalid job ID"));
        }
        let body: Option<String> = self
            .db
            .query_row("SELECT body FROM jobs WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        body.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
}

impl Index {
    pub(crate) fn cached_extraction(
        &self,
        chunk: &Chunk,
        fingerprint: &str,
    ) -> Result<Option<serde_json::Value>> {
        let value:Option<String>=self.db.query_row("SELECT body FROM extraction_cache WHERE chunk_id=?1 AND fingerprint=?2 AND hash=?3",params![chunk.id,fingerprint,crate::hash(chunk.text.as_bytes())],|r|r.get(0)).optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Error::from))
            .transpose()
    }
    pub(crate) fn cache_extraction(
        &self,
        chunk: &Chunk,
        fingerprint: &str,
        body: &serde_json::Value,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO extraction_cache VALUES(?1,?2,?3,?4)",
            params![
                chunk.id,
                fingerprint,
                crate::hash(chunk.text.as_bytes()),
                serde_json::to_string(body)?
            ],
        )?;
        Ok(())
    }
}
