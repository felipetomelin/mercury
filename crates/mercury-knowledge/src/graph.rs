use crate::backend::Model;
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub name: String,
    pub kind: String,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractedRelation {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub evidence: Vec<String>,
    pub quote: String,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    #[serde(default)]
    pub keywords: Vec<String>,
    pub entities: Vec<Entity>,
    pub relations: Vec<ExtractedRelation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunitySummary {
    pub id: String,
    pub members: Vec<String>,
    pub text: String,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Derived {
    #[serde(default)]
    pub model_metadata: Value,
    pub revision: String,
    pub fingerprint: String,
    pub vectors: BTreeMap<String, Vec<f32>>,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
    pub summaries: Vec<CommunitySummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub text: String,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grounded {
    pub insufficient: bool,
    pub claims: Vec<Claim>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub revision: String,
    pub mode: String,
    pub insufficient: bool,
    pub claims: Vec<Claim>,
    pub sources: Vec<Hit>,
    pub inferred_relations: Vec<Relation>,
    pub fingerprint: String,
}
fn grounded_schema() -> Value {
    json!({"type":"object","properties":{"insufficient":{"type":"boolean"},"claims":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"text":{"type":"string"},"evidence":{"type":"array","items":{"type":"string"}}},"required":["text","evidence"],"additionalProperties":false}}},"required":["insufficient","claims"],"additionalProperties":false})
}
fn extraction_schema() -> Value {
    json!({"type":"object","properties":{"keywords":{"type":"array","maxItems":32,"items":{"type":"string"}},"entities":{"type":"array","maxItems":50,"items":{"type":"object","properties":{"name":{"type":"string"},"kind":{"type":"string"},"evidence":{"type":"array","items":{"type":"string"}}},"required":["name","kind","evidence"],"additionalProperties":false}},"relations":{"type":"array","maxItems":100,"items":{"type":"object","properties":{"source":{"type":"string"},"target":{"type":"string"},"kind":{"type":"string","enum":["depends_on","decided_by","affected_by","resolved_by","documented_by","belongs_to","references"]},"evidence":{"type":"array","items":{"type":"string"}},"quote":{"type":"string","description":"Exact contiguous source quote containing BOTH source and target entity names and supporting the relationship"}},"required":["source","target","kind","evidence","quote"],"additionalProperties":false}}},"required":["keywords","entities","relations"],"additionalProperties":false})
}
fn checked(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn evidence_valid(ids: &[String], allowed: &BTreeSet<String>) -> bool {
    !ids.is_empty() && ids.len() <= 20 && ids.iter().all(|id| allowed.contains(id))
}
fn validate_grounded(g: &Grounded, allowed: &BTreeSet<String>) -> Result<()> {
    if g.claims.len() > 16
        || (!g.insufficient && g.claims.is_empty())
        || g.claims.iter().any(|c| {
            c.text.is_empty() || c.text.len() > 4000 || !evidence_valid(&c.evidence, allowed)
        })
    {
        return Err(Error::Invalid("ungrounded model response"));
    }
    Ok(())
}
fn grounded_call(
    model: &dyn Model,
    task: &str,
    input: &Value,
    allowed: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> Result<Grounded> {
    checked(cancel)?;
    let first = model.structured(task, input, &grounded_schema())?;
    let parsed: Result<Grounded> = serde_json::from_value(first.clone())
        .map_err(Error::from)
        .and_then(|value| {
            validate_grounded(&value, allowed)?;
            Ok(value)
        });
    if let Ok(value) = parsed {
        return Ok(value);
    }
    checked(cancel)?;
    let previous = if serde_json::to_string(&first)?.len() < 4096 {
        first
    } else {
        Value::Null
    };
    let value:Grounded=serde_json::from_value(model.structured(&format!("{task}; repair invalid citations and shape; use only allowed chunk IDs, never file names or community IDs; if unsupported return insufficient true and empty claims"),&json!({"original":input,"allowed_evidence":allowed,"previous":previous}),&grounded_schema())?)?;
    validate_grounded(&value, allowed)?;
    Ok(value)
}
fn validate_extraction(x: &Extraction, chunk: &Chunk) -> Result<()> {
    let allowed = BTreeSet::from([chunk.id.clone()]);
    if x.entities.len() > 50
        || x.relations.len() > 100
        || x.entities.iter().any(|e| {
            e.name.is_empty()
                || e.name.len() > 256
                || e.kind.len() > 64
                || !evidence_valid(&e.evidence, &allowed)
                || !chunk.text.to_lowercase().contains(&e.name.to_lowercase())
        })
    {
        return Err(Error::Invalid(
            "entity names must occur in the source; use only the supplied evidence ID",
        ));
    }
    if x.keywords.len() > 32 || x.keywords.iter().any(|k| k.len() > 128) {
        return Err(Error::Invalid("invalid semantic descriptors"));
    }
    for r in &x.relations {
        if !x.entities.iter().any(|e| e.name == r.source)
            || !x.entities.iter().any(|e| e.name == r.target)
            || !evidence_valid(&r.evidence, &allowed)
            || r.quote.is_empty()
            || !chunk.text.contains(&r.quote)
            || !r.quote.to_lowercase().contains(&r.source.to_lowercase())
            || !r.quote.to_lowercase().contains(&r.target.to_lowercase())
            || ![
                "depends_on",
                "decided_by",
                "affected_by",
                "resolved_by",
                "documented_by",
                "belongs_to",
                "references",
            ]
            .contains(&r.kind.as_str())
        {
            return Err(Error::Invalid("relation endpoints must match entity names; evidence ID must match chunk; quote must be exact source text containing both endpoint names; omit unsupported relations"));
        }
    }
    Ok(())
}
fn entity_id(project: &str, name: &str) -> String {
    hash(format!("{project}:{}", name.to_lowercase()).as_bytes())
}

/// Deterministic bounded label propagation; tie retains own label to avoid
/// collapsing a star's center and leaves into alternating partitions.
pub fn communities(edges: &[Relation]) -> Vec<Vec<String>> {
    let mut adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in edges {
        adj.entry(r.source.clone())
            .or_default()
            .insert(r.target.clone());
        adj.entry(r.target.clone())
            .or_default()
            .insert(r.source.clone());
    }
    let mut labels: BTreeMap<String, String> = adj.keys().map(|k| (k.clone(), k.clone())).collect();
    for _ in 0..20 {
        let mut changed = false;
        for (node, neighbors) in &adj {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for n in neighbors {
                *counts.entry(labels[n].clone()).or_default() += 1;
            }
            if let Some((best, _)) = counts
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            {
                if labels[node] != *best {
                    labels.insert(node.clone(), best.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (n, l) in labels {
        groups.entry(l).or_default().push(n);
    }
    groups.into_values().collect()
}
impl Index {
    pub fn build_graph(&mut self, model: &dyn Model, cancel: &AtomicBool) -> Result<Derived> {
        self.build_graph_validated(model, cancel, |_| Ok(()))
    }
    pub fn build_graph_verified(
        &mut self,
        model: &dyn Model,
        cancel: &AtomicBool,
        config: &Config,
    ) -> Result<Derived> {
        self.build_graph_validated(model, cancel, |s| crate::capture::verify_vault(config, s))
    }
    fn build_graph_validated(
        &mut self,
        model: &dyn Model,
        cancel: &AtomicBool,
        validate: impl FnOnce(&Snapshot) -> Result<()>,
    ) -> Result<Derived> {
        let s = self
            .snapshot()?
            .ok_or(Error::Invalid("index the vault first"))?;
        let mut d = Derived {
            revision: s.revision.clone(),
            fingerprint: model.fingerprint(),
            model_metadata: Value::Null,
            vectors: BTreeMap::new(),
            entities: vec![],
            relations: s.relations.clone(),
            summaries: vec![],
        };
        let mut dimension = None;
        let mut vector_bytes = 0usize;
        if s.notes.iter().map(|n| n.chunks.len()).sum::<usize>() > 4096 {
            return Err(Error::Invalid(
                "GraphRAG chunk limit exceeded; narrow the vault",
            ));
        }
        for note in &s.notes {
            for chunk in &note.chunks {
                checked(cancel)?;
                let vector = model.embed(&chunk.text)?;
                crate::backend::validate_vector(&vector)?;
                if dimension.is_some_and(|dim| dim != vector.len()) {
                    return Err(Error::Invalid("embedding dimensions changed"));
                }
                dimension = Some(vector.len());
                vector_bytes += vector.len() * 4;
                if vector_bytes > 64 * 1024 * 1024 {
                    return Err(Error::Invalid("embedding memory budget exceeded"));
                }
                d.vectors.insert(chunk.id.clone(), vector);
                let input = json!({"chunk_id":chunk.id,"text":chunk.text,"project":note.project});
                let first=match self.cached_extraction(chunk,&model.fingerprint())? {Some(value)=>value,None=>model.structured("extract entities and typed relationships; entity names must be exact source substrings; every quote must contain both endpoint names",&input,&extraction_schema())?};
                let mut x: Extraction = serde_json::from_value(first.clone())?;
                if let Err(error) = validate_extraction(&x, chunk) {
                    checked(cancel)?;
                    x=serde_json::from_value(model.structured("extract and repair invalid evidence; omit any unsupported relationship; exact entity names and quotes only",&json!({"chunk":input,"validation_error":error.to_string(),"previous":first}),&extraction_schema())?)?;
                }
                validate_extraction(&x, chunk)?;
                self.cache_extraction(chunk, &model.fingerprint(), &serde_json::to_value(&x)?)?;
                if !x.keywords.is_empty() {
                    d.vectors.insert(
                        chunk.id.clone(),
                        model.embed(&format!("{} {}", chunk.text, x.keywords.join(" ")))?,
                    );
                }
                for r in x.relations {
                    d.relations.push(Relation {
                        source: entity_id(&note.project, &r.source),
                        target: entity_id(&note.project, &r.target),
                        kind: r.kind,
                        origin: "inferred".into(),
                        evidence: r.evidence,
                    });
                }
                d.entities.extend(x.entities);
            }
        }
        let groups = communities(&d.relations);
        for members in groups {
            checked(cancel)?;
            let evidence: BTreeSet<_> = d
                .relations
                .iter()
                .filter(|r| members.contains(&r.source) || members.contains(&r.target))
                .flat_map(|r| r.evidence.iter().cloned())
                .collect();
            // Bounded windows preserve citations; no silent claims of full coverage.
            let mut chunks = Vec::new();
            let mut bytes = 0;
            for c in s
                .notes
                .iter()
                .flat_map(|n| n.chunks.iter())
                .filter(|c| evidence.contains(&c.id))
            {
                if bytes + c.text.len() > 16_000 || chunks.len() >= 20 {
                    break;
                }
                bytes += c.text.len();
                chunks.push(json!({"id":c.id,"text":c.text}));
            }
            if chunks.is_empty() {
                continue;
            }
            let allowed: BTreeSet<_> = chunks
                .iter()
                .filter_map(|v| v["id"].as_str().map(str::to_owned))
                .collect();
            let summary = grounded_call(
                model,
                "summarize this community using only provided evidence",
                &json!({"chunks":chunks}),
                &allowed,
                cancel,
            )?;
            let used: BTreeSet<_> = summary
                .claims
                .iter()
                .flat_map(|c| c.evidence.iter().cloned())
                .collect();
            if !summary.claims.is_empty() {
                d.summaries.push(CommunitySummary {
                    id: hash(members.join(":").as_bytes()),
                    members,
                    text: serde_json::to_string(&summary.claims)?,
                    evidence: used.into_iter().collect(),
                });
            }
        }
        checked(cancel)?;
        validate(&s)?;
        d.model_metadata = model.metadata();
        self.save_derived(&s.revision, &d.fingerprint, &serde_json::to_value(&d)?)?;
        Ok(d)
    }
    pub fn hybrid(
        &self,
        query: &str,
        filter: &Filter,
        limit: usize,
        model: &dyn Model,
    ) -> Result<Vec<Hit>> {
        if !(1..=100).contains(&limit) || query.len() > 2048 {
            return Err(Error::Invalid("invalid hybrid request"));
        }
        let s = self
            .snapshot()?
            .ok_or(Error::Invalid("index the vault first"))?;
        let d: Derived = serde_json::from_value(self.derived(&model.fingerprint())?.ok_or(
            Error::Invalid("GraphRAG index missing or model changed; rebuild"),
        )?)?;
        let q = model.query_vector(query)?;
        crate::backend::validate_vector(&q)?;
        let mut semantic = Vec::new();
        for n in s
            .notes
            .iter()
            .filter(|n| crate::store::matches_filter(n, filter, &s))
        {
            for c in &n.chunks {
                if let Some(v) = d.vectors.get(&c.id) {
                    semantic.push(crate::store::hit(n, c, cosine(&q, v)?));
                }
            }
        }
        semantic.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        let textual = self.search(query, filter, 100)?;
        let mut ranks: BTreeMap<String, Hit> = BTreeMap::new();
        for list in [textual, semantic.into_iter().take(100).collect()] {
            for (rank, mut h) in list.into_iter().enumerate() {
                let score = 1.0 / (60 + rank + 1) as f64;
                if let Some(v) = ranks.get_mut(&h.chunk_id) {
                    v.score += score
                } else {
                    h.score = score;
                    ranks.insert(h.chunk_id.clone(), h);
                }
            }
        }
        let mut out: Vec<_> = ranks.into_values().collect();
        out.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        out.truncate(limit);
        Ok(out)
    }
    pub fn ask(
        &self,
        question: &str,
        mode: &str,
        filter: &Filter,
        model: &dyn Model,
        cancel: &AtomicBool,
        max_chars: usize,
    ) -> Result<Answer> {
        if !["local", "global"].contains(&mode)
            || question.is_empty()
            || question.len() > 2048
            || max_chars > 64_000
        {
            return Err(Error::Invalid("invalid GraphRAG query"));
        }
        checked(cancel)?;
        let s = self
            .snapshot()?
            .ok_or(Error::Invalid("index the vault first"))?;
        let d: Derived = serde_json::from_value(self.derived(&model.fingerprint())?.ok_or(
            Error::Invalid("GraphRAG index missing or model changed; rebuild"),
        )?)?;
        let mut hits = self.hybrid(question, filter, 10, model)?;
        let mut intermediate = Vec::new();
        if mode == "local" {
            let mut seen: BTreeSet<_> = hits.iter().map(|h| h.note_id.clone()).collect();
            let mut evidence: BTreeSet<_> = hits.iter().map(|h| h.chunk_id.clone()).collect();
            for _ in 0..2 {
                let edges: Vec<_> = d
                    .relations
                    .iter()
                    .filter(|r| {
                        seen.contains(&r.source)
                            || seen.contains(&r.target)
                            || r.evidence.iter().any(|e| evidence.contains(e))
                    })
                    .take(100)
                    .collect();
                for edge in edges {
                    for id in [&edge.source, &edge.target] {
                        if seen.len() < 50 {
                            seen.insert(id.clone());
                        }
                    }
                    evidence.extend(edge.evidence.iter().cloned());
                }
            }
            for note in s
                .notes
                .iter()
                .filter(|n| crate::store::matches_filter(n, filter, &s))
            {
                for chunk in note
                    .chunks
                    .iter()
                    .filter(|c| seen.contains(&note.id) || evidence.contains(&c.id))
                {
                    if hits.len() < 20 && !hits.iter().any(|h| h.chunk_id == chunk.id) {
                        hits.push(crate::store::hit(note, chunk, 0.0));
                    }
                }
            }
        } else {
            // Global map/reduce across eligible summaries, with source chunks rechecked.
            hits.clear();
            for summary in d.summaries.iter().take(16) {
                checked(cancel)?;
                let mut source = Vec::new();
                for n in s
                    .notes
                    .iter()
                    .filter(|n| crate::store::matches_filter(n, filter, &s))
                {
                    for c in n.chunks.iter().filter(|c| summary.evidence.contains(&c.id)) {
                        source.push(crate::store::hit(n, c, 0.0));
                    }
                }
                if source.is_empty() || source.len() != summary.evidence.len() {
                    continue;
                }
                let allowed: BTreeSet<_> = source.iter().map(|h| h.chunk_id.clone()).collect();
                let partial=grounded_call(model,"answer question from community summary; if irrelevant return insufficient true and empty claims",&json!({"question":question,"summary":summary.text,"allowed_evidence":allowed}),&allowed,cancel)?;
                intermediate.extend(partial.claims);
                for h in source {
                    if !hits.iter().any(|x| x.chunk_id == h.chunk_id) {
                        hits.push(h)
                    }
                }
            }
        }
        let mut budget = question.len() + 1024;
        hits.retain(|h| {
            budget += h.text.len() + h.chunk_id.len() + 128;
            budget <= max_chars && budget <= 24_000
        });
        hits.truncate(20);
        let allowed: BTreeSet<_> = hits.iter().map(|h| h.chunk_id.clone()).collect();
        intermediate.retain(|c| evidence_valid(&c.evidence, &allowed));
        if hits.is_empty() {
            return Ok(Answer {
                revision: s.revision,
                mode: mode.into(),
                insufficient: true,
                claims: vec![],
                sources: vec![],
                inferred_relations: vec![],
                fingerprint: model.fingerprint(),
            });
        }
        checked(cancel)?;
        let grounded=grounded_call(model,"answer only supported claims; cite exact chunk IDs; if unsupported return insufficient true and empty claims",&json!({"question":question,"sources":hits,"partial_claims":intermediate}),&allowed,cancel)?;
        checked(cancel)?;
        Ok(Answer {
            revision: s.revision,
            mode: mode.into(),
            insufficient: grounded.insufficient,
            claims: grounded.claims,
            sources: hits,
            inferred_relations: d
                .relations
                .into_iter()
                .filter(|r| {
                    r.origin == "inferred" && r.evidence.iter().any(|e| allowed.contains(e))
                })
                .take(100)
                .collect(),
            fingerprint: model.fingerprint(),
        })
    }
}
fn cosine(a: &[f32], b: &[f32]) -> Result<f64> {
    crate::backend::validate_vector(b)?;
    if a.len() != b.len() {
        return Err(Error::Invalid("embedding dimensions mismatch"));
    }
    let dot: f64 = a.iter().zip(b).map(|(a, b)| *a as f64 * *b as f64).sum();
    let norm = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();
    Ok(dot / (norm(a) * norm(b)))
}
