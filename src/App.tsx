import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Gamepad2, GitBranch, Sparkles, Loader2 } from "lucide-react";

import { Card } from "./ui/Card";
import { ToastProvider } from "./ui/Toast";
import { stagger, transitions } from "./ui/motion";
import { api, type LocalConfig } from "./lib/tauri";
import { WizardShell } from "./wizard/WizardShell";

type AppState =
  | { kind: "loading" }
  | { kind: "needs-onboarding" }
  | { kind: "configured"; config: LocalConfig };

function Home({ config }: { config: LocalConfig }) {
  return (
    <main className="mx-auto min-h-screen w-full max-w-5xl px-8 py-16">
      <motion.header
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.content}
        className="mb-12"
      >
        <div className="mb-3 inline-flex items-center gap-2 rounded-full border border-border bg-surface px-3 py-1 font-mono text-[11px] text-text-3">
          <span className="h-1.5 w-1.5 rounded-full bg-good shadow-[0_0_8px] shadow-good" />
          connected · {config.machine_name}
        </div>
        <h1 className="bg-gradient-to-b from-white to-text-2 bg-clip-text text-6xl font-bold tracking-tight text-transparent">
          SaveSync
        </h1>
        <p className="mt-3 max-w-xl text-lg font-light text-text-2">
          {config.games.length === 0
            ? "No games tracked yet. Add one to start syncing."
            : `Tracking ${config.games.length} game${config.games.length === 1 ? "" : "s"}.`}
        </p>
      </motion.header>

      <motion.section
        variants={stagger.container}
        initial="initial"
        animate="animate"
        className="grid grid-cols-1 gap-4 md:grid-cols-3"
      >
        <motion.div variants={stagger.item}>
          <Card>
            <Gamepad2 className="mb-3 h-5 w-5 text-accent" />
            <div className="text-xs font-medium uppercase tracking-wider text-text-3">
              Tracked games
            </div>
            <div className="mt-1 text-3xl font-semibold">{config.games.length}</div>
            <div className="mt-1 text-xs text-text-3">
              {config.games.length === 0 ? "Add your first one" : "Synced via this repo"}
            </div>
          </Card>
        </motion.div>
        <motion.div variants={stagger.item}>
          <Card>
            <GitBranch className="mb-3 h-5 w-5 text-accent-2" />
            <div className="text-xs font-medium uppercase tracking-wider text-text-3">
              Repo
            </div>
            <div className="mt-1 truncate font-mono text-sm">{config.repo_path}</div>
            <div className="mt-1 text-xs text-text-3">Local clone</div>
          </Card>
        </motion.div>
        <motion.div variants={stagger.item}>
          <Card>
            <Sparkles className="mb-3 h-5 w-5 text-good" />
            <div className="text-xs font-medium uppercase tracking-wider text-text-3">
              Status
            </div>
            <div className="mt-1 text-3xl font-semibold">Idle</div>
            <div className="mt-1 text-xs text-text-3">Watching for game launches</div>
          </Card>
        </motion.div>
      </motion.section>

      {config.games.length > 0 && (
        <motion.section
          variants={stagger.container}
          initial="initial"
          animate="animate"
          className="mt-12"
        >
          <h2 className="mb-4 text-sm font-medium uppercase tracking-wider text-text-3">
            Games
          </h2>
          <div className="space-y-2">
            {config.games.map((game) => (
              <motion.div key={game.id} variants={stagger.item}>
                <Card>
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="font-medium">{game.id}</div>
                      <div className="mt-0.5 truncate font-mono text-xs text-text-3">
                        {game.save_path}
                      </div>
                    </div>
                    <span className="rounded-full bg-good/15 px-3 py-1 font-mono text-[10px] text-good">
                      synced
                    </span>
                  </div>
                </Card>
              </motion.div>
            ))}
          </div>
        </motion.section>
      )}
    </main>
  );
}

function LoadingScreen() {
  return (
    <div className="flex min-h-screen items-center justify-center text-text-3">
      <Loader2 className="h-5 w-5 animate-spin" />
    </div>
  );
}

export default function App() {
  const [state, setState] = useState<AppState>({ kind: "loading" });

  useEffect(() => {
    api
      .getLocalConfig()
      .then((cfg) => {
        if (cfg) setState({ kind: "configured", config: cfg });
        else setState({ kind: "needs-onboarding" });
      })
      .catch(() => {
        // If the bridge isn't ready / typecheck errors, fall through to
        // onboarding rather than blocking the whole UI.
        setState({ kind: "needs-onboarding" });
      });
  }, []);

  return (
    <ToastProvider>
      {state.kind === "loading" && <LoadingScreen />}
      {state.kind === "needs-onboarding" && (
        <WizardShell
          onComplete={(config) => setState({ kind: "configured", config })}
        />
      )}
      {state.kind === "configured" && <Home config={state.config} />}
    </ToastProvider>
  );
}
