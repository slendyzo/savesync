//! Tauri commands exposed to the React frontend.
//!
//! Thin wrappers that translate between the lib's typed Rust APIs and
//! the wire format (serde JSON) the frontend consumes. Each command:
//! - Maps to one specific user-visible action
//! - Surfaces errors as `Result<T, String>` (Tauri serializes strings
//!   easier than enums; the UI just displays the message)
//! - Keeps zero business logic — composes existing modules

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    add_game::{detect_save_state, AddReport},
    auth::{self, AuthClient, DeviceCode, UserInfo},
    credentials,
    games::TargetOs,
    git::{self, CommitInfo, GitAuth, GitIdentity},
    github::{self, Repo as GhRepo},
    lfs::LfsConfig,
    local_config::{default_config_path, LocalConfig},
    steam::{self, InstalledGame},
    sync::{self, PullOutcome, PushOutcome},
};

fn require_config() -> Result<LocalConfig, String> {
    let path = default_config_path().map_err(|e| e.to_string())?;
    LocalConfig::load_from(&path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())
}

fn resolve_auth(cfg: &LocalConfig) -> GitAuth {
    let github_pat = credentials::load(&format!("{HOST_PAT_PREFIX}https://api.github.com"))
        .ok()
        .flatten();
    let github_oauth = credentials::load(GITHUB_ACCOUNT).ok().flatten();
    if let Some(token) = github_pat.or(github_oauth) {
        return GitAuth::HttpsToken { token };
    }
    // Future: look up the host_api_base from the cfg's machine record
    // and load that host's PAT. For v1, anything that wasn't GitHub
    // PAT/OAuth falls back to no-auth (works for local-only test repos).
    let _ = cfg;
    GitAuth::None
}

/// Account name we store the GitHub access token under.
const GITHUB_ACCOUNT: &str = "github:access_token";
/// Account name for PAT auth against an arbitrary host.
const HOST_PAT_PREFIX: &str = "pat:";

/// SaveSync's GitHub OAuth App client_id.
///
/// PROJECT-MAINTAINER ONE-TIME SETUP (paste below, commit, done):
///   1. Open https://github.com/settings/applications/new
///   2. Application name: SaveSync · any URLs are fine (Device Flow
///      doesn't use callback URLs but GitHub's form demands them)
///   3. After creation, on the app's page, tick "Enable Device Flow"
///   4. Copy the Client ID (looks like `Ov23li...`) and paste below,
///      replacing `None` with `Some("Ov23li...")`
///   5. Commit the change. Every SaveSync build from then on gets the
///      seamless "Continue with GitHub" flow with zero user setup.
///
/// The client_id is a PUBLIC identifier — it's safe to commit. Device
/// Flow has no client_secret, so the client_id alone can't impersonate
/// anyone. GitHub CLI, GitHub Desktop, Tauri, Vercel CLI, and every
/// other desktop tool with "Sign in with GitHub" all ship their
/// client_id this way.
///
/// As a fallback for development, the env var GITHUB_OAUTH_CLIENT_ID
/// is still respected at build time — useful if you want to test a
/// different OAuth App without editing the source.
const OAUTH_CLIENT_ID: Option<&str> = match option_env!("GITHUB_OAUTH_CLIENT_ID") {
    Some(v) => Some(v),
    None => OAUTH_CLIENT_ID_LITERAL,
};

/// SaveSync's GitHub OAuth App, registered on slendyzo's account.
/// Public identifier — no client_secret, safe to commit.
const OAUTH_CLIENT_ID_LITERAL: Option<&str> = Some("Ov23liyxYBw79R3DfPLC");

#[tauri::command]
pub fn oauth_client_id() -> Option<String> {
    OAUTH_CLIENT_ID.map(|s| s.to_string())
}

/// Wire shape for [`InstalledGame`] — drops `PathBuf`s for the React
/// side (it doesn't care that paths are `PathBuf` vs `String`).
#[derive(Debug, Serialize)]
pub struct InstalledGameDto {
    pub steam_appid: u32,
    pub steam_display_name: String,
    pub ludusavi_name: Option<String>,
    pub install_dir: String,
    pub resolved_save_path: Option<String>,
    pub is_auto_addable: bool,
}

