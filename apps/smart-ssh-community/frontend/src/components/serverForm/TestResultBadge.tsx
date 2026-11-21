import { useTranslation } from "react-i18next";
import { Badge, type BadgeTone } from "../ui";
import { translateErrorCode } from "../../api/errorCodes";
import type { ConnectStepRecord, TestConnectionResult } from "../../api/types";

// Spec 0069, Teil A5, Test 20: exportiert (statt modulprivat), damit sich
// die code->Übersetzung ohne einen vollständigen `ServerForm`-Render
// testen lässt.
export function TestResultBadge({ result }: { result: TestConnectionResult & { steps?: ConnectStepRecord[] } }) {
  const { t } = useTranslation();
  let tone: BadgeTone = "danger";
  let text: string;
  switch (result.kind) {
    case "success": {
      tone = "success";
      // Sum of the step durations; without any measured step the plain
      // success text stays.
      const timed = (result.steps ?? []).filter((s) => s.durationMs !== null);
      text =
        timed.length > 0
          ? t("serverForm.testResult.successTimed", {
              ms: timed.reduce((sum, s) => sum + (s.durationMs ?? 0), 0),
            })
          : t("serverForm.testResult.success");
      break;
    }
    case "authenticationFailed":
      text = t("serverForm.testResult.authFailed");
      break;
    case "hostKeyUnknown":
      tone = "warning";
      text = t("serverForm.testResult.hostKeyUnknown");
      break;
    case "hostKeyMismatch":
      text = t("serverForm.testResult.hostKeyMismatch");
      break;
    case "networkError":
      // Spec 0069, Teil A5: bekannter Code → übersetzte Meldung; ohne
      // Code → wie bisher der rohe Backend-Text.
      text = translateErrorCode(
        t,
        result.code,
        t("serverForm.testResult.networkError", { message: result.message }),
      );
      break;
    case "timeout":
      text = t("serverForm.testResult.timeout");
      break;
  }
  return (
    <Badge
      tone={tone}
      square
      className="h-auto! min-h-5 py-0.5 text-left normal-case! tracking-normal!"
    >
      {text}
    </Badge>
  );
}
