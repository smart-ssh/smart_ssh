import type { TranslationKey } from "../../i18nTypes";
import { useEffect, useRef, useState, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useTranslation } from "react-i18next";
import {
  claimDroppedPaths,
  pickUploadFiles,
  pickUploadFolder,
  readLocalTextPreview,
  sftpExists,
  sftpLocalPathsAreFolders,
  sftpReadText,
  sftpUpload,
  sftpUploadFolder,
  sftpUploadFolderPreview,
} from "../../api";
import { isPointerInside } from "../../dropZone";
import {
  onSftpTransferFinished,
  onSftpTransferProgress,
  onSftpTransferStarted,
} from "../../events";
import { joinPath, localBaseName } from "../../remotePath";
import { nextRate } from "../../transferRate";
import { showToast } from "../../toastBus";
import {
  classifyPreviewReadFailure,
  type PreviewReadFailure,
} from "../../previewReadFailure";
import type {
  FolderUploadPreviewDto,
  FolderUploadSummaryDto,
  RemoteEntryDto,
} from "../../types";
import { currentCaps, type Transfer, type UploadConflict } from "./shared";

interface UseSftpTransfersOptions {
  /** Only a visible panel takes part in OS drag and drop. */
  isVisible: boolean;
  /** The panel element the drop position is checked against. */
  panelRef: RefObject<HTMLElement | null>;
  /** Expected user of the elevated channel (`null` = normal channel). */
  channelUser: string | null;
  path: string;
  pathRef: RefObject<string>;
  load: (targetPath: string, userOverride?: string | null) => void;
  /** Listed entries, used for the size of a conflicting remote file. */
  entries: RemoteEntryDto[];
  notifyOk: (key: TranslationKey, options: Record<string, unknown>, action?: () => void) => void;
  notifyFailed: (key: TranslationKey, name: string, err: unknown) => void;
  /** Suffix naming the elevated mode, appended to toasts. */
  modeSuffix: string;
}

/**
 * Uploads and downloads-in-flight of one session's file browser: the transfer
 * list fed by the `sftp-transfer-*` events (filtered by session id), upload
 * conflicts, the folder upload flow and OS drag and drop.
 */
