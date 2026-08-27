//! gaia-library デスクトップシェル。データ操作は Tauri commands を経由する。
mod cli_link;
mod client_settings;
mod commands;
mod first_run;
pub mod keychain;
mod lifecycle;
mod settings_commands;
mod state;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            lifecycle::show_main(app);
        }))
        .manage(state::bootstrap())
        .manage(lifecycle::ExitState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stderr),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: None,
                    }),
                ])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::is_initialized,
            commands::first_run_setup,
            commands::call_tool,
            commands::server_status,
            settings_commands::admin_affiliation_add,
            settings_commands::admin_affiliation_list,
            settings_commands::admin_client_add,
            settings_commands::admin_client_list,
            settings_commands::admin_client_keygen,
            settings_commands::mcp_config_snippet,
            settings_commands::cli_link_status,
            settings_commands::cli_link_create,
        ])
        .setup(lifecycle::setup)
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    log::warn!("ウィンドウを隠せません: {error}");
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building gaia-library")
        .run(lifecycle::on_run_event);
}
