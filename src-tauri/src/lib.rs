mod commands;
mod db;
mod fetch;
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
            app.manage(Mutex::new(conn));

            // A separate connection for the scheduler (see
            // scheduler::mod's doc comment: WAL mode is meant to let
            // UI reads and background-poll writes avoid sharing a lock).
            let scheduler_conn = db::init(&db_path)?;
            let scheduler_conn = Arc::new(Mutex::new(scheduler_conn));
            tauri::async_runtime::spawn(scheduler::run(scheduler_conn));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::add_feed::add_feed])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