export function useSftpTransfers(
  sessionId: string,
  {
    isVisible,
    panelRef,
    channelUser,
    path,
    pathRef,
    load,
    entries,
    notifyOk,
    notifyFailed,
    modeSuffix,
  }: UseSftpTransfersOptions,
) {
  const { t } = useTranslation();
  // Upload overwrite diff preview (Spec 0054, Teil 3): `remoteText` is `null`
  // when the remote file was not readable as text.
  const [uploadConflict, setUploadConflict] = useState<UploadConflict | null>(null);
  // Issue #128: folder upload. `folderConfirm` = the list of existing remote
  // files the upload would overwrite (one confirmation for the whole folder,
  // Spec 0054 Teil 3); `folderSummary` = result with skipped/failed entries.
  const [folderConfirm, setFolderConfirm] = useState<{
    localPath: string;
    remoteDir: string;
    preview: FolderUploadPreviewDto;
  } | null>(null);
  const [folderSummary, setFolderSummary] = useState<FolderUploadSummaryDto | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [dragOver, setDragOver] = useState(false);

  // Issue #621: subscribe once per session. The handlers read the current
  // channel user / path / load through refs, so toggling elevated mode never
  // unsubscribes (an event in the re-subscription gap would be lost).
  const channelUserRef = useRef(channelUser);
  channelUserRef.current = channelUser;
  const loadRef = useRef(load);
  loadRef.current = load;
  // Issue #655: file name of the edit upload in flight (matches the transfer
  // events of that upload) and the ids of its hidden transfers.
  const editUploadingNameRef = useRef<string | null>(null);
  const editTransferIdsRef = useRef(new Set<string>());
  useEffect(() => {
    const unlisten = [
      onSftpTransferStarted((event) => {
        if (event.sessionId !== sessionId) return;
        // Issue #655: an upload of the edit flow reports its errors only in
        // the edit footer — no transfer row for it.
        const editName = editUploadingNameRef.current;
        if (editName !== null && event.kind === "upload" && event.fileName === editName) {
          editTransferIdsRef.current.add(event.transferId);
          return;
        }
        setTransfers((prev) => [
          ...prev,
          {
            id: event.transferId,
            kind: event.kind,
            fileName: event.fileName,
            totalBytes: event.totalBytes,
            error: null,
            startedElevated: channelUserRef.current !== null,
          },
        ]);
      }),
      onSftpTransferProgress((event) => {
        if (event.sessionId !== sessionId) return;
        if (editTransferIdsRef.current.has(event.transferId)) return;
        if (event.entries) {
          // Issue #658: recursive delete/chmod count entries, monotonically.
          const entries = event.entries;
          setTransfers((prev) =>
            prev.map((row) =>
              row.id === event.transferId
                ? {
                    ...row,
                    entries: {
                      done: Math.max(row.entries?.done ?? 0, entries.done),
                      total: entries.total ?? row.entries?.total ?? null,
                    },
                  }
                : row,
            ),
          );
          return;
        }
        // Issue #623: folder transfers show the aggregate over all files.
        const done = event.folder ? event.folder.bytesDone : event.bytesDone;
        const total = event.folder ? event.folder.bytesTotal : event.totalBytes;
        const now = Date.now();
        setTransfers((prev) =>
          prev.map((row) =>
            row.id === event.transferId
              ? {
                  ...row,
                  bytesDone: Math.max(row.bytesDone ?? 0, done),
                  totalBytes: total ?? row.totalBytes,
                  rate: nextRate(row.rate, now, Math.max(row.bytesDone ?? 0, done), total ?? row.totalBytes),
                  folder: event.folder
                    ? { filesDone: event.folder.filesDone, filesTotal: event.folder.filesTotal }
                    : row.folder,
                }
              : row,
          ),
        );
      }),
      onSftpTransferFinished((event) => {
        if (event.sessionId !== sessionId) return;
        if (editTransferIdsRef.current.delete(event.transferId)) {
          // Issue #655: hidden edit-flow transfer; the footer shows errors,
          // `handleConfirmEditUpload` reloads the listing.
          return;
        }
        if (event.stopped) {
          // Issue #622: cancelled / stopped — a final state, not an error.
          const stopped = event.stopped;
          setTransfers((prev) =>
            prev.map((row) =>
              row.id === event.transferId
                ? {
                    ...row,
                    stopped,
                    incompleteRemotePath: event.incompleteRemotePath ?? undefined,
                    entriesDone: event.entriesDone ?? undefined,
                  }
                : row,
            ),
          );
        } else if (event.error) {
          setTransfers((prev) =>
            prev.map((t) =>
              t.id === event.transferId
                ? { ...t, error: event.error, entriesDone: event.entriesDone ?? undefined }
                : t,
            ),
          );
        } else {
          const elevationEnded = channelUserRef.current === null;
          setTransfers((prev) =>
            prev.flatMap((t) => {
              if (t.id !== event.transferId) return [t];
              // Issue #621 / Spec 0085: completed although elevated mode was
              // switched off meanwhile — keep the row as done, with a note.
              return t.startedElevated && elevationEnded ? [{ ...t, doneElevationEnded: true }] : [];
            }),
          );
          // Erfolgreicher Transfer kann den Inhalt des aktuell offenen
          // Verzeichnisses geändert haben (Upload/Download berührt aber nur
          // die Ziel-/Quelldatei — ein Refresh ist trotzdem billig genug,
          // um nicht auf mögliche Rennbedingungen zwischen Event und
          // Nutzer-Navigation Rücksicht nehmen zu müssen).
          loadRef.current(pathRef.current);
        }
      }),
    ];
    return () => {
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
    // Spec 0067: reload after a transfer uses the current channel (via refs).
  }, [sessionId, pathRef]);

  // Spec 0020, Abschnitt 5.1: Upload per Drag-and-Drop aus dem Betriebssystem.
  // Nur aktiv, während dieser Tab UND die Dateien-Ansicht sichtbar sind (s.
  // `isVisible`-Doc-Kommentar oben) — `onDragDropEvent` ist global für das
  // gesamte Fenster, ohne dieses Gate würden gleichzeitig mehrere
  // (unsichtbar) gemountete Dateibrowser-Instanzen anderer Tabs denselben
  // Drop ebenfalls als Upload in ihr jeweiliges Verzeichnis interpretieren.
  useEffect(() => {
    // Issue #679: no OS drag and drop on Android.
    if (!isVisible || !currentCaps().dragDrop) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        // Position-based routing: only events over this panel count; a drop
        // elsewhere is left to the other zone (or ignored).
        const payload = event.payload;
        if (payload.type === "over" || payload.type === "enter") {
          setDragOver(isPointerInside(panelRef.current, payload.position));
        } else if (payload.type === "leave") {
          setDragOver(false);
        } else if (payload.type === "drop") {
          setDragOver(false);
          if (!isPointerInside(panelRef.current, payload.position)) return;
          // Issue #29: always the `uploadMany` of the latest render — and
          // with it the current `channelUser` and `path` — never the one of
          // the render that registered this listener. Taken now, at the
          // drop, not once the claim below returns.
          const upload = uploadManyRef.current;
          // Issue #89: the payload's paths are not trusted. The backend
          // captured the same drop from the native window event; claiming it
          // grants those paths to this session and returns them.
          claimDroppedPaths(sessionId)
            .then((paths) => {
              if (paths.length > 0) upload(paths);
            })
            // Fails only if the session is already gone (tab closing).
            .catch((err) => console.warn("Could not claim the dropped files:", err));
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // Issue #29: registered only per visibility. Channel and path are read
    // through `uploadManyRef` when the drop is handled, so there is no window
    // between a channel switch (banner already committed) and a re-registered
    // listener in which a drop would still use the previous channel.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isVisible, sessionId]);

  /** Spec 0054, Teil 3: "Hochladen ... Überschreibt bestehende →
   * Diff-Vorschau (0020)". Kein Dialog für den unkritischen Normalfall
   * (Zielpfad existiert noch nicht — direkter Upload wie bisher), nur bei
   * einer echten Kollision wird die Diff-Vorschau (`uploadConflict`)
   * aufgebaut: lokale Seite über `readLocalTextPreview` (graceful bei
   * Binär/zu groß), Remote-Seite über `sftpReadText` — dessen Fehlerfall
   * (Binär/zu groß) fällt auf die bereits geladene `entries`-Liste zurück,
   * deren `size` kennt jeder sichtbare Eintrag schon ohne weiteren
   * Backend-Aufruf. */
  const startUpload = async (
    localPath: string,
    remotePath: string,
    reportFailure = true,
  ): Promise<"uploaded" | "conflict" | { failed: unknown }> => {
    try {
      const exists = await sftpExists(sessionId, remotePath, channelUser);
      if (!exists) {
        await sftpUpload(sessionId, localPath, remotePath, channelUser);
        return "uploaded";
      }
      const [localPreview, remoteRead] = await Promise.all([
        readLocalTextPreview(sessionId, localPath),
        sftpReadText(sessionId, remotePath, channelUser).then(
          (text) => ({ text, failure: null as PreviewReadFailure | null }),
          (err) => ({ text: null as string | null, failure: classifyPreviewReadFailure(err) }),
        ),
      ]);
      const remoteText = remoteRead.text;
      const remoteSize = entries.find((e) => e.path === remotePath)?.size ?? 0;
      setUploadConflict({
        localPath,
        remotePath,
        localPreview,
        remoteText,
        remoteReadFailure: remoteRead.failure,
        remoteSize,
      });
      return "conflict";
    } catch (err) {
      if (reportFailure) notifyFailed("fileToasts.uploadFailed", localBaseName(localPath), err);
      return { failed: err };
    }
  };

  /** Spec 0067, Teil B: mehrere Dateien → eine Sammelmeldung statt einer
   * pro Datei. Fehler kommen einzeln (mit Grund), Konflikte melden sich
   * nach ihrer Bestätigung selbst. */
  const uploadMany = async (allPaths: string[]) => {
    // Issue #128: folders go the folder way (preview, one confirmation,
    // summary); everything else keeps the per-file flow unchanged.
    const folderFlags = await sftpLocalPathsAreFolders(sessionId, allPaths).catch(() =>
      allPaths.map(() => false),
    );
    const localPaths = allPaths.filter((_, i) => !folderFlags[i]);
    const remoteDir = path;
    for (const folder of allPaths.filter((_, i) => folderFlags[i])) {
      void startFolderUpload(folder, remoteDir);
    }
    if (localPaths.length === 0) return;
    const outcomes = await Promise.all(
      localPaths.map((localPath) =>
        startUpload(localPath, joinPath(path, localBaseName(localPath)), false),
      ),
    );
    const uploaded = localPaths.filter((_, i) => outcomes[i] === "uploaded");
    if (uploaded.length === 1) {
      notifyOk("fileToasts.uploadedFile", { name: localBaseName(uploaded[0]) });
    } else if (uploaded.length > 1) {
      notifyOk("fileToasts.uploadedMany", { count: uploaded.length });
    }
    // Fehler ebenfalls zusammengefasst — keine Flut stehender Fehler-Toasts
    // (spec-reviewer-Fund, Spec 0067, B2).
    const failures = localPaths.flatMap((localPath, i) => {
      const outcome = outcomes[i];
      return typeof outcome === "object" ? [{ localPath, error: outcome.failed }] : [];
    });
    if (failures.length === 1) {
      notifyFailed("fileToasts.uploadFailed", localBaseName(failures[0].localPath), failures[0].error);
    } else if (failures.length > 1) {
      notifyFailed("fileToasts.uploadFailedMany", String(failures.length), failures[0].error);
    }
  };
  /** Issue #128: preview first; only if the folder would overwrite existing
   * remote files does the user get the (single) confirmation. */
  const startFolderUpload = async (localPath: string, remoteDir: string) => {
    try {
      const preview = await sftpUploadFolderPreview(sessionId, localPath, remoteDir, channelUser);
      if (preview.overwrites.length > 0) {
        setFolderConfirm({ localPath, remoteDir, preview });
        return;
      }
      await runFolderUpload(localPath, remoteDir, preview.folderName, []);
    } catch (err) {
      notifyFailed("fileToasts.uploadFolderFailed", localBaseName(localPath), err);
    }
  };

  const runFolderUpload = async (
    localPath: string,
    remoteDir: string,
    folderName: string,
    confirmedOverwrites: string[],
  ) => {
    showToast({ kind: "info", message: t("fileBrowser.uploadFolder") + ": " + folderName + modeSuffix });
    try {
      const summary = await sftpUploadFolder(
        sessionId,
        localPath,
        remoteDir,
        confirmedOverwrites,
        channelUser,
      );
      if (summary.skipped.length === 0 && summary.failed.length === 0 && summary.notAttempted === 0) {
        notifyOk("fileToasts.uploadedFolder", { name: summary.folderName, count: summary.filesUploaded });
      } else {
        setFolderSummary(summary);
      }
      if (remoteDir === path) load(path);
    } catch (err) {
      notifyFailed("fileToasts.uploadFolderFailed", folderName, err);
    }
  };

  const handleConfirmFolderUpload = () => {
    if (!folderConfirm) return;
    const { localPath, remoteDir, preview } = folderConfirm;
    setFolderConfirm(null);
    void runFolderUpload(localPath, remoteDir, preview.folderName, preview.overwrites);
  };

  const handleUploadFolderButton = async () => {
    // The dialog runs in the backend and grants the picked folder to this
    // session; the webview never names a path itself (Issue #89).
    const picked = await pickUploadFolder(sessionId, t("fileBrowser.uploadFolderDialogTitle"));
    if (!picked) return;
    void startFolderUpload(picked, path);
  };

  // Issue #29: updated during render, so a drop handled after the commit
  // of a channel switch or a navigation already sees the new `channelUser`
  // and `path` — before any passive effect has run.
  const uploadManyRef = useRef(uploadMany);
  uploadManyRef.current = uploadMany;

  const handleConfirmUpload = () => {
    if (!uploadConflict) return;
    const { localPath, remotePath } = uploadConflict;
    setUploadConflict(null);
    sftpUpload(sessionId, localPath, remotePath, channelUser)
      .then(() => {
        notifyOk("fileToasts.uploadedFile", { name: localBaseName(localPath) });
        load(path);
      })
      .catch((err) => notifyFailed("fileToasts.uploadFailed", localBaseName(localPath), err));
  };

  const handleUploadButton = async () => {
    // Issue #89: the dialog runs in the backend, which grants the picked
    // files to this session; the webview never names a path itself.
    const picked = await pickUploadFiles(sessionId, t("fileBrowser.uploadDialogTitle"));
    if (!picked || picked.length === 0) return;
    uploadMany(picked);
  };

  const dismissTransfer = (id: string) => setTransfers((prev) => prev.filter((x) => x.id !== id));

  return {
    transfers,
    dismissTransfer,
    uploadConflict,
    setUploadConflict,
    folderConfirm,
    setFolderConfirm,
    folderSummary,
    setFolderSummary,
    dragOver,
    editUploadingNameRef,
    uploadMany,
    handleConfirmUpload,
    handleUploadButton,
    handleUploadFolderButton,
    handleConfirmFolderUpload,
  };
}
