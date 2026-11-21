// Issue #548: live step list of a running (or failed) connect attempt, fed by
// `connect-progress` events. One row per step with a status icon and the
// translated detail text. A visually hidden `aria-live` region announces only
// step completions and failures, not every detail change.
import { useTranslation } from "react-i18next";
import type { ConnectStepDetail, ConnectStepSnapshot } from "../api/types";
import { Spinner, Icon } from "./ui";
import type { TranslateFn } from "../i18nTypes";

type T = TranslateFn;

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
        className="border border-border-strong bg-surface-raised text-sm"
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
                  ? "flex items-baseline gap-2 border-t border-danger-border bg-danger-subtle px-3 py-2 text-danger-fg first:border-t-0"
                  : "flex items-baseline gap-2 border-t border-border px-3 py-2 first:border-t-0"
              }
            >
              <span className="inline-flex w-4 shrink-0 justify-center text-fg-muted" aria-hidden="true">
                {step.status === "running" ? (
                  <Spinner className="h-3 w-3" />
                ) : step.status === "skipped" ? (
                  "–"
                ) : step.status === "ok" ? (
                  <Icon name="check" className="text-success-fg" />
                ) : step.status === "warning" ? (
                  <Icon name="warn" className="text-warning-fg" />
                ) : (
                  <Icon name="x" />
                )}
              </span>
              <span className="font-medium">
                {step.kind === "dnsResolution" && step.status === "skipped"
                  ? t("connectLog.step.dnsSkipped")
                  : t(`connectLog.step.${step.kind}`)}
              </span>
              {multiHop && <span className="text-fg-subtle">{step.hop}</span>}
              {detail && <span className="font-mono text-xs break-all text-fg-subtle">{detail}</span>}
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
