mod commands;
mod db;
mod fetch;
mod scheduler;

use std::sync::Mutex;

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::add_feed::add_feed])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
