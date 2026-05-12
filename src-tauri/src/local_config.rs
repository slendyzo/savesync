//! Per-machine local configuration.
//!
//! `savesync.json` (in the user's git repo) is shared across machines.
//! This file is the **per-machine** complement: where the user keeps the
//! local clone of the data repo, what their machine is called, and where
//! each registered game's save folder actually lives on this disk.
//!
//! Layout:
//! - Linux:   `$XDG_CONFIG_HOME/savesync/config.json` (typically `~/.config/savesync/`)
//! - macOS:   `~/Library/Application Support/savesync/config.json`
//! - Windows: `%APPDATA%\savesync\config.json`
//!
//! All resolved via the [`dirs`] crate.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::manifest::Platform;

#[derive(Debug, thiserror::Error)]
pub enum LocalConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no config directory available on this OS")]
    NoConfigDir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    pub schema_version: u32,
    pub machine_id: Uuid,
    pub machine_name: String,
    pub hostname: String,
    pub platform: Platform,
    /// Where the user's data repo lives on this machine.
    pub repo_path: PathBuf,
    /// Per-game save-folder mapping. Lives here (not in the repo
    /// manifest) because the same game can sit at different paths on
    /// different machines.
    #[serde(default)]
    pub games: Vec<LocalGame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalGame {
    pub id: String,
    pub save_path: PathBuf,
}

impl LocalConfig {
    pub fn new(machine_name: String, repo_path: PathBuf) -> Self {
        Self {
            schema_version: 1,
            machine_id: Uuid::new_v4(),
            machine_name,
            hostname: detect_hostname(),
            platform: current_platform(),
            repo_path,
            games: Vec::new(),
        }
    }

    pub fn upsert_game(&mut self, id: String, save_path: PathBuf) {
        if let Some(g) = self.games.iter_mut().find(|g| g.id == id) {
            g.save_path = save_path;
        } else {
            self.games.push(LocalGame { id, save_path });
        }
    }

    pub fn find_game(&self, id: &str) -> Option<&LocalGame> {
        self.games.iter().find(|g| g.id == id)
    }

    pub fn from_json(raw: &str) -> Result<Self, LocalConfigError> {
        Ok(serde_json::from_str(raw)?)
    }

    pub fn to_json_pretty(&self) -> Result<String, LocalConfigError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Save to `path`, creating the parent directory if needed.
    pub fn save_to(&self, path: &Path) -> Result<(), LocalConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json_pretty()?)?;
        Ok(())
    }

    /// Load from `path`. Returns `Ok(None)` if the file doesn't exist
    /// (callers usually treat that as "not yet initialized").
    pub fn load_from(path: &Path) -> Result<Option<Self>, LocalConfigError> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(Some(Self::from_json(&raw)?))
    }
}

/// OS-appropriate default config file path.
pub fn default_config_path() -> Result<PathBuf, LocalConfigError> {
    let base = dirs::config_dir().ok_or(LocalConfigError::NoConfigDir)?;
    Ok(base.join("savesync").join("config.json"))
}

fn detect_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "unknown-host".to_string())
}

fn current_platform() -> Platform {
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(target_os = "linux")]
    {
        Platform::Linux
    }
    #[cfg(target_os = "macos")]
    {
        Platform::Macos
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Platform::Linux
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut cfg = LocalConfig::new("Desktop-PC".into(), PathBuf::from("/tmp/repo"));
        cfg.upsert_game("elden-ring".into(), PathBuf::from("/home/u/Saves/Elden Ring"));
        cfg.upsert_game("bg3".into(), PathBuf::from("/home/u/Saves/BG3"));

        let json = cfg.to_json_pretty().unwrap();
        let parsed = LocalConfig::from_json(&json).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn upsert_game_replaces_existing_path() {
        let mut cfg = LocalConfig::new("M".into(), PathBuf::from("/tmp/r"));
        cfg.upsert_game("g".into(), PathBuf::from("/old"));
        cfg.upsert_game("g".into(), PathBuf::from("/new"));
        assert_eq!(cfg.games.len(), 1);
        assert_eq!(cfg.find_game("g").unwrap().save_path, PathBuf::from("/new"));
    }

    #[test]
    fn save_and_load_round_trip_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/config.json");
        let cfg = LocalConfig::new("M".into(), PathBuf::from("/r"));
        cfg.save_to(&path).unwrap();

        let loaded = LocalConfig::load_from(&path).unwrap().unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let result = LocalConfig::load_from(&tmp.path().join("absent.json")).unwrap();
        assert!(result.is_none());
    }
}
