import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./ui";
import { subscribeToasts, type Toast } from "../toastBus";

/** Erfolgsmeldungen verschwinden von selbst, Fehler (und Hinweise, Issue
 * #37) bleiben stehen, bis der Nutzer sie schließt (Spec 0067, Teil B2). */
export const SUCCESS_TOAST_MS = 4000;

/** Spec 0067, Teil B: zeigt Meldungen aus `toastBus`. Unten links, damit
 * sie nicht mit den Notiz-Karten unten rechts kollidieren; gleiche Optik. */
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
      className="fixed bottom-4 left-4 z-50 flex max-h-[calc(100vh-2rem)] w-96 max-w-[calc(100vw-2rem)] flex-col gap-2 overflow-y-auto"
      // eslint-disable-next-line jsx-a11y/prefer-tag-over-role -- <output> ist ein Formular-Element, dessen Live-Region-Unterstützung je nach Webview und Screenreader schwankt; role="status" auf einem einfachen Container hält die Ansage verlässlich (Issue #112).
      role="status"
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex items-start justify-between gap-2 rounded-lg border p-3 text-sm shadow-lg ${
            toast.kind === "error"
              ? "border-red-900 bg-red-950 text-red-200"
              : "border-slate-700 bg-slate-800 text-slate-100"
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
