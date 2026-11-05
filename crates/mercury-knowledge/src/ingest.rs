use crate::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Read,
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) fn check_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::UnsafePath);
    }
    let mut p = PathBuf::new();
    for c in path.components() {
        if matches!(c, Component::ParentDir) {
            return Err(Error::UnsafePath);
        }
        p.push(c.as_os_str());
        if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::UnsafePath);
        }
    }
    Ok(())
}
use std::path::PathBuf;
pub(crate) fn read_safe(path: &Path, max: usize) -> Result<Vec<u8>> {
    check_components(path)?;
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut f = o.open(path)?;
    let before = f.metadata()?;
    if !before.is_file() || before.len() > max as u64 {
        return Err(Error::Invalid("invalid or oversized knowledge file"));
    }
    let mut bytes = Vec::new();
    f.by_ref().take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::Invalid("oversized knowledge file"));
    }
    let after = f.metadata()?;
    let current = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.ino() != current.ino() || before.dev() != current.dev() {
            return Err(Error::Conflict);
        }
    }
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || current.file_type().is_symlink()
    {
        return Err(Error::Conflict);
    }
    Ok(bytes)
}
fn scalar(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') {
        serde_json::from_str::<String>(s).unwrap_or_else(|_| s.to_owned())
    } else {
        s.trim_matches('\'').replace("''", "'")
    }
}
// Deliberately restricted, inert YAML: top-level scalar and string-list fields.
// Unsupported fields remain in the original source; never construct YAML objects.
fn values(s: &str) -> Vec<String> {
    let s = s.trim();
    if let Ok(v) = serde_json::from_str::<Vec<String>>(s) {
        return v;
    }
    if s.starts_with('[') && s.ends_with(']') && !s.starts_with("[[") {
        return s[1..s.len() - 1]
            .split(',')
            .map(scalar)
            .filter(|x| !x.is_empty())
            .collect();
    }
    if s.is_empty() || s.starts_with(['!', '&', '*', '{', '|', '>']) {
        return vec![];
    }
    vec![scalar(s)]
}
pub fn parse_note(path: &str, text: &str, modified: u64) -> Result<Note> {
    let mut props: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut offset = 0;
    let mut lines = text.split_inclusive('\n');
    if lines.next().is_some_and(|l| l.trim_end() == "---") {
        offset = text.find('\n').map_or(text.len(), |p| p + 1);
        let mut key = String::new();
        let mut closed = false;
        for l in lines {
            offset += l.len();
            let t = l.trim();
            if t == "---" {
                closed = true;
                break;
            }
            if l.starts_with(char::is_whitespace) {
                if let Some(value) = t.strip_prefix("- ") {
                    if !key.is_empty() {
                        props.entry(key.clone()).or_default().push(scalar(value));
                    }
                }
            } else if let Some((k, v)) = l.split_once(':') {
                key = k.trim().to_owned();
                if key.len() <= 128 {
                    props.insert(key.clone(), values(v));
                }
            }
        }
        if !closed {
            return Err(Error::Invalid("unclosed knowledge frontmatter"));
        }
    }
    let value = |key: &str| {
        props
            .get(key)
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_default()
    };
    let explicit = value("id");
    if explicit.len() > 128 || explicit.chars().any(char::is_control) {
        return Err(Error::Invalid("invalid note identity"));
    }
    let id = if explicit.is_empty() {
        hash(path.as_bytes())
    } else {
        explicit
    };
    let mut chunks = Vec::new();
    let mut start = offset;
    let mut heading = String::new();
    let mut pos = offset;
    for line in text[offset..].split_inclusive('\n') {
        if line.starts_with('#') || pos - start >= 3500 {
            if pos > start {
                push_chunk(&mut chunks, &id, text, &heading, start, pos)
            }
            start = pos;
            if line.starts_with('#') {
                heading = line.trim().trim_start_matches('#').trim().to_owned();
            }
        }
        pos += line.len();
        // Bound long lines too, using UTF-8 boundaries and real source offsets.
        while pos - start > 4000 {
            let mut end = start + 4000;
            while !text.is_char_boundary(end) {
                end -= 1
            }
            push_chunk(&mut chunks, &id, text, &heading, start, end);
            start = end;
        }
    }
    if pos > start {
        push_chunk(&mut chunks, &id, text, &heading, start, pos)
    }
    let mut links = Vec::new();
    for chunk in &chunks {
        let mut rest = chunk.text.as_str();
        while let Some(i) = rest.find("[[") {
            rest = &rest[i + 2..];
            let Some(end) = rest.find("]]") else { break };
            let target = rest[..end].split(['|', '#']).next().unwrap_or("").trim();
            if !target.is_empty() && target.len() <= 512 {
                links.push(Link {
                    target: target.into(),
                    kind: "references".into(),
                    evidence: chunk.id.clone(),
                });
            }
            rest = &rest[end + 2..];
            if links.len() >= 1000 {
                break;
            }
        }
    }
    for kind in [
        "depends_on",
        "decided_by",
        "affected_by",
        "resolved_by",
        "documented_by",
        "belongs_to",
    ] {
        if let Some(v) = props.get(kind) {
            for target in v {
                links.push(Link {
                    target: target
                        .trim_start_matches("[[")
                        .trim_end_matches("]]")
                        .into(),
                    kind: kind.into(),
                    evidence: format!("{id}:properties"),
                });
            }
        }
    }
    if offset > 0 {
        chunks.insert(
            0,
            Chunk {
                id: format!("{id}:properties"),
                heading: "Properties".into(),
                start: 0,
                end: offset,
                text: text[..offset].into(),
            },
        );
    }
    let title = value("title");
    let project = value("project");
    let kind = value("type");
    let status = value("status");
    let status = match status.as_str() {
        "proposed" | "validated" | "deprecated" => status,
        _ => "unreviewed".into(),
    };
    let explicit_id = props.get("id").is_some_and(|v| !v.is_empty());
    Ok(Note {
        id,
        explicit_id,
        path: path.into(),
        title: if title.is_empty() {
            Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into()
        } else {
            title
        },
        project,
        kind,
        status,
        modified,
        hash: hash(text.as_bytes()),
        properties: props,
        chunks,
        links,
    })
}
fn push_chunk(
    chunks: &mut Vec<Chunk>,
    id: &str,
    text: &str,
    heading: &str,
    start: usize,
    end: usize,
) {
    chunks.push(Chunk {
        id: format!("{id}:{start}:{end}"),
        heading: heading.into(),
        start,
        end,
        text: text[start..end].into(),
    })
}

