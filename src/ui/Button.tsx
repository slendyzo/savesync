import { motion, type HTMLMotionProps } from "motion/react";
import { forwardRef } from "react";
import { cn } from "../lib/cn";
import { transitions } from "./motion";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md" | "lg";

type ButtonProps = HTMLMotionProps<"button"> & {
  variant?: Variant;
  size?: Size;
};

const variantClasses: Record<Variant, string> = {
  primary:
    "bg-accent text-bg-0 hover:bg-accent/90 shadow-[0_0_24px_-4px_rgba(167,139,250,0.4)]",
  secondary:
    "bg-surface-hi border border-border-hi text-text hover:bg-white/10",
  ghost: "text-text-2 hover:text-text hover:bg-surface",
  danger: "bg-bad/15 border border-bad/40 text-bad hover:bg-bad/20",
};

const sizeClasses: Record<Size, string> = {
  sm: "h-8 px-3 text-xs",
  md: "h-10 px-4 text-sm",
  lg: "h-12 px-6 text-base",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", children, ...rest }, ref) => {
    return (
      <motion.button
        ref={ref}
        whileHover={{ y: -1 }}
        whileTap={{ scale: 0.98 }}
        transition={transitions.spring}
        className={cn(
          "relative inline-flex items-center justify-center gap-2 rounded-[var(--radius-button)] font-medium tracking-tight outline-none transition-colors disabled:cursor-not-allowed disabled:opacity-50",
          variantClasses[variant],
          sizeClasses[size],
          className,
        )}
        {...rest}
      >
        {children}
      </motion.button>
    );
  },
);
Button.displayName = "Button";
