import { useTranslation } from "react-i18next";
import type { TerminalKeyId } from "../lib/terminalKeys";
import { Button, Icon, type IconName } from "./ui";

interface TerminalKeyRowProps {
  ctrl: boolean;
  alt: boolean;
  onToggleCtrl: () => void;
  onToggleAlt: () => void;
  onKey: (key: TerminalKeyId) => void;
}

interface KeyDef {
  id: TerminalKeyId | "ctrl" | "alt";
  label: string;
  aria?: string;
  /** Arrow keys: chevron icon and rotation class. */
  icon?: { name: IconName; rotate?: string };
  symbol?: boolean;
}

/** Issue #678 / #1370: extra keys above the soft keyboard, two fixed rows of eight (Spec 0114). */
export function TerminalKeyRow({ ctrl, alt, onToggleCtrl, onToggleAlt, onKey }: TerminalKeyRowProps) {
  const { t } = useTranslation();
  // Keep focus (and the soft keyboard) in the terminal when a key is tapped.
  const keepFocus = (e: React.MouseEvent | React.PointerEvent) => e.preventDefault();
  const rows: KeyDef[][] = [
    [
      { id: "esc", label: "Esc" },
      { id: "tab", label: "Tab" },
      { id: "ctrl", label: "Ctrl" },
      { id: "alt", label: "Alt" },
      { id: "up", label: "↑", aria: t("session.extraKeys.up"), icon: { name: "chev-u" } },
      { id: "down", label: "↓", aria: t("session.extraKeys.down"), icon: { name: "chev-d" } },
      { id: "left", label: "←", aria: t("session.extraKeys.left"), icon: { name: "chev-r", rotate: "rotate-180" } },
      { id: "right", label: "→", aria: t("session.extraKeys.right"), icon: { name: "chev-r" } },
    ],
    [
      { id: "home", label: "Home" },
      { id: "end", label: "End" },
      { id: "pgup", label: "PgUp", aria: t("session.extraKeys.pageUp") },
      { id: "pgdn", label: "PgDn", aria: t("session.extraKeys.pageDown") },
      { id: "pipe", label: "|", symbol: true },
      { id: "tilde", label: "~", symbol: true },
      { id: "slash", label: "/", symbol: true },
      { id: "dash", label: "-", symbol: true },
    ],
  ];
  const pressed = (id: KeyDef["id"]) => (id === "ctrl" ? ctrl : id === "alt" ? alt : undefined);
  const click = (id: KeyDef["id"]) => {
    if (id === "ctrl") onToggleCtrl();
    else if (id === "alt") onToggleAlt();
    else onKey(id);
  };
  return (
    <div
      role="toolbar"
      aria-label={t("session.extraKeys.group")}
      data-testid="terminal-key-row"
      className="flex shrink-0 flex-col border-t border-border-strong bg-surface-raised"
    >
      {rows.map((row, r) => (
        <div key={r} className={`flex ${r > 0 ? "border-t border-border" : ""}`}>
          {row.map((k, i) => {
            const on = pressed(k.id);
            const font = k.symbol ? "font-mono text-base" : "label-condensed text-sm";
            return (
              <Button
                variant="bare"
                key={k.id}
                type="button"
                data-testid={`terminal-key-${k.id}`}
                className={`flex min-h-11 min-w-11 flex-1 items-center justify-center rounded-none px-0 ${font} ${
                  i > 0 ? "border-l border-border" : ""
                } ${on ? "bg-accent text-on-accent" : "text-fg-soft active:bg-border"} focus-ring-flush`}
                aria-label={k.aria}
                aria-pressed={on}
                onPointerDown={keepFocus}
                onClick={() => click(k.id)}
              >
                {k.icon ? <Icon name={k.icon.name} className={k.icon.rotate} /> : k.label}
              </Button>
            );
          })}
        </div>
      ))}
    </div>
  );
}
