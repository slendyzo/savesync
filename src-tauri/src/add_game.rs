//! Existing-save detection for the "Add Game" flow.
//!
//! When the user adds a game on a new machine, we never want to silently
//! overwrite one side's save with the other. There are four cases:
//!
//! - **Neither side has data** — initial state, safe to register
//! - **Local only** — user's save dir has files but the repo doesn't.
//!   Safe to push.
//! - **Remote only** — repo has saves but local save dir is empty
//!   (typical "I just installed on machine 2"). Safe to pull.
//! - **Both have data** — actual decision required. UI shows a chooser.
//!
//! This module classifies the situation and reports metadata (mtime,
//! size, machine hint) so the UI can render a side-by-side card view
//! for the user to pick.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use git2::Repository;
use serde::Serialize;

use crate::git::{self, GitError};
use crate::manifest::GameMeta;

#[derive(Debug, thiserror::Error)]
pub enum AddGameError {
    #[error("git: {0}")]
    Git(#[from] GitError),
    #[error("libgit2: {0}")]
    LibGit2(#[from] git2::Error),
    #[error("manifest: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("invalid utf-8 in meta: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Per-side summary the UI surfaces in the 3-way chooser.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SideSummary {
    /// Most-recent file mtime across the side's save folder, used as
    /// the "when was this last touched" hint. None when the side is
    /// empty.
    pub latest_mtime: Option<DateTime<Utc>>,
    /// Total size across all files in bytes.
    pub total_bytes: u64,
    /// File count.
    pub file_count: usize,
    /// Hint about which machine last wrote this side (only available
    /// for the remote side, from .savesync-meta.json).
    pub last_machine: Option<String>,
}

impl SideSummary {
    pub fn is_empty(&self) -> bool {
        self.file_count == 0
    }
}

/// Classification of the add-game situation.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AddSituation {
    /// Neither side has saved data. Push an empty seed.
    Initial,
    /// Local save exists, remote doesn't. Safe to push.
    LocalOnly,
    /// Remote save exists, local doesn't. Safe to pull.
    RemoteOnly,
    /// Both have data. The UI shows the 3-way chooser; the caller
    /// must pass a resolution before committing the add.
    Both,
}

/// Complete report the UI needs to make a decision.
#[derive(Debug, Clone, Serialize)]
pub struct AddReport {
    pub situation: AddSituation,
    pub local: SideSummary,
    pub remote: SideSummary,
}

/// Inspect both sides and classify the situation. Doesn't mutate
/// anything — purely informational.
pub fn detect_save_state(
    repo: &Repository,
    game_id: &str,
    local_save_path: &Path,
) -> Result<AddReport, AddGameError> {
    let local = summarize_local(local_save_path)?;
    let remote = summarize_remote(repo, game_id)?;
    let situation = match (local.is_empty(), remote.is_empty()) {
        (true, true) => AddSituation::Initial,
        (false, true) => AddSituation::LocalOnly,
        (true, false) => AddSituation::RemoteOnly,
        (false, false) => AddSituation::Both,
    };
    Ok(AddReport {
        situation,
        local,
        remote,
    })
}

fn summarize_local(save_path: &Path) -> Result<SideSummary, AddGameError> {
    if !save_path.exists() {
        return Ok(empty_summary());
    }
    let mut latest = None;
    let mut total_bytes = 0u64;
    let mut count = 0usize;
    for entry in walkdir::WalkDir::new(save_path).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        total_bytes += meta.len();
        count += 1;
        if let Ok(modified) = meta.modified() {
            let m: DateTime<Utc> = modified.into();
            if latest.map(|x| m > x).unwrap_or(true) {
                latest = Some(m);
            }
        }
    }
    Ok(SideSummary {
        latest_mtime: latest,
        total_bytes,
        file_count: count,
        last_machine: None,
    })
}

fn summarize_remote(repo: &Repository, game_id: &str) -> Result<SideSummary, AddGameError> {
    let head_oid = match git::rev_parse(repo, "refs/heads/main") {
        Some(o) => o,
        None => return Ok(empty_summary()),
    };
    let commit = repo.find_commit(head_oid)?;
    let tree = commit.tree()?;
    let game_entry = match tree.get_path(Path::new(game_id)) {
        Ok(e) => e,
        Err(_) => return Ok(empty_summary()),
    };
    let obj = game_entry.to_object(repo)?;
    let game_tree = match obj.into_tree() {
        Ok(t) => t,
        Err(_) => return Ok(empty_summary()),
    };

    // Total size + file count by walking the game's tree.
    let (total_bytes, file_count) = walk_tree(repo, &game_tree, 0, 0)?;

    // Read the game's .savesync-meta.json if it exists for last_machine
    // + latest_mtime hint.
    let meta = read_game_meta(repo, head_oid, game_id)?;
    let (latest_mtime, last_machine) = match meta {
        Some(m) => {
            let latest = m.files.values().map(|f| f.mtime).max();
            (latest, Some(m.last_machine.to_string()))
        }
        None => (None, None),
    };

    Ok(SideSummary {
        latest_mtime,
        total_bytes,
        file_count,
        last_machine,
    })
}

