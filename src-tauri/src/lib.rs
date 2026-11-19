mod commands;
mod db;
mod events;
mod fetch;
mod icons;
mod resource_policy;
mod scheduler;
mod updater;

use std::sync::{Arc, Mutex};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            #[cfg(feature = "updater-test")]
            let app_data_dir = if app.config().identifier == "io.github.s0okju.aequo.updater-test" {
                std::env::var_os("AEQUO_UPDATE_TEST_DATA_DIR")
                    .or_else(|| {
                        option_env!("AEQUO_UPDATE_TEST_DATA_DIR").map(std::ffi::OsString::from)
                    })
                    .map(std::path::PathBuf::from)
                    .unwrap_or(app_data_dir)
            } else {
                app_data_dir
            };
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = db::resolve_db_path(&app_data_dir);

            let conn = db::init(&db_path)?;
            let configured = app
                .config()
                .plugins
                .0
                .get("updater")
                .and_then(|value| value.get("pubkey"))
                .and_then(|value| value.as_str())
                .is_some_and(|key| !key.is_empty());
            let updates =
                updater::Service::load(&conn, app.package_info().version.to_string(), configured)?;
            app.manage(updater::activity::Activity::default());
            app.manage(updates.clone());
            tauri::async_runtime::spawn(updates.run(app.handle().clone()));
            let policy = resource_policy::ResourcePolicy::load(&conn)?;
            app.manage(policy.clone());
            db::run_retention_cleanup(
                &conn,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            )?;
            app.manage(Mutex::new(conn));
            app.manage(commands::ai::AiRuns::default());
            app.manage(commands::article::ArticleReads::default());

            // A separate connection for the scheduler (see
            // scheduler::mod's doc comment: WAL mode is meant to let
            // UI reads and background-poll writes avoid sharing a lock).
            let scheduler_conn = db::init(&db_path)?;
            let scheduler_conn = Arc::new(Mutex::new(scheduler_conn));
            let schedule_changed = Arc::new(tokio::sync::Notify::new());
            app.manage(schedule_changed.clone());
            tauri::async_runtime::spawn(scheduler::run(
                scheduler_conn.clone(),
                app.handle().clone(),
                schedule_changed,
            ));

            // Feed icons share the scheduler's background connection.
            let icon_sync = icons::IconSync::default();
            app.manage(icon_sync.clone());
            tauri::async_runtime::spawn(icons::run(
                scheduler_conn,
                app.handle().clone(),
                icon_sync,
            ));

            #[cfg(feature = "updater-test")]
            updater::test_driver::spawn(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let Some(updates) = window.try_state::<updater::Service>() {
                if matches!(event, tauri::WindowEvent::Focused(true)) {
                    updates.wake();
                }
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    updates.forget_window(window.label());
                }
            }
            if matches!(
                event,
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(_)
            ) {
                let hidden =
                    window.is_minimized().unwrap_or(false) || !window.is_visible().unwrap_or(true);
                if let Some(policy) = window.try_state::<resource_policy::ResourcePolicy>() {
                    policy.set_native_hidden(hidden);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            updater::get_update_state,
            updater::get_update_settings,
            updater::check_for_updates,
            updater::install_available_update,
            updater::set_update_settings,
            updater::set_update_draft,
            updater::restart_after_update,
            commands::add_feed::add_feed,
            commands::article::load_article,
            commands::article::cancel_article_read,
            commands::ai::list_ai_sessions,
            commands::ai::list_all_ai_sessions,
            commands::ai::latest_ai_session_for_item,
            commands::ai::add_ai_context_items,
            commands::ai::remove_ai_context_item,
            commands::ai::load_ai_transcript,
            commands::ai::start_ai_message,
            commands::ai::cancel_ai_message,
            commands::ai::check_ai_providers,
            commands::rename_feed,
            commands::ai::delete_ai_session,
            commands::annotations::list_item_annotations,
            commands::annotations::create_annotation,
            commands::annotations::update_annotation,
            commands::annotations::delete_annotation,
            commands::refresh_feed::refresh_feed,
            commands::list_feeds,
            commands::get_feed_icons,
            commands::list_items,
            commands::get_item_summary,
            commands::count_unread_items,
            commands::list_feed_folders,
            commands::create_feed_folder,
            commands::rename_feed_folder,
            commands::delete_feed_folder,
            commands::update_feed_organization,
            commands::reorder_feeds_in_folder,
            commands::set_feed_pinned,
            commands::set_feed_refresh_interval,
            commands::mark_read,
            commands::mark_items_read,
            commands::mark_items_unread,
            commands::delete_feed,
            commands::storage::get_retention_settings,
            commands::storage::set_retention_settings,
            commands::storage::preview_retention_cleanup,
            commands::storage::run_retention_cleanup,
            commands::storage::set_item_saved,
            commands::resources::get_resource_settings,
            commands::resources::set_resource_settings,
            commands::resources::set_resource_visibility,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
