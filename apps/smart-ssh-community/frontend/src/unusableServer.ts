// Issue #100: Begründung eines nicht nutzbaren Servers (Anmeldeart
// unlesbar) als Nutzertext — geteilt von Serverliste, Sidebar und
// `UnusableServerPanel`.

import type { UnusableReason } from "./types";

export function unusableReasonText(
  t: (key: string, options?: Record<string, unknown>) => string,
  reason: UnusableReason,
): string {
  return reason === "unknown_auth_method"
    ? t("unusableServer.reason_unknown_auth_method")
    : t("unusableServer.reason_unreadable_auth_method");
}
