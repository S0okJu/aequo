//! Read-only adapters for provider-owned local session transcript files.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const PAGE_MESSAGES: usize = 20;
const PAGE_CONTENT_BYTES: usize = 512 * 1024;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;

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
    pub has_older: bool,
    pub has_newer: bool,
}

fn unavailable(note: &str) -> TranscriptSnapshot {
    TranscriptSnapshot {
        available: false,
        messages: Vec::new(),
        note: Some(note.to_string()),
        has_older: false,
        has_newer: false,
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

fn display_user_text(text: &str) -> &str {
    if text.starts_with("<aequo_article_context_v1>\n") {
        text.split_once("\n</aequo_article_context_v1>\n")
            .map(|(_, question)| question)
            .unwrap_or(text)
    } else {
        text
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
    let text = if role == "user" {
        display_user_text(text.trim())
    } else {
        text.trim()
    };
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
    let text = if role == "user" {
        display_user_text(text.trim())
    } else {
        text.trim()
    };
    if text.is_empty() {
        return None;
    }
    Some((role.to_string(), text.to_string()))
}

/// Retain at most `maximum` bytes before parsing; discard an oversized line
/// through its newline without ever allocating the whole record.
fn read_line_bounded<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    maximum: usize,
) -> std::io::Result<(usize, bool)> {
    line.clear();
    let mut total = 0;
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok((total, oversized));
        }
        let used = chunk
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(chunk.len(), |position| position + 1);
        let done = chunk[used - 1] == b'\n';
        total += used;
        if !oversized && total <= maximum {
            line.extend_from_slice(&chunk[..used]);
        } else {
            oversized = true;
            line.clear();
        }
        reader.consume(used);
        if done {
            return Ok((total, oversized));
        }
    }
}

fn shorten_message(content: &mut String) -> bool {
    if content.len() <= MAX_MESSAGE_BYTES {
        return false;
    }
    let suffix = "\n\n[Message shortened for display; original remains in the CLI transcript]";
    let mut end = MAX_MESSAGE_BYTES - suffix.len();
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    content.truncate(end);
    content.push_str(suffix);
    true
}

fn load_messages(
    path: &Path,
    provider: &str,
    session_id: i64,
    before: Option<i64>,
    after: Option<i64>,
) -> TranscriptSnapshot {
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
    let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT_BYTES));
    let mut line = Vec::new();
    let mut messages: VecDeque<TranscriptMessage> = VecDeque::new();
    let mut retained_bytes = 0;
    let mut ordinal = 0i64;
    let mut previous_fingerprint = None;
    let mut shortened = false;
    let mut skipped = false;
    let mut forward_full = false;
    loop {
        let Ok((read, oversized)) = read_line_bounded(&mut reader, &mut line, MAX_LINE_BYTES)
        else {
            return unavailable("The local CLI transcript could not be read.");
        };
        if read == 0 {
            break;
        }
        if oversized {
            skipped = true;
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
        // Constant-size adjacent duplicate detection, before display truncation.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&role, &content).hash(&mut hasher);
        let fingerprint = hasher.finish();
        if previous_fingerprint == Some(fingerprint) {
            continue;
        }
        previous_fingerprint = Some(fingerprint);
        ordinal += 1;
        if before.is_some_and(|cursor| ordinal >= cursor)
            || after.is_some_and(|cursor| ordinal <= cursor)
            || forward_full
        {
            continue;
        }
        shortened |= shorten_message(&mut content);
        let bytes = content.len();
        if after.is_some()
            && (messages.len() == PAGE_MESSAGES || retained_bytes + bytes > PAGE_CONTENT_BYTES)
        {
            forward_full = true;
            continue;
        }
        retained_bytes += bytes;
        messages.push_back(TranscriptMessage {
            id: ordinal,
            session_id,
            role,
            content,
            created_at,
        });
        while messages.len() > PAGE_MESSAGES || retained_bytes > PAGE_CONTENT_BYTES {
            retained_bytes -= messages.pop_front().expect("nonempty page").content.len();
        }
    }
    let has_older = messages.front().is_some_and(|message| message.id > 1);
    let has_newer = messages.back().is_some_and(|message| message.id < ordinal);
    let note = if ordinal == 0 {
        Some("Transcript was found, but no readable message text was recognized. You can still continue the CLI session.".to_string())
    } else if skipped || shortened {
        Some("Some transcript records were shortened or skipped for display. The original CLI transcript is unchanged.".to_string())
    } else {
        None
    };
    TranscriptSnapshot {
        available: true,
        messages: messages.into_iter().collect(),
        note,
        has_older,
        has_newer,
    }
}

