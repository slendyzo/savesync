use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

pub mod add_game;
pub mod auth;
pub mod commands;
pub mod conflict;
pub mod credentials;
pub mod games;
pub mod git;
pub mod github;
pub mod launcher;
pub mod lfs;
pub mod local_config;
pub mod manifest;
pub mod save_watcher;
pub mod snapshot;
pub mod steam;
pub mod sync;
pub mod watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::pat_connect,
            commands::oauth_client_id,
            commands::oauth_start,
            commands::oauth_poll,
            commands::github_create_repo,
            commands::init_repo,
            commands::scan_steam,
            commands::add_game,
            commands::get_local_config,
            commands::inspect_game,
            commands::open_save_folder,
            commands::force_push,
            commands::force_pull,
            commands::set_game_paused,
            commands::rename_game,
            commands::remove_game,
            commands::list_game_commits,
            commands::list_game_backups,
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray on the main window: sync needs to keep
            // running even when the user closes the visible window.
            // Quit happens explicitly via the tray's Quit menu item.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Open SaveSync", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit SaveSync", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

    let _tray = TrayIconBuilder::with_id("main")
        .tooltip("SaveSync")
        .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
            // Fallback shouldn't happen because the bundle ships an icon,
            // but build a 1x1 transparent so we never panic here.
            tauri::image::Image::new_owned(vec![0, 0, 0, 0], 1, 1)
        }))
        .menu(&menu)
        // Left-click toggles the window; right-click opens the menu.
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        show_main(app);
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
