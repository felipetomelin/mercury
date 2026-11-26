//! Durable, bounded checkpoints shared by Learn and Knowledge. Only explicitly
//! registered main sessions are observed; provider completion events delimit
//! immutable conversational windows. No model runs in this producer.
use crate::{
    storage::{ensure_private_dir, hash, Storage, StorageError},
    transcript::{self, Provider, Retained, Role},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const READ_BUDGET: u64 = 1024 * 1024;
const MAX_LINE: usize = 256 * 1024;
const SNAPSHOT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Cursor {
    offset: u64,
    identity: String,
    anchor: String,
    cwd: Option<PathBuf>,
    messages: VecDeque<Retained>,
    turns: u64,
    #[serde(default)]
    signals: u64,
    #[serde(default)]
    skipped_history: u64,
    #[serde(default)]
    empty_windows: u64,
    #[serde(default)]
    emitted: u64,
    #[serde(default)]
    requested_ends: VecDeque<u64>,
    #[serde(default)]
    baseline: u64,
    #[serde(default)]
    skipping: bool,
    #[serde(default)]
    truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub provider: Provider,
    pub session_id: String,
    pub path: PathBuf,
    pub turns: u64,
    pub content_hash: String,
}

impl Checkpoint {
    pub fn verify(&self) -> Result<(), StorageError> {
        transcript::check_path(&self.path).map_err(|_| StorageError::UnsafePath)?;
        let mut bytes = Vec::new();
        fs::File::open(&self.path)?
            .take(SNAPSHOT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if hash(&bytes) != self.content_hash {
            return Err(StorageError::InvalidInput);
        }
        Ok(())
    }
}

pub struct Journal {
    storage: Storage,
    root: PathBuf,
}
impl Journal {
    pub fn open(root: &Path) -> Result<Self, StorageError> {
        let storage = Storage::open(root)?;
        storage.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS checkpoint_sources(
            provider TEXT NOT NULL, session TEXT NOT NULL, path TEXT NOT NULL,
            excluded INTEGER NOT NULL, cursor TEXT NOT NULL, scanned INTEGER NOT NULL DEFAULT 0,
            error TEXT, PRIMARY KEY(provider,session));
            CREATE TABLE IF NOT EXISTS checkpoint_origins(identity TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS checkpoints(id TEXT PRIMARY KEY, body TEXT NOT NULL,
            learn TEXT NOT NULL DEFAULT 'pending', knowledge TEXT NOT NULL DEFAULT 'pending',
            created INTEGER NOT NULL DEFAULT (unixepoch()),learn_attempts INTEGER NOT NULL DEFAULT 0,
            learn_next INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(Self {
            storage,
            root: root.to_path_buf(),
        })
    }
    /// Persist before acknowledging a trusted SessionStart. Never discovers
    /// sessions globally or chooses a transcript based on modification time.
    pub fn register(
        &self,
        provider: Provider,
        session: &str,
        path: &Path,
        excluded: bool,
    ) -> Result<(), StorageError> {
        if session.is_empty()
            || session.len() > 128
            || !session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || !path.is_absolute()
            || path.as_os_str().len() > 4096
        {
            return Err(StorageError::UnsafePath);
        }
        let excluded =
            excluded || self.origin_excluded(&format!("{}:{session}", name(provider)))?;
        let existing: Option<String> = self
            .storage
            .connection
            .query_row(
                "SELECT path FROM checkpoint_sources WHERE provider=?1 AND session=?2",
                params![name(provider), session],
                |r| r.get(0),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|old| !old.is_empty() && Path::new(old) != path)
        {
            return Err(StorageError::InvalidInput);
        }
        let count: u32 = self.storage.connection.query_row(
            "SELECT count(*) FROM checkpoint_sources",
            [],
            |r| r.get(0),
        )?;
        if existing.is_none() && count >= 256 {
            return Err(StorageError::Corrupt);
        }
        self.storage.connection.execute("INSERT INTO checkpoint_sources(provider,session,path,excluded,cursor) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(provider,session) DO UPDATE SET excluded=max(checkpoint_sources.excluded,excluded.excluded),path=CASE WHEN checkpoint_sources.path='' THEN excluded.path ELSE checkpoint_sources.path END,cursor=CASE WHEN checkpoint_sources.path='' THEN excluded.cursor ELSE checkpoint_sources.cursor END",params![name(provider),session,path.to_string_lossy(),excluded,serde_json::to_string(&Cursor {baseline: fs::metadata(path).map_or(0,|m|m.len()), ..Cursor::default()}).map_err(|_|StorageError::Corrupt)?])?;
        Ok(())
    }
    pub fn register_pending(
        &self,
        provider: Provider,
        session: &str,
        excluded: bool,
    ) -> Result<(), StorageError> {
        if session.is_empty()
            || session.len() > 128
            || !session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(StorageError::InvalidInput);
        }
        let excluded =
            excluded || self.origin_excluded(&format!("{}:{session}", name(provider)))?;
        let count: u32 = self.storage.connection.query_row(
            "SELECT count(*) FROM checkpoint_sources",
            [],
            |r| r.get(0),
        )?;
        let exists: bool = self.storage.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM checkpoint_sources WHERE provider=?1 AND session=?2)",
            params![name(provider), session],
            |r| r.get(0),
        )?;
        if count >= 256 && !exists {
            return Err(StorageError::TooLarge);
        }
        self.storage.connection.execute("INSERT INTO checkpoint_sources(provider,session,path,excluded,cursor) VALUES(?1,?2,'',?3,?4) ON CONFLICT(provider,session) DO UPDATE SET excluded=max(checkpoint_sources.excluded,excluded.excluded)",params![name(provider),session,excluded,serde_json::to_string(&Cursor::default()).map_err(|_|StorageError::Corrupt)?])?;
        Ok(())
    }
    pub fn unresolved(&self) -> Result<Vec<(Provider, String)>, StorageError> {
        let mut q = self.storage.connection.prepare(
            "SELECT provider,session FROM checkpoint_sources WHERE path='' AND excluded=0 LIMIT 8",
        )?;
        let rows = q
            .query_map([], |r| {
                let provider: String = r.get(0)?;
                Ok((
                    if provider == "codex" {
                        Provider::Codex
                    } else {
                        Provider::Claude
                    },
                    r.get(1)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn trusted(
        &self,
        provider: Provider,
        session: &str,
        path: &Path,
    ) -> Result<bool, StorageError> {
        Ok(self.storage.connection.query_row("SELECT count(*) FROM checkpoint_sources WHERE provider=?1 AND session=?2 AND path=?3 AND excluded=0",params![name(provider),session,path.to_string_lossy()],|r|r.get::<_,u32>(0))? == 1)
    }
    pub fn exclude(&self, provider: Provider, session: &str) -> Result<(), StorageError> {
        self.exclude_origin(&format!("{}:{session}", name(provider)))?;
        self.storage.connection.execute(
            "UPDATE checkpoint_sources SET excluded=1 WHERE provider=?1 AND session=?2",
            params![name(provider), session],
        )?;
        // Unconsumed work from this origin must not start a learning loop.
        self.storage.connection.execute("UPDATE checkpoints SET learn=CASE WHEN learn='pending' THEN 'excluded' ELSE learn END,knowledge=CASE WHEN knowledge='pending' THEN 'excluded' ELSE knowledge END WHERE json_extract(body,'$.session_id')=?1 AND json_extract(body,'$.provider')=?2",params![session,serde_json::to_value(provider).map_err(|_|StorageError::Corrupt)?.as_str()])?;
        Ok(())
    }
    fn exclude_origin(&self, identity: &str) -> Result<(), StorageError> {
        let count: u32 = self.storage.connection.query_row(
            "SELECT count(*) FROM checkpoint_origins",
            [],
            |r| r.get(0),
        )?;
        if count >= 4096 {
            return Err(StorageError::TooLarge);
        }
        self.storage.connection.execute(
            "INSERT OR IGNORE INTO checkpoint_origins VALUES(?1)",
            [identity],
        )?;
        Ok(())
    }
    pub fn origin_excluded(&self, identity: &str) -> Result<bool, StorageError> {
        Ok(self.storage.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM checkpoint_origins WHERE identity=?1)",
            [identity],
            |r| r.get(0),
        )?)
    }
    pub fn exclude_next(&self, terminal: &str, provider: Provider) -> Result<(), StorageError> {
        if terminal.len() > 128 {
            return Err(StorageError::InvalidInput);
        }
        self.exclude_origin(&format!("terminal:{terminal}:{}", name(provider)))
    }
    pub fn next_excluded(&self, terminal: &str, provider: Provider) -> Result<bool, StorageError> {
        self.origin_excluded(&format!("terminal:{terminal}:{}", name(provider)))
    }
    pub fn clear_next(&self, terminal: &str, provider: Provider) -> Result<(), StorageError> {
        self.storage.connection.execute(
            "DELETE FROM checkpoint_origins WHERE identity=?1",
            [format!("terminal:{terminal}:{}", name(provider))],
        )?;
        Ok(())
    }
    /// Explicit trusted Stop/SessionEnd fallback for providers without native
    /// completion records. The flag and cursor survive a full process restart.
    pub fn request(
        &self,
        provider: Provider,
        session: &str,
        path: &Path,
    ) -> Result<bool, StorageError> {
        if !self.trusted(provider, session, path)? {
            return Ok(false);
        }
        transcript::check_path(path).map_err(|_| StorageError::UnsafePath)?;
        let end = fs::metadata(path)?.len();
        let exists:bool=self.storage.connection.query_row("SELECT EXISTS(SELECT 1 FROM checkpoint_sources,json_each(cursor,'$.requested_ends') WHERE provider=?1 AND session=?2 AND value=?3)",params![name(provider),session,end as i64],|r|r.get(0))?;
        if exists {
            return Ok(true);
        }
        Ok(self.storage.connection.execute("UPDATE checkpoint_sources SET cursor=json_insert(cursor,'$.requested_ends[#]',?3),scanned=0 WHERE provider=?1 AND session=?2 AND json_array_length(cursor,'$.requested_ends')<64",params![name(provider),session,end as i64])? != 0)
    }
    pub fn request_session(&self, provider: Provider, session: &str) -> Result<bool, StorageError> {
        let path:Option<String>=self.storage.connection.query_row("SELECT path FROM checkpoint_sources WHERE provider=?1 AND session=?2 AND excluded=0",params![name(provider),session],|r|r.get(0)).optional()?;
        match path {
            Some(path) => self.request(provider, session, Path::new(&path)),
            None => Ok(false),
        }
    }
    /// At most eight MiB per pass, including long sessions. Partial last lines
    /// remain unread until complete. Truncation/replacement is a visible conflict.
    pub fn observe(&mut self) -> Result<(), StorageError> {
        let rows = {
            let mut q=self.storage.connection.prepare("SELECT provider,session,path FROM checkpoint_sources WHERE excluded=0 AND path!='' ORDER BY scanned,provider,session LIMIT 8")?;
            let rows = q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (provider, session, path) in rows {
            let provider = if provider == "codex" {
                Provider::Codex
            } else {
                Provider::Claude
            };
            if self
                .observe_one(provider, &session, Path::new(&path))
                .is_err()
            {
                self.storage.connection.execute("UPDATE checkpoint_sources SET error='transcript unavailable, replaced or invalid',scanned=(SELECT coalesce(max(scanned),0)+1 FROM checkpoint_sources) WHERE provider=?1 AND session=?2",params![name(provider),session])?;
            }
        }
        self.prune()?;
        Ok(())
    }
    fn observe_one(
        &mut self,
        provider: Provider,
        session: &str,
        path: &Path,
    ) -> Result<(), StorageError> {
        transcript::check_path(path).map_err(|_| StorageError::UnsafePath)?;
        let tx = self
            .storage
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let body: String = tx.query_row(
            "SELECT cursor FROM checkpoint_sources WHERE provider=?1 AND session=?2",
            params![name(provider), session],
            |r| r.get(0),
        )?;
        let mut cursor: Cursor = serde_json::from_str(&body).map_err(|_| StorageError::Corrupt)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(path)?;
        let metadata = file.metadata()?;
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            format!("{}:{}", metadata.dev(), metadata.ino())
        };
        #[cfg(not(unix))]
        let identity = format!("{:?}", metadata.created().ok());
        if metadata.len() < cursor.offset
            || (!cursor.identity.is_empty() && cursor.identity != identity)
        {
            return Err(StorageError::InvalidInput);
        }
        // Check the previously consumed boundary as well as file identity. An
        // ordinary append is permitted; a rewrite at the boundary is not.
        if cursor.offset > 0 {
            file.seek(SeekFrom::Start(cursor.offset.saturating_sub(4096)))?;
            let mut anchor = vec![0; (cursor.offset.min(4096)) as usize];
            file.read_exact(&mut anchor)?;
            if hash(&anchor) != cursor.anchor {
                return Err(StorageError::InvalidInput);
            }
        }
        if cursor.identity.is_empty() {
            file.seek(SeekFrom::Start(0))?;
            let mut header = Vec::new();
            Read::by_ref(&mut file)
                .take(128 * 1024)
                .read_to_end(&mut header)?;
            let (id, cwd) = transcript::read_identity(&header, provider, session)
                .map_err(|_| StorageError::Corrupt)?;
            if id.as_deref() != Some(session) {
                return Err(StorageError::Corrupt);
            }
            if provider == Provider::Claude
                && path.file_stem().and_then(|s| s.to_str()) != Some(session)
            {
                return Err(StorageError::UnsafePath);
            }
            cursor.cwd = cwd;
            cursor.identity = identity;
        }
        file.seek(SeekFrom::Start(cursor.offset))?;
        while cursor
            .requested_ends
            .front()
            .is_some_and(|end| *end < cursor.offset)
        {
            cursor.requested_ends.pop_front();
        }
        let read_budget = cursor.requested_ends.front().map_or(READ_BUDGET, |end| {
            READ_BUDGET.min(end.saturating_sub(cursor.offset))
        });
        let mut reader = BufReader::new(Read::take(&mut file, read_budget));
        let mut consumed = 0;
        use sha2::{Digest, Sha256};
        let mut consumed_hash = Sha256::new();
        loop {
            let mut line = Vec::new();
            // Bound allocation even for a malicious or enormous JSONL record.
            let size = Read::by_ref(&mut reader)
                .take((MAX_LINE + 1) as u64)
                .read_until(b'\n', &mut line)?;
            if size == 0 {
                break;
            }
            if cursor.skipping || line.len() > MAX_LINE {
                consumed += size as u64;
                consumed_hash.update(&line);
                cursor.skipping = !line.ends_with(b"\n");
                cursor.truncated = true;
                continue;
            }
            if !line.ends_with(b"\n") {
                break;
            }
            consumed += size as u64;
            consumed_hash.update(&line);
            let event: Value = serde_json::from_slice(&line).map_err(|_| StorageError::Corrupt)?;
            if event
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != session)
                || event.get("isSidechain").and_then(Value::as_bool) == Some(true)
            {
                return Err(StorageError::UnsafePath);
            }
            if provider == Provider::Codex && event["type"] == "session_meta" {
                let (id, _) = transcript::read_identity(&line, provider, session)
                    .map_err(|_| StorageError::Corrupt)?;
                if id.as_deref() != Some(session) {
                    return Err(StorageError::InvalidInput);
                }
            }
            if let Some(item) = transcript::parse_message(&event, provider, 16 * 1024) {
                cursor.truncated |= item.clipped;
                if let Some(position) = item.identity.as_ref().and_then(|id| {
                    cursor
                        .messages
                        .iter()
                        .position(|m| m.identity.as_ref() == Some(id))
                }) {
                    cursor.messages[position] = item;
                } else {
                    if item.message.role == Role::User {
                        cursor.turns += 1;
                    }
                    cursor.messages.push_back(item);
                }
                while cursor.messages.len() > 64
                    || cursor
                        .messages
                        .iter()
                        .map(|m| m.message.text.len())
                        .sum::<usize>()
                        > 64 * 1024
                {
                    cursor.truncated = true;
                    if cursor
                        .messages
                        .front()
                        .is_some_and(|m| m.message.role == Role::User)
                        && cursor
                            .messages
                            .iter()
                            .filter(|m| m.message.role == Role::User)
                            .count()
                            == 1
                        && cursor.messages.len() > 1
                    {
                        cursor.messages.remove(1);
                    } else {
                        cursor.messages.pop_front();
                    }
                }
            }
            if provider == Provider::Codex
                && event["type"] == "event_msg"
                && event["payload"]["type"] == "task_complete"
            {
                cursor.signals += 1;
                if cursor.offset + consumed > cursor.baseline {
                    save_window(&self.root, &tx, provider, session, &mut cursor, consumed)?;
                } else {
                    cursor.skipped_history += 1;
                    cursor.messages.clear();
                    cursor.truncated = false;
                }
            }
        }
        drop(reader);
        file.seek(SeekFrom::Start(cursor.offset))?;
        let mut verify_hash = Sha256::new();
        std::io::copy(
            &mut Read::by_ref(&mut file).take(consumed),
            &mut HashWriter(&mut verify_hash),
        )?;
        if consumed_hash.finalize() != verify_hash.finalize() {
            return Err(StorageError::InvalidInput);
        }
        cursor.offset += consumed;
        // A forced checkpoint is only complete after reaching the current EOF.
        if cursor.requested_ends.front() == Some(&cursor.offset) {
            cursor.signals += 1;
            save_window(&self.root, &tx, provider, session, &mut cursor, 0)?;
            cursor.requested_ends.pop_front();
        }
        file.seek(SeekFrom::Start(cursor.offset.saturating_sub(4096)))?;
        let mut anchor = vec![0; cursor.offset.min(4096) as usize];
        file.read_exact(&mut anchor)?;
        cursor.anchor = hash(&anchor);
        tx.execute("UPDATE checkpoint_sources SET cursor=?3,scanned=(SELECT coalesce(max(scanned),0)+1 FROM checkpoint_sources),error=NULL WHERE provider=?1 AND session=?2",params![name(provider),session,serde_json::to_string(&cursor).map_err(|_|StorageError::Corrupt)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn pending(&self, consumer: &str) -> Result<Vec<Checkpoint>, StorageError> {
        let column = consumer_column(consumer)?;
        let mut q = self.storage.connection.prepare(&format!(
            "SELECT body FROM checkpoints WHERE {column}='pending' AND (CASE WHEN '{column}'='learn' THEN learn_next ELSE 0 END)<=unixepoch() ORDER BY rowid LIMIT 8"
        ))?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|b| serde_json::from_str(&b?).map_err(|_| StorageError::Corrupt))
            .collect();
        rows
    }
    pub fn state(&self, consumer: &str, id: &str) -> Result<Option<String>, StorageError> {
        let column = consumer_column(consumer)?;
        Ok(self
            .storage
            .connection
            .query_row(
                &format!("SELECT {column} FROM checkpoints WHERE id=?1"),
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn fail_learn(&self, id: &str, timestamp: u64) -> Result<(), StorageError> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.storage.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        tx.execute("UPDATE checkpoints SET learn_attempts=learn_attempts+1,learn_next=?2+(60 << min(learn_attempts,6)),learn=CASE WHEN learn_attempts+1>=5 THEN 'failed' ELSE 'pending' END WHERE id=?1 AND learn='pending'",params![id,timestamp as i64])?;
        tx.execute(
            "DELETE FROM review_triggers WHERE key=?1 AND status='failed'",
            [format!("checkpoint:{id}")],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish(&self, consumer: &str, id: &str, state: &str) -> Result<(), StorageError> {
        let column = consumer_column(consumer)?;
        if !["done", "failed", "no_change"].contains(&state) {
            return Err(StorageError::Corrupt);
        }
        self.storage.connection.execute(
            &format!("UPDATE checkpoints SET {column}=?2 WHERE id=?1 AND {column}='pending'"),
            params![id, state],
        )?;
        Ok(())
    }
    /// Explicit, focal recovery. Immutable bytes are checked even for preview;
    /// completed consumers and proposals are never reset.
    pub fn retry(&self, consumer: &str, id: &str, apply: bool) -> Result<Value, StorageError> {
        let column = consumer_column(consumer)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.storage.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let (body, state): (String, String) = self.storage.connection.query_row(
            &format!("SELECT body,{column} FROM checkpoints WHERE id=?1"),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let c: Checkpoint = serde_json::from_str(&body).map_err(|_| StorageError::Corrupt)?;
        c.verify()?;
        if apply && state == "failed" {
            self.storage.connection.execute(
                &format!(
                    "UPDATE checkpoints SET {column}='pending' WHERE id=?1 AND {column}='failed'"
                ),
                [id],
            )?;
            if consumer == "learn" {
                self.storage.connection.execute(
                    "UPDATE checkpoints SET learn_attempts=0,learn_next=0 WHERE id=?1",
                    [id],
                )?;
                self.storage.connection.execute(
                    "DELETE FROM review_triggers WHERE key=?1 AND status IN ('failed','claimed')",
                    [format!("checkpoint:{id}")],
                )?;
            }
        }
        tx.commit()?;
        Ok(
            json!({"id":id,"consumer":consumer,"state":state,"eligible":state=="failed","applied":apply && state=="failed","snapshot_verified":true}),
        )
    }
    fn prune(&self) -> Result<(), StorageError> {
        let mut q=self.storage.connection.prepare("SELECT id FROM checkpoints WHERE learn IN ('done','no_change','excluded') AND knowledge IN ('done','no_change','excluded') ORDER BY rowid DESC LIMIT -1 OFFSET 1024")?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for id in ids {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(StorageError::Corrupt);
            }
            // Remove bytes before the row; a crash only leaves a terminal row.
            let path = self.root.join("checkpoints").join(&id);
            if path.exists() {
                ensure_private_dir(&path)?;
                fs::remove_dir_all(path)?;
            }
            self.storage
                .connection
                .execute("DELETE FROM checkpoints WHERE id=?1", [id])?;
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Value, StorageError> {
        let mut counts = Vec::new();
        let mut q = self
            .storage
            .connection
            .prepare("SELECT learn,knowledge,count(*) FROM checkpoints GROUP BY learn,knowledge")?;
        for row in q.query_map([],|r|Ok(json!({"learn":r.get::<_,String>(0)?,"knowledge":r.get::<_,String>(1)?,"count":r.get::<_,i64>(2)?})))? {counts.push(row?);}
        let (sources, blocked): (i64, i64) = self.storage.connection.query_row(
            "SELECT count(*),coalesce(sum(error IS NOT NULL),0) FROM checkpoint_sources",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let counters:Value=self.storage.connection.query_row("SELECT coalesce(sum(json_extract(cursor,'$.signals')),0),coalesce(sum(json_extract(cursor,'$.skipped_history')),0),coalesce(sum(json_extract(cursor,'$.empty_windows')),0),coalesce(sum(json_extract(cursor,'$.emitted')),0),coalesce(sum(path=''),0),coalesce(sum(excluded),0) FROM checkpoint_sources",[],|r|Ok(json!({"completion_signals":r.get::<_,i64>(0)?,"skipped_history_before_registration":r.get::<_,i64>(1)?,"empty_or_incomplete_windows":r.get::<_,i64>(2)?,"persisted_checkpoints":r.get::<_,i64>(3)?,"unresolved_sources":r.get::<_,i64>(4)?,"excluded_sources":r.get::<_,i64>(5)?})))?;
        let mut q = self
            .storage
            .connection
            .prepare("SELECT id,learn,knowledge,learn_attempts,learn_next FROM checkpoints ORDER BY rowid DESC LIMIT 8")?;
        let recent=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"learn":r.get::<_,String>(1)?,"knowledge":r.get::<_,String>(2)?,"learn_attempts":r.get::<_,i64>(3)?,"learn_next":r.get::<_,i64>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(
            json!({"sources":sources,"counters":counters,"recent":recent,"blocked_sources":blocked,"checkpoints":counts,"codex_signal":"task_complete","claude_signal":"Stop/SessionEnd hook required","snapshot_text_bytes":65536,"read_bytes_per_source":READ_BUDGET}),
        )
    }
}
fn consumer_column(consumer: &str) -> Result<&'static str, StorageError> {
    match consumer {
        "learn" => Ok("learn"),
        "knowledge" => Ok("knowledge"),
        _ => Err(StorageError::Corrupt),
    }
}
pub fn name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
    }
}
fn save_window(
    root: &Path,
    tx: &rusqlite::Transaction<'_>,
    provider: Provider,
    session: &str,
    cursor: &mut Cursor,
    relative_offset: u64,
) -> Result<(), StorageError> {
    if !cursor.messages.iter().any(|m| m.message.role == Role::User)
        || cursor
            .messages
            .back()
            .is_none_or(|m| m.message.role != Role::Assistant)
    {
        cursor.empty_windows += 1;
        return Ok(());
    }
    let mut lines = Vec::new();
    if provider == Provider::Codex {
        lines.push(json!({"type":"session_meta","payload":{"id":session,"cwd":cursor.cwd,"mercury_checkpoint_truncated":cursor.truncated}}));
    }
    for (i, m) in cursor.messages.iter().enumerate() {
        let role = if m.message.role == Role::User {
            "user"
        } else {
            "assistant"
        };
        lines.push(match provider {
            Provider::Claude=>json!({"type":role,"sessionId":session,"cwd":cursor.cwd,"mercury_checkpoint_truncated":cursor.truncated,"uuid":format!("checkpoint-{i}"),"message":{"id":format!("checkpoint-{i}"),"role":role,"content":m.message.text}}),
            Provider::Codex=>json!({"type":"response_item","payload":{"type":"message","id":format!("checkpoint-{i}"),"role":role,"content":[{"type":if role=="user"{"input_text"}else{"output_text"},"text":m.message.text}]}}),
        });
    }
    let mut bytes = Vec::new();
    for line in lines {
        serde_json::to_writer(&mut bytes, &line).map_err(|_| StorageError::Corrupt)?;
        bytes.push(b'\n');
    }
    let content_hash = hash(&bytes);
    let id = hash(
        format!(
            "{}:{session}:{}:{content_hash}",
            name(provider),
            cursor.offset + relative_offset
        )
        .as_bytes(),
    );
    let count: u32 = tx.query_row("SELECT count(*) FROM checkpoints", [], |r| r.get(0))?;
    if count >= 4096 {
        return Err(StorageError::Corrupt);
    }
    let directory = root.join("checkpoints");
    ensure_private_dir(&directory)?;
    let directory = directory.join(&id);
    ensure_private_dir(&directory)?;
    let path = directory.join(format!("{session}.jsonl"));
    if !path.exists() {
        let temp = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let result = fs::hard_link(&temp, &path);
        let _ = fs::remove_file(&temp);
        if let Err(e) = result {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.into());
            }
        }
        fs::File::open(&directory)?.sync_all()?;
    }
    transcript::check_path(&path).map_err(|_| StorageError::UnsafePath)?;
    let mut stored = Vec::new();
    fs::File::open(&path)?
        .take(SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut stored)?;
    if hash(&stored) != content_hash {
        return Err(StorageError::InvalidInput);
    }
    let checkpoint = Checkpoint {
        id: id.clone(),
        provider,
        session_id: session.into(),
        path,
        turns: cursor.turns,
        content_hash,
    };
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO checkpoints(id,body) VALUES(?1,?2)",
        params![
            id,
            serde_json::to_string(&checkpoint).map_err(|_| StorageError::Corrupt)?
        ],
    )?;
    cursor.emitted += inserted as u64;
    cursor.messages.clear();
    cursor.truncated = false;
    Ok(())
}

struct HashWriter<'a>(&'a mut sha2::Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
