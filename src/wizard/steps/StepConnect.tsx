import { useState } from "react";
import { motion } from "motion/react";
import { KeyRound, ExternalLink, Loader2 } from "lucide-react";

import { Button } from "../../ui/Button";
import { api, type UserInfo } from "../../lib/tauri";
import { transitions } from "../../ui/motion";
import type { WizardState } from "../WizardShell";

type Props = {
  state: WizardState;
  setState: (s: WizardState | ((prev: WizardState) => WizardState)) => void;
  onNext: () => void;
};

type Mode = "github-pat" | "github-pat-active" | "other-host";

export function StepConnect({ state: _state, setState, onNext }: Props) {
  const [mode, setMode] = useState<Mode>("github-pat");
  const [token, setToken] = useState("");
  const [hostUrl, setHostUrl] = useState("https://forgejo.example.org/api/v1");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submitGithub() {
    setSubmitting(true);
    setError(null);
    try {
      const info: UserInfo = await api.patConnect("https://api.github.com", token);
      setState((s) => ({ ...s, user: info, hostApiBase: "https://api.github.com" }));
      onNext();
    } catch (e: unknown) {
      setError(`${e}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function submitOther() {
    setSubmitting(true);
    setError(null);
    try {
      const info: UserInfo = await api.patConnect(hostUrl, token);
      setState((s) => ({ ...s, user: info, hostApiBase: hostUrl }));
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
        Step 1 of 3
      </div>
      <h2 className="text-2xl font-semibold tracking-tight">Connect a git host</h2>
      <p className="mt-2 max-w-lg text-sm text-text-2">
        SaveSync needs a private repo to store your save data. You stay in
        control of it — we just push and pull on your behalf. Paste a personal
        access token below.
      </p>

      {mode === "github-pat" && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={transitions.content}
          className="mt-8 space-y-4"
        >
          <div className="glass rounded-[var(--radius-card)] p-5">
            <div className="mb-4">
              <div className="font-semibold">GitHub</div>
              <div className="text-xs text-text-3">
                Personal access token with{" "}
                <code className="text-accent">repo</code> scope
              </div>
            </div>
            <a
              href="https://github.com/settings/tokens/new?scopes=repo&description=SaveSync"
              target="_blank"
              rel="noreferrer"
              className="mb-3 inline-flex items-center gap-1.5 text-xs text-accent hover:underline"
            >
              <ExternalLink className="h-3 w-3" />
              Generate a new token on GitHub
            </a>
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="ghp_..."
              className="w-full rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-2.5 font-mono text-sm text-text outline-none transition-colors focus:border-accent"
              autoComplete="off"
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
              <button
                type="button"
                onClick={() => setMode("other-host")}
                className="text-xs text-text-3 transition-colors hover:text-text-2"
              >
                Use a different host (Forgejo, GitLab, Gitea…)
              </button>
              <Button
                onClick={submitGithub}
                disabled={!token || submitting}
                size="md"
              >
                {submitting ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Verifying
                  </>
                ) : (
                  <>
                    <KeyRound className="h-4 w-4" />
                    Connect
                  </>
                )}
              </Button>
            </div>
          </div>

          <p className="text-xs text-text-3">
            We store the token in your OS keychain ({osLabel()}). It never
            leaves this machine.
          </p>
        </motion.div>
      )}

      {mode === "other-host" && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={transitions.content}
          className="mt-8 space-y-4"
        >
          <div className="glass rounded-[var(--radius-card)] p-5">
            <div className="mb-4">
              <div className="font-semibold">Self-hosted git host</div>
              <div className="text-xs text-text-3">
                Forgejo, Gitea, GitLab — anything with a `/user` endpoint
              </div>
            </div>
            <label className="mb-1 block text-xs font-medium text-text-3">
              API base URL
            </label>
            <input
              type="text"
              value={hostUrl}
              onChange={(e) => setHostUrl(e.target.value)}
              placeholder="https://forgejo.example.org/api/v1"
              className="mb-3 w-full rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-2.5 font-mono text-sm text-text outline-none transition-colors focus:border-accent"
              spellCheck={false}
            />
            <label className="mb-1 block text-xs font-medium text-text-3">
              Personal access token
            </label>
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="token..."
              className="w-full rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-3 py-2.5 font-mono text-sm text-text outline-none transition-colors focus:border-accent"
              autoComplete="off"
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
              <button
                type="button"
                onClick={() => setMode("github-pat")}
                className="text-xs text-text-3 transition-colors hover:text-text-2"
              >
                Back to GitHub
              </button>
              <Button
                onClick={submitOther}
                disabled={!token || !hostUrl || submitting}
                size="md"
              >
                {submitting ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Verifying
                  </>
                ) : (
                  "Connect"
                )}
              </Button>
            </div>
          </div>
        </motion.div>
      )}
    </div>
  );
}

function osLabel(): string {
  const ua = typeof navigator !== "undefined" ? navigator.userAgent : "";
  if (ua.includes("Mac")) return "Keychain";
  if (ua.includes("Windows")) return "Credential Manager";
  return "Secret Service";
}
