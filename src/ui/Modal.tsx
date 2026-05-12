import { AnimatePresence, motion } from "motion/react";
import { useEffect, type ReactNode } from "react";
import { cn } from "../lib/cn";
import { transitions } from "./motion";

type ModalProps = {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
};

export function Modal({ open, onClose, children, className }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

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
            className="fixed inset-0 z-50 bg-bg-0/70 backdrop-blur-sm"
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
              className={cn(
                "glass-hi pointer-events-auto w-full max-w-md rounded-[var(--radius-modal)] p-6 shadow-2xl",
                className,
              )}
            >
              {children}
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
