import type { TranslationKey } from "../../i18nTypes";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../lib/format";
import type { RunningOperationDto } from "../../api/types";
import { DialogRows } from "./DialogRows";
import { Button, DialogFrame, Icon } from "../ui";

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
        icon={<Icon name="warn" />}
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
        <ul className="mt-2 space-y-2 text-xs">
          {operations.map((op) => {
            const key = OPERATION_LABELS[op.kind];
            const hasBytes = op.bytesTotal !== null && op.bytesDone !== null;
            const hasFiles = op.filesTotal !== null && op.filesDone !== null;
            const progress = hasBytes
              ? `${formatBytes(op.bytesDone ?? 0)} / ${formatBytes(op.bytesTotal ?? 0)}` +
                (hasFiles
                  ? ` — ${t("fileBrowser.transferFiles", { done: op.filesDone, total: op.filesTotal })}`
                  : "")
              : hasFiles
                ? t("fileBrowser.entriesProgress", { done: op.filesDone, total: op.filesTotal })
                : null;
            return (
              <li key={op.id} className="break-all">
                <span className="font-semibold">
                  {/* `kind` is a plain string on the wire (generated type); an unknown value is shown as is. */}
                  {key ? t(key) : op.kind}
                </span>{" "}
                <span className="font-mono">{op.path}</span>
                {progress !== null && (
                  <DialogRows className="mt-1" rows={[{ label: t("fileBrowser.stopProgress"), value: progress }]} />
                )}
              </li>
            );
          })}
        </ul>
      </DialogFrame>
  );
}
