// Issue #548: live step list of a running (or failed) connect attempt, fed by
// `connect-progress` events. One row per step with a status icon and the
// translated detail text. A visually hidden `aria-live` region announces only
// step completions and failures, not every detail change.
import { useTranslation } from "react-i18next";
import type { ConnectStepDetail, ConnectStepSnapshot } from "../types";
import { Spinner } from "./ui";

type T = (key: string, options?: Record<string, unknown>) => string;

export function snapshotDetailText(t: T, detail: ConnectStepDetail | null): string {
  if (!detail) return "";
  switch (detail.code) {
    case "contacting":
      return detail.target;
    case "tunnel":
      return t("connectTab.detail.tunnel", { hop: detail.hop });
    case "waitingForResponse":
      return t("connectTab.detail.waitingForResponse");
    case "waitingForDecision":
      return t("connectTab.detail.waitingForDecision");
    case "authMethod":
      return t("connectTab.detail.authMethod", { method: detail.method });
  }
}

/** The text announced for the most recently finished step; stays unchanged
 * while only details of running steps change. */
export function completionAnnouncement(t: T, steps: ConnectStepSnapshot[]): string {
  const finished = steps.filter((s) => s.status !== "running");
  const last = finished[finished.length - 1];
  if (!last) return "";
  return t("connectTab.announce", {
    step: t(`connectLog.step.${last.kind}`),
    result: t(last.status === "ok" || last.status === "skipped" ? "connectTab.resultOk" : "connectTab.resultFailed"),
  });
}

export function ConnectProgressList({ steps }: { steps: ConnectStepSnapshot[] }) {
  const { t } = useTranslation();
  const multiHop = new Set(steps.map((s) => s.hopIndex)).size > 1;
  return (
    <>
      <ul
        aria-label={t("connectTab.stepsLabel")}
        data-testid="connect-progress-list"
        className="space-y-1 text-sm"
      >
        {steps.map((step, i) => {
          const failed = step.status === "failed";
          const detail = snapshotDetailText(t, step.detail);
          return (
            <li
              key={`${step.hopIndex}-${step.kind}-${i}`}
              data-status={step.status}
              data-failed={failed ? "true" : undefined}
              className={
                failed
                  ? "flex items-baseline gap-2 rounded border border-red-700 bg-red-950/40 px-2 py-1 text-red-300"
                  : "flex items-baseline gap-2 px-2 py-1"
              }
            >
              <span className="inline-flex w-4 shrink-0 justify-center text-slate-300" aria-hidden="true">
                {step.status === "running" ? (
                  <Spinner className="h-3 w-3" />
                ) : step.status === "skipped" ? (
                  "–"
                ) : step.status === "ok" ? (
                  <span className="text-emerald-400">✓</span>
                ) : (
                  "✗"
                )}
              </span>
              <span className="font-medium">
                {step.kind === "dnsResolution" && step.status === "skipped"
                  ? t("connectLog.step.dnsSkipped")
                  : t(`connectLog.step.${step.kind}`)}
              </span>
              {multiHop && <span className="text-slate-400">{step.hop}</span>}
              {detail && <span className="break-all text-slate-400">{detail}</span>}
            </li>
          );
        })}
      </ul>
      <output aria-live="polite" className="sr-only">
        {completionAnnouncement(t, steps)}
      </output>
    </>
  );
}
