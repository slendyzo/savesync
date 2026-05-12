//! Thin wrapper over `git2` exposing the primitives SaveSync needs.
//!
//! No business logic lives here — only safe Rust signatures over libgit2.
//! Conflict policy, push-on-game-exit, etc. compose these primitives in
//! higher-level modules.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use git2::{
    BranchType, Cred, FetchOptions, PushOptions, RemoteCallbacks, Repository, Signature,
};

/// Auth strategy for talking to a remote.
#[derive(Debug, Clone)]
pub enum GitAuth {
    /// HTTPS with a token (GitHub PAT, Forgejo token, etc.). The username is
    /// effectively ignored by most providers when the password is a token;
    /// we send `"x-access-token"` because GitHub Apps require it and PATs
    /// don't care.
    HttpsToken { token: String },
    /// SSH using a key file on disk.
    SshKey {
        private_key: PathBuf,
        public_key: Option<PathBuf>,
        passphrase: Option<String>,
    },
    /// SSH using the running ssh-agent (no key path needed).
    SshAgent,
    /// No auth — used for local-only operations and tests.
    None,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git operation failed: {0}")]
    LibGit2(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("repository state would diverge ({0}); needs explicit resolution")]
    Diverged(String),
    #[error("LFS operation failed: {0}")]
    Lfs(String),
}

impl GitError {
    /// Re-classify a libgit2 error into one of our richer variants when we
    /// can tell what went wrong from the class/code.
    fn classify(err: git2::Error) -> Self {
        use git2::ErrorClass as C;
        match err.class() {
            C::Net | C::Http => GitError::Network(err.message().to_string()),
            C::Ssh | C::Callback => GitError::Auth(err.message().to_string()),
            _ => GitError::LibGit2(err),
        }
    }
}

/// Author/committer identity used on every commit.
#[derive(Debug, Clone)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

impl GitIdentity {
    /// Suggested default for SaveSync commits: machine name + a synthetic
    /// email so commits clearly trace back to this tool.
    pub fn for_machine(machine_name: &str) -> Self {
        Self {
            name: format!("SaveSync ({machine_name})"),
            email: "savesync@local".to_string(),
        }
    }
}

// ---------- primitives ----------

/// Open an existing repository at `path`.
pub fn open(path: &Path) -> Result<Repository, GitError> {
    Repository::open(path).map_err(GitError::classify)
}

/// Initialize a brand-new repository with an initial empty commit on `main`.
pub fn init_with_initial_commit(
    path: &Path,
    identity: &GitIdentity,
) -> Result<Repository, GitError> {
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(path, &opts)?;
    {
        let sig = signature(identity)?;
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let commit_oid =
            repo.commit(None, &sig, &sig, "chore: initialize", &tree, &[])?;
        let commit = repo.find_commit(commit_oid)?;
        repo.branch("main", &commit, true)?;
    }
    repo.set_head("refs/heads/main")?;
    Ok(repo)
}

/// Clone `url` into `dest` using the given auth.
pub fn clone(url: &str, dest: &Path, auth: &GitAuth) -> Result<Repository, GitError> {
    let callbacks = build_callbacks(auth);
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);
    builder.clone(url, dest).map_err(GitError::classify)
}

/// Fetch `origin` (or a specific remote) into the local repo's refs.
pub fn fetch_origin(repo: &Repository, auth: &GitAuth) -> Result<(), GitError> {
    let mut remote = repo.find_remote("origin")?;
    let callbacks = build_callbacks(auth);
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);
    remote
        .fetch::<&str>(&[], Some(&mut fetch_opts), None)
        .map_err(GitError::classify)?;
    Ok(())
}

/// Stage every path under `paths` (relative to the repo workdir). Equivalent
/// to `git add` on each. Skips paths that no longer exist (handles file
/// deletions correctly).
pub fn stage_paths(repo: &Repository, paths: &[&Path]) -> Result<(), GitError> {
    let workdir = repo.workdir().ok_or_else(|| {
        GitError::LibGit2(git2::Error::from_str("bare repo has no workdir"))
    })?;
    let mut index = repo.index()?;
    for p in paths {
        // `p` is repo-relative; check existence against the repo's workdir
        // rather than the process CWD.
        if workdir.join(p).exists() {
            index.add_path(p)?;
        } else {
            // Tolerate "not in index" — caller may have included a path
            // that was never tracked.
            let _ = index.remove_path(p);
        }
    }
    index.write()?;
    Ok(())
}

