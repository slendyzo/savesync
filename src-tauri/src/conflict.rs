//! Newest-wins conflict resolution with a loser-on-backup-branch policy.
//!
//! When the local `main` and `origin/main` have diverged (both have
//! commits the other doesn't), we don't merge. Game saves are opaque
//! binaries — there's no useful 3-way merge to do. Instead:
//!
//! 1. Read the game's `.savesync-meta.json` from each side's HEAD.
//! 2. Compare `last_sync_at` — whoever pushed last wins.
//! 3. Snapshot the loser's HEAD to a `backup/<game>/<machine>-<ts>`
//!    branch so nothing is ever silently dropped.
//! 4. Force-reset local `main` to the winner's commit. The caller is
//!    responsible for pushing main (force) + the backup branch.
//!
//! Edge cases:
//! - Both sides have the meta → standard newest-wins.
//! - Only one side has the meta → that side wins, still backs up the
//!   loser's commit so the user can recover any non-game state on the
//!   loser branch (e.g. other games' updates from that side).
//! - Neither side has the meta → no decision to make; returns `None`.
//! - Already up-to-date or fast-forwardable → also returns `None`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use git2::{Oid, Repository};

use crate::git::{self, GitError};
use crate::manifest::{GameMeta, ManifestError};

#[derive(Debug, thiserror::Error)]
pub enum ConflictError {
    #[error("git: {0}")]
    Git(#[from] GitError),
    #[error("libgit2: {0}")]
    LibGit2(#[from] git2::Error),
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    #[error("invalid UTF-8 in .savesync-meta.json: {0}")]
    InvalidMetaUtf8(#[from] std::str::Utf8Error),
    #[error("could not resolve {0}")]
    UnknownRef(String),
}

/// Which side won the conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct ConflictOutcome {
    pub winner: Side,
    /// Name of the newly-created branch pointing at the loser's commit.
    /// Caller must push this alongside `main` (force) to publish.
    pub backup_branch: String,
    /// OID of the loser's HEAD commit (= tip of `backup_branch`).
    pub loser_oid: Oid,
    /// OID local `main` was force-reset to.
    pub winner_oid: Oid,
    /// Whoever was named as `last_machine` in the loser's meta, if any.
    /// Used in the toast/activity log so the user can tell which
    /// machine's state went to the backup branch.
    pub loser_machine_hint: Option<String>,
    pub winner_meta: Option<GameMeta>,
    pub loser_meta: Option<GameMeta>,
}

/// Resolve a conflict on `main` using `game_id`'s meta file as the
/// deciding criterion.
///
/// Preconditions:
/// - `git::fetch_origin` has already run, so `refs/remotes/origin/main`
///   reflects the remote tip.
/// - Local `main` exists.
///
/// Returns `Ok(None)` when there's nothing to resolve (already
/// up-to-date or fast-forwardable). Otherwise returns the outcome with
/// the new backup branch name.
pub fn resolve_for_game(
    repo: &Repository,
    game_id: &str,
    local_machine_name: &str,
) -> Result<Option<ConflictOutcome>, ConflictError> {
    let local_oid = git::rev_parse(repo, "refs/heads/main")
        .ok_or_else(|| ConflictError::UnknownRef("refs/heads/main".into()))?;
    let remote_oid = git::rev_parse(repo, "refs/remotes/origin/main")
        .ok_or_else(|| ConflictError::UnknownRef("refs/remotes/origin/main".into()))?;

    if local_oid == remote_oid {
        return Ok(None);
    }
    if !git::has_diverged(repo, "main")? {
        // Fast-forward case — pull_ff_only handles this. Not our job.
        return Ok(None);
    }

    let local_meta = read_game_meta(repo, local_oid, game_id)?;
    let remote_meta = read_game_meta(repo, remote_oid, game_id)?;

    let local_ts = local_meta.as_ref().map(|m| m.last_sync_at);
    let remote_ts = remote_meta.as_ref().map(|m| m.last_sync_at);

    let winner = decide_winner(local_ts, remote_ts);
    let winner = match winner {
        Some(s) => s,
        None => return Ok(None),
    };

    let (winner_oid, loser_oid, loser_meta_for_hint) = match winner {
        Side::Local => (local_oid, remote_oid, &remote_meta),
        Side::Remote => (remote_oid, local_oid, &local_meta),
    };

    let loser_machine_hint = loser_meta_for_hint.as_ref().map(loser_machine_label);

    let backup_label = loser_machine_hint
        .clone()
        .unwrap_or_else(|| match winner {
            Side::Local => "remote".to_string(),
            Side::Remote => local_machine_name.to_string(),
        });

    let now = Utc::now();
    let backup_branch = git::backup_branch_name(game_id, &backup_label, now);

    // Save the loser to a backup branch (force = overwrite if a previous
    // backup with the same name somehow already exists).
    let loser_commit = repo.find_commit(loser_oid)?;
    repo.branch(&backup_branch, &loser_commit, true)?;

    // Force-reset local main to the winner. Don't touch the workdir
    // yet — the caller decides whether to checkout (e.g. if the game is
    // still running, you might want to wait).
    git::force_set_branch(
        repo,
        "main",
        winner_oid,
        &format!("savesync: conflict winner={winner:?} for {game_id}"),
    )?;

    let (winner_meta, loser_meta) = match winner {
        Side::Local => (local_meta, remote_meta),
        Side::Remote => (remote_meta, local_meta),
    };

    Ok(Some(ConflictOutcome {
        winner,
        backup_branch,
        loser_oid,
        winner_oid,
        loser_machine_hint,
        winner_meta,
        loser_meta,
    }))
}

/// Check out `main` in the workdir. Use after `resolve_for_game` to make
/// the filesystem match the new main.
pub fn checkout_main_after_resolve(repo: &Repository) -> Result<(), ConflictError> {
    git::checkout_branch(repo, "main")?;
    Ok(())
}

fn decide_winner(local: Option<DateTime<Utc>>, remote: Option<DateTime<Utc>>) -> Option<Side> {
    match (local, remote) {
        (Some(l), Some(r)) => {
            if l == r {
                // Identical timestamps — tie broken by Local for
                // determinism. In practice this would require both
                // machines to commit at exactly the same Utc second.
                Some(Side::Local)
            } else if l > r {
                Some(Side::Local)
            } else {
                Some(Side::Remote)
            }
        }
        (Some(_), None) => Some(Side::Local),
        (None, Some(_)) => Some(Side::Remote),
        (None, None) => None,
    }
}

fn loser_machine_label(meta: &GameMeta) -> String {
    meta.last_machine.to_string()
}

fn read_game_meta(
    repo: &Repository,
    commit_oid: Oid,
    game_id: &str,
) -> Result<Option<GameMeta>, ConflictError> {
    let path = PathBuf::from(format!("{game_id}/.savesync-meta.json"));
    let blob = match git::read_blob_at(repo, commit_oid, &path)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let text = std::str::from_utf8(&blob)?;
    Ok(Some(GameMeta::from_json(text)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{GitAuth, GitIdentity};
    use crate::manifest::{FileSnapshot, GameMeta};
    use chrono::TimeZone;
    use std::collections::BTreeMap;
    use std::fs;
    use uuid::Uuid;

    fn ts(min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 12, 12, min, 0).unwrap()
    }

    fn make_meta(machine: Uuid, last_sync_at: DateTime<Utc>) -> GameMeta {
        let mut m = GameMeta::new("elden-ring", machine);
        m.last_sync_at = last_sync_at;
        m.files.insert(
            "ER0000.sl2".into(),
            FileSnapshot {
                hash: "h".into(),
                size: 1,
                mtime: last_sync_at,
            },
        );
        m
    }

    fn identity() -> GitIdentity {
        GitIdentity::for_machine("test-machine")
    }

    fn commit_meta(
        repo: &Repository,
        workdir: &std::path::Path,
        meta: &GameMeta,
        msg: &str,
    ) -> Oid {
        let game_dir = workdir.join(&meta.game_id);
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            game_dir.join(".savesync-meta.json"),
            meta.to_json_pretty().unwrap(),
        )
        .unwrap();
        // Drop a dummy save alongside the meta so the commit is realistic.
        fs::write(game_dir.join("save.dat"), msg).unwrap();
        git::stage_all(repo).unwrap();
        git::commit(repo, msg, &identity()).unwrap()
    }

    /// Set up an origin + local pair with a shared initial commit.
    fn setup_pair() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Repository,
    ) {
        let origin_dir = tempfile::tempdir().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.bare(true).initial_head("main");
        Repository::init_opts(origin_dir.path(), &opts).unwrap();

        let local_dir = tempfile::tempdir().unwrap();
        let local = git::init_with_initial_commit(local_dir.path(), &identity()).unwrap();
        local
            .remote("origin", origin_dir.path().to_str().unwrap())
            .unwrap();

        // Seed: both clones share this commit.
        let seed_meta = make_meta(Uuid::new_v4(), ts(0));
        commit_meta(&local, local_dir.path(), &seed_meta, "seed");
        git::push(&local, "main", &GitAuth::None).unwrap();

        (origin_dir, local_dir, local)
    }

    fn clone_local(origin: &tempfile::TempDir) -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git::clone(
            origin.path().to_str().unwrap(),
            dir.path(),
            &GitAuth::None,
        )
        .unwrap();
        (dir, repo)
    }

