import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, exportSshConfig } from "../api";
import type { SshConfigExportResultDto } from "../types";

interface SshConfigExportDialogProps {
  onClose: () => void;
}

/** Spec 0075, §3.2.5/§3.2.7 — Export nach `ssh_config`. Anders als der
 * Import braucht der Export keine Vorschau (die Spec verlangt nur eine
 * Meldung danach): Der Dialog öffnet den Speichern-Dialog im Backend beim
 * Mounten und zeigt anschließend Pfad, `Include`-Zeile und — falls
 * vorhanden — umbenannte Aliase. */
export function SshConfigExportDialog({ onClose }: SshConfigExportDialogProps) {
  const { t } = useTranslation();
  const [result, setResult] = useState<SshConfigExportResultDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    let cancelled = false;
    exportSshConfig(t("sshConfigExport.dialogTitle"))
      .then((dto) => {
        if (cancelled) return;
        if (!dto) {
          onClose();
          return;
        }
        setResult(dto);
      })
      .catch((err) => !cancelled && setError(commandErrorMessage(err)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleCopyIncludeHint = async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.includeHint);
      setCopyState("copied");
    } catch (err) {
      console.warn("Konnte Include-Zeile nicht in die Zwischenablage kopieren:", err);
      setCopyState("failed");
    } finally {
      setTimeout(() => setCopyState("idle"), 2000);
    }
  };

  if (loading && !error) {
    // Kein sichtbarer Dialog während der native Speichern-Dialog offen ist
    // — nichts anzuzeigen ist hier richtig, nicht fehlend.
    return null;
  }
  if (!result && !error) return null;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-lg rounded border border-slate-600 bg-slate-900 p-6 shadow-xl">
        <h2 className="font-heading mb-3 text-lg font-semibold text-slate-100">
          {t("sshConfigExport.dialogTitle")}
        </h2>

        {error && <p className="mb-3 rounded border border-red-700 bg-red-950/50 p-2 text-sm text-red-300">{error}</p>}

        {result && (
          <div className="space-y-3 text-sm text-slate-300">
            <p>{t("sshConfigExport.result.summary", { count: result.exported.length, path: result.path })}</p>

            {result.exported.some((e) => e.renamed) && (
              <div className="rounded border border-slate-700 bg-slate-800/60 p-2">
                <p className="mb-1 font-semibold text-slate-100">{t("sshConfigExport.result.renamedHeading")}</p>
                <ul className="space-y-0.5 font-mono text-xs">
                  {result.exported
                    .filter((e) => e.renamed)
                    .map((e) => (
                      <li key={e.alias}>
                        {e.originalName} → {e.alias}
                      </li>
                    ))}
                </ul>
              </div>
            )}

            <div>
              <p className="mb-1 text-xs text-slate-400">{t("sshConfigExport.result.includeHintLabel")}</p>
              <div className="flex items-center gap-2">
                <code className="flex-1 overflow-x-auto rounded border border-slate-600 bg-slate-950 px-2 py-1.5 font-mono text-xs text-slate-100">
                  {result.includeHint}
                </code>
                <button
                  type="button"
                  onClick={handleCopyIncludeHint}
                  className="rounded border border-slate-600 px-2 py-1.5 text-xs text-slate-300 hover:bg-slate-700"
                >
                  {t("sshConfigExport.result.copy")}
                </button>
                {copyState === "copied" && <span className="text-xs text-emerald-400">{t("about.copied")}</span>}
              </div>
            </div>

            <p className="text-xs text-slate-500">{t("sshConfigExport.result.notRepresentedHint")}</p>
          </div>
        )}

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-semibold text-slate-950 hover:bg-indigo-500"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
