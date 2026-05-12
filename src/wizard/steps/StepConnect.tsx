import { useEffect, useState } from "react";
import { motion } from "motion/react";
import {
  KeyRound,
  ExternalLink,
  Loader2,
  Copy,
  Check,
  ArrowRight,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button } from "../../ui/Button";
import { api, type DeviceCode, type UserInfo } from "../../lib/tauri";
import { transitions } from "../../ui/motion";
import type { WizardState } from "../WizardShell";

type Props = {
  state: WizardState;
  setState: (s: WizardState | ((prev: WizardState) => WizardState)) => void;
  onNext: () => void;
};

type Mode = "github-oauth" | "github-pat" | "other-host";
type OAuthStage =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "awaiting"; code: DeviceCode }
  | { kind: "polling"; code: DeviceCode };

export function StepConnect({ state: _state, setState, onNext }: Props) {
  const [clientId, setClientId] = useState<string | null | undefined>(undefined);
  const [mode, setMode] = useState<Mode>("github-oauth");
  const [oauth, setOauth] = useState<OAuthStage>({ kind: "idle" });
  const [token, setToken] = useState("");
  const [hostUrl, setHostUrl] = useState("https://forgejo.example.org/api/v1");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Detect whether the binary was built with a GitHub OAuth client_id.
  // If not, fall back to PAT-only mode (still works, just less polished).
  useEffect(() => {
    api
      .oauthClientId()
      .then((id) => {
        setClientId(id);
        if (!id) setMode("github-pat");
      })
      .catch(() => {
        setClientId(null);
        setMode("github-pat");
      });
  }, []);

  async function startOauth() {
    if (!clientId) return;
    setError(null);
    setOauth({ kind: "starting" });
    try {
      const code = await api.oauthStart(clientId);
      setOauth({ kind: "awaiting", code });
      await openUrl(code.verification_uri);
      // Poll runs server-side until the user finishes or it expires.
      setOauth({ kind: "polling", code });
      const info: UserInfo = await api.oauthPoll(clientId, code);
      setState((s) => ({
        ...s,
        user: info,
        hostApiBase: "https://api.github.com",
      }));
      onNext();
    } catch (e: unknown) {
      setError(`${e}`);
      setOauth({ kind: "idle" });
    }
  }

  async function copyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard permission denied — user can type the code manually.
    }
  }

  async function submitPat(api_base: string) {
    setSubmitting(true);
    setError(null);
    try {
      const info: UserInfo = await api.patConnect(api_base, token);
      setState((s) => ({ ...s, user: info, hostApiBase: api_base }));
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
        control of it — we just push and pull on your behalf.
      </p>

      {clientId === undefined && (
        <div className="mt-8 flex items-center gap-3 text-text-3">
          <Loader2 className="h-4 w-4 animate-spin" />
          Checking auth options…
        </div>
      )}

      {mode === "github-oauth" && clientId && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={transitions.content}
          className="mt-8"
        >
          {oauth.kind === "idle" && (
            <div className="glass rounded-[var(--radius-card)] p-6">
              <div className="mb-5">
                <div className="text-lg font-semibold">Continue with GitHub</div>
                <div className="mt-1 text-sm text-text-3">
                  Opens GitHub in your browser to authorize SaveSync. No
                  passwords leave this machine.
                </div>
              </div>
              <Button onClick={startOauth} size="lg" className="w-full">
                <ArrowRight className="h-4 w-4" />
                Continue with GitHub
              </Button>
              <div className="mt-4 flex items-center justify-between gap-3 text-xs text-text-3">
                <button
                  type="button"
                  onClick={() => setMode("github-pat")}
                  className="transition-colors hover:text-text-2"
                >
                  Use a personal access token instead
                </button>
                <button
                  type="button"
                  onClick={() => setMode("other-host")}
                  className="transition-colors hover:text-text-2"
                >
                  Other host (Forgejo / GitLab / Gitea)
                </button>
              </div>
              {error && (
                <div className="mt-3 rounded border border-bad/40 bg-bad/10 px-3 py-2 text-xs text-bad">
                  {error}
                </div>
              )}
            </div>
          )}

          {oauth.kind === "starting" && (
            <div className="glass rounded-[var(--radius-card)] p-6 text-center">
              <Loader2 className="mx-auto mb-3 h-5 w-5 animate-spin text-accent" />
              <div className="text-sm text-text-2">Requesting a code from GitHub…</div>
            </div>
          )}

          {(oauth.kind === "awaiting" || oauth.kind === "polling") && (
            <div className="glass rounded-[var(--radius-card)] p-6">
              <div className="mb-1 text-xs font-medium uppercase tracking-wider text-text-3">
                Your one-time code
              </div>
              <button
                type="button"
                onClick={() => copyCode(oauth.code.user_code)}
                className="group mt-2 flex w-full items-center justify-between gap-3 rounded-[var(--radius-button)] border border-border-hi bg-bg-1 px-5 py-4 transition-colors hover:border-accent"
              >
                <code className="font-mono text-3xl font-semibold tracking-widest text-text">
                  {oauth.code.user_code}
                </code>
                <span className="flex items-center gap-1.5 text-xs text-text-3 transition-colors group-hover:text-accent">
                  {copied ? (
                    <>
                      <Check className="h-3.5 w-3.5 text-good" />
                      Copied
                    </>
                  ) : (
                    <>
                      <Copy className="h-3.5 w-3.5" />
                      Copy
                    </>
                  )}
                </span>
              </button>

              <div className="mt-5 space-y-2 text-sm text-text-2">
                <div className="flex items-start gap-3">
                  <span className="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-surface-hi text-[11px] font-medium text-accent">
                    1
                  </span>
                  <span>
                    GitHub opened in your browser at{" "}
                    <code className="text-accent">github.com/login/device</code>
                  </span>
                </div>
                <div className="flex items-start gap-3">
                  <span className="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-surface-hi text-[11px] font-medium text-accent">
                    2
                  </span>
                  <span>Paste the code above and approve SaveSync</span>
                </div>
                <div className="flex items-start gap-3">
                  <span className="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-surface-hi text-[11px] font-medium text-accent">
                    3
                  </span>
                  <span className="flex items-center gap-2">
                    Come back here
                    <Loader2 className="h-3 w-3 animate-spin text-accent" />
                  </span>
                </div>
              </div>

              <div className="mt-5 flex items-center justify-between gap-3">
                <button
                  type="button"
                  onClick={() => openUrl(oauth.code.verification_uri)}
                  className="inline-flex items-center gap-1.5 text-xs text-accent hover:underline"
                >
                  <ExternalLink className="h-3 w-3" />
                  Reopen GitHub
                </button>
                <button
                  type="button"
                  onClick={() => setOauth({ kind: "idle" })}
                  className="text-xs text-text-3 transition-colors hover:text-text-2"
                >
                  Cancel
                </button>
              </div>
              {error && (
                <div className="mt-3 rounded border border-bad/40 bg-bad/10 px-3 py-2 text-xs text-bad">
                  {error}
                </div>
              )}
            </div>
          )}
        </motion.div>
      )}

      {mode === "github-pat" && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={transitions.content}
          className="mt-8 space-y-4"
        >
          <div className="glass rounded-[var(--radius-card)] p-5">
            <div className="mb-4">
              <div className="font-semibold">GitHub personal access token</div>
              <div className="text-xs text-text-3">
                Token with <code className="text-accent">repo</code> scope
              </div>
            </div>
            <a
              href="https://github.com/settings/tokens/new?scopes=repo&description=SaveSync"
              target="_blank"
              rel="noreferrer"
              onClick={(e) => {
                e.preventDefault();
                openUrl(
                  "https://github.com/settings/tokens/new?scopes=repo&description=SaveSync",
                );
              }}
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
              <div className="flex items-center gap-3 text-xs text-text-3">
                {clientId && (
                  <button
                    type="button"
                    onClick={() => setMode("github-oauth")}
                    className="transition-colors hover:text-text-2"
                  >
                    ← Back to "Continue with GitHub"
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setMode("other-host")}
                  className="transition-colors hover:text-text-2"
                >
                  Other host
                </button>
              </div>
              <Button
                onClick={() => submitPat("https://api.github.com")}
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
                Forgejo, Gitea, GitLab — anything with a <code>/user</code> endpoint
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
                onClick={() => setMode(clientId ? "github-oauth" : "github-pat")}
                className="text-xs text-text-3 transition-colors hover:text-text-2"
              >
                ← Back to GitHub
              </button>
              <Button
                onClick={() => submitPat(hostUrl)}
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