    #[test]
    fn no_resolve_when_branches_match() {
        let (_origin, _ld, local) = setup_pair();
        git::fetch_origin(&local, &GitAuth::None).unwrap();
        let outcome = resolve_for_game(&local, "elden-ring", "Desktop-PC").unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn no_resolve_when_only_local_is_ahead() {
        let (_origin, local_dir, local) = setup_pair();
        // Local commits but doesn't push.
        let m = make_meta(Uuid::new_v4(), ts(5));
        commit_meta(&local, local_dir.path(), &m, "local advance");
        git::fetch_origin(&local, &GitAuth::None).unwrap();
        // Not diverged — local is strictly ahead.
        let outcome = resolve_for_game(&local, "elden-ring", "Desktop-PC").unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn newer_local_wins_and_backs_up_remote() {
        let (origin, local_dir, local) = setup_pair();

        // Side B (clone) commits older save and pushes.
        let (b_dir, b_repo) = clone_local(&origin);
        let b_machine = Uuid::new_v4();
        let b_meta = make_meta(b_machine, ts(3));
        commit_meta(&b_repo, b_dir.path(), &b_meta, "b @ ts=3");
        git::push(&b_repo, "main", &GitAuth::None).unwrap();

        // Side A (local) commits NEWER save locally (does not push, hasn't
        // fetched B's push either yet).
        let a_meta = make_meta(Uuid::new_v4(), ts(10));
        commit_meta(&local, local_dir.path(), &a_meta, "a @ ts=10");

        // Now A fetches and resolves.
        git::fetch_origin(&local, &GitAuth::None).unwrap();
        let outcome = resolve_for_game(&local, "elden-ring", "Desktop-PC")
            .unwrap()
            .expect("expected a conflict");

        assert_eq!(outcome.winner, Side::Local);
        assert!(outcome.backup_branch.starts_with("backup/elden-ring/"));
        // Backup branch should track B's commit.
        assert_eq!(outcome.loser_oid, git::rev_parse(&local, &outcome.backup_branch).unwrap());
        // Main now points at A's commit (winner).
        assert_eq!(
            outcome.winner_oid,
            git::rev_parse(&local, "refs/heads/main").unwrap()
        );
        // Backup branch's machine label = B's machine uuid.
        assert_eq!(outcome.loser_machine_hint.as_deref(), Some(b_machine.to_string().as_str()));
    }

    #[test]
    fn newer_remote_wins_and_backs_up_local() {
        let (origin, local_dir, local) = setup_pair();

        // Side B pushes a newer save first.
        let (b_dir, b_repo) = clone_local(&origin);
        let b_meta = make_meta(Uuid::new_v4(), ts(15));
        commit_meta(&b_repo, b_dir.path(), &b_meta, "b @ ts=15 (winner)");
        git::push(&b_repo, "main", &GitAuth::None).unwrap();

        // Side A commits an older save locally (offline scenario).
        let a_machine = Uuid::new_v4();
        let a_meta = make_meta(a_machine, ts(7));
        commit_meta(&local, local_dir.path(), &a_meta, "a @ ts=7");

        git::fetch_origin(&local, &GitAuth::None).unwrap();
        let outcome = resolve_for_game(&local, "elden-ring", "Desktop-PC")
            .unwrap()
            .expect("expected a conflict");

        assert_eq!(outcome.winner, Side::Remote);
        // Loser branch tracks A's local commit.
        assert_eq!(
            outcome.loser_oid,
            git::rev_parse(&local, &outcome.backup_branch).unwrap()
        );
        // A's content lives on the backup branch — backup label uses A's machine uuid.
        assert_eq!(outcome.loser_machine_hint.as_deref(), Some(a_machine.to_string().as_str()));
    }

    #[test]
    fn loser_content_lives_on_backup_branch() {
        let (origin, local_dir, local) = setup_pair();

        let (b_dir, b_repo) = clone_local(&origin);
        let b_meta = make_meta(Uuid::new_v4(), ts(5));
        commit_meta(&b_repo, b_dir.path(), &b_meta, "b is older");
        git::push(&b_repo, "main", &GitAuth::None).unwrap();

        let a_meta = make_meta(Uuid::new_v4(), ts(20));
        commit_meta(&local, local_dir.path(), &a_meta, "a is newer");

        git::fetch_origin(&local, &GitAuth::None).unwrap();
        let outcome = resolve_for_game(&local, "elden-ring", "Desktop-PC")
            .unwrap()
            .unwrap();

        // The backup branch must point at the loser's commit and that
        // commit must still be reachable — the loser is preserved.
        let backup_commit = local.find_commit(outcome.loser_oid).unwrap();
        let tree = backup_commit.tree().unwrap();
        let save_entry = tree.get_path(std::path::Path::new("elden-ring/save.dat")).unwrap();
        let blob = local.find_blob(save_entry.id()).unwrap();
        assert_eq!(std::str::from_utf8(blob.content()).unwrap(), "b is older");
    }

    #[test]
    fn timestamps_equal_falls_back_to_local() {
        // Hard to set up in a real repo because mtime resolution differs;
        // test the decision function directly.
        let t = ts(0);
        assert_eq!(decide_winner(Some(t), Some(t)), Some(Side::Local));
    }

    #[test]
    fn one_side_missing_meta_means_other_side_wins() {
        assert_eq!(decide_winner(Some(ts(5)), None), Some(Side::Local));
        assert_eq!(decide_winner(None, Some(ts(5))), Some(Side::Remote));
        assert_eq!(decide_winner(None, None), None);
    }
}
