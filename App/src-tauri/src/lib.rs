// EcoAlert · Tauri 库入口
mod auth;
mod commands;
mod pipeline;
mod state;
mod store;
mod stream;

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::try_init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = state::AppState::new(&handle).expect("初始化应用状态失败");
            app.manage(state.clone());
            state::spawn_status_ticker(handle.clone());
            state::spawn_scene_state_ticker(handle.clone());
            state::log_event(&handle, "info", "EcoAlert 启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::login,
            commands::logout,
            commands::check_auth,
            commands::list_sources,
            commands::list_groups,
            commands::create_source,
            commands::update_source,
            commands::delete_source,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::reorder,
            commands::report_scene_state,
            commands::get_state_history,
            commands::change_password,
            commands::get_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 失败");
}
