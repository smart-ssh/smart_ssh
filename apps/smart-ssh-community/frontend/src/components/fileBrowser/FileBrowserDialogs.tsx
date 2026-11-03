import { LocalEditCopyEditor } from "../LocalEditCopyEditor";
import { ElevationFailureDialog } from "./ElevationFailureDialog";
import { EditUploadDialog } from "./EditUploadDialog";
import { FileOperationDialogs } from "./FileOperationDialogs";
import { FolderConfirmDialog } from "./FolderConfirmDialog";
import { FolderSummaryDialog } from "./FolderSummaryDialog";
import { LocalEditStatusBar } from "./LocalEditStatusBar";
import { StopPromptDialog } from "./StopPromptDialog";
import { UploadConflictDialog } from "./UploadConflictDialog";
import { currentCaps } from "./shared";
import type { useElevation } from "./useElevation";
import type { useFileOperations } from "./useFileOperations";
import type { useLocalEditFlow } from "./useLocalEditFlow";
import type { useSftpTransfers } from "./useSftpTransfers";

type Elevation = ReturnType<typeof useElevation>;
type Transfers = ReturnType<typeof useSftpTransfers>;

interface FileBrowserDialogsProps {
  sessionId: string;
  ops: ReturnType<typeof useFileOperations>;
  edit: ReturnType<typeof useLocalEditFlow>;
  elevated: boolean;
  elevatedUser: string | null;
  channelUser: string | null;
  transfers: Pick<
    Transfers,
    | "uploadConflict"
    | "setUploadConflict"
    | "handleConfirmUpload"
    | "folderConfirm"
    | "setFolderConfirm"
    | "handleConfirmFolderUpload"
    | "folderSummary"
    | "setFolderSummary"
  >;
  elevationState: Pick<
    Elevation,
    | "stopPrompt"
    | "setStopPrompt"
    | "confirmStopPrompt"
    | "elevationFailure"
    | "setElevationFailure"
  >;
}

/** The status bar of the local-edit flow and every dialog of the file browser. */
export function FileBrowserDialogs({
  sessionId,
  ops,
  edit,
  elevated,
  elevatedUser,
  channelUser,
  transfers,
  elevationState,
}: FileBrowserDialogsProps) {
  const caps = currentCaps();
  const { localEdit } = edit;
  const { uploadConflict, folderConfirm, folderSummary } = transfers;
  const { stopPrompt, elevationFailure } = elevationState;
  return (
    <>
      {/* Spec 0054, Teil 4: Statusleiste für den "Lokal öffnen"-Flow —
          zeigt an, solange eine Bearbeitung aktiv ist (`editing`), bietet
          bei einer erkannten lokalen Änderung (`changed`) den Upload an,
          und macht das "Bearbeitung beenden" jederzeit erreichbar (Spec:
          "Watcher stoppt, wenn der Nutzer den Flow beendet"). */}
      <LocalEditStatusBar
        localEdit={localEdit}
        editIdentityMismatch={edit.editIdentityMismatch}
        channelUser={channelUser}
        externalEditor={caps.externalEditor}
        onOfferUpload={edit.handleOfferEditUpload}
        onOpenInAppEditor={() => edit.setInAppEditorOpen(true)}
      />

      {edit.inAppEditorOpen && localEdit.session && !caps.externalEditor && (
        <LocalEditCopyEditor
          sessionId={sessionId}
          localPath={localEdit.session.localPath}
          fileName={localEdit.session.entry.name}
          onClose={() => edit.setInAppEditorOpen(false)}
        />
      )}

      {edit.editUploadOffer && (
        <EditUploadDialog
          offer={edit.editUploadOffer}
          uploadAs={edit.editUploadAs}
          sessionElevatedUser={localEdit.session?.elevatedUser}
          onCancel={edit.closeEditUploadOffer}
          onConfirm={edit.handleConfirmEditUpload}
        />
      )}

      <FileOperationDialogs
        ops={ops}
        elevated={elevated}
        elevatedUser={elevatedUser}
      />

      {uploadConflict && (
        <UploadConflictDialog
          conflict={uploadConflict}
          elevated={elevated}
          elevatedUser={elevatedUser}
          onCancel={() => transfers.setUploadConflict(null)}
          onConfirm={transfers.handleConfirmUpload}
        />
      )}

      {folderConfirm && (
        <FolderConfirmDialog
          folderConfirm={folderConfirm}
          onCancel={() => transfers.setFolderConfirm(null)}
          onConfirm={transfers.handleConfirmFolderUpload}
        />
      )}

      {folderSummary && (
        <FolderSummaryDialog
          folderSummary={folderSummary}
          onClose={() => transfers.setFolderSummary(null)}
        />
      )}

      {stopPrompt && (
        <StopPromptDialog
          operations={stopPrompt.operations}
          onCancel={() => elevationState.setStopPrompt(null)}
          onConfirm={elevationState.confirmStopPrompt}
        />
      )}

      {elevationFailure && (
        <ElevationFailureDialog
          result={elevationFailure}
          onClose={() => elevationState.setElevationFailure(null)}
        />
      )}
    </>
  );
}
