import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { Check } from "lucide-react";

import { transitions } from "../ui/motion";
import type { LocalConfig, UserInfo } from "../lib/tauri";

import { StepConnect } from "./steps/StepConnect";
import { StepRepo } from "./steps/StepRepo";
import { StepGames } from "./steps/StepGames";

export type WizardState = {
  user: UserInfo | null;
  hostApiBase: string;
  config: LocalConfig | null;
};

const initialState: WizardState = {
  user: null,
  hostApiBase: "https://api.github.com",
  config: null,
};

type StepDef = { label: string };
const STEPS: StepDef[] = [
  { label: "Connect" },
  { label: "Pick repo" },
  { label: "Add games" },
];

type Props = {
  onComplete: (config: LocalConfig) => void;
};

export function WizardShell({ onComplete }: Props) {
  const [stepIndex, setStepIndex] = useState(0);
  const [state, setState] = useState<WizardState>(initialState);

  const next = () => setStepIndex((i) => Math.min(i + 1, STEPS.length - 1));
  const back = () => setStepIndex((i) => Math.max(i - 1, 0));

  return (
    <div className="mx-auto flex min-h-screen w-full max-w-2xl flex-col px-8 py-12">
      <motion.header
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transitions.content}
        className="mb-10"
      >
        <div className="mb-3 inline-flex items-center gap-2 rounded-full border border-border bg-surface px-3 py-1 font-mono text-[11px] text-text-3">
          <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-[0_0_8px] shadow-accent" />
          welcome
        </div>
        <h1 className="bg-gradient-to-b from-white to-text-2 bg-clip-text text-4xl font-bold tracking-tight text-transparent">
          Set up SaveSync
        </h1>
        <p className="mt-2 text-text-2">
          Three steps and you're syncing. About a minute.
        </p>
      </motion.header>

      <StepIndicator stepIndex={stepIndex} />

      <div className="mt-10 flex-1">
        <AnimatePresence mode="wait">
          <motion.div
            key={stepIndex}
            initial={{ opacity: 0, x: 24 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -24 }}
            transition={transitions.page}
          >
            {stepIndex === 0 && (
              <StepConnect
                state={state}
                setState={setState}
                onNext={next}
              />
            )}
            {stepIndex === 1 && (
              <StepRepo
                state={state}
                setState={setState}
                onNext={next}
                onBack={back}
              />
            )}
            {stepIndex === 2 && (
              <StepGames
                state={state}
                setState={setState}
                onBack={back}
                onDone={() => {
                  if (state.config) onComplete(state.config);
                }}
              />
            )}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

function StepIndicator({ stepIndex }: { stepIndex: number }) {
  return (
    <div className="flex items-center gap-3 text-xs text-text-3">
      {STEPS.map((step, i) => {
        const isDone = i < stepIndex;
        const isCurrent = i === stepIndex;
        return (
          <div key={step.label} className="flex items-center gap-3">
            <div
              className={`flex h-7 w-7 items-center justify-center rounded-full border text-[11px] font-medium transition-colors ${
                isCurrent
                  ? "border-accent text-accent shadow-[0_0_16px_-4px] shadow-accent"
                  : isDone
                    ? "border-good/40 bg-good/10 text-good"
                    : "border-border text-text-3"
              }`}
            >
              {isDone ? <Check className="h-3.5 w-3.5" /> : i + 1}
            </div>
            <span className={isCurrent ? "text-text" : ""}>{step.label}</span>
            {i < STEPS.length - 1 && (
              <span className="mx-1 h-px w-8 bg-border" aria-hidden />
            )}
          </div>
        );
      })}
    </div>
  );
}
