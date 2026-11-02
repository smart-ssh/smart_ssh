import { useTranslation } from "react-i18next";
import type { FolderUploadSummaryDto } from "../../types";
import { Button, DialogFrame } from "../ui";

interface FolderSummaryDialogProps {
  folderSummary: FolderUploadSummaryDto;
  onClose: () => void;
}

/** Result of a folder upload: counts plus the failed and skipped entries. */
export function FolderSummaryDialog({ folderSummary, onClose }: FolderSummaryDialogProps) {
  const { t } = useTranslation();
  return (
      <DialogFrame
        layer="sessionTab"
        title={t("fileBrowser.folderUpload.summaryTitle", { name: folderSummary.folderName })}
        onClose={() => onClose()}
        actions={
          <Button variant="primary" onClick={() => onClose()}>
            {t("fileBrowser.folderUpload.close")}
          </Button>
        }
      >
        <p className="mb-3">
          {t("fileBrowser.folderUpload.summary", {
            uploaded: folderSummary.filesUploaded,
            skipped: folderSummary.skipped.length,
            failed: folderSummary.failed.length,
          })}
        </p>
        {folderSummary.notAttempted > 0 && (
          <p className="mb-3 text-warning-fg-soft">
            {t("fileBrowser.folderUpload.notAttempted", { count: folderSummary.notAttempted })}
          </p>
        )}
        <ul className="max-h-60 overflow-y-auto border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-muted">
          {folderSummary.failed.map((f) => (
            <li key={"f:" + f.path} className="break-all">
              <span className="text-danger-fg">{t("fileBrowser.folderUpload.failed")}:</span>{" "}
              <span className="font-mono">{f.path}</span> — {f.error}
            </li>
          ))}
          {folderSummary.skipped.map((e) => (
            <li key={"s:" + e.path} className="break-all">
              <span className="text-warning-fg-soft">{t("fileBrowser.folderUpload.skipped")}:</span>{" "}
              <span className="font-mono">{e.path}</span> —{" "}
              {t("fileBrowser.folderUpload.reason." + e.reason)}
            </li>
          ))}
        </ul>
      </DialogFrame>
  );
}
