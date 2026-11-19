mod commands;
mod db;
mod events;
mod fetch;
mod icons;
mod resource_policy;
mod scheduler;

use std::sync::{Arc, Mutex};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = db::resolve_db_path(&app_data_dir);

            let conn = db::init(&db_path)?;
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

            Ok(())
        })
        .on_window_event(|window, event| {
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
