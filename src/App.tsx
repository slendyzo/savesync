import { useState } from "react";
import { motion } from "motion/react";
import { Gamepad2, GitBranch, Sparkles, RefreshCw } from "lucide-react";
import { Button } from "./ui/Button";
import { Card } from "./ui/Card";
import { Modal } from "./ui/Modal";
import { ToastProvider, useToast } from "./ui/Toast";
import { stagger, transitions } from "./ui/motion";

function Home() {
  const { push } = useToast();
  const [open, setOpen] = useState(false);

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
          phase 0 · foundation
        </div>
        <h1 className="bg-gradient-to-b from-white to-text-2 bg-clip-text text-6xl font-bold tracking-tight text-transparent">
          SaveSync
        </h1>
        <p className="mt-3 max-w-xl text-lg font-light text-text-2">
          Steam Cloud for any game. Your saves, your repo, every machine.
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
            <div className="mt-1 text-3xl font-semibold">0</div>
            <div className="mt-1 text-xs text-text-3">Add your first one</div>
          </Card>
        </motion.div>
        <motion.div variants={stagger.item}>
          <Card>
            <GitBranch className="mb-3 h-5 w-5 text-accent-2" />
            <div className="text-xs font-medium uppercase tracking-wider text-text-3">
              Backups preserved
            </div>
            <div className="mt-1 text-3xl font-semibold">0</div>
            <div className="mt-1 text-xs text-text-3">Nothing ever lost</div>
          </Card>
        </motion.div>
        <motion.div variants={stagger.item}>
          <Card>
            <Sparkles className="mb-3 h-5 w-5 text-good" />
            <div className="text-xs font-medium uppercase tracking-wider text-text-3">
              Last sync
            </div>
            <div className="mt-1 text-3xl font-semibold">—</div>
            <div className="mt-1 text-xs text-text-3">Standing by</div>
          </Card>
        </motion.div>
      </motion.section>

      <motion.section
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...transitions.content, delay: 0.2 }}
        className="mt-12 flex flex-wrap items-center gap-3"
      >
        <Button onClick={() => setOpen(true)}>
          <Sparkles className="h-4 w-4" />
          Connect GitHub
        </Button>
        <Button
          variant="secondary"
          onClick={() =>
            push({
              kind: "success",
              title: "Synced successfully",
              description: "elden-ring → main • from Desktop-PC",
            })
          }
        >
          <RefreshCw className="h-4 w-4" />
          Trigger sync toast
        </Button>
        <Button
          variant="ghost"
          onClick={() =>
            push({
              kind: "warn",
              title: "Conflict resolved",
              description: "Loser preserved at backup/bg3/ROG-Ally-2026-05-12-2031",
            })
          }
        >
          Trigger warn toast
        </Button>
      </motion.section>

      <Modal open={open} onClose={() => setOpen(false)}>
        <div className="mb-1 text-xs font-medium uppercase tracking-wider text-text-3">
          Step 1 of 3
        </div>
        <h2 className="text-xl font-semibold tracking-tight">Connect GitHub</h2>
        <p className="mt-2 text-sm text-text-2">
          We'll open your browser to authorize SaveSync. Your saves stay in a private
          repo you control — we just need permission to read and write it.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>
            Maybe later
          </Button>
          <Button
            size="sm"
            onClick={() => {
              setOpen(false);
              push({
                kind: "info",
                title: "Device flow not wired up yet",
                description: "Coming in Phase 3 (Onboarding)",
              });
            }}
          >
            Open browser
          </Button>
        </div>
      </Modal>
    </main>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <Home />
    </ToastProvider>
  );
}
