import { useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import {
  X,
  Cpu,
  GitBranch,
  Pencil,
  Check,
  Loader2,
  AlertTriangle,
  FolderOpen,
} from "lucide-react";

import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Button } from "../ui/Button";
import { useToast } from "../ui/Toast";
import { transitions } from "../ui/motion";
import { api, type LocalConfig, type Preferences } from "../lib/tauri";

type Props = {
  open: boolean;
  config: LocalConfig;
  onClose: () => void;
  onConfigChange: (config: LocalConfig) => void;
  onDisconnect: () => void;
};

export function SettingsModal({
  open,
  config,
  onClose,
  onConfigChange,
  onDisconnect,
}: Props) {
  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={transitions.short}
            onClick={onClose}
            className="fixed inset-0 z-40 bg-bg-0/70 backdrop-blur-sm"
          />
          <motion.div
            initial={{ opacity: 0, y: 16, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.98 }}
            transition={transitions.content}
            className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-6"
          >
            <div
              onClick={(e) => e.stopPropagation()}
              className="glass-hi pointer-events-auto flex max-h-[80vh] w-full max-w-lg flex-col overflow-hidden rounded-[var(--radius-modal)] shadow-2xl"
            >
              <SettingsBody
                config={config}
                onClose={onClose}
                onConfigChange={onConfigChange}
                onDisconnect={onDisconnect}
              />
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}

