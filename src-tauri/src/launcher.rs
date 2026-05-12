//! Wrapper-launcher: "Launch this game via SaveSync".
//!
//! Most users will rely on the [`crate::watcher`] poll loop to detect
//! launch/exit and trigger sync. The wrapper launcher is the paranoid
//! mode for games with weird process names, very short-lived launchers,
//! or for users who want a single-click "sync, play, sync" workflow:
//!
//!   pull → spawn(game.exe) → wait for child to exit → push
//!
//! The function is generic over the pull and push closures so the
//! caller can wire it to [`crate::sync::pull_game`] / [`push_game`]
//! at the Tauri command layer without this module taking a dependency
//! on the orchestration types.

use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("pull before launch failed: {0}")]
    PreLaunch(String),
    #[error("spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("push after exit failed: {0}")]
    PostExit(String),
}

/// Outcome of a wrapped launch.
#[derive(Debug)]
pub struct LaunchOutcome {
    pub exit_status: ExitStatus,
    pub post_exit_error: Option<String>,
}

/// Spawn `exe` (optionally with `args`), wait for it to exit, and run
/// `after_exit` whether the child exited cleanly or crashed.
///
/// If `before_launch` returns an error, the game is NOT launched —
/// pulling a fresh save before play is the whole point of the wrapper,
/// so we'd rather refuse to launch than risk the user playing on stale
/// state.
///
/// If `after_exit` fails (e.g. network down at push time), the error is
/// captured in the [`LaunchOutcome`] rather than panicking — the user
/// still played their session and we don't want to swallow the play
/// time by erroring out. The error is for the activity log / toast.
pub fn launch_wrapped<P, A>(
    exe: &Path,
    args: &[&str],
    before_launch: P,
    after_exit: A,
) -> Result<LaunchOutcome, LauncherError>
where
    P: FnOnce() -> Result<(), String>,
    A: FnOnce() -> Result<(), String>,
{
    before_launch().map_err(LauncherError::PreLaunch)?;

    let mut cmd = Command::new(exe);
    cmd.args(args);
    // Detach stdio from the parent so a CLI-spawned game doesn't print
    // into the SaveSync terminal. The GUI doesn't care either way.
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let child: Child = cmd.spawn()?;
    let exit_status = wait_for_exit(child)?;
    let post_exit_error = after_exit().err();

    Ok(LaunchOutcome {
        exit_status,
        post_exit_error,
    })
}

fn wait_for_exit(mut child: Child) -> std::io::Result<ExitStatus> {
    child.wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn sleep_cmd() -> &'static Path {
        // /bin/sleep is POSIX. Tests skip on Windows CI by gating on cfg.
        Path::new("/bin/sleep")
    }

    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn before_launch_runs_first_and_after_exit_runs_after() {
        let order: Rc<Cell<Vec<&'static str>>> = Rc::new(Cell::new(Vec::new()));

        let before_order = order.clone();
        let after_order = order.clone();

        let outcome = launch_wrapped(
            sleep_cmd(),
            &["0.05"],
            move || {
                let mut v = before_order.take();
                v.push("before");
                before_order.set(v);
                Ok(())
            },
            move || {
                let mut v = after_order.take();
                v.push("after");
                after_order.set(v);
                Ok(())
            },
        )
        .unwrap();

        assert!(outcome.exit_status.success());
        assert!(outcome.post_exit_error.is_none());
        assert_eq!(order.take(), vec!["before", "after"]);
    }

    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn before_launch_failure_aborts_before_spawning() {
        let after_called = Rc::new(Cell::new(false));
        let after_cell = after_called.clone();

        let result = launch_wrapped(
            sleep_cmd(),
            &["5"],
            || Err("pull failed: network down".into()),
            move || {
                after_cell.set(true);
                Ok(())
            },
        );

        match result {
            Err(LauncherError::PreLaunch(msg)) => assert!(msg.contains("pull failed")),
            other => panic!("expected PreLaunch error, got {other:?}"),
        }
        assert!(
            !after_called.get(),
            "after_exit must not run when before_launch failed"
        );
    }

    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn after_exit_failure_is_captured_not_propagated() {
        let outcome = launch_wrapped(
            sleep_cmd(),
            &["0.05"],
            || Ok(()),
            || Err("push failed: 403".into()),
        )
        .unwrap();

        assert!(outcome.exit_status.success(), "child still finished cleanly");
        assert_eq!(outcome.post_exit_error.as_deref(), Some("push failed: 403"));
    }

    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn spawn_failure_is_surfaced() {
        let result = launch_wrapped(
            Path::new("/definitely/not/a/real/binary"),
            &[],
            || Ok(()),
            || Ok(()),
        );
        match result {
            Err(LauncherError::Spawn(_)) => {}
            other => panic!("expected Spawn error, got {other:?}"),
        }
    }
}
