import { useTranslation } from "react-i18next";
import { translateErrorCode } from "../errorCodes";
import type { InjectionCheckState } from "../types";

/** Issue #693 (Spec 0039, §5.2): status of the AI check for injected
 * instructions, shown directly under the checked result. The reason comes
 * from untrusted content, so it is rendered as plain text only. */
export function InjectionCheckStatus({ check }: { check?: InjectionCheckState }) {
  const { t } = useTranslation();
  if (!check || check.state === "clean") return null;
  if (check.state === "running") {
    return (
      <p
        className="mt-1 flex items-center gap-2 text-xs text-slate-400"
        role="status"
        data-testid="injection-check-running"
      >
        <span
          className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-slate-500 border-t-transparent"
          aria-hidden="true"
        />
        {t("chat.injectionChecking")}
      </p>
    );
  }
  if (check.state === "unavailable") {
    return (
      <p
        className="mt-1 text-xs text-amber-300"
        role="status"
        data-testid="injection-check-unavailable"
      >
        {translateErrorCode(t, "AI_INJECTION_CHECK_UNAVAILABLE", "")}
      </p>
    );
  }
  return (
    <div
      className="mt-1 rounded border border-amber-700/60 bg-amber-950/30 px-3 py-2 text-xs text-amber-200"
      role="alert"
      data-testid="injection-check-suspected"
    >
      <p>⚠ {t("chat.injectionSuspected")}</p>
      {check.reason && (
        <p className="mt-0.5 break-words whitespace-pre-wrap">
          {t("chat.injectionSuspectedReason", { reason: check.reason })}
        </p>
      )}
    </div>
  );
}
