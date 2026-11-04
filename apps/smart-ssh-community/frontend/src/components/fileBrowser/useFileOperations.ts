import type { TranslationKey } from "../../i18nTypes";
import { useEffect, useReducer } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorCode,
  commandErrorMessage,
  sftpChmod,
  sftpChown,
  sftpCreateFile,
  sftpDelete,
  sftpDeletePreview,
  sftpExists,
  sftpMkdir,
  sftpRename,
  sftpStat,
} from "../../api";
import { translateErrorCode } from "../../errorCodes";
import { displayPath, joinPath, localBaseName, parentPath } from "../../remotePath";
import { showToast } from "../../toastBus";
import type { DeletePreviewDto, RemoteEntryDto } from "../../types";
import { isValidEntryName, type MoveKind } from "./shared";
import type { NotifyFailed, NotifyOk } from "./useFileNotify";

/**
 * The operation dialog that is open. Only one can be open at a time: opening
 * another replaces the current one (the dialogs are modal, so the user cannot
 * open a second one while one is showing).
 */
export type OperationDialog =
  | { kind: "none" }
  | { kind: "properties"; entry: RemoteEntryDto }
  | { kind: "rename"; entry: RemoteEntryDto; value: string }
  | { kind: "delete"; entry: RemoteEntryDto; preview: DeletePreviewDto | null }
  | { kind: "mkdir"; value: string; error: string | null }
  | { kind: "newFile"; value: string; error: string | null }
  | { kind: "chmod"; entry: RemoteEntryDto }
  | { kind: "chown"; entry: RemoteEntryDto }
  | {
      kind: "moveCollision";
      from: string;
      to: string;
      moveKind: MoveKind;
      /** Issue #625: ein Ordner als Ziel lässt sich nicht ersetzen. */
      targetIsDir: boolean;
    };

export interface FileOperationsState {
  dialog: OperationDialog;
  /** Spec 0054, Teil 3: the entry cut for "Verschieben"; `null` = nothing cut.
   * Not a dialog: it stays set while no dialog is open. */
  cutEntry: RemoteEntryDto | null;
}

export type FileOperationsAction =
  | { type: "open"; dialog: Exclude<OperationDialog, { kind: "none" }> }
  /** Close (cancel or submit accepted by the user): no dialog open. */
  | { type: "close" }
  | { type: "renameChanged"; value: string }
  | { type: "nameChanged"; value: string }
  /** Submit rejected: the name is not valid (mkdir / new file). */
  | { type: "nameInvalid"; error: string }
  /** Delete preview arrived; ignored if another target is showing now. */
  | { type: "deletePreview"; path: string; preview: DeletePreviewDto | null }
  | { type: "cut"; entry: RemoteEntryDto }
  | { type: "clearCut" };

export const initialFileOperationsState: FileOperationsState = {
  dialog: { kind: "none" },
  cutEntry: null,
};

/** Pure reducer of the file operation dialogs (exported for tests). */
export function fileOperationsReducer(
  state: FileOperationsState,
  action: FileOperationsAction,
): FileOperationsState {
  const { dialog } = state;
  switch (action.type) {
    case "open":
      return { ...state, dialog: action.dialog };
    case "close":
      return dialog.kind === "none" ? state : { ...state, dialog: { kind: "none" } };
    case "renameChanged":
      return dialog.kind === "rename" ? { ...state, dialog: { ...dialog, value: action.value } } : state;
    case "nameChanged":
      return dialog.kind === "mkdir" || dialog.kind === "newFile"
        ? { ...state, dialog: { ...dialog, value: action.value, error: null } }
        : state;
    case "nameInvalid":
      return dialog.kind === "mkdir" || dialog.kind === "newFile"
        ? { ...state, dialog: { ...dialog, error: action.error } }
        : state;
    case "deletePreview":
      return dialog.kind === "delete" && dialog.entry.path === action.path
        ? { ...state, dialog: { ...dialog, preview: action.preview } }
        : state;
    case "cut":
      return { ...state, cutEntry: action.entry };
    case "clearCut":
      return state.cutEntry === null ? state : { ...state, cutEntry: null };
  }
}

interface UseFileOperationsOptions {
  channelUser: string | null;
  path: string;
  load: (path: string) => void;
  notifyOk: NotifyOk;
  notifyFailed: NotifyFailed;
  modeSuffix: string;
  setError: (message: string) => void;
  /** Closes the entry menu (⋮ / context menu) that started the action. */
  closeMenu: () => void;
}

/**
 * Rename, move (cut/paste), delete, new folder/file, chmod, chown and
 * properties of the file browser (Spec 0054). The dialog state is one
 * reducer; each `handleConfirm*` runs the backend call and reports the result.
 */
