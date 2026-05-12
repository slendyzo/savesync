import { motion, type HTMLMotionProps } from "motion/react";
import { forwardRef } from "react";
import { cn } from "../lib/cn";
import { transitions } from "./motion";

type CardProps = HTMLMotionProps<"div"> & {
  interactive?: boolean;
};

export const Card = forwardRef<HTMLDivElement, CardProps>(
  ({ className, interactive = false, children, ...rest }, ref) => {
    return (
      <motion.div
        ref={ref}
        whileHover={interactive ? { y: -2 } : undefined}
        transition={transitions.spring}
        className={cn(
          "glass rounded-[var(--radius-card)] p-5 transition-colors",
          interactive && "cursor-pointer hover:border-border-hi",
          className,
        )}
        {...rest}
      >
        {children}
      </motion.div>
    );
  },
);
Card.displayName = "Card";
