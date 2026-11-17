//! Read-only adapters for provider-owned local session transcript files.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MESSAGES: usize = 2_000;
const MAX_MESSAGE_CHARS: usize = 200_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSnapshot {
    pub available: bool,
    pub messages: Vec<TranscriptMessage>,
    pub note: Option<String>,
}

fn unavailable(note: &str) -> TranscriptSnapshot {
    TranscriptSnapshot {
        available: false,
        messages: Vec::new(),
        note: Some(note.to_string()),
    }
}

fn safe_session_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn find_transcript(root: &Path, session_id: &str, max_depth: usize) -> Option<PathBuf> {
    fn visit(
        directory: &Path,
        session_id: &str,
        depth: usize,
        max_depth: usize,
        visited: &mut usize,
    ) -> Option<PathBuf> {
        if depth > max_depth || *visited > 20_000 {
            return None;
        }
        let entries = fs::read_dir(directory).ok()?;
        for entry in entries.flatten() {
            *visited += 1;
            if *visited > 20_000 {
                return None;
            }
            let entry_path = entry.path();
            let kind = entry.file_type().ok()?;
            if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.ends_with(".jsonl") && name.contains(session_id) {
                    return Some(entry_path);
                }
            } else if kind.is_dir()
                && let Some(found) = visit(&entry_path, session_id, depth + 1, max_depth, visited)
            {
                return Some(found);
            }
        }
        None
    }

    if !root.is_dir() {
        return None;
    }
    visit(root, session_id, 0, max_depth, &mut 0)
}

fn text_from_content(content: &Value, accepted_types: &[&str]) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| {
                block["type"]
                    .as_str()
                    .is_some_and(|kind| accepted_types.contains(&kind))
            })
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

// Codex may add runtime environment metadata to its recorded user message.
// It is useful to the CLI, but should not be presented as conversation text.
fn contains_environment_context(text: &str) -> bool {
    text.contains("<environment_context>") || text.contains("<environmentcontext>")
}

fn codex_message(record: &Value) -> Option<(String, String)> {
    let record_type = record["type"].as_str()?;
    let payload = &record["payload"];
    let (role, text) = match record_type {
        "response_item" if payload["type"] == "message" => {
            let role = payload["role"].as_str()?;
            let accepted = if role == "user" {
                &["input_text", "text"][..]
            } else {
                &["output_text", "text"][..]
            };
            (role, text_from_content(&payload["content"], accepted))
        }
        // Older rollout versions record user/assistant text as event messages.
        "event_msg" => match payload["type"].as_str()? {
            "user_message" => ("user", payload["message"].as_str()?.to_string()),
            "agent_message" => ("assistant", payload["message"].as_str()?.to_string()),
            _ => return None,
        },
        _ => return None,
    };
    let role = match role {
        "user" => "user",
        "assistant" => "assistant",
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() || contains_environment_context(text) {
        return None;
    }
    Some((role.to_string(), text.to_string()))
}

fn claude_message(record: &Value) -> Option<(String, String)> {
    let role = match record["type"].as_str()? {
        "user" => "user",
        "assistant" => "assistant",
        _ => return None,
    };
    let content = &record["message"]["content"];
    let text = text_from_content(content, &["text", "input_text", "output_text"]);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((role.to_string(), text.to_string()))
}

fn load_messages(path: &Path, provider: &str, session_id: i64) -> TranscriptSnapshot {
    let Ok(metadata) = fs::metadata(path) else {
        return unavailable("The local CLI transcript could not be accessed.");
    };
    if metadata.len() > MAX_TRANSCRIPT_BYTES {
        return unavailable("This transcript is larger than Aequo's 64 MB reading limit.");
    }
    let created_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
        });
    let Ok(file) = File::open(path) else {
        return unavailable("The local CLI transcript could not be accessed.");
    };
    let mut reader = BufReader::new(file).take(MAX_TRANSCRIPT_BYTES);
    let mut line = Vec::new();
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut bytes_read = 0u64;
    loop {
        line.clear();
        let Ok(read) = reader.read_until(b'\n', &mut line) else {
            return unavailable("The local CLI transcript could not be read.");
        };
        if read == 0 {
            break;
        }
        bytes_read += read as u64;
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let candidate = if provider == "codex" {
            codex_message(&record)
        } else {
            claude_message(&record)
        };
        let Some((role, mut content)) = candidate else {
            continue;
        };
        if content.chars().count() > MAX_MESSAGE_CHARS {
            content = content.chars().take(MAX_MESSAGE_CHARS).collect();
            content.push_str("\n\n[Message shortened for display]");
        }
        if messages
            .last()
            .is_some_and(|last| last.0 == role && last.1 == content)
        {
            continue;
        }
        messages.push((role, content));
        if messages.len() > MAX_MESSAGES {
            messages.remove(0);
        }
        if bytes_read >= MAX_TRANSCRIPT_BYTES {
            break;
        }
    }

    let note = if messages.is_empty() {
        Some("Transcript was found, but no readable message text was recognized. You can still continue the CLI session.".to_string())
    } else if bytes_read >= MAX_TRANSCRIPT_BYTES {
        Some("Only the first 64 MB of this transcript were read.".to_string())
    } else {
        None
    };
    TranscriptSnapshot {
        available: true,
        messages: messages
            .into_iter()
            .enumerate()
            .map(|(index, (role, content))| TranscriptMessage {
                id: index as i64 + 1,
                session_id,
                role,
                content,
                created_at,
            })
            .collect(),
        note,
    }
}

pub(super) fn load_transcript(
    provider: &str,
    external_session_id: Option<&str>,
    codex_home: &Path,
    claude_config_dir: &Path,
    session_id: i64,
) -> TranscriptSnapshot {
    let Some(external_id) = external_session_id.filter(|value| safe_session_id(value)) else {
        return unavailable("The CLI has not created a transcript for this session yet.");
    };
    let (root, depth) = match provider {
        "codex" => (codex_home.join("sessions"), 5),
        "claude" => (claude_config_dir.join("projects"), 2),
        _ => return unavailable("Unsupported AI provider."),
    };
    let Some(path) = find_transcript(&root, external_id, depth) else {
        return unavailable(
            "The CLI transcript was not found. It may have been removed by CLI history cleanup.",
        );
    };
    load_messages(&path, provider, session_id)
}
