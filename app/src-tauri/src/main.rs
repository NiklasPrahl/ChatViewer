//! ChatViewer desktop app: Tauri shell around the shared core.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod media;
mod state;

use tauri::Manager;

fn main() {
    cv_core::logging::init(None);
    log::info!("app: starting");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        .register_uri_scheme_protocol(media::SCHEME, |ctx, request| {
            media::handle(&ctx.app_handle().state::<state::AppState>(), &request)
        })
        .invoke_handler(tauri::generate_handler![
            commands::inspect,
            commands::open_archive,
            commands::open_chat,
            commands::messages,
            commands::message_index,
            commands::export_chat,
            commands::export_chats,
            commands::close_archive,
            commands::log_snapshot,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("ChatViewer failed to start");
}
