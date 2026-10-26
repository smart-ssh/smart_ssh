import { forwardRef } from "react";
import type { ButtonHTMLAttributes } from "react";

/** Button variants, see `docs/ui-guidelines.md`. */
export type ButtonVariant = "primary" | "secondary" | "danger" | "ghost";
export type ButtonSize = "sm" | "md";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const BASE =
  "inline-flex items-center justify-center gap-1.5 rounded font-semibold transition-colors " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus " +
  "focus-visible:ring-offset-1 focus-visible:ring-offset-surface " +
  "disabled:cursor-not-allowed disabled:opacity-50";

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-on-accent hover:bg-accent-strong",
  secondary:
    "border border-border-strong bg-surface-raised text-fg hover:bg-surface-hover",
  danger: "border border-danger-border bg-danger-subtle text-danger-fg-soft hover:bg-danger-subtle-hover",
  ghost: "text-accent-fg hover:bg-surface-raised hover:text-accent-fg-soft",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "px-2.5 py-1 text-xs",
  md: "px-3 py-1.5 text-sm",
};

/** Shared button. `type` defaults to `"button"` so a button inside a form
 * never submits by accident; the primary action of a form passes
 * `type="submit"`. */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { variant = "secondary", size = "md", type = "button", className, ...rest },
    ref,
  ) {
    const classes = [BASE, VARIANTS[variant], SIZES[size], className]
      .filter(Boolean)
      .join(" ");
    return <button ref={ref} type={type} className={classes} {...rest} />;
  },
);
