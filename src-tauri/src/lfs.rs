//! Auto-LFS routing for files over a size threshold.
//!
//! libgit2 has no native Git LFS support, so we shell out to a `git-lfs`
//! binary for the small set of operations we need: registering the LFS
//! filters in a repo (`git lfs install --local`) and adding tracking
//! patterns to `.gitattributes` (`git lfs track <pattern>`).
//!
//! The binary path is part of [`LfsConfig`] — in dev it points at the
//! system `git-lfs` (Homebrew, apt, etc.); in shipped builds it points at
//! the Tauri sidecar (`bin/git-lfs-<target-triple>` declared in
//! `tauri.conf.json#bundle.externalBin`).
//!
//! Once a pattern is in `.gitattributes`, every subsequent commit that
//! touches a matching file routes the bytes through LFS automatically —
//! the commit object stores a tiny pointer file, the actual bytes go to
//! `.git/lfs/objects/`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// 50 MiB. Files at or above this threshold get LFS-tracked.
pub const DEFAULT_THRESHOLD_BYTES: u64 = 50 * 1024 * 1024;

/// Runtime config for the LFS wrapper. Caller picks the binary location;
/// this module never searches `$PATH` itself so test and runtime paths
/// stay explicit.
#[derive(Debug, Clone)]
pub struct LfsConfig {
    pub threshold_bytes: u64,
    pub binary: PathBuf,
}

impl LfsConfig {
    /// Default config pointing at a binary named `git-lfs` on `$PATH`.
    /// Useful in dev and tests. Shipped builds should construct an
    /// `LfsConfig` whose `binary` resolves to the Tauri sidecar path.
    pub fn with_system_binary() -> Self {
        Self {
            threshold_bytes: DEFAULT_THRESHOLD_BYTES,
            binary: PathBuf::from("git-lfs"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LfsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("git-lfs exited with status {status}: {stderr}")]
    BinaryFailed { status: i32, stderr: String },
    #[error("path is not valid UTF-8: {0}")]
    InvalidPath(String),
}

/// True iff the file at `path` is at least `threshold` bytes long.
pub fn is_large(path: &Path, threshold: u64) -> Result<bool, LfsError> {
    let meta = std::fs::metadata(path)?;
    Ok(meta.len() >= threshold)
}

/// Filter a slice of repo-relative paths down to those that exceed the
/// LFS threshold. Skips paths that don't exist or aren't files.
pub fn find_large_files(
    workdir: &Path,
    candidates: &[&Path],
    threshold: u64,
) -> Result<Vec<PathBuf>, LfsError> {
    let mut out = Vec::new();
    for c in candidates {
        let abs = workdir.join(c);
        if abs.is_file() && is_large(&abs, threshold)? {
            out.push(c.to_path_buf());
        }
    }
    Ok(out)
}

/// The pattern we register with `git lfs track` for a single file.
///
/// We use the **exact repo-relative path** rather than a glob like
/// `*.sav`. Most saves are small (Elden Ring at 50KB); only a handful of
/// games produce >50MB blobs. A per-extension glob would route every
/// other game's tiny `.sav` through LFS too, defeating the point.
pub fn pattern_for_path(repo_relative: &Path) -> Result<String, LfsError> {
    repo_relative
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| LfsError::InvalidPath(format!("{repo_relative:?}")))
}

/// Run `git lfs install --local` in `workdir`. Registers the LFS clean +
/// smudge filters in the repo's `.git/config`. Idempotent: re-running on
/// an already-initialized repo is a no-op.
pub fn install_filters(workdir: &Path, cfg: &LfsConfig) -> Result<(), LfsError> {
    run_lfs(workdir, cfg, &["install", "--local"])
}

/// Add `pattern` to the LFS tracking list. `git-lfs` writes/updates
/// `.gitattributes` itself; we don't touch the file directly here so its
/// formatting matches anything else the user might add manually.
pub fn track_pattern(workdir: &Path, cfg: &LfsConfig, pattern: &str) -> Result<(), LfsError> {
    run_lfs(workdir, cfg, &["track", pattern])
}

/// Result of a high-level routing pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingOutcome {
    /// Paths that triggered LFS routing this call (i.e. were over the
    /// threshold). Repo-relative.
    pub routed: Vec<PathBuf>,
    /// True if `.gitattributes` was modified and needs to be staged
    /// alongside the routed files in the next commit.
    pub gitattributes_changed: bool,
}

/// For each candidate path: if it's large, ensure LFS is installed and
/// the path is tracked. Returns the routed paths and whether
/// `.gitattributes` was modified.
///
/// The caller is responsible for staging both the routed files and
/// `.gitattributes` (if modified) before committing.
pub fn route_large_files(
    workdir: &Path,
    cfg: &LfsConfig,
    candidates: &[&Path],
) -> Result<RoutingOutcome, LfsError> {
    let large = find_large_files(workdir, candidates, cfg.threshold_bytes)?;
    if large.is_empty() {
        return Ok(RoutingOutcome::default());
    }

    let gitattributes = workdir.join(".gitattributes");
    let before = read_or_empty(&gitattributes);

    install_filters(workdir, cfg)?;
    for p in &large {
        let pattern = pattern_for_path(p)?;
        track_pattern(workdir, cfg, &pattern)?;
    }

    let after = read_or_empty(&gitattributes);
    Ok(RoutingOutcome {
        routed: large,
        gitattributes_changed: before != after,
    })
}

