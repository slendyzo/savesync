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
    git::{self, GitAuth, GitIdentity},
    github::{self, Repo as GhRepo},
    local_config::{default_config_path, LocalConfig},
    steam::{self, InstalledGame},
};

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

/// Paste your registered OAuth App's client_id here, e.g.:
///   Some("Ov23liABCDEF1234567")
/// See OAUTH_CLIENT_ID above for the registration walkthrough.
const OAUTH_CLIENT_ID_LITERAL: Option<&str> = None;

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
