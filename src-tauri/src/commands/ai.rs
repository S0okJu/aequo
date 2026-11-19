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
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::Notify;

use crate::db::{self, AiSession};

use super::ai_io;
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
    /// Set only when `kind` is `error`: a coarse category the frontend maps
    /// to a friendlier message, while `text` keeps the CLI's own wording for
    /// anyone who opens "show details" (§B). `None` for every other kind.
    error_kind: Option<&'static str>,
}

/// A coarse bucket for the handful of CLI failures common enough to word
/// better than whatever the CLI's own stderr/JSON happened to say. Matching
/// is best-effort pattern matching on that raw text: a CLI wording change
/// just falls through to `Unknown`, which keeps today's raw-text display
/// instead of breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AiErrorKind {
    LoginRequired,
    RateLimit,
    Network,
    Unknown,
}

impl AiErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            AiErrorKind::LoginRequired => "loginRequired",
            AiErrorKind::RateLimit => "rateLimit",
            AiErrorKind::Network => "network",
            AiErrorKind::Unknown => "unknown",
        }
    }
}

fn classify_error(raw: &str) -> AiErrorKind {
    let lower = raw.to_lowercase();
    if lower.contains("rate limit") || lower.contains("rate_limit") || lower.contains("quota") {
        AiErrorKind::RateLimit
    } else if lower.contains("not logged in")
        || lower.contains("not authenticated")
        || lower.contains("authentication")
        || lower.contains("logged out")
        || lower.contains("log in")
    {
        AiErrorKind::LoginRequired
    } else if lower.contains("econnrefused")
        || lower.contains("enotfound")
        || lower.contains("network error")
        || lower.contains("could not connect")
        || lower.contains("connection refused")
    {
        AiErrorKind::Network
    } else {
        AiErrorKind::Unknown
    }
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
            // A single-article prompt supplies a URL, so enable the native web tool
            // instead of relying on shell network access in the read-only sandbox.
            command.arg("--search");
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
                .arg("WebFetch")
                .arg("--allowedTools")
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
    context_item_ids: &[i64],
    scope_label: Option<&str>,
    provider: &str,
    session_id: Option<i64>,
) -> Result<(AiSession, Option<String>, bool), String> {
    let session = if let Some(id) = session_id {
        let session = db::get_ai_session(conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("AI session {id} was not found"))?;
        if session.provider != provider {
            return Err("AI session does not match this provider.".into());
        }
        session
    } else {
        validate_context_ids(conn, context_item_ids)?;
        let external = if provider == "claude" {
            Some(generate_uuid()?)
        } else {
            None
        };
        let title = context_item_ids
            .first()
            .map(|id| db::get_item_summary_by_id(conn, *id))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten()
            .and_then(|i| i.title)
            .unwrap_or_else(|| "Conversation".into());
        conn.execute("INSERT INTO ai_sessions(item_id,provider,provider_session_id,title,created_at,updated_at,scope_label) VALUES(NULL,?1,?2,?3,?4,?4,?5)",rusqlite::params![provider,external,title,now_unix(),scope_label]).map_err(|e|e.to_string())?;
        let id = conn.last_insert_rowid();
        db::ai_context::add(conn, id, context_item_ids, now_unix()).map_err(|e| e.to_string())?;
        db::get_ai_session(conn, id)
            .map_err(|e| e.to_string())?
            .ok_or("Could not load conversation")?
    };
    let fresh = session_id.is_none() || session.provider_session_id.is_none();
    Ok((session.clone(), session.provider_session_id.clone(), fresh))
}

