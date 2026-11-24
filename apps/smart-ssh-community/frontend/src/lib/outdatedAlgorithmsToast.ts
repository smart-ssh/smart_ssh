// Issue #1173: toast text for a connection that negotiated outdated SSH
// algorithms. The event is only sent when there is something to warn about.
import i18next from "i18next";
import type { ConnectWarning } from "../generated/ipc";
import { showToast } from "../buses/toastBus";
import type { TranslateFn } from "../i18nTypes";

export function outdatedAlgorithmsMessage(t: TranslateFn, warnings: ConnectWarning[], hopCount = 1): string {
  // A chain always names the hop, also when only one hop is affected.
  if (warnings.length === 1 && hopCount <= 1) {
    return t("connectWarnings.outdatedAlgorithms", { algorithms: warnings[0].algorithms.join(", ") });
  }
  return warnings
    .map((w) =>
      t("connectWarnings.outdatedAlgorithmsHop", { hop: w.hop, algorithms: w.algorithms.join(", ") }),
    )
    .join("\n");
}

export function showOutdatedAlgorithmsToast(warnings: ConnectWarning[], hopCount = 1): void {
  if (warnings.length === 0) return;
  showToast({
    kind: "info",
    message: outdatedAlgorithmsMessage((key, options) => i18next.t(key, options), warnings, hopCount),
  });
}
