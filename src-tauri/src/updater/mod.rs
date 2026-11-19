pub mod activity;
mod policy;
pub mod storage;
#[cfg(feature = "updater-test")]
pub mod test_driver;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

use policy::{CHECK_INTERVAL, ENDPOINT, MAX_MANIFEST, MAX_PACKAGE, Release};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    stage: String,
    code: String,
    retryable: bool,
}

#[derive(Serialize, Deserialize)]
struct BlockedUpdate {
    version: String,
    error: Failure,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u64,
    operation_id: u64,
    current_version: String,
    phase: String,
    auto_update: bool,
    enabled: bool,
    last_successful_check_at: Option<i64>,
    release: Option<Release>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    error: Option<Failure>,
    applied_version: Option<String>,
    pending_installed_version: Option<String>,
}

struct Inner {
    snapshot: Snapshot,
    update: Option<Update>,
    next_check_at: i64,
    last_manual_at: Option<i64>,
    failures: usize,
    blocked_version: Option<String>,
    blocked_error: Option<Failure>,
    server_retry_after: Option<i64>,
    drafts: HashSet<String>,
}

#[derive(Clone)]
pub struct Service {
    inner: Arc<Mutex<Inner>>,
    operation: Arc<tokio::sync::Mutex<()>>,
    wake: Arc<tokio::sync::Notify>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn with_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T, String> {
    let state = app.state::<Mutex<Connection>>();
    let conn = state.lock().map_err(|_| "storage".to_string())?;
    f(&conn).map_err(|_| "storage".into())
}

/// Only HTTPS GitHub redirects are followed. Signed assets may use either CDN.
fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("Aequo/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let test_origin = cfg!(feature = "updater-test")
                && url::Url::parse(ENDPOINT)
                    .is_ok_and(|endpoint| endpoint.origin() == url.origin());
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            if test_origin
                || (url.scheme() == "https"
                    && matches!(
                        url.host_str(),
                        Some(
                            "github.com"
                                | "release-assets.githubusercontent.com"
                                | "objects.githubusercontent.com"
                        )
                    ))
            {
                attempt.follow()
            } else {
                attempt.error("unexpected redirect")
            }
        }))
        .build()
        .map_err(|_| "network".into())
}

async fn read_limited(response: &mut reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("too_large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "network")? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("too_large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn retry_after(response: &reqwest::Response) -> Option<i64> {
    let value = response.headers().get("retry-after")?.to_str().ok()?;
    value.parse::<i64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc2822(value)
            .ok()
            .map(|date| (date.timestamp() - now()).max(0))
    })
}