export function useFileOperations(
  sessionId: string,
  { channelUser, path, load, notifyOk, notifyFailed, modeSuffix, setError, closeMenu }: UseFileOperationsOptions,
) {
  const { t } = useTranslation();
  const [state, dispatch] = useReducer(fileOperationsReducer, initialFileOperationsState);
  const { dialog, cutEntry } = state;

  // Spec 0054, Teil 3: "Löschen ... bei Ordnern mit Hinweis auf rekursives
  // Löschen und Anzahl" — Vorschau nachladen, sobald ein Ordner als
  // Lösch-Ziel gewählt wird (bei einer Datei ist die Zählung trivial, kein
  // Backend-Aufruf nötig).
  const deleteTarget = dialog.kind === "delete" ? dialog.entry : null;
  useEffect(() => {
    if (!deleteTarget || !deleteTarget.isDir) return;
    let cancelled = false;
    sftpDeletePreview(sessionId, deleteTarget.path, channelUser)
      .then((preview) => {
        if (!cancelled) dispatch({ type: "deletePreview", path: deleteTarget.path, preview });
      })
      .catch((err) => {
        if (!cancelled) setError(commandErrorMessage(err));
      });
    return () => {
      cancelled = true;
    };
    // Spec-Reviewer-Fund: die Vorschau muss im selben Modus zählen, in dem
    // danach gelöscht wird.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the preview counts in the same mode as the later delete (spec-reviewer finding)
  }, [sessionId, deleteTarget, channelUser]);

  const open = (next: Exclude<OperationDialog, { kind: "none" }>) => {
    closeMenu();
    dispatch({ type: "open", dialog: next });
  };
  const close = () => dispatch({ type: "close" });

  const handleShowProperties = (entry: RemoteEntryDto) => open({ kind: "properties", entry });
  const handleRename = (entry: RemoteEntryDto) => open({ kind: "rename", entry, value: entry.name });
  const handleDelete = (entry: RemoteEntryDto) => open({ kind: "delete", entry, preview: null });
  const handleChmod = (entry: RemoteEntryDto) => open({ kind: "chmod", entry });
  const handleChown = (entry: RemoteEntryDto) => open({ kind: "chown", entry });
  const handleCut = (entry: RemoteEntryDto) => {
    closeMenu();
    dispatch({ type: "cut", entry });
  };
  const openMkdir = () => dispatch({ type: "open", dialog: { kind: "mkdir", value: "", error: null } });
  const openNewFile = () => dispatch({ type: "open", dialog: { kind: "newFile", value: "", error: null } });

  const handleConfirmDelete = () => {
    if (dialog.kind !== "delete") return;
    const target = dialog.entry;
    const preview = dialog.preview;
    close();
    sftpDelete(sessionId, target.path, channelUser)
      .then(() => {
        if (target.isDir && preview) {
          notifyOk("fileToasts.deletedDir", {
            name: target.name,
            files: preview.fileCount,
            dirs: Math.max(preview.dirCount - 1, 0),
          });
        } else {
          notifyOk("fileToasts.deletedFile", { name: target.name });
        }
        load(path);
      })
      .catch((err) => notifyFailed("fileToasts.deleteFailed", target.name, err));
  };

  /** Spec 0054, Teil 3: gemeinsamer Weg für "Umbenennen" (Ziel im selben
   * Ordner) UND "Verschieben" (Ziel in einem anderen Ordner) — SFTP
   * `RENAME` ist in beiden Fällen derselbe Aufruf (s.
   * `crate::commands::sftp_rename`s Doc-Kommentar). Kollisionsprüfung
   * zuerst: existiert das Ziel schon, wird NICHT stillschweigend
   * überschrieben, sondern erst nachgefragt (`moveCollision`). */
  const notifyMoved = (from: string, to: string, kind: MoveKind) => {
    if (kind === "rename") {
      notifyOk("fileToasts.renamed", { from: localBaseName(from), to: localBaseName(to) });
    } else {
      notifyOk("fileToasts.moved", { name: localBaseName(from), dir: displayPath(parentPath(to)) });
    }
  };

  const performMove = async (from: string, to: string, kind: MoveKind) => {
    try {
      const exists = await sftpExists(sessionId, to, channelUser);
      if (exists) {
        // Kann `stat` nicht antworten, gilt das Ziel als Datei; das Backend
        // lehnt das Ersetzen eines Ordners ohnehin ab.
        const targetIsDir = await sftpStat(sessionId, to, channelUser).then(
          (entry) => entry.isDir,
          () => false,
        );
        dispatch({ type: "open", dialog: { kind: "moveCollision", from, to, moveKind: kind, targetIsDir } });
        return;
      }
      await sftpRename(sessionId, from, to, channelUser);
      notifyMoved(from, to, kind);
      load(path);
    } catch (err) {
      notifyFailed("fileToasts.moveFailed", localBaseName(from), err);
    }
  };

  const handleConfirmMoveCollision = () => {
    if (dialog.kind !== "moveCollision") return;
    const { from, to, moveKind } = dialog;
    close();
    sftpRename(sessionId, from, to, channelUser, true)
      .then(() => {
        notifyMoved(from, to, moveKind);
        load(path);
      })
      .catch((err) => notifyFailed("fileToasts.moveFailed", localBaseName(from), err));
  };

  const handleConfirmRename = () => {
    if (dialog.kind !== "rename") return;
    const trimmed = dialog.value.trim();
    const from = dialog.entry.path;
    const unchanged = !trimmed || trimmed === dialog.entry.name;
    close();
    if (unchanged) return;
    performMove(from, joinPath(path, trimmed), "rename");
  };

  /** Issue #570: ein Fehler aus "Neuer Ordner"/"Neue Datei" — "Name
   * existiert" und "Zugriff verweigert" mit übersetztem Grund. */
  const notifyCreateFailed = (key: TranslationKey, name: string, err: unknown) => {
    const code = commandErrorCode(err);
    notifyFailed(
      key,
      name,
      code === "SSH_SFTP_ALREADY_EXISTS" || code === "SSH_SFTP_PERMISSION_DENIED"
        ? { message: translateErrorCode(t, code, commandErrorMessage(err)) }
        : err,
    );
  };

  const confirmCreate = (
    kind: "mkdir" | "newFile",
    create: typeof sftpMkdir,
    doneKey: TranslationKey,
    failedKey: TranslationKey,
  ) => {
    if (dialog.kind !== kind) return;
    const name = dialog.value.trim();
    if (!isValidEntryName(name)) {
      dispatch({ type: "nameInvalid", error: t("fileBrowser.renamePrompt.invalidName") });
      return;
    }
    close();
    create(sessionId, joinPath(path, name), channelUser)
      .then(() => {
        notifyOk(doneKey, { name });
        load(path);
      })
      .catch((err) => notifyCreateFailed(failedKey, name, err));
  };
  const handleConfirmMkdir = () =>
    confirmCreate("mkdir", sftpMkdir, "fileToasts.mkdirDone", "fileToasts.mkdirFailed");
  const handleConfirmNewFile = () =>
    confirmCreate("newFile", sftpCreateFile, "fileToasts.createFileDone", "fileToasts.createFileFailed");

  /** Spec 0054, Teil 3: "Einfügen" — verschiebt den ausgeschnittenen
   * Eintrag in das aktuell angezeigte Verzeichnis. */
  const handlePaste = () => {
    if (!cutEntry) return;
    const target = joinPath(path, cutEntry.name);
    dispatch({ type: "clearCut" });
    if (target === cutEntry.path) return; // bereits hier, kein no-op-Fehler
    performMove(cutEntry.path, target, "move");
  };

  const handleConfirmChmod = (mode: number, recursive: boolean, applySpecialBits: boolean) => {
    if (dialog.kind !== "chmod") return;
    const target = dialog.entry;
    close();
    const modeText = mode.toString(8).padStart(3, "0");
    sftpChmod(sessionId, target.path, mode, recursive, applySpecialBits, channelUser)
      .then((count) => {
        if (recursive && target.isDir) {
          notifyOk("fileToasts.chmodRecursive", { name: target.name, mode: modeText, count });
        } else {
          notifyOk("fileToasts.chmodDone", { name: target.name, mode: modeText });
        }
        load(path);
      })
      .catch((err) => notifyFailed("fileToasts.chmodFailed", target.name, err));
  };

  // Issue #654: a side the user did not touch arrives as `null` (unchanged).
  const handleConfirmChown = (owner: string | null, group: string | null, recursive: boolean) => {
    if (dialog.kind !== "chown") return;
    const target = dialog.entry;
    close();
    sftpChown(sessionId, target.path, owner, group, recursive, channelUser)
      .then((count) => {
        if (recursive && target.isDir) {
          notifyOk("fileToasts.chownRecursive", { name: target.name, count });
        } else {
          notifyOk("fileToasts.chownDone", { name: target.name });
        }
        load(path);
      })
      .catch((err) => {
        const code = commandErrorCode(err);
        if (code === "CHOWN_UNKNOWN_USER" || code === "CHOWN_UNKNOWN_GROUP") {
          showToast({
            kind: "error",
            message:
              t(code === "CHOWN_UNKNOWN_USER" ? "fileToasts.chownUnknownUser" : "fileToasts.chownUnknownGroup", {
                name: target.name,
              }) + modeSuffix,
          });
          return;
        }
        notifyFailed("fileToasts.chownFailed", target.name, err);
      });
  };

  return {
    dialog,
    cutEntry,
    dispatch,
    close,
    handleShowProperties,
    handleRename,
    handleDelete,
    handleChmod,
    handleChown,
    handleCut,
    handlePaste,
    openMkdir,
    openNewFile,
    handleConfirmDelete,
    handleConfirmMoveCollision,
    handleConfirmRename,
    handleConfirmMkdir,
    handleConfirmNewFile,
    handleConfirmChmod,
    handleConfirmChown,
  };
}
