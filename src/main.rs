#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(debug_assertions)]
use tauri::Manager;

mod gui;
mod core;
mod api;
mod utils;
mod types;
mod config;

fn main() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                let window = _app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // General
            gui::greet,
            gui::initialize_launcher,
            // Settings
            gui::get_config,
            gui::save_config,
            gui::get_minecraft_versions,
            gui::get_fabric_versions,
            gui::get_quilt_versions,
            gui::get_forge_versions,
            gui::get_neoforge_versions,
            // Profiles
            gui::get_profiles,
            gui::create_profile,
            gui::delete_profile,
            gui::update_profile,
            gui::launch_profile,
            // Mods - Browser
            gui::search_mods,
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
            // Shader Packs
            gui::search_shaderpacks,
            gui::install_shaderpack,
            gui::get_installed_shaderpacks,
            // Modpacks
            gui::search_modpacks,
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
