//! Process watcher: poll the OS process list and emit `Launched` /
//! `Exited` events for tracked games.
//!
//! Architecture:
//! - The watcher's [`tick`] method is synchronous and deterministic —
//!   each call samples the current process list once, advances every
//!   tracked game's state machine, and returns the events that fired
//!   this tick. The async loop (sleep + tick + emit Tauri events) is
//!   the caller's job; tests drive [`tick`] directly with a mocked
//!   [`ProcessSource`] and never need real timing.
//! - Debouncing: a `Launched` event only fires once the process has
//!   been seen for [`WatcherConfig::launch_debounce_ticks`] consecutive
//!   ticks. Default is 2, which on a 2-3s poll interval ≈ 4-6 seconds
//!   of "yes, that's really the game" before we trigger a pull. Stops
//!   us firing on splash screens that immediately spawn a child and
//!   exit.
//! - Pause: per-game flag that suspends event emission without losing
//!   state (so resuming doesn't immediately re-fire Launched).

use std::collections::{HashMap, HashSet};

/// Provides "what process names are running right now?".
///
/// Production uses [`SysinfoSource`]; tests use a mock that returns a
/// pre-canned set so state transitions can be tested deterministically.
pub trait ProcessSource {
    /// Lowercased process names of every running process. Lowercase so
    /// the watcher's `process_names` comparisons are case-insensitive
    /// (Windows ships executables with inconsistent casing).
    fn current_process_names(&mut self) -> HashSet<String>;
}

pub struct SysinfoSource {
    system: sysinfo::System,
}

impl SysinfoSource {
    pub fn new() -> Self {
        Self {
            system: sysinfo::System::new(),
        }
    }
}

impl Default for SysinfoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSource for SysinfoSource {
    fn current_process_names(&mut self) -> HashSet<String> {
        // refresh_processes is cheaper than a full refresh; we only
        // care about which processes exist, not CPU/memory.
        self.system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.system
            .processes()
            .values()
            .filter_map(|p| p.name().to_str())
            .map(|s| s.to_lowercase())
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatcherConfig {
    /// Consecutive sightings required before firing `Launched`. Two
    /// ticks ≈ 4-6 seconds at the default 2-3s poll interval.
    pub launch_debounce_ticks: u32,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            launch_debounce_ticks: 2,
        }
    }
}

/// A game the watcher should monitor.
#[derive(Debug, Clone)]
pub struct WatchedGame {
    pub id: String,
    /// One or more process names that count as "this game running".
    /// Stored lowercased. Multiple names support games that ship a
    /// launcher (e.g. `eldenring.exe` AND `start_protected_game.exe`).
    pub process_names: Vec<String>,
}

