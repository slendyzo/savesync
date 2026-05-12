//! Persisted data contracts for SaveSync.
//!
//! Two artifacts live in the user's private git repo:
//!
//! - `savesync.json` at the repo root — the [`SaveSyncManifest`]: registered
//!   games, every machine that has ever connected, schema version.
//! - `.savesync-meta.json` inside each game's folder — the [`GameMeta`]:
//!   per-file snapshot (hash + size + mtime), last-sync timestamp, last
//!   machine that pushed.
//!
//! Schema evolution: every struct carries a `schema_version`. Reads go through
//! [`SaveSyncManifest::from_json`] / [`GameMeta::from_json`], which dispatch
//! through a migration ladder and bump older payloads up to [`SCHEMA_VERSION`].

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current on-disk schema version. Bump when fields change in a
/// non-backwards-compatible way.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema version {found} is newer than this build supports (max {supported})")]
    UnsupportedNewerSchema { found: u32, supported: u32 },
    #[error("schema version {0} is too old and has no migration path")]
    UnsupportedOldSchema(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

/// Root manifest at `savesync.json`. Tracks every game and every machine that
/// has connected to this repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveSyncManifest {
    pub schema_version: u32,
    #[serde(default)]
    pub machines: Vec<Machine>,
    #[serde(default)]
    pub games: Vec<TrackedGame>,
}

impl SaveSyncManifest {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            machines: Vec::new(),
            games: Vec::new(),
        }
    }

    pub fn from_json(raw: &str) -> Result<Self, ManifestError> {
        let value: serde_json::Value = serde_json::from_str(raw)?;
        let version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        migrate_repo_manifest(value, version)
    }

    pub fn to_json_pretty(&self) -> Result<String, ManifestError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn find_machine(&self, id: Uuid) -> Option<&Machine> {
        self.machines.iter().find(|m| m.id == id)
    }

    pub fn find_game(&self, id: &str) -> Option<&TrackedGame> {
        self.games.iter().find(|g| g.id == id)
    }
}

impl Default for SaveSyncManifest {
    fn default() -> Self {
        Self::new()
    }
}

/// A machine that has connected to the SaveSync repo. Used to tag commits and
/// label the loser branch on conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: Uuid,
    /// User-facing label, renameable in Settings. Defaults to `hostname`.
    pub name: String,
    /// The OS-reported hostname at add time. Never changes.
    pub hostname: String,
    pub platform: Platform,
    pub added_at: DateTime<Utc>,
}

/// A game registered for sync. The `id` is the path-safe slug used as the
/// folder name inside the repo (e.g. `"elden-ring"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedGame {
    /// Path-safe slug. Doubles as the repo folder name.
    pub id: String,
    pub display_name: String,
    /// Reference into the bundled Ludusavi manifest. None for manually-added
    /// games that have no upstream entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ludusavi_id: Option<String>,
    pub added_at: DateTime<Utc>,
    /// Machine that first registered this game in the repo.
    pub added_by: Uuid,
    /// Process names to watch for, per platform. Lowercase.
    #[serde(default)]
    pub process_names: Vec<String>,
}

/// Per-game metadata sitting at `<game-folder>/.savesync-meta.json` in the
/// repo. Records what the save folder looked like at the last successful sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMeta {
    pub schema_version: u32,
    pub game_id: String,
    pub last_sync_at: DateTime<Utc>,
    pub last_machine: Uuid,
    /// File path (relative to the game folder) → snapshot. BTreeMap so JSON
    /// output is deterministic across machines.
    #[serde(default)]
    pub files: BTreeMap<String, FileSnapshot>,
}

impl GameMeta {
    pub fn new(game_id: impl Into<String>, machine: Uuid) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            game_id: game_id.into(),
            last_sync_at: Utc::now(),
            last_machine: machine,
            files: BTreeMap::new(),
        }
    }

    pub fn from_json(raw: &str) -> Result<Self, ManifestError> {
        let value: serde_json::Value = serde_json::from_str(raw)?;
        let version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        migrate_game_meta(value, version)
    }

    pub fn to_json_pretty(&self) -> Result<String, ManifestError> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    /// Blake3 hex hash of file contents.
    pub hash: String,
    pub size: u64,
    pub mtime: DateTime<Utc>,
}

/// Resolved save path for a game on this machine. Lives in machine-local
/// config, never persisted to the git repo (paths differ per machine).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSavePath {
    pub game_id: String,
    pub absolute_path: String,
    pub platform: Platform,
}

// ---------- migrations ----------

fn migrate_repo_manifest(
    value: serde_json::Value,
    version: u32,
) -> Result<SaveSyncManifest, ManifestError> {
    match version {
        0 => Err(ManifestError::UnsupportedOldSchema(0)),
        1 => Ok(serde_json::from_value(value)?),
        n if n > SCHEMA_VERSION => Err(ManifestError::UnsupportedNewerSchema {
            found: n,
            supported: SCHEMA_VERSION,
        }),
        // future: add ladder steps here, e.g. `2 => { ...transform...; Ok(...) }`
        n => Err(ManifestError::UnsupportedOldSchema(n)),
    }
}

