import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./ui";
import { subscribeToasts, type Toast } from "../toastBus";

/** Erfolgsmeldungen verschwinden von selbst, Fehler (und Hinweise, Issue
 * #37) bleiben stehen, bis der Nutzer sie schließt (Spec 0067, Teil B2). */
export const SUCCESS_TOAST_MS = 4000;

/** Spec 0067, Teil B: zeigt Meldungen aus `toastBus`. Oben rechts unter
 * dem Header: unten würden sie den Chat-Eingabebereich verdecken (Issue #422)
 * und mit den Notiz-Karten unten rechts kollidieren. */
export function ToastHost() {
  const { t } = useTranslation();
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(
    () =>
      subscribeToasts((toast) => {
        setToasts((prev) => [...prev, toast]);
        if (toast.kind === "success") {
          setTimeout(
            () => setToasts((prev) => prev.filter((t) => t.id !== toast.id)),
            SUCCESS_TOAST_MS,
          );
        }
      }),
    [],
  );

  const dismiss = (id: number) => setToasts((prev) => prev.filter((t) => t.id !== id));

  if (toasts.length === 0) return null;

  return (
    <div
      className="fixed right-4 top-14 narrow:left-4 z-50 flex max-h-[calc(100vh-4.5rem)] w-96 max-w-[calc(100vw-2rem)] narrow:w-auto flex-col gap-2 overflow-y-auto"
      // eslint-disable-next-line jsx-a11y/prefer-tag-over-role -- <output> ist ein Formular-Element, dessen Live-Region-Unterstützung je nach Webview und Screenreader schwankt; role="status" auf einem einfachen Container hält die Ansage verlässlich (Issue #112).
      role="status"
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex items-start justify-between gap-2 rounded-lg border p-3 text-sm shadow-lg ${
            toast.kind === "error"
              ? "border-danger-subtle-hover bg-danger-subtle text-danger-fg-softer"
              : "border-border bg-surface-raised text-fg"
          }`}
        >
          <p className="min-w-0 break-words">
            {toast.kind === "success" ? "✓ " : toast.kind === "info" ? "ℹ " : "⚠ "}
            {toast.message}
          </p>
          <div className="flex shrink-0 gap-1">
            {toast.action && (
              <Button
                size="sm"
                onClick={() => {
                  toast.action?.onClick();
                  dismiss(toast.id);
                }}
              >
                {toast.action.label}
              </Button>
            )}
            <Button
              size="sm"
              variant={toast.kind === "error" ? "danger" : "secondary"}
              onClick={() => dismiss(toast.id)}
              aria-label={t("toast.dismiss")}
            >
              ✕
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}
