//! Local Codex and Claude Code session management for article discussions.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::process::Command;

use crate::db::{self, AiSession};

use super::ai_transcript::{self, TranscriptMessage, TranscriptSnapshot};

const CLI_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSendResult {
    pub session_id: i64,
    pub provider_session_id: String,
    pub assistant_message: TranscriptMessage,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn validate_provider(provider: &str) -> Result<(), String> {
    match provider {
        "codex" | "claude" => Ok(()),
        _ => Err(format!("unsupported AI provider: {provider}")),
    }
}

fn generate_uuid() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| format!("could not generate a session id: {error}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

fn executable_path(name: &str) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join(name)));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("{name} CLI was not found. Install it and sign in first."))
}

fn build_command(
    provider: &str,
    provider_session_id: Option<&str>,
    new_session: bool,
    prompt: &str,
    working_directory: &Path,
) -> Result<Command, String> {
    let mut command = match provider {
        "codex" => {
            let mut command = Command::new(executable_path("codex")?);
            command
                .arg("exec")
                .arg("--json")
                .arg("--sandbox")
                .arg("read-only")
                .arg("--skip-git-repo-check");
            if !new_session {
                let session_id =
                    provider_session_id.ok_or_else(|| "Codex session id is missing".to_string())?;
                command.arg("resume").arg(session_id);
            }
            command.arg(prompt);
            command
        }
        "claude" => {
            let mut command = Command::new(executable_path("claude")?);
            command
                .arg("--print")
                .arg("--output-format")
                .arg("json")
                .arg("--permission-prompts")
                .arg("none")
                // The reader needs web fetch only; no filesystem or shell tools.
                .arg("--tools")
                .arg("WebFetch");
            if new_session {
                let session_id = provider_session_id
                    .map(str::to_string)
                    .unwrap_or(generate_uuid()?);
                command.arg("--session-id").arg(session_id);
            } else {
                let session_id = provider_session_id
                    .ok_or_else(|| "Claude Code session id is missing".to_string())?;
                command.arg("--resume").arg(session_id);
            }
            command.arg(prompt);
            command
        }
        _ => return Err(format!("unsupported AI provider: {provider}")),
    };

    command
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok(command)
}

fn parse_cli_response(
    provider: &str,
    output: &str,
    fallback_session_id: Option<&str>,
) -> Result<(String, String), String> {
    if provider == "claude" {
        let parsed: serde_json::Value = serde_json::from_str(output)
            .map_err(|_| "Claude Code returned an unreadable response.".to_string())?;
        if parsed["is_error"].as_bool().unwrap_or(false) {
            return Err(parsed["result"]
                .as_str()
                .unwrap_or("Claude Code could not complete this request.")
                .to_string());
        }
        let message = parsed["result"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Claude Code returned an empty response.".to_string())?
            .to_string();
        let session_id = parsed["session_id"]
            .as_str()
            .or(fallback_session_id)
            .ok_or_else(|| "Claude Code did not return a session id.".to_string())?
            .to_string();
        return Ok((session_id, message));
    }

    let mut session_id = fallback_session_id.map(str::to_string);
    let mut assistant_message = None;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match event["type"].as_str().unwrap_or_default() {
            "thread.started" => {
                session_id = event["thread_id"]
                    .as_str()
                    .map(str::to_string)
                    .or(session_id);
            }
            "item.completed" => {
                let item = &event["item"];
                let item_type = item["type"].as_str().unwrap_or_default();
                if (item_type == "agent_message" || item_type == "assistant_message")
                    && let Some(text) = item["text"].as_str()
                {
                    assistant_message = Some(text.to_string());
                }
            }
            "error" | "turn.failed" => {
                if let Some(message) = event["message"].as_str() {
                    return Err(message.to_string());
                }
            }
            _ => {}
        }
    }

    let session_id = session_id.ok_or_else(|| "Codex did not return a session id.".to_string())?;
    let assistant_message = assistant_message
        .filter(|message| !message.trim().is_empty())
        .ok_or_else(|| "Codex returned an empty response.".to_string())?;
    Ok((session_id, assistant_message))
}

