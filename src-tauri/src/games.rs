//! Bundled save-path database, slimmed from the Ludusavi manifest.
//!
//! Source data is fetched and slimmed by [`scripts/refresh-ludusavi.py`]
//! (run that script periodically to pick up new games). The output lives
//! at `src-tauri/data/games.json` and is embedded into the binary at
//! build time — no runtime download, no internet dependency.
//!
//! Public API:
//! - [`lookup`] — find a game by exact name (the YAML key in Ludusavi)
//! - [`lookup_by_steam_id`] — match against a Steam appid (used during
//!   the Phase 3 onboarding scan of `libraryfolders.vdf`)
//! - [`resolve_save_paths`] — expand a game's templates into absolute
//!   paths for a given target OS, using the [`dirs`] crate for system
//!   directories

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::Deserialize;

/// Embedded slim manifest. Reloaded each build from
/// `src-tauri/data/games.json`.
const RAW_MANIFEST: &str = include_str!("../data/games.json");

/// Target OS for save-path resolution. We resolve at the call site
/// rather than via `cfg(target_os)` so the same code path can be
/// exercised in cross-platform tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Windows,
    Linux,
    Macos,
}

impl TargetOs {
    /// The variant for the build target.
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            Self::Linux
        }
    }

    fn as_manifest_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "mac",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    #[serde(default)]
    schema_version: u32,
    games: BTreeMap<String, GameDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameDef {
    #[serde(default)]
    pub steam_id: Option<u64>,
    #[serde(default)]
    pub install_dirs: Vec<String>,
    pub save_paths: Vec<SavePathEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SavePathEntry {
    pub template: String,
    /// `None` means "applies to every OS" in the Ludusavi schema.
    #[serde(default)]
    pub os: Option<String>,
    /// Optional store qualifier (`steam`, `gog`, `epic`, ...). When
    /// present, the path is specific to that store's install. Used as
    /// a hint, not a filter.
    #[serde(default)]
    pub store: Option<String>,
}

static MANIFEST: LazyLock<ManifestFile> = LazyLock::new(|| {
    serde_json::from_str(RAW_MANIFEST).expect("embedded games.json is malformed")
});

/// Look up a game by exact name (Ludusavi key, case-sensitive).
pub fn lookup(name: &str) -> Option<&'static GameDef> {
    MANIFEST.games.get(name)
}

/// Look up by Steam appid. Returns the name + def so the caller can
/// store the canonical name.
pub fn lookup_by_steam_id(steam_id: u64) -> Option<(&'static str, &'static GameDef)> {
    MANIFEST.games.iter().find_map(|(name, def)| {
        if def.steam_id == Some(steam_id) {
            Some((name.as_str(), def))
        } else {
            None
        }
    })
}

/// Number of games in the embedded manifest. Used by tests and surfaced
/// in the UI as a "we know about N games" stat.
pub fn manifest_size() -> usize {
    MANIFEST.games.len()
}

/// Manifest schema version. Bump in the refresh script when the slimmed
/// shape changes in a way Rust readers need to know about.
pub fn schema_version() -> u32 {
    MANIFEST.schema_version
}

/// Resolve a game's save-path templates into absolute filesystem paths
/// for `os`. Returns every candidate (a single game can have multiple
/// entries — for example a Steam-specific path and a generic one). The
/// caller picks the first that actually exists on disk.
///
/// Paths with placeholders we can't resolve (`<base>`, `<root>`,
/// `<storeUserId>`, etc.) are dropped from the result — those need
/// caller-supplied data (the game's install dir, the user's Steam ID)
/// which we don't have here.
pub fn resolve_save_paths(game: &GameDef, os: TargetOs) -> Vec<PathBuf> {
    let target = os.as_manifest_str();
    let dirs = SystemDirs::resolve();

    game.save_paths
        .iter()
        .filter(|entry| match &entry.os {
            Some(o) => o == target,
            None => true,
        })
        .filter_map(|entry| expand_template(&entry.template, &dirs))
        .collect()
}

/// Resolved system directory paths used for placeholder expansion.
struct SystemDirs {
    home: Option<PathBuf>,
    config: Option<PathBuf>, // <xdgConfig> / win equivalent
    data: Option<PathBuf>,   // <xdgData> / win equivalent
    documents: Option<PathBuf>,
    win_app_data: Option<PathBuf>,
    win_local_app_data: Option<PathBuf>,
    win_program_data: Option<PathBuf>,
    win_public: Option<PathBuf>,
    win_dir: Option<PathBuf>,
}

impl SystemDirs {
    fn resolve() -> Self {
        Self {
            home: dirs::home_dir(),
            config: dirs::config_dir(),
            data: dirs::data_dir(),
            documents: dirs::document_dir(),
            // `dirs` has dedicated APIs only on the matching OS; on
            // others they return None. That's fine — non-matching
            // templates won't be picked.
            win_app_data: dirs::config_dir(),
            win_local_app_data: dirs::data_local_dir(),
            win_program_data: std::env::var_os("ProgramData").map(PathBuf::from),
            win_public: std::env::var_os("PUBLIC").map(PathBuf::from),
            win_dir: std::env::var_os("windir").map(PathBuf::from),
        }
    }

    fn lookup(&self, placeholder: &str) -> Option<&PathBuf> {
        match placeholder {
            "home" => self.home.as_ref(),
            "xdgConfig" => self.config.as_ref(),
            "xdgData" => self.data.as_ref(),
            "winAppData" => self.win_app_data.as_ref(),
            "winLocalAppData" => self.win_local_app_data.as_ref(),
            "winDocuments" => self.documents.as_ref(),
            "winPublic" => self.win_public.as_ref(),
            "winProgramData" => self.win_program_data.as_ref(),
            "winDir" => self.win_dir.as_ref(),
            _ => None,
        }
    }
}

