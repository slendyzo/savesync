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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
