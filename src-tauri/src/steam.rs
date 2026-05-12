//! Steam library scanner. Used during onboarding to populate the
//! "we found these installed games" list.
//!
//! Reads Steam's `libraryfolders.vdf` (via the `steamlocate` crate),
//! walks every configured library, lists every installed app, and
//! cross-references each app's Steam appid against our bundled
//! Ludusavi manifest. Games with a known save path become candidates;
//! games without are skipped (the user can still add them manually).
//!
//! "Steam not installed" is not an error — the function returns an
//! empty list. Users on cracked-only setups have no Steam.

use std::path::PathBuf;

use crate::games::{self, TargetOs};

#[derive(Debug, thiserror::Error)]
pub enum SteamError {
    /// Wraps steamlocate failures. Most callers should treat this as
    /// "Steam not available" rather than a hard error.
    #[error("steam locate failed: {0}")]
    Locate(String),
}

/// One installed game found in a Steam library, after matching against
/// our save-path database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGame {
    /// Steam appid (e.g. 1245620 for Elden Ring).
    pub steam_appid: u32,
    /// Display name from Steam's appmanifest (sometimes differs from
    /// the Ludusavi-canonical name; we keep both for the UI).
    pub steam_display_name: String,
    /// Name in the Ludusavi manifest if we matched the appid. Used as
    /// the game ID in the SaveSync repo.
    pub ludusavi_name: Option<String>,
    /// Absolute path to the game's install dir.
    pub install_dir: PathBuf,
    /// First resolvable save path for this OS, if our manifest knows
    /// one. None means the user has to point us at the save folder
    /// manually.
    pub resolved_save_path: Option<PathBuf>,
}

impl InstalledGame {
    /// True when we can sync this game without further user input.
    /// Used by the wizard to pre-check the "Add all" suggestion.
    pub fn is_auto_addable(&self) -> bool {
        self.ludusavi_name.is_some() && self.resolved_save_path.is_some()
    }
}

/// Scan every configured Steam library and return the installed games.
/// Returns an empty Vec when Steam isn't installed (the common case
/// for cracked-only users).
pub fn scan_installed_games(target: TargetOs) -> Result<Vec<InstalledGame>, SteamError> {
    let steam = match steamlocate::SteamDir::locate() {
        Ok(s) => s,
        // Steam isn't installed → empty list, not an error.
        Err(_) => return Ok(Vec::new()),
    };

    let libraries = match steam.libraries() {
        Ok(libs) => libs,
        Err(e) => return Err(SteamError::Locate(e.to_string())),
    };

    let mut out = Vec::new();

    for library in libraries.flatten() {
        let library_path = library.path().to_path_buf();
        for app in library.apps().flatten() {
            // Some entries are placeholder workshop/depot rows — skip
            // anything that isn't a real installed app with a name.
            if app.name.as_deref().unwrap_or("").is_empty() {
                continue;
            }
            let install_dir = library_path.join("steamapps/common").join(&app.install_dir);
            out.push(match_against_manifest(&app, install_dir, target));
        }
    }

    Ok(out)
}

fn match_against_manifest(
    app: &steamlocate::App,
    install_dir: PathBuf,
    target: TargetOs,
) -> InstalledGame {
    let display = app.name.clone().unwrap_or_else(|| app.install_dir.clone());
    if let Some((canonical, def)) = games::lookup_by_steam_id(app.app_id as u64) {
        let resolved = games::resolve_save_paths(def, target)
            .into_iter()
            .find(|p| p.is_absolute());
        InstalledGame {
            steam_appid: app.app_id,
            steam_display_name: display,
            ludusavi_name: Some(canonical.to_string()),
            install_dir,
            resolved_save_path: resolved,
        }
    } else {
        InstalledGame {
            steam_appid: app.app_id,
            steam_display_name: display,
            ludusavi_name: None,
            install_dir,
            resolved_save_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_does_not_panic_when_steam_might_or_might_not_be_installed() {
        // We don't assert presence of games — CI runners don't have
        // Steam, dev machines might. Just confirm the call returns
        // without panicking regardless.
        let result = scan_installed_games(TargetOs::current());
        // Either a clean list (with or without games) or a typed error.
        // We don't accept panics.
        let _ = result;
    }

    #[test]
    fn is_auto_addable_requires_both_match_and_resolved_path() {
        let with_match_and_path = InstalledGame {
            steam_appid: 413150,
            steam_display_name: "Stardew Valley".into(),
            ludusavi_name: Some("Stardew Valley".into()),
            install_dir: PathBuf::from("/games/Stardew"),
            resolved_save_path: Some(PathBuf::from("/home/u/.config/StardewValley/Saves")),
        };
        assert!(with_match_and_path.is_auto_addable());

        let no_manifest_match = InstalledGame {
            ludusavi_name: None,
            ..with_match_and_path.clone()
        };
        assert!(!no_manifest_match.is_auto_addable());

        let no_resolved_path = InstalledGame {
            resolved_save_path: None,
            ..with_match_and_path.clone()
        };
        assert!(!no_resolved_path.is_auto_addable());
    }
}
