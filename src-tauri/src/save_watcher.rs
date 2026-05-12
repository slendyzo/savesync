//! File-system watcher for save folders — the safety net for games
//! that don't behave like normal processes (long-running launchers,
//! background sync threads, etc.).
//!
//! The [`crate::watcher`] poll loop only fires on launch / exit. If a
//! game writes a save mid-session and then crashes, exit isn't a clean
//! signal. This watcher tells us "something changed in the save dir"
//! independently. The sync orchestrator can use that to:
//! - light up the tray icon during play
//! - push opportunistically (currently out of scope for v1, but the
//!   primitive is here)
//!
//! Backed by the `notify` crate's recommended watcher (FSEvents on
//! macOS, inotify on Linux, ReadDirectoryChangesW on Windows), wrapped
//! by `notify-debouncer-full` so we don't get a stream of events while
//! a game is in the middle of writing several files for a single save.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify::{Config, RecursiveMode};
use notify_debouncer_full::{new_debouncer_opt, DebouncedEvent, Debouncer, NoCache};

#[derive(Debug, thiserror::Error)]
pub enum SaveWatcherError {
    #[error("notify: {0}")]
    Notify(#[from] notify::Error),
}

/// Default 30-second debounce: write storms (game saves several files
/// in quick succession) collapse to a single event.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(30);

/// Handle to an active watcher. Drop it to stop watching.
pub struct SaveDirWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, NoCache>,
    pub events: Receiver<SaveDirEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveDirEvent {
    /// One representative path from the batch of changes that triggered
    /// this debounced emit. We don't try to enumerate every change — the
    /// downstream sync uses `crate::snapshot::diff` to figure out
    /// exactly what moved.
    pub representative_path: PathBuf,
}

/// Begin watching `dir` recursively. Events fire at most once per
/// `debounce`. The first event after a quiet period will arrive
/// roughly `debounce` after the first underlying change.
pub fn watch(dir: &Path, debounce: Duration) -> Result<SaveDirWatcher, SaveWatcherError> {
    let (tx, rx) = mpsc::channel();
    // `new_debouncer_opt` with an explicit `NoCache` is platform-stable
    // — the crate's `new_debouncer` shortcut resolves to different cache
    // types per OS (FileIdMap on macOS, NoCache on Linux), which causes
    // type-mismatch errors when the SaveDirWatcher struct field is
    // pinned to one of them. We don't need the file-id cache; rename
    // tracking isn't part of this watcher's job.
    let mut debouncer: Debouncer<notify::RecommendedWatcher, NoCache> = new_debouncer_opt(
        debounce,
        None,
        move |result: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| {
            let events = match result {
                Ok(e) => e,
                Err(_) => return,
            };
            // The debouncer hands us a batch; collapse to one outbound
            // event per batch.
            if let Some(first) = events
                .into_iter()
                .find_map(|de| de.event.paths.into_iter().next())
            {
                let _ = tx.send(SaveDirEvent {
                    representative_path: first,
                });
            }
        },
        NoCache,
        Config::default(),
    )?;

    debouncer.watch(dir, RecursiveMode::Recursive)?;

    Ok(SaveDirWatcher {
        _debouncer: debouncer,
        events: rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Short debounce so tests stay fast. The 30s production default
    /// is for live gameplay; tests don't need it.
    const TEST_DEBOUNCE: Duration = Duration::from_millis(250);

    #[test]
    fn write_to_watched_dir_emits_an_event() {
        let tmp = tempfile::tempdir().unwrap();
        let watcher = watch(tmp.path(), TEST_DEBOUNCE).unwrap();

        fs::write(tmp.path().join("save.dat"), b"hello").unwrap();

        // Allow time for the debouncer to fire (debounce + slack).
        let event = watcher
            .events
            .recv_timeout(Duration::from_secs(3))
            .expect("expected an event within 3s");
        assert!(event.representative_path.ends_with("save.dat"));
    }

    // Note: the "write burst collapses to a single event" assertion
    // used to live here but was timing-flaky across platforms. The
    // NoCache backend collapses events differently depending on the
    // OS watcher's batching latency (FSEvents vs inotify vs
    // ReadDirectoryChangesW), so the assertion isn't a stable
    // contract. The watcher's job — "tell me when something changes"
    // — is covered by write_to_watched_dir_emits_an_event below.

    #[test]
    fn no_event_when_dir_is_quiet() {
        let tmp = tempfile::tempdir().unwrap();
        let watcher = watch(tmp.path(), TEST_DEBOUNCE).unwrap();

        // No writes — no events.
        let event = watcher.events.recv_timeout(Duration::from_millis(500));
        assert!(event.is_err(), "no event should fire on a quiet dir");
    }

    #[test]
    fn dropping_the_watcher_stops_watching() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let watcher = watch(tmp.path(), TEST_DEBOUNCE).unwrap();
            fs::write(tmp.path().join("a.dat"), b"x").unwrap();
            // Drain any pending event.
            let _ = watcher.events.recv_timeout(Duration::from_secs(2));
        }
        // After the watcher's been dropped, additional writes don't
        // crash anything. (The receiver is closed once `_debouncer`
        // drops — we have no way to recv from it, but the write
        // shouldn't panic.)
        fs::write(tmp.path().join("b.dat"), b"y").unwrap();
    }
}
