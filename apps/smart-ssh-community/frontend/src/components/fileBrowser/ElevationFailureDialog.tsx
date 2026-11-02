import { useState } from "react";
import { useTranslation } from "react-i18next";
import { clipboardWriteText } from "../../api";
import { type ElevationResultDto } from "../../types";
import { Button, DialogFrame } from "../ui";

/** Spec 0067, A3: verständliche Erklärung, warum der erhöhte Modus nicht
 * geht — bei fehlender sudo-Regel mit der zugeschnittenen sudoers-Zeile zum
 * Kopieren und dem ehrlichen Hinweis, was diese Regel bedeutet. */
export function ElevationFailureDialog({
  result,
  onClose,
}: {
  result: ElevationResultDto;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const failure = result.failure;
  if (!failure) return null;

  const copy = async () => {
    if (!failure.sudoersLine) return;
    try {
      await clipboardWriteText(failure.sudoersLine);
      setCopied(true);
    } catch (err) {
      console.warn("Konnte sudoers-Zeile nicht kopieren:", err);
    }
  };

  return (
    <DialogFrame
      layer="sessionTab"
      title={t("fileElevation.failureTitle")}
      onClose={onClose}
      size="md"
      actions={<Button onClick={onClose}>{t("fileElevation.close")}</Button>}
    >
      <div className="space-y-3">
        <p>{t(`fileElevation.failure.${failure.kind}`, { path: failure.detail ?? "" })}</p>
        {failure.sudoersLine && (
          <>
            <p>{t("fileElevation.sudoersIntro")}</p>
            <div className="flex items-center gap-2">
              <code className="min-w-0 flex-1 select-text break-all rounded border border-border-strong bg-surface-sunken px-2 py-1.5 font-mono text-xs text-fg">
                {failure.sudoersLine}
              </code>
              <Button size="sm" onClick={copy} className="shrink-0">
                {copied ? t("fileElevation.copied") : t("fileElevation.copy")}
              </Button>
            </div>
            <p className="text-xs text-fg-subtle">{t("fileElevation.sudoersHowTo")}</p>
            <p className="border border-danger-border-strong/50 bg-danger-subtle/40 px-2 py-1.5 text-xs text-danger-fg-soft">
              {t("fileElevation.sudoersWarning", { user: result.targetUser })}
            </p>
          </>
        )}
        {failure.detail && failure.kind !== "sftpServerUnsafe" && (
          <p className="font-mono text-xs break-all text-fg-subtle">
            {t("fileElevation.detail", { detail: failure.detail })}
          </p>
        )}
      </div>
    </DialogFrame>
  );
}
