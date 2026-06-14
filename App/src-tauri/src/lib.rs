// EcoAlert · Tauri 库入口
mod auth;
mod commands;
mod pipeline;
mod state;
mod store;
mod stream;

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
            commands::get_channel_runtime_status,
            commands::list_alarms,
            commands::ack_alarm,
            commands::resolve_alarm,
            commands::get_algorithm_config,
            commands::list_algorithm_config_sources,
            commands::get_effective_algorithm_config,
            commands::update_algorithm_config,
            commands::delete_algorithm_config,
            commands::test_vlm_config,
            commands::get_roi_config,
            commands::list_roi_config_sources,
            commands::update_roi_config,
            commands::delete_roi_config,
            commands::test_roi_config,
            commands::list_notification_targets,
            commands::create_notification_target,
            commands::update_notification_target,
            commands::delete_notification_target,
            commands::list_notification_history,
            commands::test_notification_target,
            commands::resend_notification,
            commands::get_security_config,
            commands::update_security_config,
            commands::change_password,
            commands::get_data_dir,
            commands::start_oauth_binding,
            commands::check_oauth_status,
            commands::verify_channel_credentials,
            commands::open_devtools,
            commands::probe_url,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 失败");
}
