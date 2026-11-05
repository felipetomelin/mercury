use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub vault: PathBuf,
    pub exclusions: Vec<String>,
    pub allowed_providers: Vec<String>,
    pub models: BTreeMap<String, String>,
    pub executables: BTreeMap<String, PathBuf>,
    pub max_model_calls: usize,
    pub max_call_budget_usd: f64,
    pub allow_external: bool,
    pub allow_route_external: bool,
    pub route_destinations: Vec<String>,
    pub max_notes: usize,
    pub max_note_bytes: usize,
    pub max_total_bytes: usize,
    pub timeout_secs: u64,
    pub max_context_chars: usize,
    pub projects: BTreeMap<String, Vec<String>>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            vault: PathBuf::new(),
            exclusions: vec![],
            allowed_providers: vec![],
            models: BTreeMap::new(),
            executables: BTreeMap::new(),
            max_model_calls: 128,
            max_call_budget_usd: 0.10,
            allow_external: false,
            allow_route_external: false,
            route_destinations: vec![],
            max_notes: 10_000,
            max_note_bytes: 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            timeout_secs: 60,
            max_context_chars: 32_000,
            projects: BTreeMap::new(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> crate::Result<()> {
        if self.enabled
            && (!self.vault.is_absolute()
                || self
                    .vault
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)))
        {
            return Err(crate::Error::UnsafePath);
        }
        if !(1..=10_000).contains(&self.max_notes)
            || !(1..=1024 * 1024).contains(&self.max_note_bytes)
            || self.max_total_bytes == 0
            || self.max_total_bytes > 256 * 1024 * 1024
            || !(1..=300).contains(&self.timeout_secs)
            || !(1024..=64_000).contains(&self.max_context_chars)
            || self.exclusions.len() > 100
            || self.projects.len() > 100
        {
            return Err(crate::Error::Invalid("invalid knowledge limits"));
        }
        if self
            .projects
            .iter()
            .any(|(k, v)| k.len() > 128 || v.len() > 100 || v.iter().any(|a| a.len() > 256))
        {
            return Err(crate::Error::Invalid("invalid knowledge projects"));
        }
        if self
            .executables
            .iter()
            .any(|(p, path)| !["claude", "codex"].contains(&p.as_str()) || !path.is_absolute())
        {
            return Err(crate::Error::Invalid("invalid native CLI executable"));
        }
        if self.allowed_providers.len() > 2
            || self
                .allowed_providers
                .iter()
                .any(|p| !["claude", "codex"].contains(&p.as_str()))
            || self.models.iter().any(|(p, m)| {
                !["claude", "codex"].contains(&p.as_str())
                    || m.len() > 128
                    || m.chars().any(|c| c.is_control())
            })
            || !(1..=4096).contains(&self.max_model_calls)
            || !self.max_call_budget_usd.is_finite()
            || !(0.001..=5.0).contains(&self.max_call_budget_usd)
        {
            return Err(crate::Error::Invalid(
                "invalid native knowledge provider configuration",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub heading: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub target: String,
    pub kind: String,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub explicit_id: bool,
    pub path: String,
    pub title: String,
    pub project: String,
    pub kind: String,
    pub status: String,
    pub modified: u64,
    pub hash: String,
    pub properties: BTreeMap<String, Vec<String>>,
    pub chunks: Vec<Chunk>,
    pub links: Vec<Link>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub project: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub service: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub chunk_id: String,
    pub note_id: String,
    pub path: String,
    pub heading: String,
    pub text: String,
    pub status: String,
    pub project: String,
    pub modified: u64,
    pub score: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub source: String,
    pub kind: String,
    pub target: String,
    pub origin: String,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: String,
    pub notes: Vec<Note>,
    pub relations: Vec<Relation>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub schema_version: u32,
    pub revision: String,
    pub notes: usize,
    pub chunks: usize,
    pub relations: usize,
    pub summaries: usize,
    pub model: Option<String>,
    pub updated: u64,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
