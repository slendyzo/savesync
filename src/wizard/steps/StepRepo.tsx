import { useState } from "react";
import { motion } from "motion/react";
import { GitBranch, Loader2, ChevronLeft } from "lucide-react";

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

export function StepRepo({ state, setState, onNext, onBack }: Props) {
  const defaultUrl =
    state.user && state.hostApiBase === "https://api.github.com"
      ? `https://github.com/${state.user.login}/savesync-data.git`
      : "";
  const [repoUrl, setRepoUrl] = useState(defaultUrl);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
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
        Point at your data repo
      </h2>
      <p className="mt-2 max-w-lg text-sm text-text-2">
        We'll clone this repo locally and use it as the storage for every save
        you sync. If the repo doesn't have any data yet, we'll initialize it
        for you.
      </p>

      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.content}
        className="mt-8 space-y-4"
      >
        <div className="glass rounded-[var(--radius-card)] p-5">
          <div className="mb-4 flex items-center gap-3">
            <GitBranch className="h-5 w-5 text-accent-2" />
            <div>
              <div className="font-semibold">Repository URL</div>
              <div className="text-xs text-text-3">
                Create a private repo on your host called <code>savesync-data</code>{" "}
                (or use any existing one) and paste its clone URL here.
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
            <Button variant="ghost" size="sm" onClick={onBack}>
              <ChevronLeft className="h-4 w-4" />
              Back
            </Button>
            <Button onClick={submit} disabled={!repoUrl || submitting} size="md">
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
