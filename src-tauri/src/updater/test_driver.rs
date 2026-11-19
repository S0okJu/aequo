//! File-based native integration driver, compiled ONLY with `updater-test`.
//! Uses a separate bundle identifier and data directory; never present in releases.
use super::*;

pub fn spawn(app: AppHandle) {
    if app.config().identifier != "io.github.s0okju.aequo.updater-test" {
        return;
    }
    let Some(root) = std::env::var_os("AEQUO_UPDATE_TEST_CONTROL_DIR")
        .or_else(|| option_env!("AEQUO_UPDATE_TEST_CONTROL_DIR").map(std::ffi::OsString::from))
        .map(std::path::PathBuf::from)
    else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let mut last_id = 0;
        let mut work = None;
        loop {
            let service = app.state::<Service>();
            let snapshot = service.snapshot();
            let _ = std::fs::write(root.join("pid"), std::process::id().to_string());
            let _ = std::fs::write(
                root.join("state.json"),
                serde_json::to_vec(&snapshot).unwrap(),
            );
            if let Ok(bytes) = std::fs::read(root.join("request.json"))
                && let Ok(request) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                let id = request["id"].as_u64().unwrap_or(0);
                if id > last_id {
                    last_id = id;
                    let result: Result<serde_json::Value, String> =
                        match request["action"].as_str().unwrap_or("") {
                            "check" => service
                                .check(&app, true)
                                .await
                                .map(|state| serde_json::to_value(state).unwrap()),
                            "install" => service
                                .start_install(&app, false)
                                .map(|state| serde_json::to_value(state).unwrap()),
                            "auto" => set_update_settings(
                                app.clone(),
                                app.state(),
                                request["enabled"].as_bool().unwrap_or(false),
                            )
                            .map(|state| serde_json::to_value(state).unwrap()),
                            "work" => {
                                work = app.state::<activity::Activity>().begin().ok();
                                Ok(serde_json::Value::Null)
                            }
                            "idle" => {
                                work = None;
                                Ok(serde_json::Value::Null)
                            }
                            "due" => {
                                service.inner.lock().unwrap().next_check_at = now();
                                service.wake();
                                Ok(serde_json::Value::Null)
                            }
                            "reset_cooldown" => {
                                service.inner.lock().unwrap().last_manual_at = None;
                                Ok(serde_json::Value::Null)
                            }
                            "draft" => {
                                let mut inner = service.inner.lock().unwrap();
                                if request["dirty"].as_bool().unwrap_or(false) {
                                    inner.drafts.insert("test".into());
                                } else {
                                    inner.drafts.remove("test");
                                }
                                Ok(serde_json::Value::Null)
                            }
                            "restart" => restart_after_update(app.clone(), app.state())
                                .map(|()| serde_json::Value::Null),
                            "quit" => {
                                app.exit(0);
                                break;
                            }
                            _ => Err("unknown action".into()),
                        };
                    let response = serde_json::json!({"id":id,"result":result.as_ref().ok(),"error":result.as_ref().err()});
                    let _ = std::fs::write(
                        root.join("response.json"),
                        serde_json::to_vec(&response).unwrap(),
                    );
                }
            }
            // Keep a live guard across commands to exercise the actual admission gate.
            std::hint::black_box(&work);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}
