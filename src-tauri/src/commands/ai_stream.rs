//! Line-by-line parsing of the AI CLIs' JSON event streams (§3.9 / UI10).
//!
//! - Claude Code (`--output-format stream-json --verbose
//!   --include-partial-messages`) sends `stream_event` text deltas as the
//!   answer is written, then a `result` event with the full text and the
//!   session id.
//! - Codex (`exec --json`) sends no text deltas: the answer arrives whole in
//!   an `item.completed` `agent_message`; before that only step events
//!   (`item.started` for reasoning / tool use) show progress.
//!
//! Formats were taken from captured output of Claude Code 2.1.311 and
//! codex-cli 0.167.0 (tests/fixtures/ai_stream_*.jsonl).

use serde_json::Value;

pub(super) const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const OUTPUT_LIMIT_ERROR: &str =
    "AI answer exceeded the 1 MiB display limit. The original CLI transcript is unchanged.";

#[derive(Debug, Clone, PartialEq)]
pub enum StreamUpdate {
    /// Append to the answer shown so far.
    Delta(String),
    /// Replace the answer shown so far (Codex delivers whole messages).
    Replace(String),
    /// A progress step without answer text: "thinking" or "tool".
    Status(&'static str),
    /// The provider's own session/thread id.
    Session(String),
    /// The run finished; the complete answer.
    Done(String),
    Error(String),
}

pub struct StreamParser {
    provider: Provider,
    text: String,
}

#[derive(Clone, Copy)]
enum Provider {
    Claude,
    Codex,
}

impl StreamParser {
    pub fn new(provider: &str) -> Self {
        Self {
            provider: if provider == "claude" {
                Provider::Claude
            } else {
                Provider::Codex
            },
            text: String::new(),
        }
    }

    /// The answer text accumulated so far (what a cancelled run keeps).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Updates produced by one line of CLI output; non-JSON lines and event
    /// types the app doesn't use produce none.
    pub fn feed_line(&mut self, line: &str) -> Vec<StreamUpdate> {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        match self.provider {
            Provider::Claude => self.claude_event(&event),
            Provider::Codex => self.codex_event(&event),
        }
    }

    fn claude_event(&mut self, event: &Value) -> Vec<StreamUpdate> {
        match event["type"].as_str().unwrap_or_default() {
            "stream_event" => {
                let inner = &event["event"];
                match inner["type"].as_str().unwrap_or_default() {
                    "content_block_delta" if inner["delta"]["type"] == "text_delta" => {
                        let chunk = inner["delta"]["text"].as_str().unwrap_or_default();
                        if chunk.is_empty() {
                            return Vec::new();
                        }
                        if self.text.len().saturating_add(chunk.len()) > MAX_OUTPUT_BYTES {
                            return vec![StreamUpdate::Error(OUTPUT_LIMIT_ERROR.into())];
                        }
                        self.text.push_str(chunk);
                        vec![StreamUpdate::Delta(chunk.to_string())]
                    }
                    "content_block_start" if inner["content_block"]["type"] == "tool_use" => {
                        vec![StreamUpdate::Status("tool")]
                    }
                    "content_block_start" if inner["content_block"]["type"] == "thinking" => {
                        vec![StreamUpdate::Status("thinking")]
                    }
                    _ => Vec::new(),
                }
            }
            "result" => {
                let mut updates = Vec::new();
                if let Some(id) = event["session_id"].as_str() {
                    updates.push(StreamUpdate::Session(id.to_string()));
                }
                let result = event["result"].as_str().unwrap_or_default();
                if result.len() > MAX_OUTPUT_BYTES {
                    updates.push(StreamUpdate::Error(OUTPUT_LIMIT_ERROR.into()));
                    return updates;
                }
                if event["is_error"].as_bool().unwrap_or(false) {
                    updates.push(StreamUpdate::Error(if result.is_empty() {
                        "Claude Code could not complete this request.".to_string()
                    } else {
                        result.to_string()
                    }));
                } else {
                    // The result is the authoritative full answer (deltas may
                    // have skipped tool-use turns' text).
                    let text = if result.trim().is_empty() {
                        self.text.clone()
                    } else {
                        result.to_string()
                    };
                    self.text = text.clone();
                    updates.push(StreamUpdate::Done(text));
                }
                updates
            }
            _ => Vec::new(),
        }
    }