fn migrate_game_meta(value: serde_json::Value, version: u32) -> Result<GameMeta, ManifestError> {
    match version {
        0 => Err(ManifestError::UnsupportedOldSchema(0)),
        1 => Ok(serde_json::from_value(value)?),
        n if n > SCHEMA_VERSION => Err(ManifestError::UnsupportedNewerSchema {
            found: n,
            supported: SCHEMA_VERSION,
        }),
        n => Err(ManifestError::UnsupportedOldSchema(n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    fn fixed_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 12, 14, 32, 0).unwrap()
    }

    fn sample_machine() -> Machine {
        Machine {
            id: Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
            name: "Desktop-PC".into(),
            hostname: "test-host".into(),
            platform: Platform::Linux,
            added_at: fixed_ts(),
        }
    }

    fn sample_game(machine: Uuid) -> TrackedGame {
        TrackedGame {
            id: "elden-ring".into(),
            display_name: "Elden Ring".into(),
            ludusavi_id: Some("Elden Ring".into()),
            added_at: fixed_ts(),
            added_by: machine,
            process_names: vec!["eldenring.exe".into(), "start_protected_game.exe".into()],
        }
    }

    #[test]
    fn repo_manifest_round_trips() {
        let machine = sample_machine();
        let game = sample_game(machine.id);
        let manifest = SaveSyncManifest {
            schema_version: SCHEMA_VERSION,
            machines: vec![machine.clone()],
            games: vec![game.clone()],
        };

        let json = manifest.to_json_pretty().unwrap();
        let parsed = SaveSyncManifest::from_json(&json).unwrap();

        assert_eq!(parsed, manifest);
    }

    #[test]
    fn empty_manifest_serializes_with_arrays() {
        let manifest = SaveSyncManifest::new();
        let json = manifest.to_json_pretty().unwrap();
        // sanity: empty arrays still render, not omitted
        assert!(json.contains("\"machines\""));
        assert!(json.contains("\"games\""));
        assert!(json.contains("\"schema_version\": 1"));
    }

    #[test]
    fn missing_optional_fields_default_safely() {
        let raw = r#"{ "schema_version": 1 }"#;
        let parsed = SaveSyncManifest::from_json(raw).unwrap();
        assert!(parsed.machines.is_empty());
        assert!(parsed.games.is_empty());
    }

    #[test]
    fn unknown_schema_zero_is_rejected() {
        let raw = r#"{ "schema_version": 0 }"#;
        let err = SaveSyncManifest::from_json(raw).unwrap_err();
        matches!(err, ManifestError::UnsupportedOldSchema(0));
    }

    #[test]
    fn newer_schema_is_rejected_with_named_error() {
        let raw = r#"{ "schema_version": 99 }"#;
        let err = SaveSyncManifest::from_json(raw).unwrap_err();
        match err {
            ManifestError::UnsupportedNewerSchema { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedNewerSchema, got {other:?}"),
        }
    }

    #[test]
    fn game_meta_round_trips() {
        let machine = Uuid::new_v4();
        let mut meta = GameMeta::new("elden-ring", machine);
        meta.last_sync_at = fixed_ts();
        meta.files.insert(
            "ER0000.sl2".into(),
            FileSnapshot {
                hash: "abc123def456".into(),
                size: 51200,
                mtime: fixed_ts(),
            },
        );
        meta.files.insert(
            "ER0000.sl2.bak".into(),
            FileSnapshot {
                hash: "999888777666".into(),
                size: 51100,
                mtime: fixed_ts(),
            },
        );

        let json = meta.to_json_pretty().unwrap();
        let parsed = GameMeta::from_json(&json).unwrap();

        assert_eq!(parsed, meta);
    }

    #[test]
    fn game_meta_file_order_is_deterministic() {
        // BTreeMap means alphabetical key order regardless of insertion order
        let machine = Uuid::new_v4();
        let mut a = GameMeta::new("g", machine);
        a.files.insert("zebra".into(), FileSnapshot {
            hash: "h".into(), size: 1, mtime: fixed_ts(),
        });
        a.files.insert("alpha".into(), FileSnapshot {
            hash: "h".into(), size: 1, mtime: fixed_ts(),
        });

        let json = a.to_json_pretty().unwrap();
        let alpha_pos = json.find("alpha").unwrap();
        let zebra_pos = json.find("zebra").unwrap();
        assert!(alpha_pos < zebra_pos, "expected alphabetical key order");
    }

    #[test]
    fn tracked_game_omits_none_ludusavi_id() {
        let game = TrackedGame {
            id: "obscure-game".into(),
            display_name: "Obscure Game".into(),
            ludusavi_id: None,
            added_at: fixed_ts(),
            added_by: Uuid::new_v4(),
            process_names: vec![],
        };
        let json = serde_json::to_string(&game).unwrap();
        assert!(!json.contains("ludusavi_id"), "None should be skipped");
    }
}
