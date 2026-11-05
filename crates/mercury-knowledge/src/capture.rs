//! Explicit review is the only path from session data to the Obsidian vault.
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
impl Index {
    pub fn propose(
        &self,
        source: &str,
        title: &str,
        project: &str,
        markdown: &str,
    ) -> Result<Proposal> {
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
        let id = hash(format!("{source}\0{project}\0{markdown}").as_bytes());
        if let Some(p) = self.proposal(&id)? {
            return Ok(p);
        }
        let output=format!("---\nid: mercury-{id}\ntitle: {}\nproject: {}\ntype: learning\nstatus: validated\nsource: {}\n---\n\n{markdown}\n",serde_json::to_string(title)?,serde_json::to_string(project)?,serde_json::to_string(source)?);
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
                    crate::store::private_write(&path, p.markdown.as_bytes())?;
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
        let sources = self.search(
            query,
            &Filter {
                project: Some(project.into()),
                ..Filter::default()
            },
            20,
        )?;
        let mut total = 0;
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|h| {
                total += h.text.len();
                total <= 24_000
            })
            .collect();
        let revision = self.status()?.revision;
        let id = hash(
            format!(
                "{revision}:{project}:{query}:{}",
                serde_json::to_string(&sources)?
            )
            .as_bytes(),
        );
        let snapshot = self
            .snapshot()?
            .ok_or(Error::Invalid("index the vault first"))?;
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
        let b = Bundle {
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
