import { useTranslation } from "react-i18next";
import type { TerminalKeyId } from "../terminalKeys";

interface TerminalKeyRowProps {
  ctrl: boolean;
  alt: boolean;
  onToggleCtrl: () => void;
  onToggleAlt: () => void;
  onKey: (key: TerminalKeyId) => void;
}

/** Issue #678: extra keys above the soft keyboard (Spec 0114). */
export function TerminalKeyRow({ ctrl, alt, onToggleCtrl, onToggleAlt, onKey }: TerminalKeyRowProps) {
  const { t } = useTranslation();
  const base =
    "min-h-11 min-w-11 shrink-0 rounded border border-border bg-surface-raised px-3 font-mono text-sm text-fg-soft active:bg-border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg";
  // Keep focus (and the soft keyboard) in the terminal when a key is tapped.
  const keepFocus = (e: React.MouseEvent | React.PointerEvent) => e.preventDefault();
  const keys: { id: TerminalKeyId; label: string; aria?: string }[] = [
    { id: "esc", label: "Esc" },
    { id: "tab", label: "Tab" },
    { id: "up", label: "↑", aria: t("session.extraKeys.up") },
    { id: "down", label: "↓", aria: t("session.extraKeys.down") },
    { id: "left", label: "←", aria: t("session.extraKeys.left") },
    { id: "right", label: "→", aria: t("session.extraKeys.right") },
    { id: "home", label: "Home" },
    { id: "end", label: "End" },
    { id: "pgup", label: "PgUp", aria: t("session.extraKeys.pageUp") },
    { id: "pgdn", label: "PgDn", aria: t("session.extraKeys.pageDown") },
    { id: "pipe", label: "|" },
    { id: "tilde", label: "~" },
    { id: "slash", label: "/" },
    { id: "dash", label: "-" },
  ];
  const mod = (on: boolean) => `${base} ${on ? "border-accent-fg bg-accent text-fg-bright" : ""}`;
  return (
    <div
      role="toolbar"
      aria-label={t("session.extraKeys.group")}
      data-testid="terminal-key-row"
      className="flex gap-1 overflow-x-auto pt-2"
    >
      <button type="button" className={base} onPointerDown={keepFocus} onClick={() => onKey("esc")}>
        Esc
      </button>
      <button
        type="button"
        className={mod(ctrl)}
        aria-pressed={ctrl}
        onPointerDown={keepFocus}
        onClick={onToggleCtrl}
      >
        Ctrl
      </button>
      <button
        type="button"
        className={mod(alt)}
        aria-pressed={alt}
        onPointerDown={keepFocus}
        onClick={onToggleAlt}
      >
        Alt
      </button>
      {keys
        .filter((k) => k.id !== "esc")
        .map((k) => (
          <button
            key={k.id}
            type="button"
            className={base}
            aria-label={k.aria}
            onPointerDown={keepFocus}
            onClick={() => onKey(k.id)}
          >
            {k.label}
          </button>
        ))}
    </div>
  );
}