impl From<InstalledGame> for InstalledGameDto {
    fn from(g: InstalledGame) -> Self {
        let is_auto_addable = g.is_auto_addable();
        Self {
            steam_appid: g.steam_appid,
            steam_display_name: g.steam_display_name,
            ludusavi_name: g.ludusavi_name,
            install_dir: g.install_dir.to_string_lossy().into_owned(),
            resolved_save_path: g.resolved_save_path.map(|p| p.to_string_lossy().into_owned()),
            is_auto_addable,
        }
    }
}

/// Create a private repo on the authenticated GitHub user's account
/// (auto-init'd so it lands with an initial commit). Used by the
/// wizard's default "we'll make a repo for you" flow.
///
/// Reads the GitHub PAT from the keychain — caller must have run
/// `pat_connect` against `https://api.github.com` first.
#[tauri::command]
pub fn github_create_repo(name: String) -> Result<GhRepo, String> {
    let token = credentials::load(&format!("{HOST_PAT_PREFIX}https://api.github.com"))
        .map_err(|e| e.to_string())?
        .or_else(|| credentials::load(GITHUB_ACCOUNT).ok().flatten())
        .ok_or_else(|| "not authenticated yet — connect with a PAT first".to_string())?;
    github::create_private_repo(&token, &name).map_err(|e| e.to_string())
}

/// Validate a PAT against a host's API and store it in the keychain.
/// On success, returns the user info to display in the wizard.
///
/// `api_base` is the API root, not the web URL — `https://api.github.com`
/// for public GitHub, `https://forgejo.example.org/api/v1` for Forgejo.
#[tauri::command]
pub fn pat_connect(api_base: String, token: String) -> Result<UserInfo, String> {
    let info = auth::validate_pat(&api_base, &token).map_err(|e| e.to_string())?;
    let account = format!("{HOST_PAT_PREFIX}{api_base}");
    credentials::store(&account, &token).map_err(|e| e.to_string())?;
    Ok(info)
}

/// Step 1 of the GitHub OAuth Device Flow. Returns the user code +
/// verification URI for the wizard to display.
#[tauri::command]
pub fn oauth_start(client_id: String) -> Result<DeviceCode, String> {
    let client = AuthClient::github(client_id);
    client
        .start_device_flow(&["repo"])
        .map_err(|e| e.to_string())
}

/// Step 2 of the GitHub OAuth Device Flow. Blocks (in a Tauri-spawned
/// task) until the user finishes the browser handoff, then stores the
/// access token and returns the user info.
#[tauri::command]
pub fn oauth_poll(client_id: String, device: DeviceCode) -> Result<UserInfo, String> {
    let client = AuthClient::github(client_id);
    let token = client.poll_for_token(&device).map_err(|e| e.to_string())?;
    credentials::store(GITHUB_ACCOUNT, &token.access_token).map_err(|e| e.to_string())?;
    let info = auth::validate_pat(auth::GITHUB_API_BASE, &token.access_token)
        .map_err(|e| e.to_string())?;
    Ok(info)
}

#[derive(Debug, Deserialize)]
pub struct InitRepoArgs {
    pub repo_url: String,
    pub host_api_base: String,
    pub machine_name: String,
}

/// Clone the user's data repo to the per-machine repo path and write
/// the initial `LocalConfig`. Idempotent — re-running with an
/// already-configured machine returns the existing config.
#[tauri::command]
pub fn init_repo(args: InitRepoArgs) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;

    if let Some(existing) = LocalConfig::load_from(&config_path).map_err(|e| e.to_string())? {
        return Ok(existing);
    }

    let repo_path = config_path
        .parent()
        .map(|p| p.join("repo"))
        .ok_or_else(|| "invalid config path".to_string())?;
    if let Some(parent) = repo_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let token_account = format!("{HOST_PAT_PREFIX}{}", args.host_api_base);
    let token = credentials::load(&token_account)
        .map_err(|e| e.to_string())?
        .or_else(|| credentials::load(GITHUB_ACCOUNT).ok().flatten());
    let auth = match token {
        Some(t) => GitAuth::HttpsToken { token: t },
        None => GitAuth::None,
    };

    let repo = crate::git::clone(&args.repo_url, &repo_path, &auth).map_err(|e| e.to_string())?;
    crate::git::ensure_main_initialized(
        &repo,
        &GitIdentity::for_machine(&args.machine_name),
    )
    .map_err(|e| e.to_string())?;

    let cfg = LocalConfig::new(args.machine_name, repo_path);
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Return installed Steam games for the wizard's "we found these"
/// step. Empty list if Steam isn't installed (not an error).
#[tauri::command]
pub fn scan_steam() -> Result<Vec<InstalledGameDto>, String> {
    let games = steam::scan_installed_games(TargetOs::current()).map_err(|e| e.to_string())?;
    Ok(games.into_iter().map(InstalledGameDto::from).collect())
}

