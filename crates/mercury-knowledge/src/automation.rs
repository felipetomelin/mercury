//! Private durable capture journal and a conservative automatic-model budget.
use crate::{backend::Model, *};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub id: String,
    pub provider: String,
    pub session_id: String,
    pub transcript: PathBuf,
    pub fingerprint: String,
    pub state: String,
    pub proposal_id: Option<String>,
    pub attempts: u32,
    pub next_at: u64,
    pub error: Option<String>,
}
impl Capture {
    pub fn new(provider: &str, session: &str, path: PathBuf, fingerprint: String) -> Result<Self> {
        if !["claude", "codex"].contains(&provider)
            || !mercury_usage::locate::is_safe_session_id(session)
            || !path.is_absolute()
            || path.as_os_str().len() > 4096
            || !(16..=256).contains(&fingerprint.len())
            || !fingerprint
                .bytes()
                .all(|c| c.is_ascii_hexdigit() || c == b':' || c == b'-')
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(Error::Invalid("invalid automatic capture"));
        }
        Ok(Self {
            id: hash(format!("observed:{provider}:{session}:{fingerprint}").as_bytes()),
            provider: provider.into(),
            session_id: session.into(),
            transcript: path,
            fingerprint,
            state: "queued".into(),
            proposal_id: None,
            attempts: 0,
            next_at: now(),
            error: None,
        })
    }
}

