// Issue #100: Begründung eines nicht nutzbaren Servers (Anmeldeart
// unlesbar) als Nutzertext — geteilt von Serverliste, Sidebar und
// `UnusableServerPanel`.

import type { UnusableReason } from "../api/types";
import type { TranslateFn } from "../i18nTypes";

export function unusableReasonText(
  t: TranslateFn,
  reason: UnusableReason,
): string {
  return reason === "unknown_auth_method"
    ? t("unusableServer.reason_unknown_auth_method")
    : t("unusableServer.reason_unreadable_auth_method");
}
