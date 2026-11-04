import { useTranslation } from "react-i18next";
import { translateErrorCode } from "../../api/errorCodes";
import type { TestConnectionResult } from "../../api/types";

// Spec 0069, Teil A5, Test 20: exportiert (statt modulprivat), damit sich
// die code->Übersetzung ohne einen vollständigen `ServerForm`-Render
// testen lässt.
export function TestResultBadge({ result }: { result: TestConnectionResult }) {
  const { t } = useTranslation();
  switch (result.kind) {
    case "success":
      return <span className="text-sm text-success-fg">{t("serverForm.testResult.success")}</span>;
    case "authenticationFailed":
      return <span className="text-sm text-danger-fg">{t("serverForm.testResult.authFailed")}</span>;
    case "hostKeyUnknown":
      return (
        <span className="text-sm text-warning-fg">{t("serverForm.testResult.hostKeyUnknown")}</span>
      );
    case "hostKeyMismatch":
      return <span className="text-sm text-danger-fg">{t("serverForm.testResult.hostKeyMismatch")}</span>;
    case "networkError":
      // Spec 0069, Teil A5: bekannter Code → übersetzte Meldung; ohne
      // Code → wie bisher der rohe Backend-Text.
      return (
        <span className="text-sm text-danger-fg">
          {translateErrorCode(
            t,
            result.code,
            t("serverForm.testResult.networkError", { message: result.message }),
          )}
        </span>
      );
    case "timeout":
      return <span className="text-sm text-danger-fg">{t("serverForm.testResult.timeout")}</span>;
  }
}
