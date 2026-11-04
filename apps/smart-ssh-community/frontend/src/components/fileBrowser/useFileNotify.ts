import type { TranslationKey } from "../../i18nTypes";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, sftpElevationStatus } from "../../api";
import { isMacLike, isWindowsLike } from "../../platform";
import { showToast } from "../../toastBus";

/** „Im Finder/Explorer zeigen“ — Beschriftung passend zum Betriebssystem. */
function revealLabelKey(): TranslationKey {
  if (isMacLike()) return "fileToasts.revealFinder";
  if (isWindowsLike()) return "fileToasts.revealExplorer";
  return "fileToasts.revealFileManager";
}

export type NotifyOk = (key: TranslationKey, options: Record<string, unknown>, action?: () => void) => void;
export type NotifyFailed = (key: TranslationKey, name: string, err: unknown) => void;

/**
 * Spec 0067, Teil B: every file action reports its result as a toast. Only
 * names/paths, never file contents.
 */
export function useFileNotify(
  sessionId: string,
  elevated: boolean,
  elevatedUser: string | null,
  updateElevation: (user: null) => void,
) {
  const { t } = useTranslation();
  const modeSuffix = elevated ? t("fileToasts.asElevated", { user: elevatedUser }) : "";

  const notifyOk: NotifyOk = (key, options, action) =>
    showToast({
      kind: "success",
      message: t(key, options) + modeSuffix,
      action: action ? { label: t(revealLabelKey()), onClick: action } : undefined,
    });
  const notifyFailed: NotifyFailed = (key, name, err) => {
    // Issue #622: a stopped operation is not an error — its row shows the state.
    if (commandErrorCode(err) === "FILE_OPERATION_STOPPED") return;
    showToast({
      kind: "error",
      message: t(key, { name, reason: commandErrorMessage(err) }) + modeSuffix,
    });
    // Spec 0067, A5: ist der erhöhte Kanal inzwischen weg (z. B. nach einem
    // Verbindungsabbruch), wird der Modus sichtbar beendet statt weiter
    // "aktiv" anzuzeigen.
    if (elevated) {
      sftpElevationStatus(sessionId)
        .then((user) => {
          if (user === null) {
            updateElevation(null);
            showToast({ kind: "error", message: t("fileElevation.channelLost") });
          }
        })
        .catch(() => {});
    }
  };
  return { modeSuffix, notifyOk, notifyFailed };
}