fn validate_context_ids(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    if ids.len() > 20 {
        return Err("A conversation supports at most 20 articles.".into());
    }
    for id in ids {
        if db::get_item_summary_by_id(conn, *id)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Err(format!("Post {id} was not found"));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_all_ai_sessions(
    db_state: State<'_, Mutex<Connection>>,
    provider: Option<String>,
) -> Result<Vec<AiSession>, String> {
    if let Some(p) = &provider {
        validate_provider(p)?;
    }
    let conn = db_state.lock().map_err(|_| "database lock poisoned")?;
    db::ai_context::all(&conn, provider.as_deref()).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn latest_ai_session_for_item(
    db_state: State<'_, Mutex<Connection>>,
    item_id: i64,
) -> Result<Option<AiSession>, String> {
    let conn = db_state.lock().map_err(|_| "database lock poisoned")?;
    db::ai_context::latest(&conn, item_id).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn add_ai_context_items(
    db_state: State<'_, Mutex<Connection>>,
    session_id: i64,
    item_ids: Vec<i64>,
) -> Result<AiSession, String> {
    let conn = db_state.lock().map_err(|_| "database lock poisoned")?;
    validate_context_ids(&conn, &item_ids)?;
    let session = db::get_ai_session(&conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or("Conversation not found")?;
    let mut unique: std::collections::HashSet<i64> = session
        .context_items
        .iter()
        .filter_map(|i| i.item_id)
        .collect();
    unique.extend(&item_ids);
    if unique.len()
        + session
            .context_items
            .iter()
            .filter(|i| i.item_id.is_none())
            .count()
        > 20
    {
        return Err("A conversation supports at most 20 articles.".into());
    }
    db::ai_context::add(&conn, session_id, &item_ids, now_unix()).map_err(|e| e.to_string())?;
    db::get_ai_session(&conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".into())
}
#[tauri::command]
pub fn remove_ai_context_item(
    db_state: State<'_, Mutex<Connection>>,
    session_id: i64,
    position: i64,
) -> Result<AiSession, String> {
    let conn = db_state.lock().map_err(|_| "database lock poisoned")?;
    db::ai_context::remove(&conn, session_id, position).map_err(|e| e.to_string())?;
    db::get_ai_session(&conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".into())
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
pub async fn load_ai_transcript(
    app: AppHandle,
    db_state: State<'_, std::sync::Mutex<Connection>>,
    session_id: i64,
    before: Option<i64>,
    after: Option<i64>,
) -> Result<TranscriptSnapshot, String> {
    if before.is_some_and(|id| id <= 0)
        || after.is_some_and(|id| id <= 0)
        || (before.is_some() && after.is_some())
    {
        return Err("Provide one positive transcript cursor".into());
    }
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
    tauri::async_runtime::spawn_blocking(move || {
        ai_transcript::load_transcript(
            &session.provider,
            session.provider_session_id.as_deref(),
            &codex_home,
            &claude_config_dir,
            session.id,
            before,
            after,
        )
    })
    .await
    .map_err(|error| error.to_string())
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
#[allow(clippy::too_many_arguments)] // Flat Tauri IPC arguments match the public command contract.
pub async fn start_ai_message(
    app: AppHandle,
    db_state: State<'_, std::sync::Mutex<Connection>>,
    runs: State<'_, AiRuns>,
    context_item_ids: Vec<i64>,
    scope_label: Option<String>,
    provider: String,
    session_id: Option<i64>,
    prompt: String,
) -> Result<AiStart, String> {
    let activity = crate::updater::activity::begin(&app)?;
    validate_provider(&provider)?;
    if scope_label.as_ref().is_some_and(|label| label.len() > 1024) {
        return Err("Scope label is too long.".into());
    }
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

    let (session, external_session_id, new_session, prompt, sent_positions) = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let (session, external, fresh) = prepare_session(
            &conn,
            &context_item_ids,
            scope_label.as_deref(),
            &provider,
            session_id,
        )?;
        let (assembled, positions) =
            db::ai_context::prompt(&conn, &session, &prompt, fresh).map_err(|e| e.to_string())?;
        if assembled.len() > 100_000 {
            return Err("Message plus article context is too long.".into());
        }
        (session, external, fresh, assembled, positions)
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

    let stream = stream_run(
        app.clone(),
        request_id.clone(),
        session.id,
        provider,
        child,
        cancel,
        sent_positions,
    );
    tauri::async_runtime::spawn(async move {
        let _activity = activity;
        stream.await;
    });

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
    sent_positions: Vec<i64>,
) {
    let emit = |kind: &'static str, text: String| {
        let _ = app.emit(
            "ai-stream",
            AiStreamPayload {
                request_id: request_id.clone(),
                session_id,
                kind,
                text,
                error_kind: None,
            },
        );
    };

    let mut parser = StreamParser::new(&provider);
    let mut provider_session_id = None;
    let mut end = None;

    let stdout = child.stdout.take();
    let mut stderr_task = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(ai_io::drain_stderr(stderr)));
    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout);
        while end.is_none() {
            let line = tokio::select! {
                _ = cancel.notified() => { end = Some(RunEnd::Cancelled); break; }
                line = tokio::time::timeout(IDLE_TIMEOUT, ai_io::next_line(&mut lines)) => line,
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
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    end = Some(RunEnd::Error(error.to_string()));
                    break;
                }
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
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status.ok(),
        Err(_) => {
            let _ = child.kill().await;
            end = Some(RunEnd::Error(format!(
                "The {provider} CLI did not exit after its output ended."
            )));
            None
        }
    };
    let diagnostic = if let Some(task) = stderr_task.as_mut() {
        match tokio::time::timeout(Duration::from_secs(2), &mut *task).await {
            Ok(Ok(Ok(text))) => text,
            _ => {
                task.abort();
                String::new()
            }
        }
    } else {
        String::new()
    };
    let end = match end {
        Some(end) => end,
        // Output ended without a result event: report the CLI's own error.
        None => {
            let detail = diagnostic.trim().to_string();
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
        RunEnd::Done(text) => {
            if let Ok(conn) = app.state::<Mutex<Connection>>().lock() {
                let _ = db::ai_context::mark_sent(&conn, session_id, &sent_positions, now_unix());
            }
            emit("done", text);
        }
        RunEnd::Error(message) => {
            let kind = classify_error(&message);
            let _ = app.emit(
                "ai-stream",
                AiStreamPayload {
                    request_id: request_id.clone(),
                    session_id,
                    kind: "error",
                    text: message,
                    error_kind: Some(kind.as_str()),
                },
            );
        }
        RunEnd::Cancelled => emit("cancelled", parser.text().to_string()),
    }

    if let Ok(mut runs) = app.state::<AiRuns>().0.lock() {
        runs.remove(&request_id);
    }
}

#[cfg(test)]
mod error_classification_tests {
    use super::*;

    #[test]
    fn classifies_common_cli_failures() {
        assert_eq!(
            classify_error("Error: you are not logged in. Run `codex login`."),
            AiErrorKind::LoginRequired
        );
        assert_eq!(
            classify_error("401 authentication failed"),
            AiErrorKind::LoginRequired
        );
        assert_eq!(
            classify_error("rate limit exceeded, please retry later"),
            AiErrorKind::RateLimit
        );
        assert_eq!(
            classify_error("monthly quota exhausted"),
            AiErrorKind::RateLimit
        );
        assert_eq!(
            classify_error("connect ECONNREFUSED 127.0.0.1:443"),
            AiErrorKind::Network
        );
        assert_eq!(
            classify_error("The codex CLI stopped responding for 120 seconds."),
            AiErrorKind::Unknown
        );
        assert_eq!(
            classify_error("codex CLI exited with status 1"),
            AiErrorKind::Unknown
        );
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn session_provider_and_explicit_article_validation() {
        let conn = db::init_in_memory().unwrap();
        assert!(prepare_session(&conn, &[999], None, "codex", None).is_err());
        let (session, _, fresh) = prepare_session(&conn, &[], None, "codex", None).unwrap();
        assert!(fresh);
        assert!(session.context_items.is_empty());
        assert!(prepare_session(&conn, &[], None, "claude", Some(session.id)).is_err());
        assert!(prepare_session(&conn, &[], None, "codex", Some(session.id)).is_ok());
    }

    /// Manual integration gate: real provider execution, explicit 10/20 article inputs,
    /// incremental context, and provider-owned conversation continuation.
    #[tokio::test]
    #[ignore = "requires authenticated local AI CLIs and network"]
    async fn real_cli_context_roundtrip() {
        let directory =
            std::env::temp_dir().join(format!("aequo-ai-smoke-{}", generate_uuid().unwrap()));
        std::fs::create_dir_all(&directory).unwrap();
        let conn = db::init_in_memory().unwrap();
        conn.execute("INSERT INTO feeds(id,url,title,language,created_at) VALUES(1,'https://example.com','Test','en',0)",[]).unwrap();
        for id in 1..=20 {
            let code = match id {
                1 => "Apricot-731",
                10 => "Cobalt-904",
                11 => "Juniper-127",
                20 => "Willow-688",
                _ => "Unrelated",
            };
            conn.execute("INSERT INTO items(id,feed_id,guid_hash,title,summary,published_at,fetched_at) VALUES(?1,1,?2,?3,?4,0,0)",rusqlite::params![id,id.to_string(),format!("Source {id}"),format!("The unique code word in this article is {code}.")]).unwrap();
        }
        for provider in ["codex", "claude"] {
            let mut selected = None;
            for turn in 0..3 {
                let (ids, scope, question, expected) = match turn {
                    0 => (
                        (1..=10).collect::<Vec<_>>(),
                        None,
                        "Reply only with the code words from sources [1] and [10], in order. Do not use tools.",
                        ["Apricot-731", "Cobalt-904"],
                    ),
                    1 => {
                        db::ai_context::add(&conn, selected.unwrap(), &[11], now_unix()).unwrap();
                        (
                            vec![],
                            None,
                            "Reply only with the code words from sources [1] and [11], in order. Do not use tools.",
                            ["Apricot-731", "Juniper-127"],
                        )
                    }
                    _ => {
                        selected = None;
                        (
                            (1..=20).collect(),
                            Some("View · 20"),
                            "Reply only with the code words from sources [1] and [20], in order. Do not use tools.",
                            ["Apricot-731", "Willow-688"],
                        )
                    }
                };
                let (session, external, fresh) =
                    prepare_session(&conn, &ids, scope, provider, selected).unwrap();
                let (prompt, positions) =
                    db::ai_context::prompt(&conn, &session, question, fresh).unwrap();
                if turn == 1 {
                    assert!(!prompt.contains("Apricot-731"));
                    assert!(prompt.contains("Juniper-127"));
                }
                let bytes = prompt.len();
                let started = std::time::Instant::now();
                let mut command =
                    build_command(provider, external.as_deref(), fresh, &prompt, &directory)
                        .unwrap();
                let output = tokio::time::timeout(Duration::from_secs(120), command.output())
                    .await
                    .expect("CLI timed out")
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{provider}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let mut parser = StreamParser::new(provider);
                let mut answer = None;
                let mut provider_id = None;
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    for update in parser.feed_line(line) {
                        match update {
                            StreamUpdate::Session(id) => provider_id = Some(id),
                            StreamUpdate::Done(text) => answer = Some(text),
                            StreamUpdate::Error(error) => panic!("{provider}: {error}"),
                            _ => {}
                        }
                    }
                }
                let provider_id = provider_id.expect("CLI returned no session id");
                db::update_ai_session_provider_id(&conn, session.id, &provider_id, now_unix())
                    .unwrap();
                db::ai_context::mark_sent(&conn, session.id, &positions, now_unix()).unwrap();
                assert!(
                    answer
                        .as_deref()
                        .is_some_and(|a| expected.iter().all(|code| a.contains(code))),
                    "{provider}: {answer:?}"
                );
                let home = PathBuf::from(
                    std::env::var_os("HOME")
                        .expect("HOME is required for CLI transcript verification"),
                );
                let codex_home = std::env::var_os("CODEX_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".codex"));
                let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".claude"));
                let transcript = ai_transcript::load_transcript(
                    provider,
                    Some(&provider_id),
                    &codex_home,
                    &claude_home,
                    session.id,
                    None,
                    None,
                );
                assert!(transcript.available, "{provider}: {:?}", transcript.note);
                assert!(
                    transcript
                        .messages
                        .iter()
                        .any(|m| m.role == "user" && m.content == question),
                    "{provider}: current user question was not recovered from the provider transcript"
                );
                assert!(
                    transcript.messages.iter().all(|m| m.role != "user"
                        || !m.content.starts_with("<aequo_article_context_v1>")),
                    "transport envelope leaked into history"
                );
                selected = Some(session.id);
                eprintln!(
                    "{provider}: turn {turn}, context {}, input {bytes} bytes, {:.2}s, answer {:?}",
                    session.context_items.len(),
                    started.elapsed().as_secs_f64(),
                    answer.unwrap()
                );
            }
        }
    }
    #[tokio::test]
    #[ignore = "requires authenticated local AI CLIs and network"]
    async fn real_cli_single_url_context() {
        let directory =
            std::env::temp_dir().join(format!("aequo-ai-url-{}", generate_uuid().unwrap()));
        std::fs::create_dir_all(&directory).unwrap();
        let conn = db::init_in_memory().unwrap();
        conn.execute_batch("INSERT INTO feeds(id,url,created_at) VALUES(1,'https://example.com/feed',0);
          INSERT INTO items(id,feed_id,guid_hash,title,link,published_at,fetched_at) VALUES(1,1,'a','Test article','https://example.com',0,0);").unwrap();
        for provider in ["codex", "claude"] {
            let (session, external, fresh) =
                prepare_session(&conn, &[1], None, provider, None).unwrap();
            let (prompt,_)=db::ai_context::prompt(&conn,&session,"Fetch the selected URL and reply only with the visible heading of that page. Use the web tool, do not infer from the title.",fresh).unwrap();
            let started = std::time::Instant::now();
            let output = tokio::time::timeout(
                Duration::from_secs(120),
                build_command(provider, external.as_deref(), fresh, &prompt, &directory)
                    .unwrap()
                    .output(),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                output.status.success(),
                "{provider}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut parser = StreamParser::new(provider);
            let mut answer = None;
            let mut used_tool = false;
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                for update in parser.feed_line(line) {
                    match update {
                        StreamUpdate::Status("tool") => used_tool = true,
                        StreamUpdate::Done(text) => answer = Some(text),
                        StreamUpdate::Error(error) => panic!("{provider}: {error}"),
                        _ => {}
                    }
                }
            }
            assert!(used_tool, "{provider} did not fetch the URL");
            assert!(
                answer
                    .as_deref()
                    .is_some_and(|a| a.contains("Example Domain")),
                "{provider}: {answer:?}"
            );
            eprintln!(
                "{provider}: single URL fetch passed in {:.2}s",
                started.elapsed().as_secs_f64()
            );
        }
    }
}
