//! High-level orchestration: glue the manifest + git + lfs + snapshot +
//! conflict modules into the two operations the world cares about:
//! `push_game` and `pull_game`.
//!
//! Both consume the per-machine [`crate::local_config::LocalConfig`] so
//! they know where the user's save folder lives on this disk.

use std::path::{Path, PathBuf};

use git2::Repository;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::conflict::{self, ConflictError, ConflictOutcome};
use crate::git::{self, GitAuth, GitError, GitIdentity};
use crate::lfs::{self, LfsConfig};
use crate::local_config::LocalConfig;
use crate::manifest::GameMeta;
use crate::snapshot::{self, SnapshotConfig};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("git: {0}")]
    Git(#[from] GitError),
    #[error("libgit2: {0}")]
    LibGit2(#[from] git2::Error),
    #[error("snapshot: {0}")]
    Snapshot(#[from] crate::snapshot::SnapshotError),
    #[error("conflict: {0}")]
    Conflict(#[from] ConflictError),
    #[error("lfs: {0}")]
    Lfs(#[from] crate::lfs::LfsError),
    #[error("manifest: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("walkdir: {0}")]
    Walk(#[from] walkdir::Error),
    #[error("game not registered locally: {0}")]
    UnknownGame(String),
    #[error("no save folder at {0}")]
    NoSavePath(PathBuf),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PushOutcome {
    pub committed: bool,
    pub commit_message: Option<String>,
    /// Files that were routed through LFS this push (subset of staged
    /// files that exceeded the threshold).
    pub lfs_routed: Vec<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PullOutcome {
    /// Whether the local repo's main branch was fast-forwarded.
    pub fast_forwarded: bool,
    /// Set if a conflict was resolved during this pull. The caller
    /// should still push afterwards (force) so the backup branch lands
    /// on the remote.
    #[serde(skip_serializing)]
    pub conflict: Option<ConflictOutcome>,
    /// Files that were copied from repo→save folder.
    pub files_synced: usize,
}

/// Push the local save folder's contents up to the repo's `<game>/`
/// directory.
///
/// Flow:
/// 1. Snapshot the user's save folder.
/// 2. Mirror the snapshot into `<repo>/<game>/` (overwriting any
///    existing files there, deleting ones that no longer exist).
/// 3. Write the updated `.savesync-meta.json`.
/// 4. Route any file >LFS threshold through git-lfs.
/// 5. Stage everything in the game folder + `.gitattributes`.
/// 6. Commit + push.
///
/// If nothing changed since the last sync, this is a no-op and returns
/// `committed: false`.
pub fn push_game(
    local: &LocalConfig,
    repo: &Repository,
    game_id: &str,
    auth: &GitAuth,
    lfs_cfg: &LfsConfig,
) -> Result<PushOutcome, SyncError> {
    let local_game = local
        .find_game(game_id)
        .ok_or_else(|| SyncError::UnknownGame(game_id.to_string()))?;
    if !local_game.save_path.exists() {
        return Err(SyncError::NoSavePath(local_game.save_path.clone()));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::LibGit2(git2::Error::from_str("repo has no workdir")))?;
    let repo_game_dir = workdir.join(game_id);

    let snap_cfg = SnapshotConfig::default();

    // Snapshot the user's actual save folder. This is the source of
    // truth for what we want to push.
    let user_snap = snapshot::snapshot_dir(&local_game.save_path, &snap_cfg, None)?;

    // Load the previous .savesync-meta.json if it exists — used to
    // determine whether this push is a no-op.
    let meta_path = repo_game_dir.join(".savesync-meta.json");
    let previous = read_meta_if_present(&meta_path)?;
    if let Some(prev) = &previous {
        if prev.files == user_snap {
            return Ok(PushOutcome {
                committed: false,
                commit_message: None,
                lfs_routed: Vec::new(),
            });
        }
    }

    // Mirror save_path → repo/game_id/.
    mirror_dir(&local_game.save_path, &repo_game_dir, &snap_cfg)?;

    // Write the new meta. `last_sync_at` is set to the latest file
    // mtime in the snapshot (not Utc::now()) so newest-wins conflict
    // resolution reflects "when the user last edited the save", not
    // "when this machine happened to call sync". The fallback to
    // Utc::now() only triggers for empty save folders.
    let mut meta = GameMeta::new(game_id, local.machine_id);
    meta.last_sync_at = user_snap
        .values()
        .map(|f| f.mtime)
        .max()
        .unwrap_or_else(chrono::Utc::now);
    meta.files = user_snap;
    std::fs::write(&meta_path, meta.to_json_pretty()?)?;

    // Route large files through LFS. Collect the repo-relative paths
    // for the routing call.
    let game_id_path = PathBuf::from(game_id);
    let candidate_paths: Vec<PathBuf> = WalkDir::new(&repo_game_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let rel = e.path().strip_prefix(workdir).unwrap();
            rel.to_path_buf()
        })
        .collect();
    let candidate_refs: Vec<&Path> = candidate_paths.iter().map(PathBuf::as_path).collect();
    let lfs_outcome = lfs::route_large_files(workdir, lfs_cfg, &candidate_refs)?;

    // Stage everything we care about.
    git::stage_all(repo)?;

    // Commit with our conventional subject line.
    let subject = git::save_commit_subject(game_id, &local.machine_name, meta.last_sync_at);
    git::commit(repo, &subject, &GitIdentity::for_machine(&local.machine_name))?;

    // Push. If this is the very first commit on `main`, refs/heads/main
    // already exists from the initial-commit helper.
    git::push(repo, "main", auth)?;

    let _ = game_id_path; // currently unused, but reserved for future
    let _ = local.machine_id; // placeholder for the meta wiring above

    Ok(PushOutcome {
        committed: true,
        commit_message: Some(subject),
        lfs_routed: lfs_outcome.routed,
    })
}

/// Pull the latest state of `<game>/` from the repo into the user's
/// save folder.
///
/// Flow:
/// 1. Fetch origin.
/// 2. Attempt fast-forward pull. If the branches have diverged, fall
///    back to `conflict::resolve_for_game`, which creates a backup
///    branch and rewrites local main to the winner. The backup branch
///    is left for the caller to push.
/// 3. Check out the new main.
/// 4. Mirror `<repo>/<game>/` → user's save folder.
pub fn pull_game(
    local: &LocalConfig,
    repo: &Repository,
    game_id: &str,
    auth: &GitAuth,
) -> Result<PullOutcome, SyncError> {
    let local_game = local
        .find_game(game_id)
        .ok_or_else(|| SyncError::UnknownGame(game_id.to_string()))?;

    git::fetch_origin(repo, auth)?;

    let (fast_forwarded, conflict_outcome) = match git::pull_ff_only(repo, "main", auth) {
        Ok(git::PullOutcome::FastForwarded) => (true, None),
        Ok(git::PullOutcome::AlreadyUpToDate) => (false, None),
        Err(GitError::Diverged(_)) => {
            let resolved = conflict::resolve_for_game(repo, game_id, &local.machine_name)?;
            // After resolve, main points at the winner. Make the
            // workdir match.
            conflict::checkout_main_after_resolve(repo)?;
            (true, resolved)
        }
        Err(e) => return Err(SyncError::Git(e)),
    };

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::LibGit2(git2::Error::from_str("repo has no workdir")))?;
    let repo_game_dir = workdir.join(game_id);

    let files_synced = if repo_game_dir.is_dir() {
        let snap_cfg = SnapshotConfig::default();
        std::fs::create_dir_all(&local_game.save_path)?;
        mirror_dir(&repo_game_dir, &local_game.save_path, &snap_cfg)?
    } else {
        0
    };

    let _ = Uuid::new_v4; // shut up unused-import warning if any

    Ok(PullOutcome {
        fast_forwarded,
        conflict: conflict_outcome,
        files_synced,
    })
}

fn read_meta_if_present(path: &Path) -> Result<Option<GameMeta>, SyncError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(Some(GameMeta::from_json(&raw)?))
}

/// Mirror `src` → `dst`. After the call, `dst` looks exactly like `src`
/// w.r.t. game files (ephemeral files like `.tmp` are skipped both
/// directions; `.savesync-meta.json` in the dst is preserved on
/// repo→save direction since it isn't in `src`).
///
/// Returns the number of files copied (additions + overwrites).
fn mirror_dir(src: &Path, dst: &Path, cfg: &SnapshotConfig) -> Result<usize, SyncError> {
    use std::collections::HashSet;

    // Snapshot the source so we know what files SHOULD exist.
    let src_snap = snapshot::snapshot_dir(src, cfg, None)?;
    let src_files: HashSet<String> = src_snap.keys().cloned().collect();

    std::fs::create_dir_all(dst)?;

    let mut copied = 0usize;
    for (rel, _meta) in &src_snap {
        let src_path = src.join(rel);
        let dst_path = dst.join(rel);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src_path, &dst_path)?;
        copied += 1;
    }

    // Delete files in `dst` that aren't in `src` (mirroring). Skip the
    // `.savesync-meta.json` file — it's owned by the repo side and
    // shouldn't be deleted during repo→save mirror.
    let dst_snap = snapshot::snapshot_dir(dst, cfg, None)?;
    for (rel, _) in &dst_snap {
        if rel == ".savesync-meta.json" {
            continue;
        }
        if !src_files.contains(rel) {
            let _ = std::fs::remove_file(dst.join(rel));
        }
    }

    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Platform;
    use std::fs;
    use std::path::PathBuf;

    fn make_local_config(
        machine_name: &str,
        repo_path: PathBuf,
        game_id: &str,
        save_path: PathBuf,
    ) -> LocalConfig {
        let mut cfg = LocalConfig {
            schema_version: 1,
            machine_id: Uuid::new_v4(),
            machine_name: machine_name.into(),
            hostname: "test-host".into(),
            platform: Platform::Linux,
            repo_path,
            games: Vec::new(),
        };
        cfg.upsert_game(game_id.into(), save_path);
        cfg
    }

    /// Set up a bare origin + two clones, register a game in each
    /// clone with its own save folder. Returns everything needed to
    /// drive a two-machine test.
    struct TwoMachineFixture {
        _origin: tempfile::TempDir,
        a_root: tempfile::TempDir,
        b_root: tempfile::TempDir,
        a_repo: Repository,
        b_repo: Repository,
        a_cfg: LocalConfig,
        b_cfg: LocalConfig,
        a_save: PathBuf,
        b_save: PathBuf,
    }

    fn setup_two_machines(game_id: &str) -> TwoMachineFixture {
        let origin = tempfile::tempdir().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.bare(true).initial_head("main");
        Repository::init_opts(origin.path(), &opts).unwrap();

        // Machine A: repo + save folder
        let a_root = tempfile::tempdir().unwrap();
        let a_repo_path = a_root.path().join("repo");
        let a_save = a_root.path().join("save");
        fs::create_dir_all(&a_save).unwrap();
        let a_repo = git::init_with_initial_commit(
            &a_repo_path,
            &GitIdentity::for_machine("Machine-A"),
        )
        .unwrap();
        a_repo
            .remote("origin", origin.path().to_str().unwrap())
            .unwrap();
        // Publish the initial commit so clones can fast-forward.
        git::push(&a_repo, "main", &GitAuth::None).unwrap();
        let a_cfg = make_local_config("Machine-A", a_repo_path.clone(), game_id, a_save.clone());

        // Machine B: clone + save folder
        let b_root = tempfile::tempdir().unwrap();
        let b_repo_path = b_root.path().join("repo");
        let b_save = b_root.path().join("save");
        fs::create_dir_all(&b_save).unwrap();
        let b_repo = git::clone(
            origin.path().to_str().unwrap(),
            &b_repo_path,
            &GitAuth::None,
        )
        .unwrap();
        let b_cfg = make_local_config("Machine-B", b_repo_path.clone(), game_id, b_save.clone());

        TwoMachineFixture {
            _origin: origin,
            a_root,
            b_root,
            a_repo,
            b_repo,
            a_cfg,
            b_cfg,
            a_save,
            b_save,
        }
    }

    fn small_lfs_cfg() -> LfsConfig {
        // Threshold high enough that no test file triggers LFS, so the
        // sync tests don't depend on git-lfs being installed.
        LfsConfig {
            threshold_bytes: 10 * 1024 * 1024,
            ..LfsConfig::with_system_binary()
        }
    }

    #[test]
    fn push_then_pull_round_trips_a_save() {
        let fx = setup_two_machines("elden-ring");

        // A writes a save and pushes.
        fs::write(fx.a_save.join("ER0000.sl2"), b"hello from A").unwrap();
        fs::create_dir_all(fx.a_save.join("subdir")).unwrap();
        fs::write(fx.a_save.join("subdir/nested.dat"), b"nested").unwrap();
        let outcome = push_game(&fx.a_cfg, &fx.a_repo, "elden-ring", &GitAuth::None, &small_lfs_cfg())
            .unwrap();
        assert!(outcome.committed);

        // B pulls.
        let pull = pull_game(&fx.b_cfg, &fx.b_repo, "elden-ring", &GitAuth::None).unwrap();
        assert!(pull.fast_forwarded);
        assert!(pull.conflict.is_none());
        assert!(pull.files_synced >= 2);

        // B's save folder should now match A's save folder.
        assert_eq!(
            fs::read_to_string(fx.b_save.join("ER0000.sl2")).unwrap(),
            "hello from A"
        );
        assert_eq!(
            fs::read_to_string(fx.b_save.join("subdir/nested.dat")).unwrap(),
            "nested"
        );
    }

    #[test]
    fn second_push_with_no_changes_is_a_noop() {
        let fx = setup_two_machines("elden-ring");
        fs::write(fx.a_save.join("ER0000.sl2"), b"x").unwrap();
        let first = push_game(&fx.a_cfg, &fx.a_repo, "elden-ring", &GitAuth::None, &small_lfs_cfg())
            .unwrap();
        assert!(first.committed);

        let second = push_game(&fx.a_cfg, &fx.a_repo, "elden-ring", &GitAuth::None, &small_lfs_cfg())
            .unwrap();
        assert!(!second.committed, "expected no-op when nothing changed");
    }

    #[test]
    fn pull_with_divergence_resolves_via_conflict_module() {
        let fx = setup_two_machines("elden-ring");

        // A pushes initial save.
        fs::write(fx.a_save.join("ER0000.sl2"), b"a-v1").unwrap();
        push_game(&fx.a_cfg, &fx.a_repo, "elden-ring", &GitAuth::None, &small_lfs_cfg())
            .unwrap();

        // B pulls (gets A's initial state), then writes locally without
        // pushing.
        pull_game(&fx.b_cfg, &fx.b_repo, "elden-ring", &GitAuth::None).unwrap();
        // Tiny sleep so B's last_sync_at clearly differs from A's later push.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(fx.b_save.join("ER0000.sl2"), b"b-offline").unwrap();
        // B can't push directly because its push hasn't pulled A's
        // newer save yet — simulate B pushing while A also has unsynced
        // local changes by having A push *first* AND B commits but
        // doesn't push.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(fx.a_save.join("ER0000.sl2"), b"a-newer").unwrap();
        push_game(&fx.a_cfg, &fx.a_repo, "elden-ring", &GitAuth::None, &small_lfs_cfg())
            .unwrap();

        // B records a local commit through push_game's commit step but
        // its push will fail because origin is now ahead.
        // For this test, drive the commit via a direct call so we don't
        // need to handle push-rejection in push_game itself yet.
        // (push_game IS the canonical write; here we want B to *try*
        // pushing and then resolve.)
        let push_result = push_game(
            &fx.b_cfg,
            &fx.b_repo,
            "elden-ring",
            &GitAuth::None,
            &small_lfs_cfg(),
        );
        // Push fails because main can't fast-forward on the remote.
        assert!(push_result.is_err(), "expected push to fail on diverged remote");

        // B pulls — this should resolve the conflict by creating a
        // backup branch and rewinding/advancing main to the winner.
        let pull = pull_game(&fx.b_cfg, &fx.b_repo, "elden-ring", &GitAuth::None).unwrap();
        let conflict = pull.conflict.expect("expected a conflict to be resolved");
        assert!(conflict.backup_branch.starts_with("backup/elden-ring/"));

        // Winner is whoever wrote later. A wrote a-newer after B wrote
        // b-offline, so A's content should now be in B's save folder.
        assert_eq!(
            fs::read_to_string(fx.b_save.join("ER0000.sl2")).unwrap(),
            "a-newer"
        );
        // The loser branch points at a commit whose tree has b-offline.
        let loser = fx.b_repo.find_commit(conflict.loser_oid).unwrap();
        let tree = loser.tree().unwrap();
        let entry = tree
            .get_path(std::path::Path::new("elden-ring/ER0000.sl2"))
            .unwrap();
        let blob = fx.b_repo.find_blob(entry.id()).unwrap();
        assert_eq!(std::str::from_utf8(blob.content()).unwrap(), "b-offline");
    }

    #[test]
    fn unknown_game_id_returns_typed_error() {
        let fx = setup_two_machines("elden-ring");
        let err = push_game(
            &fx.a_cfg,
            &fx.a_repo,
            "stardew",
            &GitAuth::None,
            &small_lfs_cfg(),
        )
        .unwrap_err();
        match err {
            SyncError::UnknownGame(id) => assert_eq!(id, "stardew"),
            other => panic!("expected UnknownGame, got {other:?}"),
        }
    }

    #[test]
    fn missing_save_path_returns_typed_error() {
        let mut fx = setup_two_machines("elden-ring");
        // Point the game at a non-existent path.
        let bogus = fx.a_root.path().join("does-not-exist");
        fx.a_cfg.upsert_game("elden-ring".into(), bogus.clone());
        let err = push_game(
            &fx.a_cfg,
            &fx.a_repo,
            "elden-ring",
            &GitAuth::None,
            &small_lfs_cfg(),
        )
        .unwrap_err();
        match err {
            SyncError::NoSavePath(p) => assert_eq!(p, bogus),
            other => panic!("expected NoSavePath, got {other:?}"),
        }
    }
}
