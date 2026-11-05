use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

pub struct Parsed {
    pub session_id: String,
    pub cwd: Option<String>,
    pub timestamp: String,
    pub messages: Vec<String>,
}

pub fn read(path: &Path, provider: Provider) -> std::io::Result<Option<Parsed>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = Vec::new();
    let mut parsed = Parsed {
        session_id: String::new(),
        cwd: None,
        timestamp: String::new(),
        messages: Vec::new(),
    };
    let mut seen = HashMap::new();
    if provider == Provider::Claude {
        parsed.session_id = path
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_owned();
    }
    if !safe_id(&parsed.session_id) && provider == Provider::Claude {
        return Ok(None);
    }
    loop {
        line.clear();
        let count = reader.read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if line.len() > 4 * 1024 * 1024 {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let timestamp = event.get("timestamp").and_then(Value::as_str).unwrap_or("");
        if timestamp > parsed.timestamp.as_str() && timestamp.len() <= 40 {
            parsed.timestamp = timestamp.to_owned();
        }
        match provider {
            Provider::Claude => parse_claude(&event, &mut parsed, &mut seen),
            Provider::Codex => parse_codex(&event, &mut parsed, &mut seen),
        }
    }
    if !safe_id(&parsed.session_id) {
        return Ok(None);
    }
    Ok(Some(parsed))
}

fn parse_claude(event: &Value, parsed: &mut Parsed, seen: &mut HashMap<String, usize>) {
    if parsed.cwd.is_none() {
        parsed.cwd = absolute_cwd(event.get("cwd").and_then(Value::as_str));
    }
    if event.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return;
    }
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    if kind != "user" && kind != "assistant" {
        return;
    }
    let message = &event["message"];
    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    if role != kind {
        return;
    }
    let identity = if role == "assistant" {
        message.get("id").and_then(Value::as_str).map(|id| {
            format!(
                "a:{id}:{}",
                event.get("requestId").and_then(Value::as_str).unwrap_or("")
            )
        })
    } else {
        event
            .get("uuid")
            .and_then(Value::as_str)
            .map(|id| format!("u:{id}"))
    };
    let Some(text) = content_text(&message["content"], "text") else {
        return;
    };
    if let Some(identity) = identity {
        if let Some(index) = seen.get(&identity) {
            parsed.messages[*index] = text;
            return;
        }
        seen.insert(identity, parsed.messages.len());
    }
    parsed.messages.push(text);
}

fn parse_codex(event: &Value, parsed: &mut Parsed, seen: &mut HashMap<String, usize>) {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = &event["payload"];
    if kind == "session_meta" {
        if let Some(id) = payload.get("session_id").and_then(Value::as_str) {
            if safe_id(id) {
                parsed.session_id = id.to_owned();
            }
        }
        if parsed.cwd.is_none() {
            parsed.cwd = absolute_cwd(payload.get("cwd").and_then(Value::as_str));
        }
    } else if kind == "response_item"
        && payload.get("type").and_then(Value::as_str) == Some("message")
    {
        let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" && role != "assistant" {
            return;
        }
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            let identity = format!("{role}:{id}");
            if let Some(text) = content_text(
                &payload["content"],
                if role == "user" {
                    "input_text"
                } else {
                    "output_text"
                },
            ) {
                if let Some(index) = seen.get(&identity) {
                    parsed.messages[*index] = text;
                } else {
                    seen.insert(identity, parsed.messages.len());
                    parsed.messages.push(text);
                }
            }
            return;
        }
        if let Some(text) = content_text(
            &payload["content"],
            if role == "user" {
                "input_text"
            } else {
                "output_text"
            },
        ) {
            parsed.messages.push(text);
        }
    }
}

fn content_text(value: &Value, accepted_type: &str) -> Option<String> {
    let mut text = String::new();
    if let Some(content) = value.as_str() {
        text.push_str(content);
    } else if let Some(blocks) = value.as_array() {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some(accepted_type) {
                if let Some(fragment) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(fragment);
                }
            }
        }
    }
    (!text.trim().is_empty()).then(|| text.chars().take(64 * 1024).collect())
}

pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn absolute_cwd(value: Option<&str>) -> Option<String> {
    let path = Path::new(value?);
    (path.is_absolute() && value?.len() <= 4096).then(|| path.to_string_lossy().into_owned())
}