fn read_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn run_lfs(workdir: &Path, cfg: &LfsConfig, args: &[&str]) -> Result<(), LfsError> {
    let output = Command::new(&cfg.binary)
        .args(args)
        .current_dir(workdir)
        .output()?;
    if !output.status.success() {
        return Err(LfsError::BinaryFailed {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Skip an integration test when no `git-lfs` is on `$PATH`. CI
    /// installs git-lfs before running tests; local dev gets it via brew.
    fn require_git_lfs() -> Option<LfsConfig> {
        if Command::new("git-lfs")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            Some(LfsConfig::with_system_binary())
        } else {
            eprintln!("skipping: git-lfs not installed");
            None
        }
    }

    fn small_threshold() -> LfsConfig {
        LfsConfig {
            threshold_bytes: 1024, // 1 KiB so tests stay fast
            ..LfsConfig::with_system_binary()
        }
    }

    #[test]
    fn is_large_compares_against_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let small = tmp.path().join("small");
        fs::write(&small, b"tiny").unwrap();
        assert!(!is_large(&small, 1024).unwrap());

        let big = tmp.path().join("big");
        fs::write(&big, vec![0u8; 2048]).unwrap();
        assert!(is_large(&big, 1024).unwrap());
    }

    #[test]
    fn find_large_files_filters_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("small.txt"), b"tiny").unwrap();
        fs::write(tmp.path().join("medium.bin"), vec![0u8; 800]).unwrap();
        fs::write(tmp.path().join("huge.bin"), vec![0u8; 4096]).unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir/also-huge.bin"), vec![0u8; 4096]).unwrap();

        let result = find_large_files(
            tmp.path(),
            &[
                Path::new("small.txt"),
                Path::new("medium.bin"),
                Path::new("huge.bin"),
                Path::new("subdir/also-huge.bin"),
            ],
            1024,
        )
        .unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.contains(&PathBuf::from("huge.bin")));
        assert!(result.contains(&PathBuf::from("subdir/also-huge.bin")));
    }

    #[test]
    fn pattern_for_path_uses_repo_relative_string() {
        let p = pattern_for_path(Path::new("elden-ring/ER0000.sl2")).unwrap();
        assert_eq!(p, "elden-ring/ER0000.sl2");
    }

    #[test]
    fn route_large_files_skips_when_all_small() {
        // No git-lfs binary should be invoked when there's nothing to do,
        // so this test runs regardless of whether git-lfs is installed.
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("tiny"), b"x").unwrap();
        let outcome = route_large_files(
            tmp.path(),
            &small_threshold(),
            &[Path::new("tiny")],
        )
        .unwrap();
        assert!(outcome.routed.is_empty());
        assert!(!outcome.gitattributes_changed);
    }

    // ---------- integration: needs real git-lfs + git repo ----------

    fn init_repo(path: &Path) {
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .unwrap();
        // Some CI runners ship a git without a default identity; set
        // per-repo so commits don't fail.
        Command::new("git")
            .args(["config", "user.email", "test@savesync.local"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "savesync-test"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn install_filters_writes_local_git_config() {
        let Some(cfg) = require_git_lfs() else { return };
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());

        install_filters(tmp.path(), &cfg).unwrap();

        let cfg_text = fs::read_to_string(tmp.path().join(".git/config")).unwrap();
        assert!(cfg_text.contains("[filter \"lfs\"]"));
        assert!(cfg_text.contains("clean = git-lfs clean"));
    }

    #[test]
    fn route_then_commit_produces_pointer_file() {
        let Some(_) = require_git_lfs() else { return };
        let cfg = small_threshold();
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());

        // Drop a 4 KiB binary blob.
        let game_dir = tmp.path().join("rdr2");
        fs::create_dir(&game_dir).unwrap();
        let big_path = game_dir.join("save.bin");
        fs::write(&big_path, vec![0xAA; 4096]).unwrap();

        let outcome = route_large_files(
            tmp.path(),
            &cfg,
            &[Path::new("rdr2/save.bin")],
        )
        .unwrap();
        assert_eq!(outcome.routed, vec![PathBuf::from("rdr2/save.bin")]);
        assert!(outcome.gitattributes_changed);

        // Stage everything (including the .gitattributes git-lfs wrote)
        // and commit. The LFS clean filter should convert the file to a
        // pointer at commit time.
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let commit_out = Command::new("git")
            .args(["commit", "-m", "feat: huge save"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(
            commit_out.status.success(),
            "commit failed: {}",
            String::from_utf8_lossy(&commit_out.stderr)
        );

        // The committed version of the file should be a pointer, not the
        // 4 KiB of 0xAA. `git show` reveals what's actually in the blob.
        let shown = Command::new("git")
            .args(["show", "HEAD:rdr2/save.bin"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&shown.stdout);
        assert!(text.starts_with("version https://git-lfs.github.com/spec"));
        assert!(text.contains("oid sha256:"));
        // And the .gitattributes entry should be present.
        let attrs = fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
        assert!(attrs.contains("rdr2/save.bin"));
        assert!(attrs.contains("filter=lfs"));
    }

    #[test]
    fn route_is_idempotent_for_same_file() {
        let Some(_) = require_git_lfs() else { return };
        let cfg = small_threshold();
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());

        fs::create_dir(tmp.path().join("g")).unwrap();
        fs::write(tmp.path().join("g/big.bin"), vec![0u8; 4096]).unwrap();

        let first = route_large_files(tmp.path(), &cfg, &[Path::new("g/big.bin")]).unwrap();
        assert!(first.gitattributes_changed);

        // Second call: same paths, no further change to .gitattributes.
        let second = route_large_files(tmp.path(), &cfg, &[Path::new("g/big.bin")]).unwrap();
        assert!(!second.gitattributes_changed);
    }
}
