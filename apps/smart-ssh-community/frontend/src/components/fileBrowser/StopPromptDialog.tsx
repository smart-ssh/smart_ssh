import type { TranslationKey } from "../../i18nTypes";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../lib/format";
import type { RunningOperationDto } from "../../api/types";
import { Button, DialogFrame } from "../ui";

interface StopPromptDialogProps {
  operations: RunningOperationDto[];
  onCancel: () => void;
  onConfirm: () => void;
}

/** Issue #622: asked before ending elevated rights while elevated operations still run. */
const OPERATION_LABELS: Record<string, TranslationKey | undefined> = {
  download: "fileElevation.opDownload",
  upload: "fileElevation.opUpload",
  delete: "fileElevation.opDelete",
  chmod: "fileElevation.opChmod",
  chown: "fileElevation.opChown",
};

export function StopPromptDialog({ operations, onCancel, onConfirm }: StopPromptDialogProps) {
  const { t } = useTranslation();
  return (
      <DialogFrame
        layer="sessionTab"
        size="sm"
        title={t("fileElevation.stopTitle")}
        onClose={() => onCancel()}
        actions={
          <>
            <Button onClick={() => onCancel()}>{t("fileBrowser.cancel")}</Button>
            <Button variant="danger" onClick={onConfirm}>
              {t("fileElevation.toggleOff")}
            </Button>
          </>
        }
      >
        <p>{t("fileElevation.stopBody")}</p>
        <ul className="mt-2 space-y-1 text-xs">
          {operations.map((op) => (
            <li key={op.id} className="break-all">
              <span className="font-semibold">
                {
                  // `kind` is a plain string on the wire (generated type); an
                  // unknown value is shown as is.
                  (() => {
                    const key = OPERATION_LABELS[op.kind];
                    return key ? t(key) : op.kind;
                  })()
                }
              </span>{" "}
              <span className="font-mono">{op.path}</span>
              {op.bytesTotal !== null && op.bytesDone !== null ? (
                <span className="text-fg-subtle">
                  {" "}
                  ({formatBytes(op.bytesDone)} / {formatBytes(op.bytesTotal)}
                  {op.filesTotal !== null &&
                    op.filesDone !== null &&
                    ` — ${t("fileBrowser.transferFiles", { done: op.filesDone, total: op.filesTotal })}`}
                  )
                </span>
              ) : (
                op.filesTotal !== null &&
                op.filesDone !== null && (
                  <span className="text-fg-subtle">
                    {" "}
                    ({t("fileBrowser.entriesProgress", { done: op.filesDone, total: op.filesTotal })})
                  </span>
                )
              )}
            </li>
          ))}
        </ul>
      </DialogFrame>
  );
}
