export type StatusTone = "success" | "warning" | "danger" | "local" | "neutral" | "accent";

const TONES: Record<StatusTone, string> = {
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
  local: "bg-local",
  neutral: "bg-fg-faint",
  accent: "bg-accent",
};

/** 8 px solid status square (never a dot). Decorative: the state is always
 * also said in text. `size` 7 is used inside 20 px badges. */
export function StatusSquare({
  tone = "neutral",
  pulse = false,
  size = 8,
  className = "",
  title,
}: {
  tone?: StatusTone;
  pulse?: boolean;
  size?: 7 | 8;
  className?: string;
  title?: string;
}) {
  return (
    <span
      aria-hidden="true"
      title={title}
      data-status-square={tone}
      style={{ width: size, height: size }}
      className={`inline-block shrink-0 ${TONES[tone]}${pulse ? " animate-pulse" : ""}${className ? ` ${className}` : ""}`}
    />
  );
}
