import { useTranslation } from "react-i18next";
import { formatBytes } from "../../lib/format";
import { previewReadFailureHint } from "../../lib/previewReadFailure";
import { Button, DialogFrame } from "../ui";
import { NoteDiffPreview } from "../NoteDiffPreview";
import type { UploadConflict } from "./shared";

interface UploadConflictDialogProps {
  conflict: UploadConflict;
  elevated: boolean;
  elevatedUser: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Asks before an upload overwrites an existing remote file, with a diff preview. */
export function UploadConflictDialog({ conflict, elevated, elevatedUser, onCancel, onConfirm }: UploadConflictDialogProps) {
  const { t } = useTranslation();
  const uploadConflict = conflict;
  return (
      <DialogFrame
        layer="sessionTab"
        title={elevated ? t("fileElevation.overwriteTitle", { user: elevatedUser }) : t("fileBrowser.uploadConflict.title")}
        onClose={() => onCancel()}
        actions={
          <>
            <Button onClick={() => onCancel()}>{t("fileBrowser.cancel")}</Button>
            <Button variant="danger" onClick={onConfirm}>
              {t("fileBrowser.overwrite")}
            </Button>
          </>
        }
      >
        <p className="mb-3">
          <span className="font-mono text-xs break-all">{uploadConflict.remotePath}</span>{" "}
          {t("fileBrowser.uploadConflict.body")}
        </p>
        {uploadConflict.localPreview.text !== null && uploadConflict.remoteText !== null ? (
          <NoteDiffPreview
            previousContent={uploadConflict.remoteText}
            newContent={uploadConflict.localPreview.text}
          />
        ) : (
          <p className="border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
            {uploadConflict.remoteReadFailure
              ? t(
                  previewReadFailureHint(uploadConflict.remoteReadFailure).key,
                  previewReadFailureHint(uploadConflict.remoteReadFailure).options,
                )
              : t("fileBrowser.uploadConflict.noDiff", {
                  remoteSize: formatBytes(uploadConflict.remoteSize),
                  localSize: formatBytes(uploadConflict.localPreview.size),
                })}
          </p>
        )}
      </DialogFrame>
  );
}
