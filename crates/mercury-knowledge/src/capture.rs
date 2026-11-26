//! Journaled publication; automatic notes retain proposed epistemic status.
use crate::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub source: String,
    pub title: String,
    pub project: String,
    pub markdown: String,
    pub hash: String,
    pub state: String,
    pub created: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<ContextBudget>,
    pub id: String,
    pub revision: String,
    pub project: String,
    pub sources: Vec<Hit>,
    #[serde(default)]
    pub validated_decisions: Vec<String>,
    #[serde(default)]
    pub pending_sources: Vec<String>,
    #[serde(default)]
    pub deprecated_sources: Vec<String>,
    #[serde(default)]
    pub inferred_relations: Vec<Relation>,
    pub created: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_bytes: usize,
    pub max_sources: usize,
    pub estimated_tokens: usize,
    pub estimate_method: String,
}
impl Index {
    pub fn propose(
        &self,
        source: &str,
        title: &str,
        project: &str,
        markdown: &str,
    ) -> Result<Proposal> {
        self.propose_with_status(source, title, project, markdown, "validated")
    }
    pub fn propose_automatic(
        &self,
        source: &str,
        title: &str,
        project: &str,
        markdown: &str,
    ) -> Result<Proposal> {
        validate_proposal_input(source, title, project, markdown)?;
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS automatic_lessons(project TEXT NOT NULL,digest TEXT NOT NULL,proposal TEXT NOT NULL,PRIMARY KEY(project,digest)); CREATE TABLE IF NOT EXISTS lesson_observations(source TEXT PRIMARY KEY,proposal TEXT NOT NULL);")?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let digest = hash(markdown.trim().as_bytes());
        let known: Option<String> = self
            .db
            .query_row(
                "SELECT proposal FROM automatic_lessons WHERE project=?1 AND digest=?2",
                params![project, digest],
                |r| r.get(0),
            )
            .optional()?;
        let proposal = match known {
            Some(id) => self
                .proposal(&id)?
                .ok_or(Error::Invalid("missing deduplicated proposal"))?,
            None => self.propose_with_status(source, title, project, markdown, "proposed")?,
        };
        self.db.execute(
            "INSERT OR IGNORE INTO automatic_lessons VALUES(?1,?2,?3)",
            params![project, digest, proposal.id],
        )?;
        self.db.execute(
            "INSERT OR IGNORE INTO lesson_observations VALUES(?1,?2)",
            params![source, proposal.id],
        )?;
        tx.commit()?;
        Ok(proposal)
    }
    fn propose_with_status(
        &self,
        source: &str,
        title: &str,
        project: &str,
        markdown: &str,
        status: &str,
    ) -> Result<Proposal> {
        validate_proposal_input(source, title, project, markdown)?;
        let id = hash(format!("{source}\0{project}\0{markdown}").as_bytes());
        if let Some(p) = self.proposal(&id)? {
            return Ok(p);
        }
        let output=format!("---\nid: mercury-{id}\ntitle: {}\nproject: {}\ntype: learning\nstatus: {status}\nsource: {}\n---\n\n{markdown}\n",serde_json::to_string(title)?,serde_json::to_string(project)?,serde_json::to_string(source)?);
        let p = Proposal {
            id,
            source: source.into(),
            title: title.into(),
            project: project.into(),
            hash: hash(output.as_bytes()),
            markdown: output,
            state: "proposed".into(),
            created: now(),
        };
        self.write_proposal(&p)?;
        Ok(p)
    }
    pub fn proposal(&self, id: &str) -> Result<Option<Proposal>> {
        valid_id(id)?;
        let body: Option<String> = self
            .db
            .query_row("SELECT body FROM proposals WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        body.map(|b| serde_json::from_str(&b).map_err(Error::from))
            .transpose()
    }
    pub fn proposals(&self) -> Result<Vec<Proposal>> {
        let mut q = self
            .db
            .prepare("SELECT body FROM proposals ORDER BY rowid DESC LIMIT 100")?;
        let bodies = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        bodies
            .into_iter()
            .map(|b| serde_json::from_str(&b).map_err(Error::from))
            .collect()
    }
    fn write_proposal(&self, p: &Proposal) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO proposals VALUES(?1,?2)",
            params![p.id, serde_json::to_string(p)?],
        )?;
        Ok(())
    }
    pub fn review(&self, config: &Config, id: &str, action: &str) -> Result<Proposal> {
        let mut p = self
            .proposal(id)?
            .ok_or(Error::Invalid("unknown capture proposal"))?;
        crate::ingest::check_components(&config.vault)?;
        let vault = fs::canonicalize(&config.vault)?;
        let managed = vault.join("Mercury");
        if !managed.exists() {
            crate::store::private_dir(&managed)?;
        }
        crate::ingest::check_components(&managed)?;
        let path = managed.join(format!("{}.md", p.id));
        match action {
            "accept" => {
                if p.state == "accepted" {
                    if hash(&crate::ingest::read_safe(&path, 64_000)?) != p.hash {
                        return Err(Error::Conflict);
                    }
                    return Ok(p);
                }
                if !["proposed", "prepared"].contains(&p.state.as_str()) {
                    return Err(Error::Conflict);
                }
                // Journal first; crash recovery only accepts exactly our previously prepared bytes.
                if p.state == "prepared" && path.exists() {
                    if hash(&crate::ingest::read_safe(&path, 64_000)?) != p.hash {
                        return Err(Error::Conflict);
                    }
                } else {
                    if path.exists() {
                        return Err(Error::Conflict);
                    }
                    p.state = "prepared".into();
                    self.write_proposal(&p)?;
                    crate::store::publish_new(&path, p.markdown.as_bytes())?;
                    fs::File::open(&managed)?.sync_all()?;
                }
                p.state = "accepted".into();
                self.write_proposal(&p)?;
            }
            "reject" => {
                if p.state != "proposed" {
                    return Err(Error::Conflict);
                }
                p.state = "rejected".into();
                self.write_proposal(&p)?;
            }
            "undo" => {
                if !["accepted", "undo_prepared"].contains(&p.state.as_str()) {
                    return Err(Error::Conflict);
                }
                if path.exists() {
                    if hash(&crate::ingest::read_safe(&path, 64_000)?) != p.hash {
                        return Err(Error::Conflict);
                    }
                    p.state = "undo_prepared".into();
                    self.write_proposal(&p)?;
                    fs::remove_file(&path)?;
                    fs::File::open(&managed)?.sync_all()?;
                } else if p.state != "undo_prepared" {
                    return Err(Error::Conflict);
                }
                p.state = "undone".into();
                self.write_proposal(&p)?;
            }
            _ => return Err(Error::Invalid("invalid capture review action")),
        }
        Ok(p)
    }
    pub fn context(&self, query: &str, project: &str) -> Result<Bundle> {
        if project.is_empty() || project.len() > 128 {
            return Err(Error::Invalid("context requires a project"));
        }
        const MAX_BYTES: usize = 8192;
        let revision = self.status()?.revision;
        // This endpoint is deliberately model-free. Its cache includes the
        // retrieval policy, trust rules and complete output budget.
        let id = hash(&serde_json::to_vec(
            &serde_json::json!({"policy":"local-context-v2:fts+hash+explicit-hop1:trust-status","max_bytes":8192,"max_sources":8,"revision":revision,"project":project,"query":query}),
        )?);
        if query.len() > 2048 || query.chars().any(char::is_control) {
            return Err(Error::Invalid("invalid knowledge search"));
        }
        let cached: Option<String> = self
            .db
            .query_row(
                "SELECT body FROM bundles WHERE id=?1 AND revision=?2",
                params![id, revision],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(body) = cached {
            return Ok(serde_json::from_str(&body)?);
        }
        let snapshot = self
            .snapshot()?
            .ok_or(Error::Invalid("index the vault first"))?;
        let mut candidates = self.search(
            query,
            &Filter {
                project: Some(project.into()),
                ..Filter::default()
            },
            20,
        )?;
        let vector = crate::backend::local_features(query);
        let mut semantic = Vec::new();
        for n in snapshot
            .notes
            .iter()
            .filter(|n| n.project == project && n.status != "deprecated")
        {
            for c in &n.chunks {
                let v = crate::backend::local_features(&c.text);
                let score: f64 = vector
                    .iter()
                    .zip(v)
                    .map(|(a, b)| f64::from(*a) * f64::from(b))
                    .sum();
                if score > 0.05 {
                    semantic.push(crate::store::hit(n, c, score));
                }
            }
        }
        semantic.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        candidates.extend(semantic.into_iter().take(20));
        let seeds: std::collections::BTreeSet<_> = candidates
            .iter()
            .take(4)
            .map(|h| h.note_id.clone())
            .collect();
        // Expand one explicit hop only; inferred edges never become decisions.
        let neighbors: std::collections::BTreeSet<_> = snapshot
            .relations
            .iter()
            .filter_map(|r| {
                if r.origin != "explicit" {
                    return None;
                }
                if seeds.contains(&r.source) {
                    Some(r.target.clone())
                } else if seeds.contains(&r.target) {
                    Some(r.source.clone())
                } else {
                    None
                }
            })
            .collect();
        for n in snapshot
            .notes
            .iter()
            .filter(|n| {
                n.project == project && n.status != "deprecated" && neighbors.contains(&n.id)
            })
            .take(2)
        {
            if let Some(c) = n
                .chunks
                .iter()
                .find(|c| c.heading != "Properties")
                .or(n.chunks.first())
            {
                candidates.insert(candidates.len().min(2), crate::store::hit(n, c, 0.0));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut notes = std::collections::BTreeMap::<String, usize>::new();
        let sources: Vec<_> = candidates
            .into_iter()
            .map(|mut h| {
                h.text
                    .truncate(h.text.floor_char_boundary(768.min(h.text.len())));
                h
            })
            .filter(|h| {
                let count = notes.entry(h.note_id.clone()).or_default();
                if *count >= 2 || !seen.insert((h.status.clone(), hash(h.text.trim().as_bytes()))) {
                    return false;
                }
                *count += 1;
                true
            })
            .take(8)
            .collect();
        let validated_decisions = sources
            .iter()
            .filter(|h| {
                h.status == "validated"
                    && snapshot
                        .notes
                        .iter()
                        .any(|n| n.id == h.note_id && n.kind == "decision")
            })
            .map(|h| h.chunk_id.clone())
            .collect();
        let pending_sources = sources
            .iter()
            .filter(|h| ["proposed", "unreviewed"].contains(&h.status.as_str()))
            .map(|h| h.chunk_id.clone())
            .collect();
        let deprecated_sources = sources
            .iter()
            .filter(|h| h.status == "deprecated")
            .map(|h| h.chunk_id.clone())
            .collect();
        let mut b = Bundle {
            budget: Some(ContextBudget {
                max_bytes: MAX_BYTES,
                max_sources: 8,
                estimated_tokens: 0,
                estimate_method: "UTF-8 JSON bytes / 4, approximate; not tokenizer measured".into(),
            }),
            validated_decisions,
            pending_sources,
            deprecated_sources,
            inferred_relations: vec![],
            id,
            revision,
            project: project.into(),
            sources,
            created: now(),
        };
        // Count the complete serialized response, including citations, paths,
        // status and JSON escaping. Trim on UTF-8 boundaries, then rebuild IDs.
        loop {
            let estimate = serde_json::to_vec(&b)?.len().div_ceil(4);
            if let Some(budget) = &mut b.budget {
                budget.estimated_tokens = estimate;
            }
            if serde_json::to_vec(&b)?.len() <= MAX_BYTES {
                break;
            }
            let Some(last) = b.sources.last_mut() else {
                return Err(Error::Invalid("context metadata exceeds budget"));
            };
            if last.text.len() > 256 {
                last.text.truncate(
                    last.text
                        .floor_char_boundary(last.text.len().saturating_sub(256)),
                );
            } else {
                b.sources.pop();
            }
            let ids: std::collections::BTreeSet<_> =
                b.sources.iter().map(|h| h.chunk_id.clone()).collect();
            b.validated_decisions.retain(|id| ids.contains(id));
            b.pending_sources.retain(|id| ids.contains(id));
            b.deprecated_sources.retain(|id| ids.contains(id));
        }
        self.db.execute(
            "DELETE FROM bundles WHERE id IN (SELECT id FROM bundles ORDER BY rowid DESC LIMIT -1 OFFSET 511)", [],
        )?;
        self.db.execute(
            "INSERT OR REPLACE INTO bundles VALUES(?1,?2,?3)",
            params![b.id, b.revision, serde_json::to_string(&b)?],
        )?;
        Ok(b)
    }
    pub fn route_bundle(&self, id: &str, project: &str, allow_external: bool) -> Result<Bundle> {
        valid_id(id)?;
        if !allow_external {
            return Err(Error::Invalid("route context egress is not enabled"));
        }
        let body: Option<String> = self
            .db
            .query_row(
                "SELECT body FROM bundles WHERE id=?1 AND revision=?2",
                params![id, self.status()?.revision],
                |r| r.get(0),
            )
            .optional()?;
        let b: Bundle = serde_json::from_str(&body.ok_or(Error::Conflict)?)?;
        if b.project != project {
            return Err(Error::Conflict);
        }
        Ok(b)
    }
}
fn valid_id(id: &str) -> Result<()> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid("invalid knowledge ID"));
    }
    Ok(())
}

/// Check after generation and before publication to detect a concurrent source
/// edit. This supplements scan-time checks; it is not filesystem-wide CAS.
pub fn verify_vault(config: &Config, snapshot: &Snapshot) -> Result<()> {
    let current = crate::scan(config, &std::sync::atomic::AtomicBool::new(false))?;
    if current.revision != snapshot.revision {
        return Err(Error::Conflict);
    }
    Ok(())
}

fn validate_proposal_input(source: &str, title: &str, project: &str, markdown: &str) -> Result<()> {
    if source.is_empty()
        || source.len() > 512
        || title.is_empty()
        || title.len() > 256
        || project.len() > 128
        || markdown.is_empty()
        || markdown.len() > 32_000
        || title.chars().any(char::is_control)
        || project.chars().any(char::is_control)
    {
        return Err(Error::Invalid("invalid capture proposal"));
    }
    Ok(())
}
