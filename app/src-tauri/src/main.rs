//! ChatViewer desktop app: Tauri shell around the shared core.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod media;
mod state;

use tauri::Manager;

/// `ChatViewer cli <args>` runs the bundled `chatviewer` command-line tool
/// that sits next to the app executable (useful for AppImages, where the
/// CLI is not on the PATH).
fn run_bundled_cli() -> Option<std::process::ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "cli" {
        return None;
    }
    let name = if cfg!(windows) {
        "chatviewer.exe"
    } else {
        "chatviewer"
    };
    let cli = std::env::current_exe().ok()?.parent()?.join(name);
    let status = std::process::Command::new(&cli).args(args).status();
    Some(match status {
        Ok(s) => std::process::ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("error: the bundled chatviewer CLI could not be started: {e}");
            std::process::ExitCode::FAILURE
        }
    })
}

fn main() -> std::process::ExitCode {
    if let Some(code) = run_bundled_cli() {
        return code;
    }
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
            commands::about,
        ])
        .run(tauri::generate_context!())
        .expect("ChatViewer failed to start");
    std::process::ExitCode::SUCCESS
}