fn prepare_session(
    conn: &Connection,
    item_id: i64,
    provider: &str,
    session_id: Option<i64>,
) -> Result<(AiSession, Option<String>, bool), String> {
    let item = db::get_item_by_id(conn, item_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("post {item_id} was not found"))?;
    let now = now_unix();

    let new_session = session_id.is_none();
    let session = if let Some(session_id) = session_id {
        let session = db::get_ai_session(conn, session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("AI session {session_id} was not found"))?;
        if session.item_id != item_id || session.provider != provider {
            return Err("AI session does not match this post and provider.".to_string());
        }
        session
    } else {
        let claude_session_id = if provider == "claude" {
            Some(generate_uuid()?)
        } else {
            None
        };
        let title = item
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Post discussion");
        let new_id = db::create_ai_session(
            conn,
            item_id,
            provider,
            claude_session_id.as_deref(),
            title,
            now,
        )
        .map_err(|error| error.to_string())?;
        db::get_ai_session(conn, new_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "could not load the new AI session".to_string())?
    };

    Ok((
        session.clone(),
        session.provider_session_id.clone(),
        new_session || session.provider_session_id.is_none(),
    ))
}

#[tauri::command]
pub fn list_ai_sessions(
    db_state: State<'_, std::sync::Mutex<Connection>>,
    item_id: i64,
    provider: String,
) -> Result<Vec<AiSession>, String> {
    validate_provider(&provider)?;
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::list_ai_sessions(&conn, item_id, &provider).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn load_ai_transcript(
    app: AppHandle,
    db_state: State<'_, std::sync::Mutex<Connection>>,
    session_id: i64,
) -> Result<TranscriptSnapshot, String> {
    let session = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db::get_ai_session(&conn, session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("AI session {session_id} was not found"))?
    };
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let claude_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    Ok(ai_transcript::load_transcript(
        &session.provider,
        session.provider_session_id.as_deref(),
        &codex_home,
        &claude_config_dir,
        session.id,
    ))
}

#[tauri::command]
pub fn delete_ai_session(
    db_state: State<'_, std::sync::Mutex<Connection>>,
    session_id: i64,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::delete_ai_session(&conn, session_id).map_err(|error| error.to_string())? {
        Ok(())
    } else {
        Err(format!("AI session {session_id} was not found"))
    }
}

#[tauri::command]
pub async fn send_ai_message(
    app: AppHandle,
    db_state: State<'_, std::sync::Mutex<Connection>>,
    item_id: i64,
    provider: String,
    session_id: Option<i64>,
    prompt: String,
) -> Result<AiSendResult, String> {
    validate_provider(&provider)?;
    if prompt.trim().is_empty() {
        return Err("message cannot be empty".to_string());
    }
    if prompt.len() > 100_000 {
        return Err("message is too long".to_string());
    }

    let working_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&working_directory).map_err(|error| error.to_string())?;

    let (session, external_session_id, new_session) = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        prepare_session(&conn, item_id, &provider, session_id)?
    };

    let mut command = build_command(
        &provider,
        external_session_id.as_deref(),
        new_session,
        &prompt,
        &working_directory,
    )?;
    let output = tokio::time::timeout(CLI_TIMEOUT, command.output())
        .await
        .map_err(|_| "AI session timed out after five minutes".to_string())?
        .map_err(|error| format!("could not start the {provider} CLI: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("{provider} CLI exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(detail);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (provider_session_id, assistant_text) =
        parse_cli_response(&provider, &stdout, external_session_id.as_deref())?;

    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let now = now_unix();
    db::update_ai_session_provider_id(&conn, session.id, &provider_session_id, now)
        .map_err(|error| error.to_string())?;

    Ok(AiSendResult {
        session_id: session.id,
        provider_session_id,
        assistant_message: TranscriptMessage {
            id: now,
            session_id: session.id,
            role: "assistant".to_string(),
            content: assistant_text,
            created_at: now,
        },
    })
}