/// Stage everything in the workdir (including deletions). Equivalent to
/// `git add -A`.
pub fn stage_all(repo: &Repository) -> Result<(), GitError> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

/// Create a commit on the currently-checked-out branch with the given message.
/// Uses the index's current contents as the tree.
pub fn commit(
    repo: &Repository,
    message: &str,
    identity: &GitIdentity,
) -> Result<git2::Oid, GitError> {
    let sig = signature(identity)?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let parent_commit = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        // No HEAD yet means this is the initial commit.
        Err(_) => None,
    };

    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
    Ok(oid)
}

/// Push `branch` to `origin`. Creates the branch on the remote if missing.
pub fn push(repo: &Repository, branch: &str, auth: &GitAuth) -> Result<(), GitError> {
    push_refspecs(repo, &[branch.to_string()], auth, false)
}

/// Force-push `branch` to `origin`, overwriting whatever the remote has.
/// Used by conflict resolution after rewriting local `main` to the winner.
pub fn push_force(repo: &Repository, branch: &str, auth: &GitAuth) -> Result<(), GitError> {
    push_refspecs(repo, &[branch.to_string()], auth, true)
}

/// Push multiple branches in one network round-trip. Useful for pushing
/// `main` + a new `backup/...` branch together after conflict resolution.
pub fn push_many(
    repo: &Repository,
    branches: &[String],
    auth: &GitAuth,
    force: bool,
) -> Result<(), GitError> {
    push_refspecs(repo, branches, auth, force)
}

fn push_refspecs(
    repo: &Repository,
    branches: &[String],
    auth: &GitAuth,
    force: bool,
) -> Result<(), GitError> {
    let mut remote = repo.find_remote("origin")?;
    let prefix = if force { "+" } else { "" };
    let refspecs: Vec<String> = branches
        .iter()
        .map(|b| format!("{prefix}refs/heads/{b}:refs/heads/{b}"))
        .collect();
    let callbacks = build_callbacks(auth);
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);
    let refs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
    remote
        .push(&refs, Some(&mut push_opts))
        .map_err(GitError::classify)?;
    Ok(())
}

/// Fast-forward pull. If `origin/branch` has diverged from local `branch`,
/// returns [`GitError::Diverged`] — divergence is resolved by callers using
/// the conflict-resolution module, not silently merged here.
pub fn pull_ff_only(
    repo: &Repository,
    branch: &str,
    auth: &GitAuth,
) -> Result<PullOutcome, GitError> {
    fetch_origin(repo, auth)?;

    let upstream_ref = repo.find_reference(&format!("refs/remotes/origin/{branch}"))?;
    let upstream_oid = upstream_ref.target().ok_or_else(|| {
        GitError::LibGit2(git2::Error::from_str("origin ref has no target oid"))
    })?;
    let upstream_commit = repo.find_annotated_commit(upstream_oid)?;

    let (analysis, _) = repo.merge_analysis(&[&upstream_commit])?;

    if analysis.is_up_to_date() {
        return Ok(PullOutcome::AlreadyUpToDate);
    }
    if analysis.is_fast_forward() {
        let mut local_ref = repo.find_reference(&format!("refs/heads/{branch}"))?;
        local_ref.set_target(upstream_oid, "savesync: fast-forward pull")?;
        repo.set_head(&format!("refs/heads/{branch}"))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        return Ok(PullOutcome::FastForwarded);
    }

    Err(GitError::Diverged(format!(
        "local '{branch}' and origin/'{branch}' have diverged"
    )))
}

/// Outcome of a fast-forward-only pull.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    AlreadyUpToDate,
    FastForwarded,
}

