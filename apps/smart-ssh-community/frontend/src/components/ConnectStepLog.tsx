// Issue #51: zugeklappte "Details"-Ansicht des Schritt-Protokolls eines
// Verbindungsversuchs — unter dem Testergebnis im Server-Formular und unter
// dem Verbindungsfehler in der Serverliste. Der gescheiterte Schritt ist
// hervorgehoben; "Kopieren" legt die Textfassung in die Zwischenablage.
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  formatDuration,
  formatStepLogText,
  groupByHop,
  stepDetails,
  stepLabel,
  stepStatusText,
} from "../connectStepLog";
import type { ConnectStepRecord } from "../types";

export function ConnectStepLog({ steps }: { steps: ConnectStepRecord[] }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<"ok" | "failed" | null>(null);

  if (steps.length === 0) return null;
  const groups = groupByHop(steps);
  const multiHop = groups.length > 1;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(formatStepLogText(t, steps));
      setCopied("ok");
    } catch {
      // Sichtbar statt still: ohne Zwischenablage (kein sicherer Kontext)
      // bekommt der Nutzer einen Hinweis, das Protokoll bleibt lesbar.
      setCopied("failed");
    }
  };

  return (
    <details className="mt-2 w-full text-xs text-slate-300" data-testid="connect-step-log">
      <summary className="cursor-pointer select-none text-slate-400 hover:text-slate-200">
        {t("connectLog.title")}
      </summary>
      <div className="mt-2 space-y-2 rounded border border-slate-700 bg-slate-900/60 p-2">
        {groups.map((group) => (
          <div key={group.hopIndex}>
            {multiHop && (
              <div className="mb-1 font-medium text-slate-200">
                {t("connectLog.hop", { index: group.hopIndex + 1, hop: group.hop })}
              </div>
            )}
            <ol className="space-y-1">
              {group.steps.map((record, i) => {
                const failed = record.status.state === "failed";
                const details = stepDetails(t, record.step);
                return (
                  <li
                    key={i}
                    data-failed={failed ? "true" : undefined}
                    className={
                      failed
                        ? "rounded border border-red-700 bg-red-950/40 px-1 py-0.5 text-red-300"
                        : "px-1 py-0.5"
                    }
                  >
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span aria-hidden="true">
                        {failed ? "✗" : record.status.state === "ok" ? "✓" : "…"}
                      </span>
                      <span className="font-medium">{stepLabel(t, record.step)}</span>
                      <span>{stepStatusText(t, record)}</span>
                      <span className="text-slate-500">{formatDuration(record.durationMs)}</span>
                    </div>
                    {details && <div className="ml-5 break-all text-slate-400">{details}</div>}
                  </li>
                );
              })}
            </ol>
          </div>
        ))}
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void copy()}
            className="rounded bg-slate-800 px-2 py-1 hover:bg-slate-700"
          >
            {t("connectLog.copy")}
          </button>
          {copied === "ok" && <span className="text-emerald-400">{t("connectLog.copied")}</span>}
          {copied === "failed" && <span className="text-red-400">{t("connectLog.copyFailed")}</span>}
        </div>
      </div>
    </details>
  );
}