impl WatchedGame {
    pub fn new(id: impl Into<String>, process_names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            id: id.into(),
            process_names: process_names
                .into_iter()
                .map(|n| n.into().to_lowercase())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEventKind {
    Launched,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    pub game_id: String,
    pub kind: WatchEventKind,
    /// Monotonic tick number that produced this event. Useful for
    /// debugging and for ordering correlation in the activity log.
    pub tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameState {
    /// Not running. `consecutive_sightings` tracks how many times in a
    /// row the process has been seen during the launch-debounce window.
    NotRunning { consecutive_sightings: u32 },
    Running,
}

struct GameSlot {
    game: WatchedGame,
    state: GameState,
    paused: bool,
}

pub struct ProcessWatcher<S: ProcessSource> {
    source: S,
    config: WatcherConfig,
    games: HashMap<String, GameSlot>,
    tick_count: u64,
}

impl<S: ProcessSource> ProcessWatcher<S> {
    pub fn new(source: S, config: WatcherConfig) -> Self {
        Self {
            source,
            config,
            games: HashMap::new(),
            tick_count: 0,
        }
    }

    pub fn register(&mut self, game: WatchedGame) {
        self.games.insert(
            game.id.clone(),
            GameSlot {
                game,
                state: GameState::NotRunning {
                    consecutive_sightings: 0,
                },
                paused: false,
            },
        );
    }

    pub fn unregister(&mut self, game_id: &str) {
        self.games.remove(game_id);
    }

    pub fn pause(&mut self, game_id: &str) {
        if let Some(slot) = self.games.get_mut(game_id) {
            slot.paused = true;
        }
    }

    pub fn resume(&mut self, game_id: &str) {
        if let Some(slot) = self.games.get_mut(game_id) {
            slot.paused = false;
        }
    }

    pub fn is_paused(&self, game_id: &str) -> bool {
        self.games.get(game_id).map(|s| s.paused).unwrap_or(false)
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Sample the process list once, advance every game's state
    /// machine, return the events that fired. Idempotent w.r.t. the
    /// process list — calling tick twice in a row with identical
    /// process state advances states but emits at most one event per
    /// game.
    pub fn tick(&mut self) -> Vec<WatchEvent> {
        self.tick_count += 1;
        let tick_now = self.tick_count;
        let processes = self.source.current_process_names();
        let mut events = Vec::new();

        for slot in self.games.values_mut() {
            let seen = slot
                .game
                .process_names
                .iter()
                .any(|name| processes.contains(name));

            let event_kind =
                advance(&mut slot.state, seen, self.config.launch_debounce_ticks);

            // Pause swallows events but state still advances so we
            // don't re-fire on resume.
            if slot.paused {
                continue;
            }

            if let Some(kind) = event_kind {
                events.push(WatchEvent {
                    game_id: slot.game.id.clone(),
                    kind,
                    tick: tick_now,
                });
            }
        }

        events
    }
}

fn advance(state: &mut GameState, seen: bool, launch_debounce: u32) -> Option<WatchEventKind> {
    match state {
        GameState::NotRunning {
            consecutive_sightings,
        } => {
            if seen {
                *consecutive_sightings += 1;
                if *consecutive_sightings >= launch_debounce.max(1) {
                    *state = GameState::Running;
                    return Some(WatchEventKind::Launched);
                }
            } else {
                *consecutive_sightings = 0;
            }
            None
        }
        GameState::Running => {
            if seen {
                None
            } else {
                *state = GameState::NotRunning {
                    consecutive_sightings: 0,
                };
                Some(WatchEventKind::Exited)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Test source whose current set of "running" processes can be
    /// rewritten between ticks. The Rc<RefCell<...>> dance lets tests
    /// retain a handle to mutate it while the watcher holds the
    /// trait object.
    #[derive(Clone, Default)]
    struct MockSource {
        running: Rc<RefCell<HashSet<String>>>,
    }

    impl MockSource {
        fn set(&self, names: &[&str]) {
            *self.running.borrow_mut() =
                names.iter().map(|s| s.to_lowercase()).collect();
        }
    }

    impl ProcessSource for MockSource {
        fn current_process_names(&mut self) -> HashSet<String> {
            self.running.borrow().clone()
        }
    }

    fn watcher(source: MockSource, debounce: u32) -> ProcessWatcher<MockSource> {
        ProcessWatcher::new(
            source,
            WatcherConfig {
                launch_debounce_ticks: debounce,
            },
        )
    }

    fn elden_ring() -> WatchedGame {
        WatchedGame::new("elden-ring", ["eldenring.exe"])
    }

    #[test]
    fn no_events_when_no_games_registered() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 2);
        src.set(&["eldenring.exe"]);
        assert!(w.tick().is_empty());
    }

    #[test]
    fn launch_event_fires_after_debounce_consecutive_sightings() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 2);
        w.register(elden_ring());

        // Tick 1: process appears, sighting 1 of 2. No event yet.
        src.set(&["eldenring.exe"]);
        assert!(w.tick().is_empty());

        // Tick 2: still there, sighting 2 of 2 → Launched fires.
        let events = w.tick();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].game_id, "elden-ring");
        assert_eq!(events[0].kind, WatchEventKind::Launched);

        // Tick 3: still running, no further event.
        let events = w.tick();
        assert!(events.is_empty());
    }

    #[test]
    fn launch_debounce_resets_on_a_missing_sighting() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 3);
        w.register(elden_ring());