#[derive(Debug, Deserialize)]
pub struct AddGameArgs {
    pub game_id: String,
    pub save_path: String,
}

/// Register one or more games with the local config. The wizard calls
/// this once per game the user picks.
#[tauri::command]
pub fn add_game(args: AddGameArgs) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized — run init_repo first".to_string())?;
    cfg.upsert_game(args.game_id, PathBuf::from(args.save_path));
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Read the per-machine config. Returns `Ok(None)` when uninitialized
/// (the wizard's "show me on first run" trigger).
#[tauri::command]
pub fn get_local_config() -> Result<Option<LocalConfig>, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    LocalConfig::load_from(&config_path).map_err(|e| e.to_string())
}

/// Open the user's save folder in their native file manager. Used by
/// the per-game detail "Open folder" button.
#[tauri::command]
pub fn open_save_folder(
    app: tauri::AppHandle,
    game_id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    let game = cfg
        .find_game(&game_id)
        .ok_or_else(|| format!("game '{game_id}' is not registered"))?;
    let path = game.save_path.to_string_lossy().into_owned();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| e.to_string())
}

/// Force a sync push from the per-game detail panel. Same as the
/// process-watcher's exit-push, but user-triggered.
#[tauri::command]
pub fn force_push(game_id: String) -> Result<PushOutcome, String> {
    let cfg = require_config()?;
    let repo = git::open(&cfg.repo_path).map_err(|e| e.to_string())?;
    let auth = resolve_auth(&cfg);
    sync::push_game(
        &cfg,
        &repo,
        &game_id,
        &auth,
        &LfsConfig::with_system_binary(),
    )
    .map_err(|e| e.to_string())
}

/// Force a sync pull from the per-game detail panel.
#[tauri::command]
pub fn force_pull(game_id: String) -> Result<PullOutcome, String> {
    let cfg = require_config()?;
    let repo = git::open(&cfg.repo_path).map_err(|e| e.to_string())?;
    let auth = resolve_auth(&cfg);
    sync::pull_game(&cfg, &repo, &game_id, &auth).map_err(|e| e.to_string())
}

/// Toggle a game's paused flag. Persists to LocalConfig immediately so
/// state survives app restart.
#[tauri::command]
pub fn set_game_paused(game_id: String, paused: bool) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    let game = cfg
        .find_game_mut(&game_id)
        .ok_or_else(|| format!("game '{game_id}' is not registered"))?;
    game.paused = paused;
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Set a friendly display name. Doesn't change the underlying repo
/// folder name — purely cosmetic for the UI.
#[derive(Debug, Deserialize)]
pub struct RenameGameArgs {
    pub game_id: String,
    pub display_name: Option<String>,
}

#[tauri::command]
pub fn rename_game(args: RenameGameArgs) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    let game = cfg
        .find_game_mut(&args.game_id)
        .ok_or_else(|| format!("game '{}' is not registered", args.game_id))?;
    game.display_name = args.display_name.filter(|s| !s.trim().is_empty());
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Rename this machine. Cosmetic — affects how commits are labeled
/// going forward (and the backup-branch labels on this machine's side
/// of any future conflicts).
#[tauri::command]
pub fn rename_machine(new_name: String) -> Result<LocalConfig, String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err("machine name can't be empty".to_string());
    }
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    cfg.machine_name = trimmed.to_string();
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

#[derive(Debug, Deserialize)]
pub struct PreferencesArgs {
    pub polling_interval_seconds: u32,
    pub lfs_threshold_mb: u32,
    pub sync_on_startup: bool,
}

