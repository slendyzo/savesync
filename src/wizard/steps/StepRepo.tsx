import { useState } from "react";
import { motion } from "motion/react";
import {
  GitBranch,
  Loader2,
  ChevronLeft,
  Sparkles,
  ExternalLink,
} from "lucide-react";

import { Button } from "../../ui/Button";
import { api } from "../../lib/tauri";
import { transitions } from "../../ui/motion";
import type { WizardState } from "../WizardShell";

type Props = {
  state: WizardState;
  setState: (s: WizardState | ((prev: WizardState) => WizardState)) => void;
  onNext: () => void;
  onBack: () => void;
};

type Mode = "create" | "existing";

export function StepRepo({ state, setState, onNext, onBack }: Props) {
  const isGitHub = state.hostApiBase === "https://api.github.com";
  // GitHub gets the auto-create flow by default. Self-hosted hosts
  // start in "existing" mode since we don't speak their APIs yet.
  const [mode, setMode] = useState<Mode>(isGitHub ? "create" : "existing");
  const [name, setName] = useState("savesync-data");
  const [repoUrl, setRepoUrl] = useState(
    state.user && isGitHub
      ? `https://github.com/${state.user.login}/savesync-data.git`
      : "",
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submitCreate() {
    setSubmitting(true);
    setError(null);
    try {
      const created = await api.githubCreateRepo(name);
      const config = await api.initRepo({
        repoUrl: created.clone_url,
        hostApiBase: state.hostApiBase,
        machineName: defaultMachineName(),
      });
      setState((s) => ({ ...s, config }));
      onNext();
    } catch (e: unknown) {
      const msg = `${e}`;
      // Friendlier message for the most common failure.
      if (msg.includes("name already exists")) {
        setError(
          `A repo called "${name}" already exists on your account. Pick a different name or use "I have one already".`,
        );
      } else {
        setError(msg);
      }
    } finally {
      setSubmitting(false);
    }
  }

  async function submitExisting() {
    setSubmitting(true);
    setError(null);
    try {
      const config = await api.initRepo({
        repoUrl,
        hostApiBase: state.hostApiBase,
        machineName: defaultMachineName(),
      });
      setState((s) => ({ ...s, config }));
      onNext();
    } catch (e: unknown) {
      setError(`${e}`);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div>
      <div className="mb-1 text-xs font-medium uppercase tracking-wider text-text-3">
        Step 2 of 3
      </div>
      <h2 className="text-2xl font-semibold tracking-tight">
        {mode === "create" ? "Create your data repo" : "Connect existing repo"}
      </h2>
      <p className="mt-2 max-w-lg text-sm text-text-2">
        {mode === "create"
          ? "We'll create a private repo on your GitHub to store every save. Only you can see it — it never leaves your account."
          : "Paste the clone URL of an existing repo you want to use for saves."}
      </p>

      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.content}
        className="mt-8 space-y-4"
      >
        {mode === "create" && (
          <div className="glass rounded-[var(--radius-card)] p-5">
            <div className="mb-4 flex items-center gap-3">
              <Sparkles className="h-5 w-5 text-accent" />
              <div>
                <div className="font-semibold">New private repo</div>
                <div className="text-xs text-text-3">
                  Created via the GitHub API with{" "}
                  <code className="text-accent">auto_init: true</code>
                </div>
              </div>
            </div>
            <label className="mb-1 block text-xs font-medium text-text-3">
              Repo name
            </label>
            <div className="mb-3 flex items-center gap-2">
              <span className="font-mono text-xs text-text-3">
                github.com/{state.user?.login ?? "you"}/
              </span>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="savesync-data"
                className="flex-1 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-2 font-mono text-sm text-text outline-none transition-colors focus:border-accent"
                spellCheck={false}
              />
            </div>
            {error && (
              <motion.div
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                className="mb-3 rounded border border-bad/40 bg-bad/10 px-3 py-2 text-xs text-bad"
              >
                {error}
              </motion.div>
            )}
            <div className="flex items-center justify-between gap-3">
              {isGitHub && (
                <button
                  type="button"
                  onClick={() => setMode("existing")}
                  className="text-xs text-text-3 transition-colors hover:text-text-2"
                >
                  I have one already
                </button>
              )}
              <Button
                onClick={submitCreate}
                disabled={!name || submitting}
                size="md"
              >
                {submitting ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Creating
                  </>
                ) : (
                  <>
                    <Sparkles className="h-4 w-4" />
                    Create and clone
                  </>
                )}
              </Button>
            </div>
            <p className="mt-3 text-[11px] text-text-3">
              Token needs <code className="text-accent">repo</code> scope. The
              new repo will be private and visible only to you.
            </p>
          </div>
        )}

        {mode === "existing" && (
          <div className="glass rounded-[var(--radius-card)] p-5">
            <div className="mb-4 flex items-center gap-3">
              <GitBranch className="h-5 w-5 text-accent-2" />
              <div>
                <div className="font-semibold">Existing repo URL</div>
                <div className="text-xs text-text-3">
                  Clone URL ending in <code>.git</code>
                </div>
              </div>
            </div>
            <input
              type="text"
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              placeholder="https://github.com/you/savesync-data.git"
              className="w-full rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-2.5 font-mono text-sm text-text outline-none transition-colors focus:border-accent"
              spellCheck={false}
            />
            {error && (
              <motion.div
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                className="mt-3 rounded border border-bad/40 bg-bad/10 px-3 py-2 text-xs text-bad"
              >
                {error}
              </motion.div>
            )}
            <div className="mt-4 flex items-center justify-between gap-3">
              {isGitHub && (
                <button
                  type="button"
                  onClick={() => setMode("create")}
                  className="inline-flex items-center gap-1 text-xs text-text-3 transition-colors hover:text-text-2"
                >
                  <ExternalLink className="h-3 w-3" />
                  Or have us create one
                </button>
              )}
              {!isGitHub && <span className="text-xs text-text-3">&nbsp;</span>}
              <Button
                onClick={submitExisting}
                disabled={!repoUrl || submitting}
                size="md"
              >
                {submitting ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Cloning
                  </>
                ) : (
                  "Clone and continue"
                )}
              </Button>
            </div>
          </div>
        )}

        <div className="flex justify-start">
          <Button variant="ghost" size="sm" onClick={onBack}>
            <ChevronLeft className="h-4 w-4" />
            Back
          </Button>
        </div>
      </motion.div>
    </div>
  );
}

function defaultMachineName(): string {
  if (typeof navigator === "undefined") return "this-machine";
  const ua = navigator.userAgent;
  if (ua.includes("Mac")) return "Mac";
  if (ua.includes("Windows")) return "PC";
  return "Linux";
}