fn walk_tree(
    repo: &Repository,
    tree: &git2::Tree,
    mut bytes: u64,
    mut count: usize,
) -> Result<(u64, usize), AddGameError> {
    for entry in tree.iter() {
        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                let obj = entry.to_object(repo)?;
                if let Some(blob) = obj.as_blob() {
                    bytes += blob.size() as u64;
                    count += 1;
                }
            }
            Some(git2::ObjectType::Tree) => {
                let obj = entry.to_object(repo)?;
                if let Some(subtree) = obj.as_tree() {
                    let (b, c) = walk_tree(repo, subtree, bytes, count)?;
                    bytes = b;
                    count = c;
                }
            }
            _ => {}
        }
    }
    Ok((bytes, count))
}

fn read_game_meta(
    repo: &Repository,
    commit_oid: git2::Oid,
    game_id: &str,
) -> Result<Option<GameMeta>, AddGameError> {
    let path = PathBuf::from(format!("{game_id}/.savesync-meta.json"));
    let bytes = match git::read_blob_at(repo, commit_oid, &path)? {
        Some(b) => b,
        None => return Ok(None),
    };
    let text = std::str::from_utf8(&bytes)?;
    Ok(Some(GameMeta::from_json(text)?))
}

fn empty_summary() -> SideSummary {
    SideSummary {
        latest_mtime: None,
        total_bytes: 0,
        file_count: 0,
        last_machine: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitIdentity;
    use std::fs;

    fn identity() -> GitIdentity {
        GitIdentity::for_machine("test-machine")
    }

    fn fresh_repo() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git::init_with_initial_commit(tmp.path(), &identity()).unwrap();
        (tmp, repo)
    }

    fn fresh_save() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn initial_when_neither_side_has_data() {
        let (_repo_dir, repo) = fresh_repo();
        let save = fresh_save();
        let report = detect_save_state(&repo, "elden-ring", save.path()).unwrap();
        assert_eq!(report.situation, AddSituation::Initial);
        assert!(report.local.is_empty());
        assert!(report.remote.is_empty());
    }

    #[test]
    fn local_only_when_repo_has_no_game_folder() {
        let (_repo_dir, repo) = fresh_repo();
        let save = fresh_save();
        fs::write(save.path().join("ER0000.sl2"), b"data").unwrap();

        let report = detect_save_state(&repo, "elden-ring", save.path()).unwrap();
        assert_eq!(report.situation, AddSituation::LocalOnly);
        assert_eq!(report.local.file_count, 1);
        assert_eq!(report.local.total_bytes, 4);
        assert!(report.remote.is_empty());
    }

    #[test]
    fn remote_only_when_local_save_dir_missing() {
        let (repo_dir, repo) = fresh_repo();
        let save = fresh_save();

        // Seed the repo with a fake game folder.
        let game_dir = repo_dir.path().join("elden-ring");
        fs::create_dir(&game_dir).unwrap();
        fs::write(game_dir.join("ER0000.sl2"), b"remote-save").unwrap();
        let meta = GameMeta::new("elden-ring", uuid::Uuid::new_v4());
        fs::write(
            game_dir.join(".savesync-meta.json"),
            meta.to_json_pretty().unwrap(),
        )
        .unwrap();
        git::stage_all(&repo).unwrap();
        git::commit(&repo, "feat: seed", &identity()).unwrap();

        let report = detect_save_state(&repo, "elden-ring", save.path()).unwrap();
        assert_eq!(report.situation, AddSituation::RemoteOnly);
        assert!(report.local.is_empty());
        assert!(report.remote.file_count >= 1);
    }

    #[test]
    fn both_when_each_side_has_data() {
        let (repo_dir, repo) = fresh_repo();
        let save = fresh_save();
        // Local has a save.
        fs::write(save.path().join("ER0000.sl2"), b"local").unwrap();

        // Remote has a different save.
        let game_dir = repo_dir.path().join("elden-ring");
        fs::create_dir(&game_dir).unwrap();
        fs::write(game_dir.join("ER0000.sl2"), b"remote").unwrap();
        let meta_machine = uuid::Uuid::new_v4();
        let meta = GameMeta::new("elden-ring", meta_machine);
        fs::write(
            game_dir.join(".savesync-meta.json"),
            meta.to_json_pretty().unwrap(),
        )
        .unwrap();
        git::stage_all(&repo).unwrap();
        git::commit(&repo, "feat: seed", &identity()).unwrap();

        let report = detect_save_state(&repo, "elden-ring", save.path()).unwrap();
        assert_eq!(report.situation, AddSituation::Both);
        assert_eq!(report.local.file_count, 1);
        assert!(report.remote.file_count >= 1);
        // The last_machine hint comes from the meta on the remote side.
        assert_eq!(
            report.remote.last_machine.as_deref(),
            Some(meta_machine.to_string().as_str()),
        );
    }

    #[test]
    fn nonexistent_local_path_is_treated_as_empty() {
        let (_repo_dir, repo) = fresh_repo();
        let bogus = PathBuf::from("/does/not/exist/savesync-test");
        let report = detect_save_state(&repo, "any-game", &bogus).unwrap();
        assert!(report.local.is_empty());
        assert_eq!(report.situation, AddSituation::Initial);
    }
}