/// Replace the entire `preferences` block in one call. The settings UI
/// edits all fields together so we don't need fine-grained setters.
#[tauri::command]
pub fn update_preferences(prefs: PreferencesArgs) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    if prefs.polling_interval_seconds == 0 || prefs.polling_interval_seconds > 60 {
        return Err("polling_interval_seconds must be 1..=60".to_string());
    }
    if prefs.lfs_threshold_mb == 0 || prefs.lfs_threshold_mb > 5000 {
        return Err("lfs_threshold_mb must be 1..=5000".to_string());
    }
    cfg.preferences = crate::local_config::Preferences {
        polling_interval_seconds: prefs.polling_interval_seconds,
        lfs_threshold_mb: prefs.lfs_threshold_mb,
        sync_on_startup: prefs.sync_on_startup,
    };
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Tear down this machine's SaveSync setup. Removes the local config
/// and every credential we stored. Does NOT touch the git repo on
/// disk or the user's save folders — those are still under the user's
/// control. After this, the next launch shows the wizard again.
#[tauri::command]
pub fn disconnect_machine() -> Result<(), String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    // Wipe credentials. Best-effort — keychain delete on a missing
    // entry is already a no-op in our wrapper.
    let _ = credentials::delete(GITHUB_ACCOUNT);
    let _ = credentials::delete(&format!("{HOST_PAT_PREFIX}https://api.github.com"));
    if config_path.exists() {
        std::fs::remove_file(&config_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Remove a game from the local config. Doesn't touch the data in the
/// git repo — the user can re-add the same game later and the repo
/// folder is still there.
#[tauri::command]
pub fn remove_game(game_id: String) -> Result<LocalConfig, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let mut cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized".to_string())?;
    cfg.remove_game(&game_id);
    cfg.save_to(&config_path).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Commit history filtered to a single game's folder. Used by the
/// per-game detail's history panel.
#[tauri::command]
pub fn list_game_commits(game_id: String, limit: usize) -> Result<Vec<CommitInfo>, String> {
    let cfg = require_config()?;
    let repo = git::open(&cfg.repo_path).map_err(|e| e.to_string())?;
    git::list_commits_touching(
        &repo,
        "refs/heads/main",
        std::path::Path::new(&game_id),
        limit,
    )
    .map_err(|e| e.to_string())
}

/// List backup branches preserved for this game by the conflict
/// resolver. The drawer lists them with restore-to-main affordances.
#[tauri::command]
pub fn list_game_backups(game_id: String) -> Result<Vec<String>, String> {
    let cfg = require_config()?;
    let repo = git::open(&cfg.repo_path).map_err(|e| e.to_string())?;
    git::list_branches_with_prefix(&repo, &format!("backup/{game_id}/"))
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct InspectGameArgs {
    pub game_id: String,
    pub save_path: String,
}

/// Inspect a game's existing-save state before committing the add.
/// Returns the four-way classification (Initial / LocalOnly /
/// RemoteOnly / Both) plus per-side stats so the UI can render the
/// 3-way chooser when both sides have data.
#[tauri::command]
pub fn inspect_game(args: InspectGameArgs) -> Result<AddReport, String> {
    let config_path = default_config_path().map_err(|e| e.to_string())?;
    let cfg = LocalConfig::load_from(&config_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not initialized — run init_repo first".to_string())?;
    let repo = git::open(&cfg.repo_path).map_err(|e| e.to_string())?;
    detect_save_state(&repo, &args.game_id, std::path::Path::new(&args.save_path))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_game_dto_round_trips() {
        let g = InstalledGame {
            steam_appid: 413150,
            steam_display_name: "Stardew Valley".into(),
            ludusavi_name: Some("Stardew Valley".into()),
            install_dir: PathBuf::from("/games/Stardew"),
            resolved_save_path: Some(PathBuf::from("/home/u/.config/StardewValley/Saves")),
        };
        let dto: InstalledGameDto = g.into();
        assert_eq!(dto.steam_appid, 413150);
        assert!(dto.is_auto_addable);
        let s = serde_json::to_string(&dto).unwrap();
        assert!(s.contains("Stardew Valley"));
        assert!(s.contains("\"is_auto_addable\":true"));
    }
}
