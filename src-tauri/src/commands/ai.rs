//! Local Codex and Claude Code session management for article discussions.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Notify;

use crate::db::{self, AiSession};

use super::ai_stream::{StreamParser, StreamUpdate};
use super::ai_transcript::{self, TranscriptSnapshot};

/// Give up when the CLI prints nothing for this long. Measured between
/// output lines, not over the whole run, so a long answer that keeps
/// streaming is never cut off.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Cancellation handles for in-flight runs, keyed by request id.
#[derive(Default)]
pub struct AiRuns(Mutex<HashMap<String, Arc<Notify>>>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStart {
    pub request_id: String,
    pub session_id: i64,
}

/// Payload of the `ai-stream` event. `kind` is `delta` (append `text`),
/// `replace` (the answer so far is `text`), `status` (`text` is "thinking"
/// or "tool"), or a terminal `done` / `error` / `cancelled`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiStreamPayload {
    request_id: String,
    session_id: i64,
    kind: &'static str,
    text: String,
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
                .arg("stream-json")
                // stream-json requires --verbose with --print; partial
                // messages turn on token-level text deltas.
                .arg("--verbose")
                .arg("--include-partial-messages")
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderStatus {
    pub provider: &'static str,
    pub installed: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// Whether each AI CLI is installed and runs (`--version`). This can't
/// tell whether the user is signed in; the settings page says so rather
/// than guessing.
#[tauri::command]
pub async fn check_ai_providers() -> Vec<AiProviderStatus> {
    let mut statuses = Vec::new();
    for provider in ["codex", "claude"] {
        let status = match executable_path(provider) {
            Err(error) => AiProviderStatus {
                provider,
                installed: false,
                version: None,
                error: Some(error),
            },
            Ok(path) => {
                let output = tokio::time::timeout(
                    Duration::from_secs(10),
                    Command::new(path)
                        .arg("--version")
                        .stdin(Stdio::null())
                        .kill_on_drop(true)
                        .output(),
                )
                .await;
                match output {
                    Ok(Ok(output)) if output.status.success() => AiProviderStatus {
                        provider,
                        installed: true,
                        version: String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .next()
                            .map(|line| line.trim().to_string())
                            .filter(|line| !line.is_empty()),
                        error: None,
                    },
                    Ok(Ok(output)) => AiProviderStatus {
                        provider,
                        installed: true,
                        version: None,
                        error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
                    },
                    Ok(Err(error)) => AiProviderStatus {
                        provider,
                        installed: true,
                        version: None,
                        error: Some(error.to_string()),
                    },
                    Err(_) => AiProviderStatus {
                        provider,
                        installed: true,
                        version: None,
                        error: Some("timed out".to_string()),
                    },
                }
            }
        };
        statuses.push(status);
    }
    statuses
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

/// Starts a CLI run for a message and returns at once; the answer streams
/// to the frontend as `ai-stream` events tagged with the returned request
/// id. The run keeps going (and its session is kept) if the user moves to
/// another post meanwhile.
#[tauri::command]
pub fn start_ai_message(
    app: AppHandle,
    db_state: State<'_, std::sync::Mutex<Connection>>,
    runs: State<'_, AiRuns>,
    item_id: i64,
    provider: String,
    session_id: Option<i64>,
    prompt: String,
) -> Result<AiStart, String> {
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
    let child = command
        .spawn()
        .map_err(|error| format!("could not start the {provider} CLI: {error}"))?;

    let request_id = generate_uuid()?;
    let cancel = Arc::new(Notify::new());
    runs.0
        .lock()
        .map_err(|_| "AI run registry poisoned".to_string())?
        .insert(request_id.clone(), cancel.clone());

    tauri::async_runtime::spawn(stream_run(
        app.clone(),
        request_id.clone(),
        session.id,
        provider,
        child,
        cancel,
    ));

    Ok(AiStart {
        request_id,
        session_id: session.id,
    })
}

/// Stops an in-flight run; the answer streamed so far is kept and the run
/// ends with a `cancelled` event. Unknown or finished ids are ignored.
#[tauri::command]
pub fn cancel_ai_message(runs: State<'_, AiRuns>, request_id: String) -> Result<(), String> {
    if let Some(cancel) = runs
        .0
        .lock()
        .map_err(|_| "AI run registry poisoned".to_string())?
        .get(&request_id)
    {
        cancel.notify_one();
    }
    Ok(())
}

enum RunEnd {
    Done(String),
    Error(String),
    Cancelled,
}

async fn stream_run(
    app: AppHandle,
    request_id: String,
    session_id: i64,
    provider: String,
    mut child: tokio::process::Child,
    cancel: Arc<Notify>,
) {
    let emit = |kind: &'static str, text: String| {
        let _ = app.emit(
            "ai-stream",
            AiStreamPayload {
                request_id: request_id.clone(),
                session_id,
                kind,
                text,
            },
        );
    };

    let mut parser = StreamParser::new(&provider);
    let mut provider_session_id = None;
    let mut end = None;

    let stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        while end.is_none() {
            let line = tokio::select! {
                _ = cancel.notified() => { end = Some(RunEnd::Cancelled); break; }
                line = tokio::time::timeout(IDLE_TIMEOUT, lines.next_line()) => line,
            };
            let line = match line {
                Err(_) => {
                    end = Some(RunEnd::Error(format!(
                        "The {provider} CLI stopped responding for {} seconds.",
                        IDLE_TIMEOUT.as_secs()
                    )));
                    break;
                }
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) | Ok(Err(_)) => break,
            };
            for update in parser.feed_line(&line) {
                match update {
                    StreamUpdate::Delta(text) => emit("delta", text),
                    StreamUpdate::Replace(text) => emit("replace", text),
                    StreamUpdate::Status(step) => emit("status", step.to_string()),
                    StreamUpdate::Session(id) => provider_session_id = Some(id),
                    StreamUpdate::Done(text) => end = Some(RunEnd::Done(text)),
                    StreamUpdate::Error(message) => end = Some(RunEnd::Error(message)),
                }
            }
        }
    }

    if matches!(end, Some(RunEnd::Cancelled) | Some(RunEnd::Error(_))) {
        let _ = child.kill().await;
    }
    let status = child.wait().await.ok();
    let end = match end {
        Some(end) => end,
        // Output ended without a result event: report the CLI's own error.
        None => {
            let mut detail = String::new();
            if let Some(stderr) = stderr.as_mut() {
                let _ = stderr.read_to_string(&mut detail).await;
            }
            let detail = detail.trim().to_string();
            if detail.is_empty() {
                match status {
                    Some(status) if !status.success() => {
                        RunEnd::Error(format!("{provider} CLI exited with status {status}"))
                    }
                    _ => RunEnd::Error(format!("The {provider} CLI returned an empty response.")),
                }
            } else {
                RunEnd::Error(detail)
            }
        }
    };

    // Remember the provider's session id even for a stopped run, so the
    // conversation can be continued.
    if let Some(provider_session_id) = &provider_session_id
        && let Ok(conn) = app.state::<std::sync::Mutex<Connection>>().lock()
    {
        let _ =
            db::update_ai_session_provider_id(&conn, session_id, provider_session_id, now_unix());
    }

    match end {
        RunEnd::Done(text) => emit("done", text),
        RunEnd::Error(message) => emit("error", message),
        RunEnd::Cancelled => emit("cancelled", parser.text().to_string()),
    }

    if let Ok(mut runs) = app.state::<AiRuns>().0.lock() {
        runs.remove(&request_id);
    }
}