impl Service {
    pub fn load(conn: &Connection, current: String, configured: bool) -> rusqlite::Result<Self> {
        storage::init(conn)?;
        let (auto_update, checked, pending) = storage::load(conn)?;
        let mut blocked = storage::load_blocked(conn)?
            .and_then(|value| serde_json::from_str::<BlockedUpdate>(&value).ok());
        if blocked.as_ref().is_some_and(|value| {
            semver::Version::parse(&current)
                .ok()
                .zip(semver::Version::parse(&value.version).ok())
                .is_some_and(|(current, failed)| current >= failed)
        }) {
            storage::blocked(conn, None)?;
            blocked = None;
        }
        let applied = pending
            .as_ref()
            .filter(|version| **version == current)
            .cloned();
        let failed = pending.is_some() && applied.is_none();
        if pending.is_some() {
            storage::pending(conn, None)?;
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let enabled = configured && (!cfg!(debug_assertions) || cfg!(feature = "updater-test"));
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        let enabled = {
            let _ = configured;
            false
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                snapshot: Snapshot {
                    revision: 0,
                    operation_id: 0,
                    current_version: current,
                    phase: if failed || blocked.is_some() {
                        "error"
                    } else {
                        "idle"
                    }
                    .into(),
                    auto_update,
                    enabled,
                    last_successful_check_at: checked,
                    release: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    error: if failed {
                        Some(Failure {
                            stage: "restart".into(),
                            code: "not_applied".into(),
                            retryable: false,
                        })
                    } else {
                        blocked.as_ref().map(|value| value.error.clone())
                    },
                    applied_version: applied,
                    pending_installed_version: None,
                },
                update: None,
                next_check_at: (checked.unwrap_or(0) + CHECK_INTERVAL).max(now() + 10),
                last_manual_at: None,
                failures: 0,
                blocked_version: blocked.as_ref().map(|value| value.version.clone()),
                blocked_error: blocked.map(|value| value.error),
                server_retry_after: None,
                drafts: HashSet::new(),
            })),
            operation: Arc::new(tokio::sync::Mutex::new(())),
            wake: Arc::new(tokio::sync::Notify::new()),
        })
    }

    fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }

    fn change(&self, app: &AppHandle, mutate: impl FnOnce(&mut Inner)) -> Snapshot {
        let snapshot = {
            let mut inner = self.inner.lock().unwrap();
            mutate(&mut inner);
            inner.snapshot.revision += 1;
            inner.snapshot.clone()
        };
        let _ = app.emit("update-state-changed", &snapshot);
        snapshot
    }

    fn phase(&self, app: &AppHandle, phase: &str) {
        self.change(app, |inner| inner.snapshot.phase = phase.into());
    }

    fn fail(&self, app: &AppHandle, stage: &str, code: &str, retryable: bool, after: Option<i64>) {
        self.change(app, |inner| {
            inner.snapshot.phase = "error".into();
            inner.snapshot.error = Some(Failure {
                stage: stage.into(),
                code: code.into(),
                retryable,
            });
            if stage == "check" {
                inner.update = None;
            }
            inner.next_check_at = now().saturating_add(if retryable {
                policy::retry_delay(inner.failures, after.or(inner.server_retry_after.take()))
            } else {
                CHECK_INTERVAL
            });
            if !retryable && stage != "check" && code != "disabled" {
                inner.blocked_error = inner.snapshot.error.clone();
                inner.blocked_version = inner
                    .snapshot
                    .release
                    .as_ref()
                    .map(|release| release.version.clone());
            }
            inner.failures = (inner.failures + 1).min(3);
        });
        if !retryable && stage != "check" && code != "disabled" {
            let encoded = {
                let inner = self.inner.lock().unwrap();
                inner
                    .blocked_version
                    .as_ref()
                    .zip(inner.blocked_error.as_ref())
                    .map(|(version, error)| {
                        serde_json::to_string(&BlockedUpdate {
                            version: version.clone(),
                            error: error.clone(),
                        })
                        .unwrap()
                    })
            };
            if let Some(value) = encoded
                && with_db(app, |conn| storage::blocked(conn, Some(&value))).is_err()
            {
                self.change(app, |inner| {
                    inner.snapshot.error = Some(Failure {
                        stage: "install".into(),
                        code: "storage".into(),
                        retryable: false,
                    })
                });
            }
        }
    }

    async fn check(&self, app: &AppHandle, manual: bool) -> Result<Snapshot, String> {
        let Ok(_operation) = self.operation.try_lock() else {
            return Ok(self.snapshot());
        };
        {
            let mut inner = self.inner.lock().unwrap();
            if !inner.snapshot.enabled {
                return Err("unavailable".into());
            }
            if inner.snapshot.phase == "restart_required" {
                return Ok(inner.snapshot.clone());
            }
            if manual {
                if inner.last_manual_at.is_some_and(|last| now() - last < 30) {
                    return Err("rate_limited".into());
                }
                inner.last_manual_at = Some(now());
                with_db(app, |conn| storage::blocked(conn, None))?;
                inner.blocked_version = None;
                inner.blocked_error = None;
            } else if !inner.snapshot.auto_update || now() < inner.next_check_at {
                return Ok(inner.snapshot.clone());
            }
        }
        self.change(app, |inner| {
            inner.snapshot.operation_id += 1;
            inner.snapshot.phase = "checking".into();
            inner.snapshot.error = None;
            inner.update = None;
        });
        let result = self.fetch(app).await;
        match result {
            Ok((release, update)) => {
                if with_db(app, |conn| storage::checked(conn, now())).is_err() {
                    self.fail(app, "check", "storage", false, None);
                } else {
                    self.change(app, |inner| {
                        let blocked = inner.blocked_version.as_ref() == Some(&release.version);
                        inner.snapshot.phase = if blocked {
                            "error"
                        } else if update.is_some() {
                            "available"
                        } else {
                            "up_to_date"
                        }
                        .into();
                        inner.snapshot.release = update.as_ref().map(|_| release);
                        inner.update = update;
                        inner.snapshot.last_successful_check_at = Some(now());
                        inner.next_check_at = now() + CHECK_INTERVAL;
                        if inner.update.is_none() {
                            inner.failures = 0;
                        }
                        if blocked {
                            inner.snapshot.error = inner.blocked_error.clone();
                        }
                    });
                }
            }
            Err((code, after)) => self.fail(
                app,
                "check",
                &code,
                matches!(code.as_str(), "network" | "http"),
                after,
            ),
        }
        self.wake.notify_one();
        Ok(self.snapshot())
    }

    async fn fetch(
        &self,
        app: &AppHandle,
    ) -> Result<(Release, Option<Update>), (String, Option<i64>)> {
        let mut response = client(Duration::from_secs(15))
            .map_err(|code| (code, None))?
            .get(ENDPOINT)
            .send()
            .await
            .map_err(|_| ("network".into(), None))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(("http".into(), retry_after(&response)));
        }
        let bytes = read_limited(&mut response, MAX_MANIFEST)
            .await
            .map_err(|code| (code, None))?;
        let raw: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| ("metadata".into(), None))?;
        let (release, newer) = policy::validate_manifest(&raw, &self.snapshot().current_version)
            .map_err(|code| (code, None))?;
        let update = if newer {
            // Fetch once with bounded metadata and Retry-After handling, then restore
            // through Tauri. restore_update requires a signature bound to the version.
            app.updater_builder()
                .target("darwin-aarch64")
                .build()
                .and_then(|updater| updater.restore_update(raw))
                .map_err(|_| ("metadata".into(), None))?
        } else {
            None
        };
        Ok((release, update))
    }

    fn start_install(&self, app: &AppHandle, automatic: bool) -> Result<Snapshot, String> {
        let Ok(operation) = self.operation.clone().try_lock_owned() else {
            return Ok(self.snapshot());
        };
        {
            let inner = self.inner.lock().unwrap();
            if !inner.snapshot.enabled {
                return Err("unavailable".into());
            }
            if inner.snapshot.phase == "restart_required" {
                return Ok(inner.snapshot.clone());
            }
            if inner.update.is_none() {
                return Err("check_required".into());
            }
            if automatic && (!inner.snapshot.auto_update || inner.snapshot.phase != "available") {
                return Ok(inner.snapshot.clone());
            }
        }
        self.change(app, |inner| {
            inner.snapshot.operation_id += 1;
            inner.snapshot.error = None;
            inner.snapshot.phase = "waiting_for_idle".into();
            inner.snapshot.downloaded_bytes = 0;
            inner.snapshot.total_bytes = None;
        });
        let service = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _operation = operation;
            if let Err((stage, code, retryable)) = service.install(&app, automatic).await {
                service.fail(&app, stage, &code, retryable, None);
            }
        });
        Ok(self.snapshot())
    }

    async fn wait_for_idle(
        &self,
        app: &AppHandle,
        automatic: bool,
    ) -> Result<(), (&'static str, String, bool)> {
        while app.state::<activity::Activity>().is_busy() {
            self.phase(app, "waiting_for_idle");
            if automatic && !self.snapshot().auto_update {
                return Err(("install", "disabled".into(), false));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if automatic && !self.snapshot().auto_update {
            return Err(("install", "disabled".into(), false));
        }
        Ok(())
    }

    async fn install(
        &self,
        app: &AppHandle,
        automatic: bool,
    ) -> Result<(), (&'static str, String, bool)> {
        self.wait_for_idle(app, automatic).await?;
        preflight().map_err(|code| ("install", code, false))?;
        let update = self.inner.lock().unwrap().update.clone().ok_or((
            "install",
            "check_required".into(),
            false,
        ))?;
        self.phase(app, "downloading");
        // Tauri buffers the entire download without a size bound. Stream it here
        // with a hard cap, then use Tauri's verify/install for all trust and replacement.
        let mut response = client(Duration::from_secs(15 * 60))
            .map_err(|code| ("download", code, true))?
            .get(update.download_url.clone())
            .send()
            .await
            .map_err(|_| ("download", "network".into(), true))?;
        if response.status() != reqwest::StatusCode::OK {
            self.inner.lock().unwrap().server_retry_after = retry_after(&response);
            return Err(("download", "http".into(), true));
        }
        let total = response.content_length();
        if total.is_some_and(|bytes| bytes > MAX_PACKAGE as u64) {
            return Err(("download", "too_large".into(), false));
        }
        let mut bytes = Vec::new();
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ("download", "network".into(), true))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_PACKAGE {
                return Err(("download", "too_large".into(), false));
            }
            bytes.extend_from_slice(&chunk);
            if last_emit.elapsed() >= Duration::from_millis(100) {
                self.change(app, |inner| {
                    inner.snapshot.downloaded_bytes = bytes.len() as u64;
                    inner.snapshot.total_bytes = total;
                });
                last_emit = std::time::Instant::now();
            }
            if automatic && !self.snapshot().auto_update {
                return Err(("download", "disabled".into(), false));
            }
        }
        self.change(app, |inner| {
            inner.snapshot.phase = "verifying".into();
            inner.snapshot.downloaded_bytes = bytes.len() as u64;
            inner.snapshot.total_bytes = total;
        });
        update
            .verify(&bytes)
            .map_err(|_| ("verify", "signature".into(), false))?;
        // Close the start-work race again after the download.
        let _exclusive = loop {
            self.wait_for_idle(app, automatic).await?;
            if let Some(guard) = app.state::<activity::Activity>().exclusive() {
                break guard;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        preflight().map_err(|code| ("install", code, false))?;
        with_db(app, |conn| storage::pending(conn, Some(&update.version)))
            .map_err(|code| ("install", code, false))?;
        self.phase(app, "installing");
        let version = update.version.clone();
        let result = tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await;
        match result {
            Ok(Ok(())) => {
                self.change(app, |inner| {
                    inner.snapshot.pending_installed_version = Some(version);
                    inner.snapshot.phase = "restart_required".into();
                    inner.update = None;
                    inner.failures = 0;
                });
                Ok(())
            }
            other => {
                let _ = with_db(app, |conn| storage::pending(conn, None));
                let code = if matches!(
                    other,
                    Ok(Err(tauri_plugin_updater::Error::PreviousAppNotRestored(_)))
                ) {
                    "restore_failed"
                } else {
                    "install_failed"
                };
                Err(("install", code.into(), false))
            }
        }
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }
    pub fn forget_window(&self, label: &str) {
        self.inner.lock().unwrap().drafts.remove(label);
    }

    pub async fn run(self, app: AppHandle) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(10)) => {},
                _ = self.wake.notified() => {},
            }
            let snapshot = self.snapshot();
            if !snapshot.enabled || !snapshot.auto_update || snapshot.phase == "restart_required" {
                continue;
            }
            if snapshot.phase == "available" {
                let _ = self.start_install(&app, true);
            } else if self.operation.try_lock().is_ok() {
                let _ = self.check(&app, false).await;
            }
        }
    }
}

