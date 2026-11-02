import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { explainConfirmation } from "../../api";
import { translateErrorCode } from "../../errorCodes";
import type { ConfirmationExplanationDto } from "../../types";

/** Issue #596: backend reason codes as a translated, de-duplicated list. */
export function reasonList(
  t: (key: string, options?: Record<string, unknown>) => string,
  codes: string[],
): string {
  return [...new Set(codes)]
    .map((code) => translateErrorCode(t, code, t("confirmDialog.explain.reasonOther")))
    .join("; ");
}

/** Issue #596, Spec 0011: which segments of the pending command existing
 * rules already cover, which need a rule and which ask regardless of rules.
 * Renders nothing while loading or when the backend returns no
 * segments (single segment without a rule-independent reason); a muted
 * line on failure. */
export function CoverageList({ sessionId, command }: { sessionId: string; command: string }) {
  const { t } = useTranslation();
  const [explanation, setExplanation] = useState<ConfirmationExplanationDto | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let stale = false;
    explainConfirmation(sessionId, command, "Global")
      .then((r) => {
        if (!stale) {
          setExplanation(r);
          setFailed(false);
        }
      })
      .catch(() => {
        // The backend logs the error; the card says the lookup failed.
        if (!stale) {
          setExplanation(null);
          setFailed(true);
        }
      });
    return () => {
      stale = true;
    };
  }, [sessionId, command]);
  if (failed) {
    return <div className="text-xs text-fg-subtle">{t("confirmDialog.explain.loadFailed")}</div>;
  }
  if (!explanation || explanation.segments.length === 0) return null;
  return (
    <div className="space-y-1 text-xs">
      <div className="font-heading font-semibold tracking-wide text-fg-subtle uppercase">
        {t("confirmDialog.explain.heading")}
      </div>
      <ul className="space-y-0.5">
        {explanation.segments.map((segment) => (
          <li key={segment.index} className="flex flex-wrap gap-x-2">
            <code className="font-mono break-all text-fg-soft">{segment.text}</code>
            {segment.state === "covered" && (
              <span className="text-success-fg-soft">
                ✓{" "}
                {segment.rule
                  ? t("confirmDialog.explain.covered", { rule: segment.rule.patternValue })
                  : t("confirmDialog.explain.coveredUnknown")}
              </span>
            )}
            {segment.state === "needsRule" && (
              <span className="text-warning-fg-soft">{t("confirmDialog.explain.needsRule")}</span>
            )}
            {segment.state === "blocked" && (
              <span className="text-warning-fg-soft">
                {t("confirmDialog.explain.blocked", { reasons: reasonList(t, segment.reasons) })}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