/// Create a branch named `name` pointing at the current HEAD commit. If
/// `force` is true, an existing branch with that name is moved.
pub fn create_branch_from_head(
    repo: &Repository,
    name: &str,
    force: bool,
) -> Result<(), GitError> {
    let head = repo.head()?.peel_to_commit()?;
    repo.branch(name, &head, force)?;
    Ok(())
}

/// Check out `branch`. The branch must already exist locally.
pub fn checkout_branch(repo: &Repository, branch: &str) -> Result<(), GitError> {
    let (obj, reference) = repo.revparse_ext(branch)?;
    repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().force()))?;
    match reference {
        Some(r) => repo.set_head(r.name().unwrap_or(branch))?,
        None => repo.set_head_detached(obj.id())?,
    }
    Ok(())
}

/// Returns true when `branch` (e.g. `"main"`) exists locally.
pub fn branch_exists(repo: &Repository, branch: &str) -> bool {
    repo.find_branch(branch, BranchType::Local).is_ok()
}

/// Returns the OID `name` resolves to, or None if absent.
pub fn rev_parse(repo: &Repository, name: &str) -> Option<git2::Oid> {
    repo.revparse_single(name).ok().map(|obj| obj.id())
}

/// Read the contents of a file at a specific commit. Returns `Ok(None)`
/// if the path doesn't exist in that commit's tree (no error — the
/// caller usually wants to treat missing-on-one-side as a valid case).
pub fn read_blob_at(
    repo: &Repository,
    commit_oid: git2::Oid,
    path_in_repo: &Path,
) -> Result<Option<Vec<u8>>, GitError> {
    let commit = repo.find_commit(commit_oid)?;
    let tree = commit.tree()?;
    let entry = match tree.get_path(path_in_repo) {
        Ok(e) => e,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(e) => return Err(GitError::LibGit2(e)),
    };
    let obj = entry.to_object(repo)?;
    let blob = obj
        .as_blob()
        .ok_or_else(|| GitError::LibGit2(git2::Error::from_str("expected blob at path")))?;
    Ok(Some(blob.content().to_vec()))
}

/// If `main` doesn't exist in this repo, create it with an empty
/// initial commit and point HEAD at it. Idempotent — a no-op if `main`
/// is already present. Used after cloning an empty bare repo so the
/// first push has a branch to push.
pub fn ensure_main_initialized(
    repo: &Repository,
    identity: &GitIdentity,
) -> Result<(), GitError> {
    if rev_parse(repo, "refs/heads/main").is_some() {
        return Ok(());
    }
    let sig = signature(identity)?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let commit_oid = repo.commit(None, &sig, &sig, "chore: initialize", &tree, &[])?;
    let commit = repo.find_commit(commit_oid)?;
    repo.branch("main", &commit, true)?;
    repo.set_head("refs/heads/main")?;
    Ok(())
}

/// Move `branch` to point at `target_oid`, regardless of ancestry. Used
/// by conflict resolution to rewrite local `main` to the winner. The
/// caller is responsible for checking out HEAD afterwards if they want
/// the workdir to match.
pub fn force_set_branch(
    repo: &Repository,
    branch: &str,
    target_oid: git2::Oid,
    reflog_msg: &str,
) -> Result<(), GitError> {
    let refname = format!("refs/heads/{branch}");
    let mut r = repo.find_reference(&refname)?;
    r.set_target(target_oid, reflog_msg)?;
    Ok(())
}

/// Check whether `local_branch` and `origin/local_branch` have diverged. A
/// branch has diverged when each side has commits not present on the other.
pub fn has_diverged(repo: &Repository, branch: &str) -> Result<bool, GitError> {
    let local = match rev_parse(repo, &format!("refs/heads/{branch}")) {
        Some(oid) => oid,
        None => return Ok(false),
    };
    let remote = match rev_parse(repo, &format!("refs/remotes/origin/{branch}")) {
        Some(oid) => oid,
        None => return Ok(false),
    };
    let (ahead, behind) = repo.graph_ahead_behind(local, remote)?;
    Ok(ahead > 0 && behind > 0)
}

