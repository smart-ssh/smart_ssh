import { useTranslation } from "react-i18next";
import { previewReadFailureHint } from "../../previewReadFailure";
import { formatBytes } from "../../format";
import type { UploadOffer } from "../../hooks/useLocalEditSession";
import { NoteDiffPreview } from "../NoteDiffPreview";
import { Button, DialogFrame } from "../ui";

interface EditUploadDialogProps {
  offer: UploadOffer;
  /** Identity chosen by "Upload as <user>"; `null` = the edit session's own. */
  uploadAs: { user: string | null } | null;
  sessionElevatedUser: string | null | undefined;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Spec 0054, Teil 4: confirmation (with diff) before an edited copy is uploaded. */
export function EditUploadDialog({
  offer,
  uploadAs,
  sessionElevatedUser,
  onCancel,
  onConfirm,
}: EditUploadDialogProps) {
  const { t } = useTranslation();
  const user = uploadAs ? uploadAs.user : sessionElevatedUser;
  return (
    <DialogFrame
      layer="sessionTab"
      title={
        user ? t("fileElevation.editUploadTitle", { user }) : t("fileBrowser.localEdit.uploadTitle")
      }
      onClose={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button variant="primary" onClick={onConfirm}>
            {t("fileBrowser.upload")}
          </Button>
        </>
      }
    >
      {offer.remoteChangedSinceDownload && (
        <p className="mb-3 border border-danger-border-strong/50 bg-danger-subtle/40 px-2 py-1.5 text-xs text-danger-fg-soft">
          {t("fileBrowser.localEdit.remoteChanged")}
        </p>
      )}
      {offer.localText !== null && offer.remoteText !== null ? (
        <NoteDiffPreview previousContent={offer.remoteText} newContent={offer.localText} />
      ) : (
        <p className="border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
          {offer.remoteReadFailure
            ? t(
                previewReadFailureHint(offer.remoteReadFailure).key,
                previewReadFailureHint(offer.remoteReadFailure).options,
              )
            : t("fileBrowser.localEdit.noDiff", { size: formatBytes(offer.localSize) })}
        </p>
      )}
    </DialogFrame>
  );
}
