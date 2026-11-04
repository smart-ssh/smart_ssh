import type { TranslationKey } from "../../i18nTypes";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  claimClipboardFiles,
  clipboardHasFiles,
  clipboardWriteText,
  commandErrorCode,
  commandErrorMessage,
  sftpCopyContent,
  sftpCopyFile,
} from "../../api";
import { translateErrorCode } from "../../errorCodes";
import { showToast } from "../../toastBus";
import type { RemoteEntryDto } from "../../types";
import type { NotifyFailed, NotifyOk } from "./useFileNotify";

/** Issue #357: ab dieser Größe zeigt "Dateiinhalt kopieren" Fortschritt. */
export const COPY_PROGRESS_FROM_BYTES = 5 * 1024 * 1024;

export interface CopyJob {
  requestId: string;
  name: string;
  done: number;
  total: number | null;
}

interface UseFileClipboardOptions {
  channelUser: string | null;
  path: string;
  notifyOk: NotifyOk;
  notifyFailed: NotifyFailed;
  modeSuffix: string;
  closeMenu: () => void;
  uploadMany: (paths: string[]) => Promise<unknown>;
}

/**
 * Clipboard actions of the file browser: copy path / content / file to the OS
 * clipboard (Spec 0054, Teil 2; Issues #357, #358) and paste local files from
 * the OS clipboard as an upload (Issue #571).
 */
export function useFileClipboard(
  sessionId: string,
  { channelUser, path, notifyOk, notifyFailed, modeSuffix, closeMenu, uploadMany }: UseFileClipboardOptions,
) {
  const { t } = useTranslation();
  /** Laufendes "Dateiinhalt kopieren" (Issue #357). */
  const [copyJob, setCopyJob] = useState<CopyJob | null>(null);
  // Issue #571: whether the OS clipboard holds local files, checked each time
  // the empty-space menu opens (no paths, only a flag).
  const [clipboardFiles, setClipboardFiles] = useState(false);

  /** Spec 0054, Teil 2: "Pfad kopieren" — reine Zwischenablage-Aktion, kein
   * Backend-Aufruf nötig. */
  const handleCopyPath = async (entry: RemoteEntryDto) => {
    closeMenu();
    try {
      await navigator.clipboard.writeText(entry.path);
      notifyOk("fileToasts.pathCopied", {});
    } catch (err) {
      console.warn("Konnte Pfad nicht in die Zwischenablage kopieren:", err);
      notifyFailed("fileToasts.copyFailed", entry.name, err);
    }
  };

  const runCopyJob = async (
    entry: RemoteEntryDto,
    run: (requestId: string, onProgress: (p: { done: number; total: number | null }) => void) => Promise<void>,
    passThroughCodes: string[],
    okKey: TranslationKey,
  ) => {
    closeMenu();
    const requestId = crypto.randomUUID();
    setCopyJob({ requestId, name: entry.name, done: 0, total: entry.size });
    try {
      await run(requestId, (p) =>
        setCopyJob((job) =>
          job?.requestId === requestId ? { ...job, done: p.done, total: p.total ?? job.total } : job,
        ),
      );
      notifyOk(okKey, { name: entry.name });
    } catch (err) {
      const code = commandErrorCode(err);
      // Ein Abbruch durch den Nutzer ist keine Störung: nichts anzeigen.
      if (code === "SFTP_COPY_CANCELLED") return;
      notifyFailed(
        "fileToasts.copyFailed",
        entry.name,
        code !== null && passThroughCodes.includes(code)
          ? { message: translateErrorCode(t, code, commandErrorMessage(err)) }
          : err,
      );
    } finally {
      setCopyJob((job) => (job?.requestId === requestId ? null : job));
    }
  };

  /** Spec 0054, Teil 2 / Issue #357 (Spec 0086 A1.1, A1.5): "Dateiinhalt
   * kopieren" — liest den Inhalt begrenzt bis 50 MB (das Backend lehnt zu
   * große und nicht-Text-Dateien ab, s. `sftp_copy_content`) und kopiert ihn
   * in die Zwischenablage. Ab 5 MB zeigt das Panel Fortschritt und einen
   * Abbrechen-Knopf. */
  const handleCopyContent = (entry: RemoteEntryDto) =>
    runCopyJob(
      entry,
      async (requestId, onProgress) => {
        const content = await sftpCopyContent(sessionId, entry.path, channelUser, requestId, onProgress);
        await clipboardWriteText(content);
      },
      ["SFTP_CONTENT_TOO_LARGE"],
      "fileToasts.contentCopied",
    );

  /** Issue #358: "Datei kopieren" — legt die Datei (bis 50 MB) als
   * Dateiverweis in die Zwischenablage; Fortschritt und Abbrechen wie bei
   * "Dateiinhalt kopieren". Bei Abbruch bleibt die Zwischenablage unverändert. */
  const handleCopyFile = (entry: RemoteEntryDto) =>
    runCopyJob(
      entry,
      async (requestId, onProgress) => {
        await sftpCopyFile(sessionId, entry.path, channelUser, requestId, onProgress);
      },
      ["SFTP_CONTENT_TOO_LARGE", "CLIPBOARD_FILE_FAILED"],
      "fileToasts.fileCopied",
    );

  /** Issue #571: whether the OS clipboard holds files; asked when the
   * empty-space menu opens. */
  const refreshClipboardFiles = (ask: boolean) => {
    setClipboardFiles(false);
    if (ask) {
      clipboardHasFiles()
        .then(setClipboardFiles)
        .catch(() => setClipboardFiles(false));
    }
  };

  /** Issue #571: "Paste" of local files from the OS clipboard. The backend
   * reads the clipboard and asks in a native dialog; the paths arrive here
   * only after the user confirmed and go through the normal upload flow. */
  const handlePasteClipboardFiles = async () => {
    try {
      const claimed = await claimClipboardFiles(sessionId, path, {
        title: t("fileBrowser.clipboardPaste.title"),
        intro: t("fileBrowser.clipboardPaste.intro"),
        targetLabel: t("fileBrowser.clipboardPaste.target"),
        moreLabel: t("fileBrowser.clipboardPaste.more", { count: "{count}", interpolation: { escapeValue: false } }),
        confirmLabel: t("fileBrowser.clipboardPaste.confirm"),
        cancelLabel: t("fileBrowser.clipboardPaste.cancel"),
      });
      if (claimed === null) {
        showToast({ kind: "info", message: t("fileToasts.nothingToPaste") + modeSuffix });
        return;
      }
      if (claimed.length === 0) return; // cancelled
      await uploadMany(claimed);
    } catch (err) {
      notifyFailed("fileToasts.uploadFailed", path, err);
    }
  };

  return {
    copyJob,
    clipboardFiles,
    refreshClipboardFiles,
    handleCopyPath,
    handleCopyContent,
    handleCopyFile,
    handlePasteClipboardFiles,
  };
}
