import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Gamepad2, Loader2, ChevronLeft, FolderSearch, Sparkles } from "lucide-react";

import { Button } from "../../ui/Button";
import { api, type InstalledGameDto } from "../../lib/tauri";
import { transitions, stagger } from "../../ui/motion";
import type { WizardState } from "../WizardShell";

type Props = {
  state: WizardState;
  setState: (s: WizardState | ((prev: WizardState) => WizardState)) => void;
  onBack: () => void;
  onDone: () => void;
};

type ScanState =
  | { kind: "idle" }
  | { kind: "scanning" }
  | { kind: "ready"; games: InstalledGameDto[] }
  | { kind: "error"; message: string };

export function StepGames({ state, setState, onBack, onDone }: Props) {
  const [scan, setScan] = useState<ScanState>({ kind: "idle" });
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setScan({ kind: "scanning" });
    api
      .scanSteam()
      .then((games) => {
        if (cancelled) return;
        setScan({ kind: "ready", games });
        // Pre-select auto-addable games.
        setPicked(
          new Set(
            games
              .filter((g) => g.is_auto_addable)
              .map((g) => g.ludusavi_name ?? g.steam_display_name),
          ),
        );
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setScan({ kind: "error", message: `${e}` });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  function toggle(id: string) {
    setPicked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function commit() {
    if (scan.kind !== "ready") return;
    setAdding(true);
    try {
      let last = state.config;
      for (const game of scan.games) {
        const id = game.ludusavi_name ?? game.steam_display_name;
        if (!picked.has(id) || !game.resolved_save_path) continue;
        last = await api.addGame({
          gameId: slugify(id),
          savePath: game.resolved_save_path,
        });
      }
      if (last) setState((s) => ({ ...s, config: last }));
      onDone();
    } catch (e: unknown) {
      console.error(e);
    } finally {
      setAdding(false);
    }
  }

  function skip() {
    onDone();
  }

  return (
    <div>
      <div className="mb-1 text-xs font-medium uppercase tracking-wider text-text-3">
        Step 3 of 3
      </div>
      <h2 className="text-2xl font-semibold tracking-tight">Add your games</h2>
      <p className="mt-2 max-w-lg text-sm text-text-2">
        Scanning your Steam library for games we already know the save path
        for. You can always add more later.
      </p>

      {scan.kind === "scanning" && (
        <div className="mt-8 flex items-center gap-3 text-text-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          Scanning Steam library…
        </div>
      )}

      {scan.kind === "error" && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-8 rounded-[var(--radius-card)] border border-warn/30 bg-warn/5 p-5 text-sm text-text-2"
        >
          <div className="mb-1 font-medium text-warn">Couldn't scan Steam</div>
          <div className="text-xs text-text-3">{scan.message}</div>
          <p className="mt-3 text-xs">
            You can still add games one at a time after finishing.
          </p>
        </motion.div>
      )}

      {scan.kind === "ready" && scan.games.length === 0 && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-8 rounded-[var(--radius-card)] border border-border bg-surface p-6 text-center"
        >
          <FolderSearch className="mx-auto mb-3 h-6 w-6 text-text-3" />
          <div className="text-sm font-medium">No Steam games detected</div>
          <div className="mt-1 text-xs text-text-3">
            That's fine — Steam isn't required. Add games manually from the
            main view.
          </div>
        </motion.div>
      )}

      {scan.kind === "ready" && scan.games.length > 0 && (
        <motion.div
          variants={stagger.container}
          initial="initial"
          animate="animate"
          className="mt-6 grid max-h-[420px] grid-cols-1 gap-2 overflow-y-auto pr-2"
        >
          {scan.games.map((game) => {
            const id = game.ludusavi_name ?? game.steam_display_name;
            const isPicked = picked.has(id);
            const canPick = game.is_auto_addable;
            return (
              <motion.button
                key={game.steam_appid}
                variants={stagger.item}
                onClick={() => canPick && toggle(id)}
                disabled={!canPick}
                className={`group flex items-start gap-3 rounded-[var(--radius-card)] border p-3 text-left transition-colors ${
                  !canPick
                    ? "border-border bg-surface opacity-60"
                    : isPicked
                      ? "border-accent/60 bg-accent/10"
                      : "border-border bg-surface hover:border-border-hi"
                }`}
              >
                <div
                  className={`mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded border ${
                    isPicked
                      ? "border-accent bg-accent text-bg-0"
                      : "border-border-hi"
                  }`}
                >
                  {isPicked && <Sparkles className="h-3 w-3" />}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 text-sm font-medium text-text">
                    {game.steam_display_name}
                    {!canPick && (
                      <span className="rounded-full bg-warn/15 px-2 py-0.5 font-mono text-[10px] text-warn">
                        manual setup
                      </span>
                    )}
                  </div>
                  {game.resolved_save_path && (
                    <div className="mt-0.5 truncate font-mono text-[11px] text-text-3">
                      {game.resolved_save_path}
                    </div>
                  )}
                  {!game.resolved_save_path && (
                    <div className="mt-0.5 text-[11px] text-text-3">
                      Save path not in our database — add manually later
                    </div>
                  )}
                </div>
              </motion.button>
            );
          })}
        </motion.div>
      )}

      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.content}
        className="mt-6 flex items-center justify-between gap-3"
      >
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ChevronLeft className="h-4 w-4" />
          Back
        </Button>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="md" onClick={skip}>
            Skip for now
          </Button>
          <Button
            onClick={commit}
            disabled={picked.size === 0 || adding}
            size="md"
          >
            {adding ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Adding
              </>
            ) : (
              <>
                <Gamepad2 className="h-4 w-4" />
                Add {picked.size > 0 ? `${picked.size} ` : ""}
                {picked.size === 1 ? "game" : "games"}
              </>
            )}
          </Button>
        </div>
      </motion.div>
    </div>
  );
}

function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
