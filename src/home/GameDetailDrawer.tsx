import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  X,
  FolderOpen,
  ArrowDownToLine,
  ArrowUpFromLine,
  Pause,
  Play,
  Pencil,
  Check,
  Trash2,
  Loader2,
  GitBranch,
  History,
  AlertTriangle,
} from "lucide-react";

import { Button } from "../ui/Button";
import { useToast } from "../ui/Toast";
import { transitions, stagger } from "../ui/motion";
import {
  api,
  type CommitInfo,
  type LocalConfig,
  type LocalGame,
} from "../lib/tauri";

type Props = {
  game: LocalGame | null;
  onClose: () => void;
  onConfigChange: (config: LocalConfig) => void;
};

export function GameDetailDrawer({ game, onClose, onConfigChange }: Props) {
  return (
    <AnimatePresence>
      {game && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={transitions.short}
            onClick={onClose}
            className="fixed inset-0 z-40 bg-bg-0/70 backdrop-blur-sm"
          />
          <motion.aside
            key={game.id}
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%" }}
            transition={transitions.page}
            className="glass-hi fixed right-0 top-0 z-50 flex h-full w-full max-w-md flex-col overflow-y-auto border-l border-border-hi shadow-2xl"
          >
            <DrawerBody
              game={game}
              onClose={onClose}
              onConfigChange={onConfigChange}
            />
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  );
}

function DrawerBody({ game, onClose, onConfigChange }: Props & { game: LocalGame }) {
  const { push } = useToast();
  const [commits, setCommits] = useState<CommitInfo[] | null>(null);
  const [backups, setBackups] = useState<string[] | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState(
    game.display_name ?? prettify(game.id),
  );
  const [working, setWorking] = useState<null | "push" | "pull">(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const reloadLists = useCallback(async () => {
    const [c, b] = await Promise.allSettled([
      api.listGameCommits(game.id, 20),
      api.listGameBackups(game.id),
    ]);
    if (c.status === "fulfilled") setCommits(c.value);
    if (b.status === "fulfilled") setBackups(b.value);
  }, [game.id]);

  useEffect(() => {
    reloadLists();
  }, [reloadLists]);

  async function handleForcePush() {
    setWorking("push");
    try {
      const r = await api.forcePush(game.id);
      push({
        kind: r.committed ? "success" : "info",
        title: r.committed ? "Pushed" : "No changes",
        description: r.commit_message ?? "Save folder matches the repo",
      });
      reloadLists();
    } catch (e: unknown) {
      push({ kind: "error", title: "Push failed", description: `${e}` });
    } finally {
      setWorking(null);
    }
  }

  async function handleForcePull() {
    setWorking("pull");
    try {
      const r = await api.forcePull(game.id);
      push({
        kind: r.fast_forwarded ? "success" : "info",
        title: r.fast_forwarded ? "Pulled" : "Already up to date",
        description: r.fast_forwarded
          ? `${r.files_synced} file(s) synced`
          : "Save folder already matches the repo",
      });
      reloadLists();
    } catch (e: unknown) {
      push({ kind: "error", title: "Pull failed", description: `${e}` });
    } finally {
      setWorking(null);
    }
  }

  async function togglePaused() {
    try {
      const cfg = await api.setGamePaused(game.id, !game.paused);
      onConfigChange(cfg);
      push({
        kind: "info",
        title: game.paused ? "Resumed" : "Paused",
        description: game.paused
          ? "Auto-sync re-enabled for this game"
          : "Auto-sync paused — game will be ignored on launch/exit",
      });
    } catch (e: unknown) {
      push({ kind: "error", title: "Couldn't update", description: `${e}` });
    }
  }

  async function commitRename() {
    try {
      const cfg = await api.renameGame({
        gameId: game.id,
        displayName: renameValue.trim() || null,
      });
      onConfigChange(cfg);
      setRenaming(false);
    } catch (e: unknown) {
      push({ kind: "error", title: "Rename failed", description: `${e}` });
    }
  }

  async function handleOpenFolder() {
    try {
      await api.openSaveFolder(game.id);
    } catch (e: unknown) {
      push({ kind: "error", title: "Couldn't open folder", description: `${e}` });
    }
  }

  async function handleRemove() {
    try {
      const cfg = await api.removeGame(game.id);
      onConfigChange(cfg);
      onClose();
    } catch (e: unknown) {
      push({ kind: "error", title: "Couldn't remove", description: `${e}` });
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="sticky top-0 z-10 flex items-start justify-between gap-3 border-b border-border bg-bg-1/80 p-6 backdrop-blur-md">
        <div className="min-w-0 flex-1">
          <div className="mb-1 inline-flex items-center gap-2 rounded-full border border-border bg-surface px-2 py-0.5 font-mono text-[10px] text-text-3">
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                game.paused ? "bg-text-3" : "bg-good shadow-[0_0_6px] shadow-good"
              }`}
            />
            {game.paused ? "paused" : "idle · watching"}
          </div>
          {renaming ? (
            <div className="flex items-center gap-2">
              <input
                autoFocus
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") setRenaming(false);
                }}
                className="flex-1 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-1.5 text-lg font-semibold text-text outline-none focus:border-accent"
              />
              <button
                type="button"
                onClick={commitRename}
                className="rounded-full p-1.5 text-good hover:bg-surface"
                aria-label="Save name"
              >
                <Check className="h-4 w-4" />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <h2 className="truncate text-2xl font-semibold tracking-tight">
                {game.display_name ?? prettify(game.id)}
              </h2>
              <button
                type="button"
                onClick={() => setRenaming(true)}
                className="rounded-full p-1.5 text-text-3 transition-colors hover:bg-surface hover:text-text-2"
                aria-label="Rename"
              >
                <Pencil className="h-3.5 w-3.5" />
              </button>
            </div>
          )}
          <div className="mt-1 truncate font-mono text-[11px] text-text-3">
            {game.save_path}
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="-mr-2 -mt-2 rounded-full p-2 text-text-3 transition-colors hover:bg-surface hover:text-text"
          aria-label="Close drawer"
        >
          <X className="h-4 w-4" />
        </button>
      </header>

      <div className="flex-1 space-y-7 p-6">
        <Section>
          <SectionTitle icon={<ArrowUpFromLine className="h-3 w-3" />}>
            Sync actions
          </SectionTitle>
          <div className="grid grid-cols-2 gap-2">
            <Button
              variant="secondary"
              size="md"
              onClick={handleForcePull}
              disabled={working !== null}
            >
              {working === "pull" ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ArrowDownToLine className="h-4 w-4" />
              )}
              Force pull
            </Button>
            <Button
              size="md"
              onClick={handleForcePush}
              disabled={working !== null}
            >
              {working === "push" ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ArrowUpFromLine className="h-4 w-4" />
              )}
              Force push
            </Button>
          </div>
          <div className="mt-2 flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={handleOpenFolder}
              className="flex-1 justify-start"
            >
              <FolderOpen className="h-4 w-4" />
              Open save folder
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={togglePaused}
              className="flex-1 justify-start"
            >
              {game.paused ? (
                <>
                  <Play className="h-4 w-4" />
                  Resume sync
                </>
              ) : (
                <>
                  <Pause className="h-4 w-4" />
                  Pause sync
                </>
              )}
            </Button>
          </div>
        </Section>

        <Section>
          <SectionTitle icon={<History className="h-3 w-3" />}>
            Recent commits
          </SectionTitle>
          {commits === null ? (
            <SectionLoading />
          ) : commits.length === 0 ? (
            <EmptyHint>No syncs yet for this game.</EmptyHint>
          ) : (
            <motion.ul
              variants={stagger.container}
              initial="initial"
              animate="animate"
              className="space-y-1.5"
            >
              {commits.map((c) => (
                <motion.li
                  key={c.oid}
                  variants={stagger.item}
                  className="glass rounded-[var(--radius-button)] px-3 py-2"
                >
                  <div className="flex items-baseline justify-between gap-3">
                    <div className="min-w-0 flex-1 truncate text-sm text-text">
                      {c.summary}
                    </div>
                    <div className="flex-shrink-0 font-mono text-[10px] text-text-3">
                      {relativeTime(c.timestamp)}
                    </div>
                  </div>
                  <div className="mt-0.5 truncate font-mono text-[10px] text-text-3">
                    {c.author_name} · {c.oid.slice(0, 7)}
                  </div>
                </motion.li>
              ))}
            </motion.ul>
          )}
        </Section>

        <Section>
          <SectionTitle icon={<GitBranch className="h-3 w-3" />}>
            Backup branches
          </SectionTitle>
          {backups === null ? (
            <SectionLoading />
          ) : backups.length === 0 ? (
            <EmptyHint>No conflict backups — nothing's been overwritten.</EmptyHint>
          ) : (
            <ul className="space-y-1.5">
              {backups.map((branch) => (
                <li
                  key={branch}
                  className="glass rounded-[var(--radius-button)] px-3 py-2 font-mono text-[11px] text-text-2"
                >
                  {branch}
                </li>
              ))}
            </ul>
          )}
          {backups && backups.length > 0 && (
            <p className="mt-2 text-[11px] text-text-3">
              Restore-from-backup ships in a follow-up. For now you can
              <code className="mx-1 text-accent">git checkout</code>
              the branch in the repo manually.
            </p>
          )}
        </Section>

        <Section>
          <SectionTitle icon={<AlertTriangle className="h-3 w-3 text-bad" />}>
            Danger zone
          </SectionTitle>
          {confirmDelete ? (
            <div className="rounded-[var(--radius-card)] border border-bad/40 bg-bad/10 p-3">
              <div className="text-sm font-medium text-bad">Remove this game?</div>
              <div className="mt-1 text-xs text-text-2">
                Stops syncing this game. Your existing save folder and the
                repo's data folder are untouched — you can re-add the game
                later.
              </div>
              <div className="mt-3 flex items-center justify-end gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setConfirmDelete(false)}
                >
                  Cancel
                </Button>
                <Button variant="danger" size="sm" onClick={handleRemove}>
                  Remove
                </Button>
              </div>
            </div>
          ) : (
            <Button
              variant="danger"
              size="sm"
              onClick={() => setConfirmDelete(true)}
            >
              <Trash2 className="h-4 w-4" />
              Remove from SaveSync
            </Button>
          )}
        </Section>
      </div>
    </div>
  );
}

function Section({ children }: { children: React.ReactNode }) {
  return <section>{children}</section>;
}

function SectionTitle({
  children,
  icon,
}: {
  children: React.ReactNode;
  icon: React.ReactNode;
}) {
  return (
    <h3 className="mb-2 flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-[0.14em] text-text-3">
      {icon}
      {children}
    </h3>
  );
}

function SectionLoading() {
  return (
    <div className="flex items-center gap-2 text-xs text-text-3">
      <Loader2 className="h-3 w-3 animate-spin" />
      Loading…
    </div>
  );
}

function EmptyHint({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-[var(--radius-button)] border border-border bg-surface px-3 py-2 text-xs text-text-3">
      {children}
    </div>
  );
}

function prettify(id: string): string {
  return id
    .split("-")
    .map((w) => (w ? w[0].toUpperCase() + w.slice(1) : ""))
    .join(" ");
}

function relativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}d`;
  return new Date(unixSeconds * 1000).toLocaleDateString();
}
