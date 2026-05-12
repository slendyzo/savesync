pub mod auth;
pub mod conflict;
pub mod credentials;
pub mod games;
pub mod git;
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
