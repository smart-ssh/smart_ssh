import { Trans, useTranslation } from "react-i18next";
import { Button, DialogFrame } from "../ui";
import { ChmodDialog } from "./ChmodDialog";
import { ChownDialog } from "./ChownDialog";
import { FilePropertiesDialog } from "./FilePropertiesDialog";
import { RenamePrompt } from "./RenamePrompt";
import type { useFileOperations } from "./useFileOperations";

interface FileOperationDialogsProps {
  ops: ReturnType<typeof useFileOperations>;
  elevated: boolean;
  elevatedUser: string | null;
}

/** The one open operation dialog of the file browser (see `useFileOperations`). */
export function FileOperationDialogs({ ops, elevated, elevatedUser }: FileOperationDialogsProps) {
  const { t } = useTranslation();
  const { dialog, dispatch, close } = ops;
  switch (dialog.kind) {
    case "none":
      return null;
    case "properties":
      return <FilePropertiesDialog entry={dialog.entry} onClose={close} />;
    case "rename":
      return (
        <RenamePrompt
          initialValue={dialog.value}
          onChange={(value) => dispatch({ type: "renameChanged", value })}
          onCancel={close}
          onConfirm={ops.handleConfirmRename}
        />
      );
    case "mkdir":
      return (
        <RenamePrompt
          title={t("fileBrowser.renamePrompt.newFolderTitle")}
          initialValue={dialog.value}
          error={dialog.error}
          onChange={(value) => dispatch({ type: "nameChanged", value })}
          onCancel={close}
          onConfirm={ops.handleConfirmMkdir}
        />
      );
    case "newFile":
      return (
        <RenamePrompt
          title={t("fileBrowser.renamePrompt.newFileTitle")}
          initialValue={dialog.value}
          error={dialog.error}
          onChange={(value) => dispatch({ type: "nameChanged", value })}
          onCancel={close}
          onConfirm={ops.handleConfirmNewFile}
        />
      );
    case "delete": {
      const { entry, preview } = dialog;
      return (
        <DialogFrame
          layer="sessionTab"
          size="sm"
          title={
            elevated
              ? t(entry.isDir ? "fileElevation.deleteDirTitle" : "fileElevation.deleteFileTitle", {
                  user: elevatedUser,
                })
              : entry.isDir
                ? t("fileBrowser.deleteDialog.dirTitle")
                : t("fileBrowser.deleteDialog.fileTitle")
          }
          onClose={close}
          actions={
            <>
              <Button onClick={close}>{t("fileBrowser.cancel")}</Button>
              <Button variant="danger" onClick={ops.handleConfirmDelete}>
                {t("fileBrowser.delete")}
              </Button>
            </>
          }
        >
          <p>
            <span className="font-mono text-xs break-all">{entry.path}</span>{" "}
            {t("fileBrowser.deleteDialog.body")}
            {entry.isDir &&
              (preview ? (
                <>
                  {" "}
                  <Trans
                    t={t}
                    i18nKey="fileBrowser.deleteDialog.contents"
                    values={{
                      fileCount: preview.fileCount,
                      dirCount: preview.dirCount,
                      files: t("fileBrowser.deleteDialog.files", { count: preview.fileCount }),
                      dirs: t("fileBrowser.deleteDialog.dirs", { count: preview.dirCount }),
                    }}
                    components={{ strong: <strong /> }}
                  />
                </>
              ) : (
                ` ${t("fileBrowser.deleteDialog.counting")}`
              ))}
          </p>
        </DialogFrame>
      );
    }
    case "chmod":
      return (
        <ChmodDialog
          entry={dialog.entry}
          modeNote={elevated ? t("fileElevation.chmodNote", { user: elevatedUser }) : null}
          onCancel={close}
          onConfirm={ops.handleConfirmChmod}
        />
      );
    case "chown":
      return (
        <ChownDialog
          entry={dialog.entry}
          modeNote={elevated ? t("fileElevation.chownNote", { user: elevatedUser }) : null}
          onCancel={close}
          onConfirm={ops.handleConfirmChown}
        />
      );
    case "moveCollision":
      return (
        <DialogFrame
          layer="sessionTab"
          size="sm"
          title={
            elevated && !dialog.targetIsDir
              ? t("fileElevation.moveCollisionTitle", { user: elevatedUser })
              : t("fileBrowser.moveCollision.title")
          }
          onClose={close}
          actions={
            <>
              <Button onClick={close}>{t("fileBrowser.cancel")}</Button>
              {!dialog.targetIsDir && (
                <Button variant="danger" onClick={ops.handleConfirmMoveCollision}>
                  {t("fileBrowser.overwrite")}
                </Button>
              )}
            </>
          }
        >
          <p>
            <span className="font-mono text-xs break-all">{dialog.to}</span>{" "}
            {dialog.targetIsDir
              ? t("fileBrowser.moveCollision.folderBody")
              : t("fileBrowser.moveCollision.body")}
          </p>
        </DialogFrame>
      );
  }
}