function SettingsBody({
  config,
  onClose,
  onConfigChange,
  onDisconnect,
}: Omit<Props, "open">) {
  const { push } = useToast();

  const [machineName, setMachineName] = useState(config.machine_name);
  const [renaming, setRenaming] = useState(false);
  const [prefs, setPrefs] = useState<Preferences>(config.preferences);
  const [saving, setSaving] = useState(false);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);

  const dirty =
    prefs.polling_interval_seconds !== config.preferences.polling_interval_seconds ||
    prefs.lfs_threshold_mb !== config.preferences.lfs_threshold_mb ||
    prefs.sync_on_startup !== config.preferences.sync_on_startup;

  async function commitRename() {
    try {
      const cfg = await api.renameMachine(machineName.trim());
      onConfigChange(cfg);
      setRenaming(false);
      push({
        kind: "success",
        title: "Renamed",
        description: `This machine is now "${cfg.machine_name}"`,
      });
    } catch (e: unknown) {
      push({ kind: "error", title: "Rename failed", description: `${e}` });
    }
  }

  async function savePrefs() {
    setSaving(true);
    try {
      const cfg = await api.updatePreferences(prefs);
      onConfigChange(cfg);
      push({ kind: "success", title: "Settings saved" });
    } catch (e: unknown) {
      push({ kind: "error", title: "Couldn't save", description: `${e}` });
    } finally {
      setSaving(false);
    }
  }

  async function disconnect() {
    try {
      await api.disconnectMachine();
      onDisconnect();
    } catch (e: unknown) {
      push({ kind: "error", title: "Couldn't disconnect", description: `${e}` });
    }
  }

  return (
    <>
      <header className="flex items-center justify-between border-b border-border p-5">
        <div>
          <div className="font-semibold text-text">Settings</div>
          <div className="text-xs text-text-3">
            Machine, preferences, and the disconnect button.
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="-mr-1 rounded-full p-1.5 text-text-3 transition-colors hover:bg-surface hover:text-text"
          aria-label="Close settings"
        >
          <X className="h-4 w-4" />
        </button>
      </header>

      <div className="flex-1 space-y-7 overflow-y-auto p-5">
        <section>
          <SectionTitle icon={<Cpu className="h-3 w-3" />}>This machine</SectionTitle>
          <div className="space-y-2">
            <Row label="Name">
              {renaming ? (
                <div className="flex items-center gap-2">
                  <input
                    autoFocus
                    value={machineName}
                    onChange={(e) => setMachineName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") commitRename();
                      if (e.key === "Escape") {
                        setMachineName(config.machine_name);
                        setRenaming(false);
                      }
                    }}
                    className="flex-1 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-1.5 text-sm text-text outline-none focus:border-accent"
                  />
                  <button
                    type="button"
                    onClick={commitRename}
                    className="rounded-full p-1.5 text-good hover:bg-surface"
                  >
                    <Check className="h-4 w-4" />
                  </button>
                </div>
              ) : (
                <div className="flex items-center justify-between gap-2">
                  <span className="text-sm">{config.machine_name}</span>
                  <button
                    type="button"
                    onClick={() => setRenaming(true)}
                    className="rounded-full p-1 text-text-3 hover:bg-surface hover:text-text-2"
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </button>
                </div>
              )}
            </Row>
            <Row label="Hostname">
              <span className="font-mono text-xs text-text-2">{config.hostname}</span>
            </Row>
            <Row label="Platform">
              <span className="font-mono text-xs text-text-2">
                {config.platform}
              </span>
            </Row>
            <Row label="Machine ID">
              <span className="truncate font-mono text-[10px] text-text-3">
                {config.machine_id}
              </span>
            </Row>
          </div>
        </section>

        <section>
          <SectionTitle icon={<GitBranch className="h-3 w-3" />}>Repo</SectionTitle>
          <div className="rounded-[var(--radius-button)] border border-border bg-surface px-3 py-2 font-mono text-xs text-text-2">
            <div className="truncate">{config.repo_path}</div>
            <button
              type="button"
              onClick={async () => {
                try {
                  await revealItemInDir(config.repo_path);
                } catch (e: unknown) {
                  push({
                    kind: "error",
                    title: "Couldn't open folder",
                    description: `${e}`,
                  });
                }
              }}
              className="mt-2 inline-flex items-center gap-1.5 text-[11px] text-accent hover:underline"
            >
              <FolderOpen className="h-3 w-3" />
              Reveal in file manager
            </button>
          </div>
        </section>

        <section>
          <SectionTitle icon={<Cpu className="h-3 w-3" />}>Sync preferences</SectionTitle>
          <div className="space-y-3">
            <PrefField
              label="Polling interval (seconds)"
              hint="How often the watcher samples the process list. Lower = more responsive, higher = lower CPU."
            >
              <input
                type="number"
                min={1}
                max={60}
                value={prefs.polling_interval_seconds}
                onChange={(e) =>
                  setPrefs((p) => ({
                    ...p,
                    polling_interval_seconds: Math.max(1, Number(e.target.value)),
                  }))
                }
                className="w-20 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-1.5 text-right text-sm text-text outline-none focus:border-accent"
              />
            </PrefField>
            <PrefField
              label="LFS threshold (MB)"
              hint="Files above this size are auto-routed through Git LFS so the repo stays small."
            >
              <input
                type="number"
                min={1}
                max={5000}
                value={prefs.lfs_threshold_mb}
                onChange={(e) =>
                  setPrefs((p) => ({
                    ...p,
                    lfs_threshold_mb: Math.max(1, Number(e.target.value)),
                  }))
                }
                className="w-20 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-1.5 text-right text-sm text-text outline-none focus:border-accent"
              />
            </PrefField>
            <PrefField
              label="Start syncing on launch"
              hint="When off, the watcher starts paused. You can resume from any game's detail panel."
            >
              <Toggle
                checked={prefs.sync_on_startup}
                onChange={(v) => setPrefs((p) => ({ ...p, sync_on_startup: v }))}
              />
            </PrefField>
          </div>
          <div className="mt-4 flex justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setPrefs(config.preferences)}
              disabled={!dirty || saving}
            >
              Reset
            </Button>
            <Button size="sm" onClick={savePrefs} disabled={!dirty || saving}>
              {saving ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Saving
                </>
              ) : (
                "Save preferences"
              )}
            </Button>
          </div>
        </section>

        <section>
          <SectionTitle icon={<AlertTriangle className="h-3 w-3 text-bad" />}>
            Danger zone
          </SectionTitle>
          {confirmDisconnect ? (
            <div className="rounded-[var(--radius-card)] border border-bad/40 bg-bad/10 p-3">
              <div className="text-sm font-medium text-bad">
                Disconnect this machine?
              </div>
              <div className="mt-1 text-xs text-text-2">
                Removes the local config and all stored credentials. Your game
                saves and the git repo stay where they are. The next launch
                will show the onboarding wizard.
              </div>
              <div className="mt-3 flex items-center justify-end gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setConfirmDisconnect(false)}
                >
                  Cancel
                </Button>
                <Button variant="danger" size="sm" onClick={disconnect}>
                  Disconnect
                </Button>
              </div>
            </div>
          ) : (
            <Button
              variant="danger"
              size="sm"
              onClick={() => setConfirmDisconnect(true)}
            >
              <AlertTriangle className="h-4 w-4" />
              Disconnect this machine
            </Button>
          )}
        </section>
      </div>
    </>
  );
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

function Row({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-[var(--radius-button)] border border-border bg-surface px-3 py-2">
      <span className="text-[11px] uppercase tracking-wider text-text-3">
        {label}
      </span>
      <div className="min-w-0 max-w-[70%] text-right">{children}</div>
    </div>
  );
}

function PrefField({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-[var(--radius-button)] border border-border bg-surface px-3 py-3">
      <div className="min-w-0 flex-1 pr-3">
        <div className="text-sm font-medium text-text">{label}</div>
        <div className="mt-0.5 text-[11px] text-text-3">{hint}</div>
      </div>
      <div className="flex-shrink-0">{children}</div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
        checked ? "bg-accent" : "bg-surface-hi"
      }`}
      role="switch"
      aria-checked={checked}
    >
      <motion.span
        layout
        transition={transitions.spring}
        className={`inline-block h-4 w-4 transform rounded-full bg-white shadow ${
          checked ? "translate-x-[18px]" : "translate-x-[2px]"
        }`}
      />
    </button>
  );
}
