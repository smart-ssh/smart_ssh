import { translateErrorCode } from "../../api/errorCodes";
import type { TranslateFn } from "../../i18nTypes";

/** Issue #596: backend reason codes as a translated, de-duplicated list. */
export function reasonList(t: TranslateFn, codes: string[]): string {
  return [...new Set(codes)]
    .map((code) => translateErrorCode(t, code, t("confirmDialog.explain.reasonOther")))
    .join("; ");
}
