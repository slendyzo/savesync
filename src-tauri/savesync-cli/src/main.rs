//! Headless CLI for driving SaveSync without the GUI. Mirrors the
//! operations the GUI's Tauri commands will eventually call.
//!
//! Useful for:
//! - Two-terminal end-to-end tests (the SAVE-14 acceptance loop)
//! - Power users who want to script syncs
//! - Debugging — every step prints what it did
//!
//! Auth: pass `--token <PAT>` or set `SAVESYNC_TOKEN`. SSH keys aren't
//! supported via the CLI yet (planned for the Phase 3 PAT-fallback work).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use savesync_lib::git::{GitAuth, GitIdentity};
use savesync_lib::lfs::LfsConfig;
use savesync_lib::local_config::{default_config_path, LocalConfig, LocalConfigError};
use savesync_lib::sync::{self, PullOutcome, PushOutcome, SyncError};

#[derive(Parser)]
#[command(name = "savesync", version, about = "Headless sync driver for SaveSync repos")]
struct Cli {
    /// Path to the local config file. Defaults to the OS-standard
    /// location (e.g. `~/.config/savesync/config.json`).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// HTTPS token for GitHub / Forgejo / etc. Falls back to
    /// `$SAVESYNC_TOKEN` if not provided. Not needed for local-only
    /// remotes.
    #[arg(long, global = true, env = "SAVESYNC_TOKEN", hide_env_values = true)]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Clone a SaveSync repo and write the per-machine config.
    Init {
        /// URL of the SaveSync data repo (https://github.com/.../savesync-data).
        repo_url: String,
        /// Local path to clone into. Defaults to `<config-dir>/repo`.
        #[arg(long)]
        repo_path: Option<PathBuf>,
        /// Human-readable name for this machine (defaults to hostname).
        #[arg(long)]
        machine_name: Option<String>,
    },
    /// Register a game with this machine's save folder.
    Add {
        /// Path-safe game slug, e.g. `elden-ring`.
        game_id: String,
        /// Absolute path to the game's save folder on this disk.
        #[arg(long)]
        save_path: PathBuf,
    },
    /// Push the current save folder up to the repo.
    Push { game_id: String },
    /// Pull the latest save state down into the save folder.
    Pull { game_id: String },
    /// Show config + registered games.
    Status,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    let config_path = match cli.config {
        Some(p) => p,
        None => default_config_path()?,
    };

    let auth = match &cli.token {
        Some(t) => GitAuth::HttpsToken { token: t.clone() },
        None => GitAuth::None,
    };

    match cli.command {
        Command::Init {
            repo_url,
            repo_path,
            machine_name,
        } => cmd_init(&config_path, repo_url, repo_path, machine_name, &auth),
        Command::Add { game_id, save_path } => cmd_add(&config_path, game_id, save_path),
        Command::Push { game_id } => cmd_push(&config_path, &game_id, &auth),
        Command::Pull { game_id } => cmd_pull(&config_path, &game_id, &auth),
        Command::Status => cmd_status(&config_path),
    }
}

fn cmd_init(
    config_path: &std::path::Path,
    repo_url: String,
    repo_path: Option<PathBuf>,
    machine_name: Option<String>,
    auth: &GitAuth,
) -> Result<(), CliError> {
    if LocalConfig::load_from(config_path)?.is_some() {
        return Err(CliError::AlreadyInitialized(config_path.to_path_buf()));
    }

    let resolved_repo_path = match repo_path {
        Some(p) => p,
        None => config_path
            .parent()
            .map(|p| p.join("repo"))
            .ok_or(CliError::InvalidConfigPath)?,
    };

    println!("Cloning {repo_url} → {}", resolved_repo_path.display());
    if let Some(parent) = resolved_repo_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let repo = savesync_lib::git::clone(&repo_url, &resolved_repo_path, auth)?;

    let machine = machine_name.unwrap_or_else(|| {
        hostname::get()
            .ok()
            .and_then(|s| s.into_string().ok())
            .unwrap_or_else(|| "savesync".into())
    });

    // Cloning an empty bare repo leaves us with no `main` branch.
    // Seed it so the first push has somewhere to go.
    savesync_lib::git::ensure_main_initialized(&repo, &GitIdentity::for_machine(&machine))?;

    let cfg = LocalConfig::new(machine, resolved_repo_path);
    cfg.save_to(config_path)?;
    println!("Wrote config → {}", config_path.display());
    println!("Machine: {} ({})", cfg.machine_name, cfg.machine_id);
    Ok(())
}

