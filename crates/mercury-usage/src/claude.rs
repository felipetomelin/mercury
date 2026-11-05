//! Claude Code transcript reader (`~/.claude/projects/<slug>/<sessionId>.jsonl`).
//!
//! Each assistant line carries `message.usage`. Streaming writes the same
//! response several times (same `message.id` + `requestId`), so lines are
//! deduplicated by that pair, last write wins. `<synthetic>` messages are local
//! placeholders without a real request and are ignored.

use std::collections::HashMap;

use serde::Deserialize;

use crate::model::{ContextUsage, Provider, SessionUsage, TokenCounts, UsageEvent};

const SYNTHETIC_MODEL: &str = "<synthetic>";
/// Context size of every current Claude model unless the 1M variant is in use.
pub const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;
pub const LARGE_CONTEXT_WINDOW: u64 = 1_000_000;

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize)]
struct OutputDetails {
    #[serde(default)]
    thinking_tokens: u64,
}

impl From<&Usage> for TokenCounts {
    fn from(usage: &Usage) -> Self {
        Self {
            input: usage.input_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_write: usage.cache_creation_input_tokens,
            output: usage.output_tokens,
            reasoning: usage
                .output_tokens_details
                .as_ref()
                .map_or(0, |details| details.thinking_tokens),
        }
    }
}

/// Context window for a Claude model id. Transcripts don't record the window,
/// so this is a guess the statusline bridge overrides with the exact value.
pub fn context_window_for_model(model: Option<&str>, observed_prompt_tokens: u64) -> u64 {
    let large = model.is_some_and(|model| {
        let model = model.to_ascii_lowercase();
        model.contains("[1m]") || model.ends_with("-1m")
    });
    if large || observed_prompt_tokens > DEFAULT_CONTEXT_WINDOW {
        LARGE_CONTEXT_WINDOW
    } else {
        DEFAULT_CONTEXT_WINDOW
    }
}

#[derive(Debug, Clone, Copy)]
struct LastRequest {
    prompt_tokens: u64,
    event_index: usize,
}

/// Accumulates usage of one transcript file. Feed it lines with
/// [`ClaudeTranscript::ingest_line`] (typically from a [`crate::JsonlTail`]).
#[derive(Debug, Clone, Default)]
pub struct ClaudeTranscript {
    session_id: Option<String>,
    events: Vec<UsageEvent>,
    dedupe: HashMap<(String, String), usize>,
    last_main_request: Option<LastRequest>,
    skipped_lines: usize,
}