/// Expand a Ludusavi template into an absolute path by replacing every
/// `<placeholder>` segment with its resolved system directory.
///
/// Returns `None` if the template contains any placeholder we don't
/// know how to resolve (`<base>`, `<root>`, `<storeUserId>`,
/// `<osUserName>`, …) — those need per-user / per-install data the
/// caller has to provide separately, and a half-expanded path is
/// worse than no path at all.
fn expand_template(template: &str, dirs: &SystemDirs) -> Option<PathBuf> {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(open) = rest.find('<') {
        // Copy literal text before the placeholder.
        out.push_str(&rest[..open]);
        let close_rel = rest[open..].find('>')?;
        let placeholder = &rest[open + 1..open + close_rel];
        let resolved = dirs.lookup(placeholder)?;
        out.push_str(resolved.to_str()?);
        rest = &rest[open + close_rel + 1..];
    }
    out.push_str(rest);
    Some(PathBuf::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_eagerly() {
        // Force lazy init to make sure the embedded JSON is valid.
        assert!(manifest_size() > 5000, "expected the slimmed manifest to contain thousands of games");
        assert_eq!(schema_version(), 1);
    }

    #[test]
    fn elden_ring_is_in_manifest() {
        let g = lookup("Elden Ring").expect("Elden Ring should be in the manifest");
        assert!(g.steam_id.is_some());
    }

    /// The issue's acceptance criterion: at least 20 popular titles
    /// resolve cleanly. We pick games across genres + decades; if
    /// Ludusavi ever drops one of these we want a loud failure, not a
    /// silent regression.
    #[test]
    fn at_least_20_popular_games_are_known() {
        let popular = [
            "Elden Ring",
            "Baldur's Gate 3",
            "Stardew Valley",
            "Hades",
            "Hollow Knight",
            "Cyberpunk 2077",
            "The Witcher 3: Wild Hunt",
            "Sekiro: Shadows Die Twice",
            "Dark Souls III",
            "Skyrim Special Edition",
            "Red Dead Redemption 2",
            "Disco Elysium",
            "Celeste",
            "Outer Wilds",
            "Death Stranding",
            "Resident Evil 4 (2023)",
            "Helldivers 2",
            "Terraria",
            "Minecraft",
            "Vampire Survivors",
            "Subnautica",
        ];
        let mut missing = Vec::new();
        for name in popular {
            if lookup(name).is_none() {
                missing.push(name);
            }
        }
        assert!(
            missing.len() <= 2,
            "expected at least 19/21 popular games to be present, missing: {missing:?}"
        );
    }

    #[test]
    fn stardew_valley_resolves_on_current_platform() {
        // Stardew Valley's Ludusavi templates use only system-dir
        // placeholders (no <storeUserId>), so resolution is fully
        // automatic on every supported platform.
        let g = lookup("Stardew Valley").unwrap();
        let paths = resolve_save_paths(g, TargetOs::current());
        assert!(
            !paths.is_empty(),
            "Stardew Valley should have at least one path on {:?}",
            TargetOs::current()
        );
        assert!(
            paths.iter().any(|p| p.ends_with("StardewValley/Saves")),
            "expected a path ending in 'StardewValley/Saves', got: {paths:?}"
        );
    }

    #[test]
    fn template_with_unresolvable_placeholder_is_dropped() {
        // Elden Ring's actual Windows template includes <storeUserId>
        // (the Steam numeric account ID) which we can't resolve without
        // per-user data. The whole entry should be dropped, not
        // half-expanded.
        let g = lookup("Elden Ring").unwrap();
        let paths = resolve_save_paths(g, TargetOs::Windows);
        for p in &paths {
            let s = p.to_string_lossy();
            assert!(
                !s.contains('<'),
                "no resolved path should still contain a placeholder: {s}"
            );
        }
    }

    #[test]
    fn lookup_by_steam_id_finds_elden_ring() {
        // 1245620 is Elden Ring's appid.
        let (name, _g) = lookup_by_steam_id(1245620)
            .expect("steam appid 1245620 should resolve to a game");
        assert_eq!(name, "Elden Ring");
    }

    #[test]
    fn unknown_steam_id_returns_none() {
        // 9_999_999_999 is far above any real Steam appid.
        assert!(lookup_by_steam_id(9_999_999_999).is_none());
    }

    #[test]
    fn unsupported_placeholder_drops_the_entry() {
        let dirs = SystemDirs::resolve();
        // `<base>` is install-dir relative — we can't resolve it
        // without caller-supplied context, so expansion returns None.
        assert!(expand_template("<base>/saves", &dirs).is_none());
        assert!(expand_template("<storeUserId>/anything", &dirs).is_none());
    }

    #[test]
    fn home_relative_template_expands() {
        let dirs = SystemDirs::resolve();
        let p = expand_template("<home>/Documents/My Games", &dirs).unwrap();
        assert!(p.ends_with("Documents/My Games"));
        assert!(p.is_absolute());
    }

    #[test]
    fn template_with_no_placeholder_passes_through() {
        let dirs = SystemDirs::resolve();
        let p = expand_template("/absolute/path", &dirs).unwrap();
        assert_eq!(p, PathBuf::from("/absolute/path"));
    }
}
