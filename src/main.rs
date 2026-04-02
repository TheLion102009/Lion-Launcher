#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;

mod gui;
mod core;
mod api;
mod utils;
mod types;
mod config;

fn main() {
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        if std::env::var("DISPLAY").is_err() {
            std::env::set_var("DISPLAY", ":0");
        }
    }

    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Fenster-Icon aus eingebetteten Bytes setzen (Titelleiste / Taskleiste)
            let window = app.get_webview_window("main").unwrap();
            let icon_bytes = include_bytes!("../icons/icon.png");
            if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
                window.set_icon(icon).ok();
            }
            #[cfg(debug_assertions)]
            window.open_devtools();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // General
            gui::greet,
            gui::get_embedded_logo_data_url,
            gui::initialize_launcher,
            // Settings
            gui::get_config,
            gui::save_config,
            gui::get_minecraft_versions,
            gui::get_fabric_versions,
            gui::get_quilt_versions,
            gui::get_forge_versions,
            gui::get_neoforge_versions,
            gui::get_system_memory,
            // Profiles
            gui::get_profiles,
            gui::create_profile,
            gui::delete_profile,
            gui::update_profile,
            gui::launch_profile,
            // Mods - Browser
            gui::get_modrinth_categories,
            gui::search_mods,
            gui::get_mod_info,
            gui::get_mod_versions,
            gui::install_mod,
            gui::uninstall_mod,
            // Mods - Verwaltung
            gui::get_installed_mods,
            gui::toggle_mod,
            gui::delete_mod,
            gui::bulk_toggle_mods,
            gui::bulk_delete_mods,
            gui::check_mod_updates,
            // Resource Packs
            gui::get_installed_resourcepacks,
            gui::search_resourcepacks,
            gui::install_resourcepack,
            gui::delete_resourcepack,
            // Shader Packs
            gui::search_shaderpacks,
            gui::install_shaderpack,
            gui::get_installed_shaderpacks,
            gui::delete_shaderpack,
            // Modpacks
            gui::search_modpacks,
            // Worlds
            gui::get_worlds,
            gui::launch_world,
            // Servers
            gui::get_servers,
            gui::launch_server,
            // Auth
            gui::auth::get_accounts,
            gui::auth::get_active_account,
            gui::auth::set_active_account,
            gui::auth::begin_microsoft_login,
            gui::auth::poll_microsoft_login,
            gui::auth::add_offline_account,
            gui::auth::remove_account,
            gui::auth::refresh_account,
            gui::auth::open_auth_url,
            // Logs & Folders
            gui::get_profile_logs,
            gui::open_profile_folder,
            gui::get_log_files,
            // Instance Management
            gui::stop_profile,
            gui::get_running_profiles,
            // Profile Maintenance
            gui::repair_profile,
            gui::clear_profile_cache,
            // Settings Sync
            gui::sync_settings_to_profile,
            gui::sync_settings_from_profile,
            gui::toggle_settings_sync,
            gui::get_settings_sync_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
