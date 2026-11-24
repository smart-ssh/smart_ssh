import type { ReactNode } from "react";

import { StatusSquare } from "./StatusSquare";

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger" | "local";

const TONES: Record<BadgeTone, string> = {
  neutral: "border-border-strong text-fg-muted",
  accent: "border-accent-border text-accent-fg",
  success: "border-success-border text-success-fg",
  warning: "border-warning-border text-warning-fg",
  danger: "border-danger-border text-danger-fg",
  local: "border-local-border text-local-fg",
};

/** Outline badge: 20 px high, condensed 11 px uppercase. `square` adds a
 * leading `StatusSquare` of the same tone. */
export function Badge({
  tone = "neutral",
  square = false,
  className = "",
  children,
}: {
  tone?: BadgeTone;
  square?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={`label-condensed inline-flex h-5 items-center gap-1.5 border px-1.5 text-[11px] tracking-[0.1em] ${TONES[tone]}${className ? ` ${className}` : ""}`}
    >
      {square ? <StatusSquare tone={tone} size={7} /> : null}
      {children}
    </span>
  );
}
