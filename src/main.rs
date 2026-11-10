#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod api;
mod config;
mod core;
mod gui;
mod types;
mod utils;

fn main() {
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        if std::env::var("DISPLAY").is_err() {
            std::env::set_var("DISPLAY", ":0");
        }
    }

    utils::logging::init_logging();
    gui::queue_lion_file_from_args();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // Externe Links niemals im Launcher-Fenster öffnen, sondern im System-Browser
        .plugin(gui::external_links::navigation_guard())
        .setup(|app| {
            // Hauptfenster selbst erzeugen ("create": false in tauri.conf.json), damit
            // Popups/neue Fenster abgefangen und im System-Browser geöffnet werden können.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("Window config 'main' missing in tauri.conf.json")?;
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .on_new_window(|url, _features| {
                    gui::external_links::handle_new_window_request(&url);
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;

            // Fenster-Icon aus eingebetteten Bytes setzen (Titelleiste / Taskleiste)
            let icon_bytes = include_bytes!("../icons/icon.png");
            if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
                window.set_icon(icon).ok();
            }
            #[cfg(debug_assertions)]
            window.open_devtools();

            // Freunde & geteilte Instanzen (P2P, ohne eigenen Server)
            gui::friends::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // General
            gui::greet,
            gui::get_embedded_logo_data_url,
            gui::external_links::open_external_url,
            // Launcher-Updates
            gui::updater::updater_check,
            gui::updater::updater_install,
            // Freunde
            gui::friends::friends_state,
            gui::friends::friends_add,
            gui::friends::friends_accept,
            gui::friends::friends_remove,
            gui::friends::friends_messages,
            gui::friends::friends_send_message,
            // Geteilte Instanzen
            gui::friends::share::share_status,
            gui::friends::share::share_create,
            gui::friends::share::share_pending_changes,
            gui::friends::share::share_push,
            gui::friends::share::share_invite_friend,
            gui::friends::share::share_remove_member,
            gui::friends::share::share_stop,
            gui::friends::share::share_list_config_files,
            gui::friends::share::share_set_config_files,
            gui::friends::share::share_preview,
            gui::friends::share::share_accept,
            gui::friends::share::share_decline_invite,
            gui::friends::share::share_check_update,
            gui::friends::share::share_check_all_updates,
            gui::friends::share::share_apply_update,
            gui::friends::share::share_leave,
            gui::initialize_launcher,
            // Settings
            gui::get_config,
            gui::save_config,
            gui::get_minecraft_versions,
            gui::get_fabric_versions,
            gui::get_quilt_versions,
            gui::get_forge_versions,
            gui::get_forge_supported_mc_versions,
            gui::get_fabric_supported_mc_versions,
            gui::get_quilt_supported_mc_versions,
            gui::get_neoforge_supported_mc_versions,
            gui::get_neoforge_versions,
            gui::get_system_memory,
            gui::get_latest_stable_jdk_major,
            gui::get_java_runtime_options,
            gui::ensure_java_runtime_for_settings,
            // Profiles
            gui::get_profiles,
            gui::create_profile,
            gui::delete_profile,
            gui::update_profile,
            gui::launch_profile,
            gui::prepare_profile_download,
            gui::export_profile_share_data,
            gui::import_profile_share_data,
            gui::content_share::export_profile_content,
            gui::content_share::import_profile_content,
            gui::save_lion_file_with_dialog,
            gui::take_pending_lion_file,
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
            gui::update_mod,
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
            gui::install_modpack,
            // Worlds
            gui::get_worlds,
            gui::launch_world,
            // Servers
            gui::get_servers,
            gui::launch_server,
            gui::add_server,
            gui::remove_server,
            // Local Server Profiles
            gui::get_server_profiles,
            gui::create_server_profile,
            gui::get_server_software_versions,
            gui::delete_server_profile,
            gui::get_server_properties,
            gui::save_server_properties,
            gui::get_server_content,
            gui::import_server_content_file,
            gui::remove_server_content_file,
            gui::get_server_console_output,
            gui::open_server_folder,
            gui::start_server_profile,
            gui::stop_server_profile,
            gui::kill_server_profile,
            gui::restart_server_profile,
            gui::get_running_server_profiles,
            gui::get_server_runtime,
            gui::send_server_console_command,
            gui::update_server_profile_settings,
            gui::list_server_files,
            gui::read_server_text_file,
            gui::write_server_text_file,
            gui::delete_server_path,
            gui::move_server_path,
            gui::create_server_folder,
            gui::upload_server_file,
            gui::get_server_content_context,
            gui::search_server_content,
            gui::get_server_installed_content,
            gui::toggle_server_content,
            gui::delete_server_content,
            gui::install_server_content,
            gui::check_server_content_updates,
            gui::list_server_backups,
            gui::create_server_backup,
            gui::restore_server_backup,
            gui::delete_server_backup,
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
            gui::auth::upload_skin_file,
            gui::auth::apply_skin_from_url,
            gui::auth::get_skin_texture,
            gui::auth::resolve_player_uuid,
            gui::auth::save_skin_locally,
            gui::auth::load_saved_skin,
            gui::auth::delete_saved_skin,
            // Logs & Folders
            gui::get_profile_logs,
            gui::get_live_launcher_logs,
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
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {
            // macOS übergibt per Doppelklick geöffnete Dateien als Event statt als Argument
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = _event {
                use tauri::Emitter;
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        gui::queue_lion_file(path);
                    }
                }
                let _ = _app.emit("lion-file-opened", ());
            }
        });
}
