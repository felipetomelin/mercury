//! Bounded, role-aware conversational snapshot for the reviewer. Tool output,
//! thinking, sidechains and provider telemetry never become message text.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub provider: Provider,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    pub messages: Vec<Message>,
    pub truncated: bool,
    /// Only exact when the full file was read without discarded messages.
    pub user_turns: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub header_bytes: usize,
    pub tail_bytes: usize,
    pub line_bytes: usize,
    pub message_bytes: usize,
    pub total_text_bytes: usize,
    pub messages: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            header_bytes: 128 * 1024,
            tail_bytes: 4 * 1024 * 1024,
            line_bytes: 256 * 1024,
            message_bytes: 16 * 1024,
            total_text_bytes: 64 * 1024,
            messages: 64,
        }
    }
}

impl Limits {
    pub fn validate(self) -> Result<Self, TranscriptError> {
        if self.header_bytes == 0
            || self.header_bytes > 128 * 1024
            || self.tail_bytes == 0
            || self.tail_bytes > 8 * 1024 * 1024
            || self.line_bytes == 0
            || self.line_bytes > 256 * 1024
            || self.message_bytes < 4
            || self.message_bytes > 16 * 1024
            || self.total_text_bytes < 4
            || self.total_text_bytes > 128 * 1024
            || self.message_bytes > self.total_text_bytes
            || self.messages == 0
            || self.messages > 128
        {
            return Err(TranscriptError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug)]
pub enum TranscriptError {
    Io(std::io::Error),
    InvalidLimits,
    InvalidPath,
    InvalidSession,
    MissingIdentity,
    Incomplete,
    Changed,
    NoConversation,
}

impl From<std::io::Error> for TranscriptError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

struct Retained {
    identity: Option<String>,
    message: Message,
    clipped: bool,
}

pub fn read(
    path: &Path,
    provider: Provider,
    expected_session_id: &str,
    limits: Limits,
) -> Result<Transcript, TranscriptError> {
    let limits = limits.validate()?;
    if !safe_id(expected_session_id) {
        return Err(TranscriptError::InvalidSession);
    }
    check_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(TranscriptError::InvalidPath);
    }
    if provider == Provider::Claude
        && path.file_stem().and_then(|stem| stem.to_str()) != Some(expected_session_id)
    {
        return Err(TranscriptError::InvalidSession);
    }
    let file_len = metadata.len();
    let mut header = Vec::new();
    Read::by_ref(&mut file)
        .take(limits.header_bytes as u64)
        .read_to_end(&mut header)?;
    let (found_id, cwd) = read_identity(&header, provider, expected_session_id)?;
    if found_id.as_deref() != Some(expected_session_id) {
        return Err(TranscriptError::MissingIdentity);
    }

    let tail_start = file_len.saturating_sub(limits.tail_bytes as u64);
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail = Vec::new();
    Read::by_ref(&mut file)
        .take(limits.tail_bytes as u64 + 1)
        .read_to_end(&mut tail)?;
    if tail.len() > limits.tail_bytes || file.metadata()?.len() != file_len {
        return Err(TranscriptError::Changed);
    }
    let live = fs::symlink_metadata(path)?;
    if !same_file(&metadata, &live) {
        return Err(TranscriptError::Changed);
    }
    let mut truncated = tail_start > 0;
    let mut retained = VecDeque::new();
    let mut start = 0;
    if tail_start > 0 {
        start = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |index| index + 1);
    }
    let mut lines = tail[start..].split(|byte| *byte == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        if line.len() > limits.line_bytes {
            truncated = true;
            continue;
        }
        let event: Value = match serde_json::from_slice(line) {
            Ok(event) => event,
            Err(_) if lines.peek().is_none() && !tail.ends_with(b"\n") => {
                return Err(TranscriptError::Incomplete);
            }
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        if provider == Provider::Claude {
            if event
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != expected_session_id)
            {
                return Err(TranscriptError::InvalidSession);
            }
        } else if event.get("type").and_then(Value::as_str) == Some("session_meta")
            && event["payload"].get("session_id").and_then(Value::as_str)
                != Some(expected_session_id)
        {
            return Err(TranscriptError::InvalidSession);
        }
        if let Some(item) = parse_message(&event, provider, limits.message_bytes) {
            truncated |= item.clipped;
            let position = item.identity.as_deref().and_then(|identity| {
                retained
                    .iter()
                    .position(|existing: &Retained| existing.identity.as_deref() == Some(identity))
            });
            if let Some(position) = position {
                retained[position] = item;
            } else {
                retained.push_back(item);
            }
            while retained.len() > limits.messages
                || retained
                    .iter()
                    .map(|item| item.message.text.len())
                    .sum::<usize>()
                    > limits.total_text_bytes
            {
                retained.pop_front();
                truncated = true;
            }
        }
    }
    if retained.is_empty() || !retained.iter().any(|item| item.message.role == Role::User) {
        return Err(TranscriptError::NoConversation);
    }
    let user_turns = (!truncated).then(|| {
        retained
            .iter()
            .filter(|item| item.message.role == Role::User)
            .count()
    });
    Ok(Transcript {
        provider,
        session_id: expected_session_id.to_owned(),
        cwd,
        messages: retained.into_iter().map(|item| item.message).collect(),
        truncated,
        user_turns,
    })
}