pub fn scan(config: &Config, cancel: &AtomicBool) -> Result<Snapshot> {
    config.validate()?;
    check_components(&config.vault)?;
    let root = fs::canonicalize(&config.vault)?;
    let mut paths = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        for entry in fs::read_dir(dir)? {
            let e = entry?;
            let path = e.path();
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| Error::UnsafePath)?
                .to_string_lossy()
                .replace('\\', "/");
            let name = e.file_name();
            let name = name.to_string_lossy();
            if [".git", ".obsidian", ".trash"].contains(&name.as_ref())
                || config
                    .exclusions
                    .iter()
                    .any(|x| relative == *x || relative.starts_with(&format!("{x}/")))
            {
                continue;
            }
            let ty = e.file_type()?;
            if ty.is_symlink() {
                return Err(Error::UnsafePath);
            }
            if ty.is_dir() {
                stack.push(path)
            } else if ty.is_file() && path.extension().is_some_and(|s| s == "md") {
                paths.push(path);
                if paths.len() > config.max_notes {
                    return Err(Error::Invalid("knowledge note limit exceeded"));
                }
            }
        }
    }
    paths.sort();
    let mut notes = Vec::new();
    let mut total = 0;
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let b = read_safe(&path, config.max_note_bytes)?;
        total += b.len();
        if total > config.max_total_bytes {
            return Err(Error::Invalid("knowledge vault byte limit exceeded"));
        }
        let m = fs::metadata(&path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let text =
            std::str::from_utf8(&b).map_err(|_| Error::Invalid("knowledge note is not UTF-8"))?;
        let mut note = parse_note(
            &path
                .strip_prefix(&root)
                .map_err(|_| Error::UnsafePath)?
                .to_string_lossy(),
            text,
            m,
        )?;
        for (id, aliases) in &config.projects {
            if aliases.contains(&note.project) {
                note.project = id.clone();
                break;
            }
        }
        notes.push(note);
    }
    let mut ids = BTreeSet::new();
    for n in &notes {
        if !ids.insert(n.id.clone()) {
            return Err(Error::Invalid("duplicate knowledge identity"));
        }
    }
    let mut snapshot = Snapshot {
        revision: String::new(),
        notes,
        relations: vec![],
        diagnostics: vec![],
    };
    resolve(&mut snapshot);
    snapshot.revision = hash(
        format!(
            "{}:{}",
            root.display(),
            serde_json::to_string(&snapshot.notes)?
        )
        .as_bytes(),
    );
    Ok(snapshot)
}
pub(crate) fn resolve(s: &mut Snapshot) {
    s.relations.clear();
    s.diagnostics.clear();
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for note in &s.notes {
        let mut keys = vec![
            note.id.clone(),
            note.path.clone(),
            note.path.trim_end_matches(".md").into(),
            note.title.clone(),
        ];
        if let Some(aliases) = note.properties.get("aliases") {
            keys.extend(aliases.clone());
        }
        for key in keys {
            names.entry(key).or_default().insert(note.id.clone());
        }
    }
    for note in &s.notes {
        for link in &note.links {
            let candidates = names.get(&link.target);
            if let Some(target) = candidates.filter(|v| v.len() == 1).and_then(|v| v.first()) {
                s.relations.push(Relation {
                    source: note.id.clone(),
                    target: target.clone(),
                    kind: link.kind.clone(),
                    origin: "explicit".into(),
                    evidence: vec![link.evidence.clone()],
                });
            } else if s.diagnostics.len() < 1000 {
                s.diagnostics.push(format!(
                    "{}: {} link target(s) for {}",
                    note.path,
                    candidates.map_or(0, BTreeSet::len),
                    link.target
                ));
            }
        }
    }
}
