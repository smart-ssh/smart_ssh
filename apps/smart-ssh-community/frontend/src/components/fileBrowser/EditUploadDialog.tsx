import { useTranslation } from "react-i18next";
import { previewReadFailureHint } from "../../lib/previewReadFailure";
import { formatBytes } from "../../lib/format";
import type { UploadOffer } from "../../hooks/useLocalEditSession";
import { NoteDiffPreview } from "../NoteDiffPreview";
import { DialogRows } from "./DialogRows";
import { Button, DialogFrame, Icon } from "../ui";

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
      icon={<Icon name="upload" />}
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
        <>
          {offer.remoteReadFailure ? (
            <p className="border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
              {t(
                previewReadFailureHint(offer.remoteReadFailure).key,
                previewReadFailureHint(offer.remoteReadFailure).options,
              )}
            </p>
          ) : (
            <>
              <p className="mb-2 text-xs text-fg-subtle">{t("fileBrowser.noTextDiff")}</p>
              <DialogRows
                rows={[{ label: t("fileBrowser.uploadConflict.newSize"), value: formatBytes(offer.localSize) }]}
              />
            </>
          )}
        </>
      )}
    </DialogFrame>
  );
}
