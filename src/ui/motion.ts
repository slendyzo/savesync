import type { Transition } from "motion/react";

export const ease = {
  smooth: [0.16, 1, 0.3, 1] as const,
  page: [0.22, 1, 0.36, 1] as const,
};

export const transitions = {
  content: { duration: 0.4, ease: ease.smooth } satisfies Transition,
  short: { duration: 0.2, ease: ease.smooth } satisfies Transition,
  page: { duration: 0.5, ease: ease.page } satisfies Transition,
  spring: { type: "spring", stiffness: 400, damping: 30 } satisfies Transition,
  springSoft: { type: "spring", stiffness: 260, damping: 28 } satisfies Transition,
};

export const stagger = {
  container: {
    initial: {},
    animate: { transition: { staggerChildren: 0.06, delayChildren: 0.05 } },
  },
  item: {
    initial: { opacity: 0, y: 8 },
    animate: { opacity: 1, y: 0, transition: transitions.content },
    exit: { opacity: 0, y: 8, transition: transitions.short },
  },
};