pub(super) fn load_transcript(
    provider: &str,
    external_session_id: Option<&str>,
    codex_home: &Path,
    claude_config_dir: &Path,
    session_id: i64,
    before: Option<i64>,
    after: Option<i64>,
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
    load_messages(&path, provider, session_id, before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn context_envelope_is_hidden_only_for_user_messages_in_both_provider_formats() {
        let transport = "<aequo_article_context_v1>\nSource material and instructions\n</aequo_article_context_v1>\nCompare the sources";
        let codex = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":transport}});
        let claude = serde_json::json!({"type":"user","message":{"content":[{"type":"text","text":transport}]}});
        for message in [codex_message(&codex), claude_message(&claude)] {
            assert_eq!(message, Some(("user".into(), "Compare the sources".into())));
        }
        let assistant = serde_json::json!({"type":"assistant","message":{"content":transport}});
        assert_eq!(
            claude_message(&assistant),
            Some(("assistant".into(), transport.into()))
        );
        assert_eq!(
            display_user_text("An ordinary question"),
            "An ordinary question"
        );
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(lines: &[String]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "aequo-transcript-{}-{}.jsonl",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::write(&path, lines.join("\n")).unwrap();
            Self(path)
        }
        fn page(&self, before: Option<i64>, after: Option<i64>) -> TranscriptSnapshot {
            load_messages(&self.0, "claude", 7, before, after)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn message(content: &str) -> String {
        serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":content}]}}).to_string()
    }
    #[test]
    fn bounded_reader_discards_large_records_before_allocation_and_resumes() {
        let mut data = vec![b'x'; MAX_LINE_BYTES * 3];
        data.extend_from_slice(b"\nnext\n");
        let mut reader = BufReader::with_capacity(1024, Cursor::new(data));
        let mut line = Vec::new();
        let (bytes, oversized) = read_line_bounded(&mut reader, &mut line, MAX_LINE_BYTES).unwrap();
        assert!(oversized);
        assert_eq!(bytes, MAX_LINE_BYTES * 3 + 1);
        assert!(line.is_empty());
        assert!(line.capacity() <= MAX_LINE_BYTES);
        assert_eq!(
            read_line_bounded(&mut reader, &mut line, MAX_LINE_BYTES).unwrap(),
            (5, false)
        );
        assert_eq!(line, b"next\n");
    }
    #[test]
    fn stable_cursor_pages_reach_oldest_and_return_without_gaps_or_duplicates() {
        let fixture = Fixture::new(
            &(1..=103)
                .map(|id| message(&format!("answer {id}")))
                .collect::<Vec<_>>(),
        );
        let mut page = fixture.page(None, None);
        assert_eq!(page.messages.first().unwrap().id, 84);
        assert!(!page.has_newer);
        let mut backwards = Vec::new();
        loop {
            assert!(page.messages.len() <= PAGE_MESSAGES);
            backwards.extend(page.messages.iter().rev().map(|message| message.id));
            if !page.has_older {
                break;
            }
            page = fixture.page(Some(page.messages.first().unwrap().id), None);
        }
        assert_eq!(backwards, (1..=103).rev().collect::<Vec<_>>());
        let mut forwards: Vec<_> = page.messages.iter().map(|message| message.id).collect();
        while page.has_newer {
            page = fixture.page(None, Some(page.messages.last().unwrap().id));
            forwards.extend(page.messages.iter().map(|message| message.id));
        }
        assert_eq!(forwards, (1..=103).collect::<Vec<_>>());
        // Appending preserves old ordinal cursors.
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(&fixture.0)
                .unwrap(),
            "\n{}",
            message("answer 104")
        )
        .unwrap();
        let new = fixture.page(None, Some(103));
        assert_eq!(new.messages[0].id, 104);
    }
    #[test]
    fn utf8_content_and_page_bytes_are_bounded_without_modifying_originals() {
        let large = "한😀".repeat(20_000);
        let lines = (1..=50)
            .map(|id| message(&format!("{id}: {large}")))
            .collect::<Vec<_>>();
        let fixture = Fixture::new(&lines);
        let original = fs::read(&fixture.0).unwrap();
        let mut page = fixture.page(None, None);
        let mut ids = Vec::new();
        loop {
            assert!(
                page.messages
                    .iter()
                    .all(|message| message.content.len() <= MAX_MESSAGE_BYTES)
            );
            assert!(
                page.messages
                    .iter()
                    .map(|message| message.content.len())
                    .sum::<usize>()
                    <= PAGE_CONTENT_BYTES
            );
            assert!(page.note.is_some());
            ids.extend(page.messages.iter().rev().map(|message| message.id));
            if !page.has_older {
                break;
            }
            page = fixture.page(Some(page.messages[0].id), None);
        }
        assert_eq!(ids, (1..=50).rev().collect::<Vec<_>>());
        assert_eq!(fs::read(&fixture.0).unwrap(), original);
    }
    #[test]
    fn adjacent_provider_duplicates_are_removed_before_assigning_cursor_ids() {
        let fixture = Fixture::new(&[message("hello"), message("hello"), message("next")]);
        let page = fixture.page(None, None);
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[1].id, 2);
    }
}
