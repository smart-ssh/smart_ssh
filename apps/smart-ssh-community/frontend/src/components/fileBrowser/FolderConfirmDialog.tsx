import { useTranslation } from "react-i18next";
import { Button, DialogFrame } from "../ui";
import type { useSftpTransfers } from "./useSftpTransfers";

type FolderConfirm = NonNullable<ReturnType<typeof useSftpTransfers>["folderConfirm"]>;

/** Confirmation before a folder upload overwrites existing files. */
export function FolderConfirmDialog({
  folderConfirm,
  onCancel,
  onConfirm,
}: {
  folderConfirm: FolderConfirm;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  return (
    <DialogFrame
      layer="sessionTab"
      title={t("fileBrowser.folderUpload.conflictTitle")}
      onClose={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button variant="danger" onClick={onConfirm}>
            {t("fileBrowser.overwrite")}
          </Button>
        </>
      }
    >
      <p className="mb-3">
        {t("fileBrowser.folderUpload.conflictBody", {
          name: folderConfirm.preview.folderName,
          count: folderConfirm.preview.overwrites.length,
        })}
      </p>
      <ul className="max-h-48 overflow-y-auto border border-border bg-surface-sunken px-2 py-1.5 font-mono text-xs break-all text-fg-muted">
        {folderConfirm.preview.overwrites.map((p) => (
          <li key={p}>{p}</li>
        ))}
      </ul>
    </DialogFrame>
  );
}