impl ClaudeTranscript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one JSONL line. Returns `true` when usage changed.
    /// Malformed or non-assistant lines are skipped (counted in [`Self::skipped_lines`]
    /// only when they look like assistant lines but fail to parse).
    pub fn ingest_line(&mut self, line: &str) -> bool {
        // Cheap prefilter: most lines are user/tool payloads, often large.
        if !line.contains("\"assistant\"") {
            return false;
        }
        let Ok(parsed) = serde_json::from_str::<Line>(line) else {
            self.skipped_lines += 1;
            return false;
        };
        if parsed.kind.as_deref() != Some("assistant") {
            return false;
        }
        if self.session_id.is_none() {
            self.session_id = parsed.session_id.clone();
        }
        let Some(message) = parsed.message else {
            return false;
        };
        if message.model.as_deref() == Some(SYNTHETIC_MODEL) {
            return false;
        }
        let Some(usage) = message.usage.as_ref() else {
            return false;
        };
        let Some(at) = parsed.timestamp.as_deref().and_then(crate::parse_timestamp) else {
            self.skipped_lines += 1;
            return false;
        };
        let tokens = TokenCounts::from(usage);
        let event = UsageEvent {
            provider: Provider::Claude,
            session_id: parsed
                .session_id
                .or_else(|| self.session_id.clone())
                .unwrap_or_default(),
            at,
            model: message.model,
            tokens,
        };

        let key = message.id.zip(parsed.request_id);
        let index = match key.as_ref().and_then(|key| self.dedupe.get(key)).copied() {
            Some(index) => {
                if self.events[index] == event {
                    return false;
                }
                self.events[index] = event;
                index
            }
            None => {
                self.events.push(event);
                let index = self.events.len() - 1;
                if let Some(key) = key {
                    self.dedupe.insert(key, index);
                }
                index
            }
        };

        if !parsed.is_sidechain
            && self
                .last_main_request
                .is_none_or(|last| last.event_index <= index)
        {
            self.last_main_request = Some(LastRequest {
                prompt_tokens: tokens.prompt(),
                event_index: index,
            });
        }
        true
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Deduplicated per-request usage, in file order.
    pub fn events(&self) -> &[UsageEvent] {
        &self.events
    }

    pub fn totals(&self) -> TokenCounts {
        self.events.iter().map(|event| event.tokens).sum()
    }

    pub fn skipped_lines(&self) -> usize {
        self.skipped_lines
    }

    /// Latest model and context estimate from the newest main-chain request.
    pub fn session_usage(&self) -> Option<SessionUsage> {
        let last_event = self.events.last()?;
        let context = self.last_main_request.map(|last| {
            let model = self.events[last.event_index].model.as_deref();
            ContextUsage {
                used_tokens: last.prompt_tokens,
                window_tokens: Some(context_window_for_model(model, last.prompt_tokens)),
                exact: false,
            }
        });
        let model = self
            .last_main_request
            .and_then(|last| self.events[last.event_index].model.clone())
            .or_else(|| last_event.model.clone());
        Some(SessionUsage {
            provider: Provider::Claude,
            session_id: self
                .session_id
                .clone()
                .unwrap_or_else(|| last_event.session_id.clone()),
            model,
            totals: self.totals(),
            context,
            cost_usd: None,
            updated_at: self.events.iter().map(|event| event.at).max(),
        })
    }

    /// Clears all state, e.g. after [`crate::TailRead::reset`].
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Sums several transcripts of one session (main file + `subagents/*.jsonl`).
/// Context and model come from the main transcript only.
pub fn merge_session(
    main: &ClaudeTranscript,
    subagents: &[ClaudeTranscript],
) -> Option<SessionUsage> {
    let mut usage = main.session_usage()?;
    for subagent in subagents {
        usage.totals += subagent.totals();
        let newest = subagent.events.iter().map(|event| event.at).max();
        usage.updated_at = usage.updated_at.max(newest);
    }
    Some(usage)
}

/// Convenience for tests and one-shot scans.
pub fn parse_transcript(text: &str) -> ClaudeTranscript {
    let mut transcript = ClaudeTranscript::new();
    for line in text.lines() {
        transcript.ingest_line(line);
    }
    transcript
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/claude-session.jsonl");

    #[test]
    fn dedupes_streamed_lines_and_ignores_synthetic_and_user_lines() {
        let transcript = parse_transcript(FIXTURE);
        // 3 unique requests: msg_A (streamed twice), msg_B (streamed 3x, growing
        // output), msg_C (sidechain). The synthetic line is ignored.
        assert_eq!(transcript.events().len(), 3);
        assert_eq!(
            transcript.session_id(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(
            transcript.totals(),
            TokenCounts {
                input: 2 + 6 + 3,
                cache_read: 18_000 + 37_666 + 1_000,
                cache_write: 37_666 + 400,
                output: 116 + 900 + 50,
                reasoning: 23,
            }
        );
        assert_eq!(transcript.skipped_lines(), 1);
    }

    #[test]
    fn context_comes_from_last_main_chain_request() {
        let usage = parse_transcript(FIXTURE).session_usage().unwrap();
        assert_eq!(usage.provider, Provider::Claude);
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-5"));
        let context = usage.context.unwrap();
        // msg_B: 6 + 37_666 + 400 (the sidechain msg_C must not count).
        assert_eq!(context.used_tokens, 38_072);
        assert_eq!(context.window_tokens, Some(DEFAULT_CONTEXT_WINDOW));
        assert!(!context.exact);
        assert_eq!(usage.cost_usd, None);
        assert_eq!(
            usage.updated_at.map(|at| at.unix_timestamp()),
            crate::parse_timestamp("2026-10-05T14:31:00.000Z").map(|at| at.unix_timestamp())
        );
    }

    #[test]
    fn incremental_ingest_matches_one_shot_parse() {
        let mut transcript = ClaudeTranscript::new();
        let mut tail = crate::JsonlTail::new();
        let bytes = FIXTURE.as_bytes();
        let split = bytes.len() / 2;
        for end in [split, bytes.len()] {
            tail.read_new(&mut std::io::Cursor::new(bytes[..end].to_vec()), |line| {
                transcript.ingest_line(line);
            })
            .unwrap();
        }
        assert_eq!(transcript.events(), parse_transcript(FIXTURE).events());
    }

    #[test]
    fn large_context_is_inferred() {
        assert_eq!(
            context_window_for_model(Some("claude-opus-5[1m]"), 0),
            LARGE_CONTEXT_WINDOW
        );
        assert_eq!(
            context_window_for_model(Some("claude-opus-5"), 250_000),
            LARGE_CONTEXT_WINDOW
        );
        assert_eq!(context_window_for_model(None, 10), DEFAULT_CONTEXT_WINDOW);
    }

    #[test]
    fn subagent_tokens_are_added_without_touching_context() {
        let main = parse_transcript(FIXTURE);
        let sub = parse_transcript(
            r#"{"type":"assistant","sessionId":"11111111-2222-3333-4444-555555555555","isSidechain":true,"requestId":"req_S","timestamp":"2026-10-05T15:00:00.000Z","message":{"id":"msg_S","model":"claude-haiku-5","usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":5}}}"#,
        );
        let merged = merge_session(&main, &[sub]).unwrap();
        assert_eq!(merged.totals.input, main.totals().input + 10);
        assert_eq!(merged.context, main.session_usage().unwrap().context);
        assert_eq!(
            merged.updated_at.map(|at| at.unix_timestamp()),
            crate::parse_timestamp("2026-10-05T15:00:00Z").map(|at| at.unix_timestamp())
        );
    }
}