// ---------- helpers ----------

fn signature(identity: &GitIdentity) -> Result<Signature<'_>, GitError> {
    Signature::now(&identity.name, &identity.email).map_err(GitError::classify)
}

fn build_callbacks(auth: &GitAuth) -> RemoteCallbacks<'_> {
    let mut cb = RemoteCallbacks::new();
    let auth_owned = auth.clone();
    cb.credentials(move |_url, username_from_url, _allowed_types| match &auth_owned {
        GitAuth::HttpsToken { token } => Cred::userpass_plaintext("x-access-token", token),
        GitAuth::SshKey {
            private_key,
            public_key,
            passphrase,
        } => Cred::ssh_key(
            username_from_url.unwrap_or("git"),
            public_key.as_deref(),
            private_key,
            passphrase.as_deref(),
        ),
        GitAuth::SshAgent => Cred::ssh_key_from_agent(username_from_url.unwrap_or("git")),
        GitAuth::None => Cred::default(),
    });
    cb
}

/// Conventional commit subject for a save push.
///
/// Example output: `save(elden-ring): Desktop-PC @ 2026-05-12T21:14:00Z`
pub fn save_commit_subject(game_id: &str, machine: &str, when: DateTime<Utc>) -> String {
    format!(
        "save({game_id}): {machine} @ {ts}",
        ts = when.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

/// Conventional commit subject for the loser-on-conflict backup branch.
///
/// Example: `save(elden-ring): backup from ROG-Ally @ 2026-05-12T21:14:00Z`
pub fn backup_commit_subject(game_id: &str, loser_machine: &str, when: DateTime<Utc>) -> String {
    format!(
        "save({game_id}): backup from {loser_machine} @ {ts}",
        ts = when.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

/// Format the canonical backup branch name for a conflict loser.
///
/// Example: `backup/elden-ring/ROG-Ally-2026-05-12T21-14-00Z`
pub fn backup_branch_name(game_id: &str, machine: &str, when: DateTime<Utc>) -> String {
    let ts = when.format("%Y-%m-%dT%H-%M-%SZ");
    // git refs can't contain colons; replace machine-name spaces too.
    let safe_machine = machine.replace(' ', "-");
    format!("backup/{game_id}/{safe_machine}-{ts}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 12, 21, 14, 0).unwrap()
    }

    fn identity() -> GitIdentity {
        GitIdentity::for_machine("Desktop-PC")
    }

    fn make_temp_repo() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_with_initial_commit(tmp.path(), &identity()).unwrap();
        (tmp, repo)
    }

    fn make_remote_pair() -> (tempfile::TempDir, tempfile::TempDir, Repository) {
        // Create a bare "origin" repo + a local clone that has it set up
        // as a real remote. Avoids needing a network for push/pull tests.
        let origin_dir = tempfile::tempdir().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.bare(true).initial_head("main");
        Repository::init_opts(origin_dir.path(), &opts).unwrap();

        let local_dir = tempfile::tempdir().unwrap();
        let local = init_with_initial_commit(local_dir.path(), &identity()).unwrap();
        local
            .remote("origin", origin_dir.path().to_str().unwrap())
            .unwrap();
        (origin_dir, local_dir, local)
    }

    #[test]
    fn init_creates_repo_with_initial_commit() {
        let (_tmp, repo) = make_temp_repo();
        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        assert_eq!(commit.message().unwrap(), "chore: initialize");
        assert!(branch_exists(&repo, "main"));
    }

    #[test]
    fn open_existing_repo_succeeds() {
        let (tmp, _repo) = make_temp_repo();
        let path = tmp.path().to_path_buf();
        let reopened = open(&path).unwrap();
        assert!(reopened.head().is_ok());
    }

    #[test]
    fn open_missing_repo_errors() {
        let tmp = tempfile::tempdir().unwrap();
        // No git repo here. `Repository` isn't Debug, so we can't unwrap_err.
        match open(tmp.path()) {
            Err(GitError::LibGit2(_)) => {}
            Err(other) => panic!("expected LibGit2, got {other:?}"),
            Ok(_) => panic!("expected error for non-repo dir"),
        }
    }

    #[test]
    fn stage_and_commit_a_file() {
        let (tmp, repo) = make_temp_repo();
        fs::write(tmp.path().join("save.dat"), b"some bytes").unwrap();

        stage_paths(&repo, &[Path::new("save.dat")]).unwrap();
        let oid = commit(&repo, "save(elden-ring): test", &identity()).unwrap();

        let commit_obj = repo.find_commit(oid).unwrap();
        assert!(commit_obj.message().unwrap().contains("save(elden-ring)"));

        // The blob should be present in the tree.
        let tree = commit_obj.tree().unwrap();
        let entry = tree.get_name("save.dat").unwrap();
        let blob = repo.find_blob(entry.id()).unwrap();
        assert_eq!(blob.content(), b"some bytes");
    }

    #[test]
    fn stage_all_picks_up_new_files() {
        let (tmp, repo) = make_temp_repo();
        fs::write(tmp.path().join("a"), b"a").unwrap();
        fs::write(tmp.path().join("b"), b"b").unwrap();

        stage_all(&repo).unwrap();
        let oid = commit(&repo, "feat: add a + b", &identity()).unwrap();

        let tree = repo.find_commit(oid).unwrap().tree().unwrap();
        assert!(tree.get_name("a").is_some());
        assert!(tree.get_name("b").is_some());
    }

    #[test]
    fn stage_paths_handles_deletion() {
        let (tmp, repo) = make_temp_repo();
        fs::write(tmp.path().join("file"), b"v1").unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "feat: add file", &identity()).unwrap();

        fs::remove_file(tmp.path().join("file")).unwrap();
        stage_paths(&repo, &[Path::new("file")]).unwrap();
        let oid = commit(&repo, "feat: delete file", &identity()).unwrap();

        let tree = repo.find_commit(oid).unwrap().tree().unwrap();
        assert!(tree.get_name("file").is_none());
    }

    #[test]
    fn create_and_checkout_branch() {
        let (tmp, repo) = make_temp_repo();
        fs::write(tmp.path().join("a"), b"a").unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "feat: a", &identity()).unwrap();

        create_branch_from_head(&repo, "backup/elden-ring/test", false).unwrap();
        assert!(branch_exists(&repo, "backup/elden-ring/test"));

        checkout_branch(&repo, "backup/elden-ring/test").unwrap();
        let head_name = repo.head().unwrap().shorthand().unwrap().to_string();
        assert_eq!(head_name, "backup/elden-ring/test");
    }

    #[test]
    fn branch_force_overwrite_moves_pointer() {
        let (tmp, repo) = make_temp_repo();
        fs::write(tmp.path().join("a"), b"a").unwrap();
        stage_all(&repo).unwrap();
        let first = commit(&repo, "feat: first", &identity()).unwrap();
        create_branch_from_head(&repo, "moving", false).unwrap();

        fs::write(tmp.path().join("a"), b"v2").unwrap();
        stage_all(&repo).unwrap();
        let second = commit(&repo, "feat: second", &identity()).unwrap();
        assert_ne!(first, second);

        create_branch_from_head(&repo, "moving", true).unwrap();
        let branch_oid = rev_parse(&repo, "moving").unwrap();
        assert_eq!(branch_oid, second);
    }

    #[test]
    fn push_to_local_bare_remote() {
        let (_origin, local_dir, local) = make_remote_pair();
        fs::write(local_dir.path().join("hello"), b"hi").unwrap();
        stage_all(&local).unwrap();
        commit(&local, "feat: hello", &identity()).unwrap();

        push(&local, "main", &GitAuth::None).unwrap();

        // Re-clone from bare to verify the push landed.
        let verify_dir = tempfile::tempdir().unwrap();
        let cloned = clone(
            local.find_remote("origin").unwrap().url().unwrap(),
            verify_dir.path(),
            &GitAuth::None,
        )
        .unwrap();
        let head = cloned.head().unwrap().peel_to_commit().unwrap();
        assert!(head.message().unwrap().contains("hello"));
    }

    #[test]
    fn fetch_then_ff_pull_advances_local() {
        let (_origin, local_dir, local) = make_remote_pair();
        // Make a commit and push.
        fs::write(local_dir.path().join("seed"), b"s").unwrap();
        stage_all(&local).unwrap();
        commit(&local, "feat: seed", &identity()).unwrap();
        push(&local, "main", &GitAuth::None).unwrap();

        // Second clone simulating a different machine, advances + pushes.
        let other_dir = tempfile::tempdir().unwrap();
        let other = clone(
            local.find_remote("origin").unwrap().url().unwrap(),
            other_dir.path(),
            &GitAuth::None,
        )
        .unwrap();
        fs::write(other_dir.path().join("from-b"), b"b").unwrap();
        stage_all(&other).unwrap();
        commit(&other, "feat: from b", &identity()).unwrap();
        push(&other, "main", &GitAuth::None).unwrap();

        // Original machine pulls — should fast-forward.
        let outcome = pull_ff_only(&local, "main", &GitAuth::None).unwrap();
        assert_eq!(outcome, PullOutcome::FastForwarded);
        assert!(local_dir.path().join("from-b").exists());
    }

    #[test]
    fn diverged_branches_are_detected() {
        let (_origin, local_dir, local) = make_remote_pair();
        fs::write(local_dir.path().join("seed"), b"s").unwrap();
        stage_all(&local).unwrap();
        commit(&local, "feat: seed", &identity()).unwrap();
        push(&local, "main", &GitAuth::None).unwrap();

        // Clone -> commit on the side -> push, advancing origin.
        let other_dir = tempfile::tempdir().unwrap();
        let other = clone(
            local.find_remote("origin").unwrap().url().unwrap(),
            other_dir.path(),
            &GitAuth::None,
        )
        .unwrap();
        fs::write(other_dir.path().join("from-b"), b"b").unwrap();
        stage_all(&other).unwrap();
        commit(&other, "feat: from b", &identity()).unwrap();
        push(&other, "main", &GitAuth::None).unwrap();

        // Original machine ALSO commits locally (without pulling first).
        fs::write(local_dir.path().join("from-a"), b"a").unwrap();
        stage_all(&local).unwrap();
        commit(&local, "feat: from a", &identity()).unwrap();

        // Fetch so the local repo knows about origin's new tip.
        fetch_origin(&local, &GitAuth::None).unwrap();
        assert!(has_diverged(&local, "main").unwrap());

        // ff-only pull must refuse.
        let err = pull_ff_only(&local, "main", &GitAuth::None).unwrap_err();
        match err {
            GitError::Diverged(_) => {}
            other => panic!("expected Diverged, got {other:?}"),
        }
    }

    #[test]
    fn ff_pull_when_already_up_to_date_is_a_noop() {
        let (_origin, _local_dir, local) = make_remote_pair();
        // No commits beyond initial — fetch from empty bare doesn't change
        // anything. We just want to make sure the function returns the right
        // variant when there's nothing to pull.
        let _ = fetch_origin(&local, &GitAuth::None);
        // After empty fetch, origin/main doesn't exist yet, so this is a
        // not-found error rather than UpToDate — let's seed first.
        push(&local, "main", &GitAuth::None).unwrap();
        let outcome = pull_ff_only(&local, "main", &GitAuth::None).unwrap();
        assert_eq!(outcome, PullOutcome::AlreadyUpToDate);
    }

    #[test]
    fn save_commit_subject_is_conventional() {
        let subject = save_commit_subject("elden-ring", "Desktop-PC", ts());
        assert_eq!(
            subject,
            "save(elden-ring): Desktop-PC @ 2026-05-12T21:14:00Z"
        );
    }

    #[test]
    fn backup_branch_name_is_path_safe() {
        let name = backup_branch_name("elden-ring", "ROG Ally", ts());
        assert_eq!(name, "backup/elden-ring/ROG-Ally-2026-05-12T21-14-00Z");
        assert!(!name.contains(':'), "no colons allowed in git refs");
    }
}
