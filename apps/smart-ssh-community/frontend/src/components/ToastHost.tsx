import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { subscribeToasts, type Toast } from "../toastBus";

/** Erfolgsmeldungen verschwinden von selbst, Fehler bleiben stehen, bis der
 * Nutzer sie schließt (Spec 0067, Teil B2). */
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
    <div className="fixed bottom-4 left-4 z-50 flex w-96 flex-col gap-2" role="status">
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
            {toast.kind === "success" ? "✓ " : "⚠ "}
            {toast.message}
          </p>
          <div className="flex shrink-0 gap-1">
            {toast.action && (
              <button
                type="button"
                onClick={() => {
                  toast.action?.onClick();
                  dismiss(toast.id);
                }}
                className="rounded bg-slate-700 px-2 py-1 text-xs hover:bg-slate-600"
              >
                {toast.action.label}
              </button>
            )}
            <button
              type="button"
              onClick={() => dismiss(toast.id)}
              aria-label={t("toast.dismiss")}
              className={`rounded px-2 py-1 text-xs ${
                toast.kind === "error"
                  ? "bg-red-900 text-red-200 hover:bg-red-800"
                  : "bg-slate-700 text-slate-300 hover:bg-slate-600"
              }`}
            >
              ✕
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