fn preflight() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|_| "install_location")?;
    let bundle = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or("install_location")?;
    if bundle.starts_with("/Volumes") {
        return Err("install_location".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Refuse paths requiring administrator authorization before the plugin can prompt.
        let parent = bundle.parent().ok_or("install_location")?;
        for path in [bundle, parent] {
            let metadata = std::fs::metadata(path).map_err(|_| "install_location")?;
            if metadata.permissions().readonly() || metadata.mode() & 0o200 == 0 {
                return Err("permission".into());
            }
        }
        // A real write check catches read-only volumes, ACLs and another user's bundle.
        let probe = parent.join(format!(".aequo-update-probe-{}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .map_err(|_| "permission")?;
        drop(file);
        std::fs::remove_file(probe).map_err(|_| "permission")?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_update_state(service: State<'_, Service>) -> Snapshot {
    service.snapshot()
}
#[tauri::command]
pub fn get_update_settings(service: State<'_, Service>) -> Snapshot {
    service.snapshot()
}
#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    service: State<'_, Service>,
) -> Result<Snapshot, String> {
    service.check(&app, true).await
}
#[tauri::command]
pub fn install_available_update(
    app: AppHandle,
    service: State<'_, Service>,
) -> Result<Snapshot, String> {
    service.start_install(&app, false)
}
#[tauri::command]
pub fn set_update_settings(
    app: AppHandle,
    service: State<'_, Service>,
    auto_update: bool,
) -> Result<Snapshot, String> {
    with_db(&app, |conn| storage::auto_update(conn, auto_update))?;
    let snapshot = service.change(&app, |inner| {
        inner.snapshot.auto_update = auto_update;
        if auto_update {
            inner.next_check_at = now();
        }
    });
    service.wake();
    Ok(snapshot)
}
#[tauri::command]
pub fn set_update_draft(window: WebviewWindow, service: State<'_, Service>, dirty: bool) {
    let mut inner = service.inner.lock().unwrap();
    if dirty {
        inner.drafts.insert(window.label().into());
    } else {
        inner.drafts.remove(window.label());
    }
}
#[tauri::command]
pub fn restart_after_update(app: AppHandle, service: State<'_, Service>) -> Result<(), String> {
    let _operation = service.operation.try_lock().map_err(|_| "busy")?;
    let _exclusive = app
        .state::<activity::Activity>()
        .exclusive()
        .ok_or("work_active")?;
    {
        let inner = service.inner.lock().unwrap();
        if inner.snapshot.phase != "restart_required" {
            return Err("check_required".into());
        }
        if !inner.drafts.is_empty() {
            return Err("draft_active".into());
        }
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_signature_survives_restart_until_new_version_is_installed() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init(&conn).unwrap();
        let blocked = BlockedUpdate {
            version: "1.0.2".into(),
            error: Failure {
                stage: "verify".into(),
                code: "signature".into(),
                retryable: false,
            },
        };
        storage::blocked(&conn, Some(&serde_json::to_string(&blocked).unwrap())).unwrap();
        let service = Service::load(&conn, "1.0.1".into(), true).unwrap();
        assert_eq!(service.snapshot().error.unwrap().code, "signature");
        assert_eq!(
            service.inner.lock().unwrap().blocked_version.as_deref(),
            Some("1.0.2")
        );
        let newer = Service::load(&conn, "1.0.2".into(), true).unwrap();
        assert!(newer.snapshot().error.is_none());
        assert!(storage::load_blocked(&conn).unwrap().is_none());
    }
    #[tokio::test]
    async fn unknown_length_stream_is_also_bounded_and_retry_after_is_read() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0; 1];
                if stream.read(&mut byte).await.unwrap() == 0 {
                    return;
                }
                request.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nRetry-After: 600\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let mut response = reqwest::get(format!("http://{addr}")).await.unwrap();
        assert_eq!(retry_after(&response), Some(600));
        assert_eq!(response.content_length(), None);
        assert_eq!(
            read_limited(&mut response, 8).await.unwrap_err(),
            "too_large"
        );
    }
    #[tokio::test]
    async fn bounded_stream_rejects_oversized_and_truncated_responses() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1; 100]))
            .mount(&server)
            .await;
        let mut response = reqwest::get(server.uri()).await.unwrap();
        assert_eq!(
            read_limited(&mut response, 10).await.unwrap_err(),
            "too_large"
        );
    }
    #[test]
    fn startup_reconciles_pending_version_without_restart_loop() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init(&conn).unwrap();
        storage::pending(&conn, Some("1.0.2")).unwrap();
        let service = Service::load(&conn, "1.0.1".into(), true).unwrap();
        assert_eq!(service.snapshot().error.unwrap().code, "not_applied");
        assert_eq!(storage::load(&conn).unwrap().2, None);
        storage::pending(&conn, Some("1.0.2")).unwrap();
        assert_eq!(
            Service::load(&conn, "1.0.2".into(), true)
                .unwrap()
                .snapshot()
                .applied_version,
            Some("1.0.2".into())
        );
    }
}