fn read_identity(
    header: &[u8],
    provider: Provider,
    expected_id: &str,
) -> Result<(Option<String>, Option<PathBuf>), TranscriptError> {
    let mut found_id = None;
    let mut cwd = None;
    for line in header.split(|byte| *byte == b'\n') {
        if line.is_empty() || line.len() > 256 * 1024 {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match provider {
            Provider::Claude => {
                if let Some(id) = event.get("sessionId").and_then(Value::as_str) {
                    if id != expected_id {
                        return Err(TranscriptError::InvalidSession);
                    }
                    found_id = Some(id.to_owned());
                }
                if cwd.is_none() {
                    cwd = absolute_cwd(event.get("cwd").and_then(Value::as_str));
                }
            }
            Provider::Codex
                if event.get("type").and_then(Value::as_str) == Some("session_meta") =>
            {
                let id = event["payload"]
                    .get("session_id")
                    .and_then(Value::as_str)
                    .ok_or(TranscriptError::MissingIdentity)?;
                if id != expected_id {
                    return Err(TranscriptError::InvalidSession);
                }
                found_id = Some(id.to_owned());
                cwd = absolute_cwd(event["payload"].get("cwd").and_then(Value::as_str));
                break;
            }
            Provider::Codex => {}
        }
    }
    Ok((found_id, cwd))
}

fn parse_message(event: &Value, provider: Provider, max_bytes: usize) -> Option<Retained> {
    match provider {
        Provider::Claude => {
            if event.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let kind = event.get("type").and_then(Value::as_str)?;
            if kind != "user" && kind != "assistant" {
                return None;
            }
            let message = event.get("message")?;
            if message.get("role").and_then(Value::as_str) != Some(kind)
                || message.get("model").and_then(Value::as_str) == Some("<synthetic>")
            {
                return None;
            }
            let role = if kind == "user" {
                Role::User
            } else {
                Role::Assistant
            };
            let (text, clipped) = content_text(&message["content"], "text", max_bytes)?;
            let identity = if role == Role::User {
                event
                    .get("uuid")
                    .and_then(Value::as_str)
                    .map(|id| format!("u:{id}"))
            } else {
                message.get("id").and_then(Value::as_str).map(|id| {
                    format!(
                        "a:{id}:{}",
                        event.get("requestId").and_then(Value::as_str).unwrap_or("")
                    )
                })
            };
            Some(Retained {
                identity,
                message: Message { role, text },
                clipped,
            })
        }
        Provider::Codex => {
            if event.get("type").and_then(Value::as_str) != Some("response_item")
                || event["payload"].get("type").and_then(Value::as_str) != Some("message")
            {
                return None;
            }
            let role = match event["payload"].get("role").and_then(Value::as_str) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => return None,
            };
            let accepted = if role == Role::User {
                "input_text"
            } else {
                "output_text"
            };
            let (text, clipped) = content_text(&event["payload"]["content"], accepted, max_bytes)?;
            let identity = event["payload"]
                .get("id")
                .and_then(Value::as_str)
                .map(|id| format!("{accepted}:{id}"));
            Some(Retained {
                identity,
                message: Message { role, text },
                clipped,
            })
        }
    }
}

fn content_text(content: &Value, accepted: &str, max_bytes: usize) -> Option<(String, bool)> {
    let mut text = String::new();
    if let Some(value) = content.as_str() {
        text.push_str(value);
    } else if let Some(blocks) = content.as_array() {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some(accepted) {
                if let Some(fragment) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(fragment);
                }
            }
        }
    }
    let original_len = text.len();
    text.retain(|ch| !ch.is_control() || ch == '\n' || ch == '\t');
    if text.trim().is_empty() {
        return None;
    }
    let end = text.floor_char_boundary(max_bytes.min(text.len()));
    let clipped = end < text.len() || text.len() != original_len;
    text.truncate(end);
    Some((text, clipped))
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn absolute_cwd(value: Option<&str>) -> Option<PathBuf> {
    let value = value?;
    let path = Path::new(value);
    (path.is_absolute() && value.len() <= 4096).then(|| path.to_path_buf())
}

fn check_path(path: &Path) -> Result<(), TranscriptError> {
    if !path.is_absolute() {
        return Err(TranscriptError::InvalidPath);
    }
    let mut prefix = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            return Err(TranscriptError::InvalidPath);
        }
        prefix.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&prefix)?;
        if metadata.file_type().is_symlink()
            || (index + 1 < components.len() && !metadata.file_type().is_dir())
            || (index + 1 == components.len() && !metadata.file_type().is_file())
        {
            return Err(TranscriptError::InvalidPath);
        }
    }
    Ok(())
}

fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if !after.file_type().is_file() || before.len() != after.len() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return false;
        }
    }
    before.modified().ok() == after.modified().ok()
}
