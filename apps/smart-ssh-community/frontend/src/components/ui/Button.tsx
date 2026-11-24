import { forwardRef } from "react";
import type { ButtonHTMLAttributes } from "react";

/** Button variants, see `docs/ui-guidelines.md`. */
export type ButtonVariant = "primary" | "secondary" | "danger" | "ghost" | "bare";
export type ButtonSize = "sm" | "md";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Square icon button (32 px, 28 px for `sm`); needs an `aria-label`. */
  iconOnly?: boolean;
}

const BASE =
  "label-condensed inline-flex items-center justify-center gap-1.5 rounded transition-colors " +
  "focus-ring-flush disabled:cursor-not-allowed disabled:opacity-50";

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-on-accent hover:bg-accent-strong",
  secondary: "border border-border-strong bg-transparent text-fg-soft hover:bg-surface-hover",
  danger: "border border-danger-border bg-transparent text-danger-fg hover:bg-danger-subtle",
  ghost: "text-fg-muted hover:bg-surface-raised hover:text-fg",
  // No look of its own: the caller supplies every class (tabs, list rows,
  // icon toggles, link-like text). Still gets `type="button"` and the ref.
  bare: "",
};

// 32 px for md, 28 px for sm, at least 44 px in the narrow layout.
const SIZES: Record<ButtonSize, string> = {
  sm: "h-7 px-2.5 text-xs narrow:min-h-11",
  md: "h-8 px-3.5 text-[13px] narrow:min-h-11",
};
const ICON_SIZES: Record<ButtonSize, string> = {
  sm: "h-7 w-7 narrow:min-h-11 narrow:min-w-11",
  md: "h-8 w-8 narrow:min-h-11 narrow:min-w-11",
};

/** Shared button. `type` defaults to `"button"` so a button inside a form
 * never submits by accident; the primary action of a form passes
 * `type="submit"`. */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { variant = "secondary", size = "md", type = "button", iconOnly = false, className, ...rest },
    ref,
  ) {
    const bare = variant === "bare";
    const classes = [bare ? "" : BASE, VARIANTS[variant], bare ? "" : (iconOnly ? ICON_SIZES : SIZES)[size], className]
      .filter(Boolean)
      .join(" ");
    return <button ref={ref} type={type} className={classes} {...rest} />;
  },
);