fn cmd_add(
    config_path: &std::path::Path,
    game_id: String,
    save_path: PathBuf,
) -> Result<(), CliError> {
    let mut cfg = LocalConfig::load_from(config_path)?
        .ok_or_else(|| CliError::NotInitialized(config_path.to_path_buf()))?;
    cfg.upsert_game(game_id.clone(), save_path.clone());
    cfg.save_to(config_path)?;
    println!("Registered {game_id} → {}", save_path.display());
    Ok(())
}

fn cmd_push(
    config_path: &std::path::Path,
    game_id: &str,
    auth: &GitAuth,
) -> Result<(), CliError> {
    let cfg = LocalConfig::load_from(config_path)?
        .ok_or_else(|| CliError::NotInitialized(config_path.to_path_buf()))?;
    let repo = savesync_lib::git::open(&cfg.repo_path)?;
    let lfs_cfg = LfsConfig::with_system_binary();
    let outcome = sync::push_game(&cfg, &repo, game_id, auth, &lfs_cfg)?;
    print_push_outcome(game_id, outcome);
    Ok(())
}

fn cmd_pull(
    config_path: &std::path::Path,
    game_id: &str,
    auth: &GitAuth,
) -> Result<(), CliError> {
    let cfg = LocalConfig::load_from(config_path)?
        .ok_or_else(|| CliError::NotInitialized(config_path.to_path_buf()))?;
    let repo = savesync_lib::git::open(&cfg.repo_path)?;
    let outcome = sync::pull_game(&cfg, &repo, game_id, auth)?;
    print_pull_outcome(game_id, outcome);
    Ok(())
}

fn cmd_status(config_path: &std::path::Path) -> Result<(), CliError> {
    let cfg = LocalConfig::load_from(config_path)?
        .ok_or_else(|| CliError::NotInitialized(config_path.to_path_buf()))?;
    println!("config         {}", config_path.display());
    println!("machine        {} ({})", cfg.machine_name, cfg.machine_id);
    println!("hostname       {}", cfg.hostname);
    println!("platform       {:?}", cfg.platform);
    println!("repo           {}", cfg.repo_path.display());
    println!();
    if cfg.games.is_empty() {
        println!("(no games registered — run `savesync add <game> --save-path <path>`)");
    } else {
        println!("games:");
        for g in &cfg.games {
            println!("  - {:<24} {}", g.id, g.save_path.display());
        }
    }
    let _ = GitIdentity::for_machine(&cfg.machine_name); // touch to silence unused if any
    Ok(())
}

fn print_push_outcome(game_id: &str, outcome: PushOutcome) {
    if outcome.committed {
        println!("push: {game_id} committed and pushed");
        if let Some(msg) = outcome.commit_message {
            println!("  commit: {msg}");
        }
        if !outcome.lfs_routed.is_empty() {
            println!("  lfs-routed:");
            for p in outcome.lfs_routed {
                println!("    - {}", p.display());
            }
        }
    } else {
        println!("push: {game_id} no changes since last sync (no-op)");
    }
}

fn print_pull_outcome(game_id: &str, outcome: PullOutcome) {
    if let Some(conflict) = outcome.conflict {
        println!("pull: {game_id} CONFLICT RESOLVED");
        println!("  winner: {:?}", conflict.winner);
        println!("  backup branch: {}", conflict.backup_branch);
        if let Some(machine) = conflict.loser_machine_hint {
            println!("  loser machine: {machine}");
        }
    } else if outcome.fast_forwarded {
        println!("pull: {game_id} fast-forwarded, {} file(s) synced", outcome.files_synced);
    } else {
        println!("pull: {game_id} already up-to-date");
    }
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("config: {0}")]
    Config(#[from] LocalConfigError),
    #[error("sync: {0}")]
    Sync(#[from] SyncError),
    #[error("git: {0}")]
    Git(#[from] savesync_lib::git::GitError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("already initialized — config exists at {0}")]
    AlreadyInitialized(PathBuf),
    #[error("not initialized — run `savesync init <repo-url>` first (looked at {0})")]
    NotInitialized(PathBuf),
    #[error("could not derive a default repo path from the config path")]
    InvalidConfigPath,
}
