mod commands;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new().expect("FilePilot platform state could not be initialized");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_platform,
            commands::open_full_disk_access_settings,
            commands::save_settings,
            commands::remember_path,
            commands::start_scan,
            commands::start_large_files,
            commands::start_duplicates,
            commands::cleanup_duplicates,
            commands::start_system_data,
            commands::cleanup_system_data,
            commands::preview_rename,
            commands::preview_organize,
            commands::preview_metadata,
            commands::apply_operation,
            commands::apply_metadata,
            commands::undo,
            commands::get_task,
            commands::cancel_task,
            commands::list_operations,
            commands::export_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running FilePilot");
}