    fn codex_event(&mut self, event: &Value) -> Vec<StreamUpdate> {
        let item = &event["item"];
        match event["type"].as_str().unwrap_or_default() {
            "thread.started" => event["thread_id"]
                .as_str()
                .map(|id| vec![StreamUpdate::Session(id.to_string())])
                .unwrap_or_default(),
            "item.started" => match item["type"].as_str().unwrap_or_default() {
                "reasoning" => vec![StreamUpdate::Status("thinking")],
                "web_search" | "command_execution" | "mcp_tool_call" => {
                    vec![StreamUpdate::Status("tool")]
                }
                _ => Vec::new(),
            },
            "item.completed" => {
                let kind = item["type"].as_str().unwrap_or_default();
                match item["text"].as_str() {
                    Some(text) if kind == "agent_message" || kind == "assistant_message" => {
                        if text.len() > MAX_OUTPUT_BYTES {
                            return vec![StreamUpdate::Error(OUTPUT_LIMIT_ERROR.into())];
                        }
                        self.text = text.to_string();
                        vec![StreamUpdate::Replace(text.to_string())]
                    }
                    _ => Vec::new(),
                }
            }
            "turn.completed" => vec![StreamUpdate::Done(self.text.clone())],
            "error" | "turn.failed" => {
                let message = event["message"]
                    .as_str()
                    .or_else(|| event["error"]["message"].as_str())
                    .unwrap_or("Codex could not complete this request.");
                vec![StreamUpdate::Error(message.to_string())]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ai_stream_claude.jsonl"
    ));
    const CODEX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ai_stream_codex.jsonl"
    ));

    fn run(provider: &str, output: &str) -> Vec<StreamUpdate> {
        let mut parser = StreamParser::new(provider);
        output
            .lines()
            .flat_map(|line| parser.feed_line(line))
            .collect()
    }

    #[test]
    fn claude_streams_deltas_then_the_full_result_and_session() {
        let updates = run("claude", CLAUDE);
        let deltas: String = updates
            .iter()
            .filter_map(|update| match update {
                StreamUpdate::Delta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, "hello world");
        assert!(updates.contains(&StreamUpdate::Session(
            "11111111-2222-4333-8444-555555555555".to_string()
        )));
        assert_eq!(
            updates.last(),
            Some(&StreamUpdate::Done("hello world".to_string()))
        );
    }

    #[test]
    fn codex_delivers_the_whole_message_then_done() {
        assert_eq!(
            run("codex", CODEX),
            [
                StreamUpdate::Session("11111111-2222-4333-8444-555555555555".to_string()),
                StreamUpdate::Replace("hello world".to_string()),
                StreamUpdate::Done("hello world".to_string()),
            ]
        );
    }

    #[test]
    fn bounds_multibyte_accumulation_and_whole_answer_replacements() {
        let mut claude = StreamParser::new("claude");
        let chunk = "한".repeat(MAX_OUTPUT_BYTES / 6);
        let event = serde_json::json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":chunk}}}).to_string();
        assert!(matches!(
            &claude.feed_line(&event)[0],
            StreamUpdate::Delta(_)
        ));
        assert!(matches!(
            &claude.feed_line(&event)[0],
            StreamUpdate::Delta(_)
        ));
        let before = claude.text().len();
        assert!(matches!(
            &claude.feed_line(&event)[0],
            StreamUpdate::Error(_)
        ));
        assert_eq!(claude.text().len(), before);
        assert!(before <= MAX_OUTPUT_BYTES);
        let oversized = "x".repeat(MAX_OUTPUT_BYTES + 1);
        let mut codex = StreamParser::new("codex");
        let event = serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":oversized}}).to_string();
        assert!(matches!(
            &codex.feed_line(&event)[0],
            StreamUpdate::Error(_)
        ));
        assert!(codex.text().is_empty());
        let event =
            serde_json::json!({"type":"result","result":oversized,"session_id":"kept-session"})
                .to_string();
        assert_eq!(
            claude.feed_line(&event),
            [
                StreamUpdate::Session("kept-session".into()),
                StreamUpdate::Error(OUTPUT_LIMIT_ERROR.into())
            ]
        );
    }

    #[test]
    fn reports_errors_and_progress_steps() {
        let mut claude = StreamParser::new("claude");
        assert_eq!(
            claude.feed_line(r#"{"type":"result","is_error":true,"result":"Not logged in"}"#),
            [StreamUpdate::Error("Not logged in".to_string())]
        );

        let mut codex = StreamParser::new("codex");
        assert_eq!(
            codex.feed_line(r#"{"type":"item.started","item":{"type":"web_search"}}"#),
            [StreamUpdate::Status("tool")]
        );
        assert_eq!(
            codex.feed_line(r#"{"type":"turn.failed","error":{"message":"quota exceeded"}}"#),
            [StreamUpdate::Error("quota exceeded".to_string())]
        );
        assert!(
            codex
                .feed_line("Reading additional input from stdin...")
                .is_empty()
        );
    }
}
