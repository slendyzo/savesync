import { AnimatePresence, motion } from "motion/react";
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { CheckCircle2, AlertCircle, AlertTriangle, Info } from "lucide-react";
import { cn } from "../lib/cn";
import { transitions } from "./motion";

type ToastKind = "success" | "error" | "warn" | "info";
type Toast = { id: string; kind: ToastKind; title: string; description?: string };

type ToastContextValue = {
  push: (t: Omit<Toast, "id">) => void;
};

const ToastContext = createContext<ToastContextValue | null>(null);

export function useToast() {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside <ToastProvider>");
  return ctx;
}

const kindStyle: Record<ToastKind, { ring: string; icon: ReactNode }> = {
  success: { ring: "border-good/40", icon: <CheckCircle2 className="h-4 w-4 text-good" /> },
  error: { ring: "border-bad/40", icon: <AlertCircle className="h-4 w-4 text-bad" /> },
  warn: { ring: "border-warn/40", icon: <AlertTriangle className="h-4 w-4 text-warn" /> },
  info: { ring: "border-accent/40", icon: <Info className="h-4 w-4 text-accent" /> },
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const push = useCallback<ToastContextValue["push"]>((t) => {
    const id = crypto.randomUUID();
    setToasts((prev) => [...prev, { ...t, id }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((x) => x.id !== id));
    }, 4500);
  }, []);

  const value = useMemo(() => ({ push }), [push]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed bottom-6 right-6 z-[60] flex w-full max-w-sm flex-col gap-2">
        <AnimatePresence>
          {toasts.map((t) => (
            <motion.div
              key={t.id}
              initial={{ opacity: 0, y: 12, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, transition: transitions.short }}
              transition={transitions.content}
              className={cn(
                "glass-hi pointer-events-auto flex items-start gap-3 rounded-[var(--radius-card)] border p-3 shadow-xl",
                kindStyle[t.kind].ring,
              )}
            >
              <div className="mt-0.5">{kindStyle[t.kind].icon}</div>
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-text">{t.title}</div>
                {t.description && (
                  <div className="mt-0.5 text-xs text-text-2">{t.description}</div>
                )}
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
}