impl Index {
    pub fn automation_setup(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS auto_capture(id TEXT PRIMARY KEY, state TEXT NOT NULL, body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS auto_state(id TEXT PRIMARY KEY, body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS auto_budget(day INTEGER PRIMARY KEY, used INTEGER NOT NULL);",
        )?;
        Ok(())
    }
    pub fn enqueue_capture(&self, capture: &Capture) -> Result<bool> {
        self.automation_setup()?;
        if self.capture_record(&capture.id)?.is_some() {
            return Ok(false);
        }
        let count: u32 = self.db.query_row(
            "SELECT count(*) FROM auto_capture WHERE state NOT IN ('done','failed')",
            [],
            |r| r.get(0),
        )?;
        if count >= 256 {
            return Err(Error::Invalid("automatic capture backlog full"));
        }
        Ok(self.db.execute(
            "INSERT OR IGNORE INTO auto_capture VALUES(?1,?2,?3)",
            params![capture.id, capture.state, serde_json::to_string(capture)?],
        )? != 0)
    }
    pub fn capture_record(&self, id: &str) -> Result<Option<Capture>> {
        self.automation_setup()?;
        let body: Option<String> = self
            .db
            .query_row("SELECT body FROM auto_capture WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        body.map(|b| serde_json::from_str(&b).map_err(Error::from))
            .transpose()
    }
    pub fn save_capture(&self, capture: &Capture) -> Result<()> {
        self.db.execute(
            "UPDATE auto_capture SET state=?2,body=?3 WHERE id=?1",
            params![capture.id, capture.state, serde_json::to_string(capture)?],
        )?;
        Ok(())
    }
    pub fn pending_captures(&self, timestamp: u64) -> Result<Vec<Capture>> {
        self.automation_setup()?;
        let mut query=self.db.prepare("SELECT body FROM auto_capture WHERE state NOT IN ('done','failed') ORDER BY rowid LIMIT 256")?;
        let rows = query
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut pending = Vec::new();
        for body in rows {
            let capture: Capture = serde_json::from_str(&body)?;
            if capture.next_at <= timestamp {
                pending.push(capture);
            }
        }
        Ok(pending)
    }
    pub fn auto_state(&self, id: &str) -> Result<Value> {
        self.automation_setup()?;
        let body: Option<String> = self
            .db
            .query_row("SELECT body FROM auto_state WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(body
            .map(|b| serde_json::from_str(&b))
            .transpose()?
            .unwrap_or(Value::Null))
    }
    pub fn save_auto_state(&self, id: &str, value: &Value) -> Result<()> {
        self.automation_setup()?;
        self.db.execute(
            "INSERT OR REPLACE INTO auto_state VALUES(?1,?2)",
            params![id, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
    pub fn automation_status(&self) -> Result<Value> {
        self.automation_setup()?;
        let mut counts = std::collections::BTreeMap::<String, u32>::new();
        let mut q = self
            .db
            .prepare("SELECT state,count(*) FROM auto_capture GROUP BY state")?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))? {
            let (state, count) = row?;
            counts.insert(state, count);
        }
        let used: u32 = self
            .db
            .query_row(
                "SELECT used FROM auto_budget WHERE day=?1",
                [(now() / 86400) as i64],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let mut failures = Vec::new();
        let mut query = self.db.prepare(
            "SELECT body FROM auto_capture WHERE state='failed' ORDER BY rowid DESC LIMIT 8",
        )?;
        for body in query.query_map([], |r| r.get::<_, String>(0))? {
            let c: Capture = serde_json::from_str(&body?)?;
            failures.push(json!({"id":c.id,"provider":c.provider,"attempts":c.attempts,"error":c.error,"proposal_id":c.proposal_id}));
        }
        Ok(
            json!({"captures":counts,"failures":failures,"runtime":self.auto_state("runtime")?,"graph":self.auto_state("graph")?,"reserved_calls_today":used}),
        )
    }
    /// Explicit rebuild is also the recovery action for exhausted automatic work.
    pub fn retry_automation(&self) -> Result<()> {
        self.automation_setup()?;
        let mut q = self
            .db
            .prepare("SELECT body FROM auto_capture WHERE state='failed'")?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(q);
        for body in rows {
            let mut capture: Capture = serde_json::from_str(&body)?;
            capture.state = "queued".into();
            capture.attempts = 0;
            capture.next_at = now();
            capture.error = None;
            self.save_capture(&capture)?;
        }
        self.save_auto_state("graph", &Value::Null)
    }
    /// Reserve two provider attempts per structured call, covering native fallback.
    pub fn reserve_automatic_call(&self, timestamp: u64, limit: u32) -> Result<()> {
        self.automation_setup()?;
        let day = (timestamp / 86400) as i64;
        self.db
            .execute("INSERT OR IGNORE INTO auto_budget VALUES(?1,0)", [day])?;
        let changed = self.db.execute(
            "UPDATE auto_budget SET used=used+2 WHERE day=?1 AND used+2<=?2",
            params![day, limit],
        )?;
        if changed == 0 {
            return Err(Error::Invalid("automatic daily model budget exhausted"));
        }
        self.db
            .execute("DELETE FROM auto_budget WHERE day+7<?1", [day])?;
        Ok(())
    }
    pub fn proposal_by_source(&self, source: &str) -> Result<Option<crate::capture::Proposal>> {
        let body:Option<String>=self.db.query_row("SELECT body FROM proposals WHERE json_extract(body,'$.source')=?1 ORDER BY rowid LIMIT 1",[source],|r|r.get(0)).optional()?;
        body.map(|b| serde_json::from_str(&b).map_err(Error::from))
            .transpose()
    }
}

pub struct BudgetedModel<'a> {
    pub model: &'a dyn Model,
    pub index: &'a Index,
    pub daily_limit: u32,
}
impl Model for BudgetedModel<'_> {
    fn fingerprint(&self) -> String {
        self.model.fingerprint()
    }
    fn metadata(&self) -> Value {
        self.model.metadata()
    }
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.model.embed(text)
    }
    fn structured(&self, task: &str, input: &Value, schema: &Value) -> Result<Value> {
        if crate::backend::contains_recognizable_secret(&serde_json::to_string(input)?) {
            return Err(Error::Invalid(
                "automatic model input contains a recognizable secret",
            ));
        }
        self.index.reserve_automatic_call(now(), self.daily_limit)?;
        self.model.structured(task, input, schema)
    }
}