        // Two sightings, then one gap, then we need three more
        // consecutive sightings to fire.
        src.set(&["eldenring.exe"]);
        w.tick();
        w.tick();
        src.set(&[]);
        w.tick();
        src.set(&["eldenring.exe"]);
        assert!(w.tick().is_empty()); // 1/3
        assert!(w.tick().is_empty()); // 2/3
        let events = w.tick(); // 3/3 → fires
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Launched);
    }

    #[test]
    fn exit_event_fires_immediately_on_first_missing_sighting() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(elden_ring());

        src.set(&["eldenring.exe"]);
        let events = w.tick();
        assert_eq!(events[0].kind, WatchEventKind::Launched);

        src.set(&[]);
        let events = w.tick();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Exited);
    }

    #[test]
    fn launched_then_exited_then_launched_again() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(elden_ring());

        src.set(&["eldenring.exe"]);
        assert_eq!(w.tick()[0].kind, WatchEventKind::Launched);

        src.set(&[]);
        assert_eq!(w.tick()[0].kind, WatchEventKind::Exited);

        src.set(&["eldenring.exe"]);
        assert_eq!(w.tick()[0].kind, WatchEventKind::Launched);
    }

    #[test]
    fn case_insensitive_process_matching() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(WatchedGame::new("game", ["MixedCase.EXE"]));
        // The source sees the OS-reported name, often a different case.
        src.set(&["mixedcase.exe"]);
        let events = w.tick();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Launched);
    }

    #[test]
    fn multiple_process_names_match_any() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(WatchedGame::new(
            "elden-ring",
            ["eldenring.exe", "start_protected_game.exe"],
        ));
        // Only the launcher exe is visible — should still fire
        // Launched.
        src.set(&["start_protected_game.exe"]);
        assert_eq!(w.tick()[0].kind, WatchEventKind::Launched);
    }

    #[test]
    fn paused_game_swallows_events_but_state_still_advances() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(elden_ring());
        w.pause("elden-ring");

        src.set(&["eldenring.exe"]);
        assert!(w.tick().is_empty(), "no Launched event while paused");
        src.set(&[]);
        assert!(w.tick().is_empty(), "no Exited event while paused");

        // After resume, state is back to NotRunning. A fresh launch
        // should fire normally.
        w.resume("elden-ring");
        src.set(&["eldenring.exe"]);
        let events = w.tick();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Launched);
    }

    #[test]
    fn resume_from_paused_running_state_doesnt_double_fire_launched() {
        // If the game went Paused while running, then resumed, we
        // don't want a phantom Launched event — it's still the same
        // run.
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(elden_ring());

        src.set(&["eldenring.exe"]);
        assert_eq!(w.tick()[0].kind, WatchEventKind::Launched);

        w.pause("elden-ring");
        // Process still running.
        w.tick();
        w.tick();
        w.resume("elden-ring");
        // No event — state was already Running.
        assert!(w.tick().is_empty());
    }

    #[test]
    fn unregister_removes_the_game_silently() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(elden_ring());

        src.set(&["eldenring.exe"]);
        w.tick();
        w.unregister("elden-ring");

        src.set(&[]);
        assert!(w.tick().is_empty(), "no events after unregister");
    }

    #[test]
    fn multiple_games_emit_in_parallel() {
        let src = MockSource::default();
        let mut w = watcher(src.clone(), 1);
        w.register(WatchedGame::new("a", ["a.exe"]));
        w.register(WatchedGame::new("b", ["b.exe"]));

        src.set(&["a.exe", "b.exe"]);
        let events = w.tick();
        assert_eq!(events.len(), 2);
        let ids: HashSet<&str> = events.iter().map(|e| e.game_id.as_str()).collect();
        assert!(ids.contains("a"));
        assert!(ids.contains("b"));
    }

    // ---------- integration: real sysinfo ----------

    /// Sanity check that SysinfoSource returns SOMETHING. We always
    /// have at least one process running (the test binary itself).
    #[test]
    fn sysinfo_source_returns_running_processes() {
        let mut src = SysinfoSource::new();
        let names = src.current_process_names();
        assert!(!names.is_empty(), "expected at least one running process");
    }
}
