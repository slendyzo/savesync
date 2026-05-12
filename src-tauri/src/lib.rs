pub mod conflict;
pub mod git;
pub mod lfs;
pub mod local_config;
pub mod manifest;
pub mod snapshot;
pub mod sync;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
