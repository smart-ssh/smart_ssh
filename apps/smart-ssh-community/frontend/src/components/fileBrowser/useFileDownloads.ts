import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { sftpDownload, sftpDownloadDefault, sftpDownloadDir } from "../../api";
import type { DownloadResultDto, RemoteEntryDto } from "../../types";
import { currentCaps } from "./shared";
import type { NotifyFailed, NotifyOk } from "./useFileNotify";

/** Verzeichnis-Anteil eines lokalen Pfads (POSIX oder Windows). */
function localDirName(localPath: string): string {
  const cut = Math.max(localPath.lastIndexOf("/"), localPath.lastIndexOf("\\"));
  return cut > 0 ? localPath.slice(0, cut) : localPath;
}

/** Spec 0054, Teil 2: "Herunterladen" and "Herunterladen nach…". */
export function useFileDownloads(
  sessionId: string,
  channelUser: string | null,
  notifyOk: NotifyOk,
  notifyFailed: NotifyFailed,
  closeMenu: () => void,
) {
  const { t } = useTranslation();

  const notifyDownloaded = (entryName: string, result: DownloadResultDto) => {
    // Issue #679: no file manager window on Android (and the result is a
    // content URI there).
    const reveal = currentCaps().revealInFolder
      ? () => {
          revealItemInDir(result.localPath).catch((err) =>
            console.warn("Konnte Download nicht im Dateimanager zeigen:", err),
          );
        }
      : undefined;
    if (result.isDir) {
      notifyOk("fileToasts.downloadedDir", { name: entryName, count: result.fileCount }, reveal);
    } else {
      notifyOk(
        "fileToasts.downloadedFile",
        { name: entryName, dir: localDirName(result.localPath) },
        reveal,
      );
    }
  };

  /** Spec 0054, Teil 2: "Herunterladen nach…" — Dialog für einen präzisen
   * Zielpfad. Für Dateien der bestehende Speichern-Dialog (exakter
   * Dateiname wählbar), für Ordner ein Zielordner-Dialog (ein Ordner hat
   * keinen einzelnen Dateinamen zum Speichern). */
  const handleDownloadChoose = (entry: RemoteEntryDto) => {
    closeMenu();
    const download = entry.isDir
      ? sftpDownloadDir(sessionId, entry.path, t("fileBrowser.downloadDirDialogTitle"), channelUser)
      : sftpDownload(sessionId, entry.path, channelUser);
    download
      .then((result) => {
        // `null` = Dialog abgebrochen, keine Meldung.
        if (result) notifyDownloaded(entry.name, result);
      })
      .catch((err) => notifyFailed("fileToasts.downloadFailed", entry.name, err));
  };

  /** Spec 0054, Teil 2: "Herunterladen" — direkt ins Standard-
   * Downloadverzeichnis, ohne Dialog. Datei oder Ordner (rekursiv, Backend
   * entscheidet anhand `entry.isDir`, s. `sftp_download_default`). */
  const handleDownloadDefault = (entry: RemoteEntryDto) => {
    // Issue #679: Android has no default folder to write into; files go
    // through the system save picker (which offers Downloads).
    if (!currentCaps().downloadWithoutPicker && !entry.isDir) {
      handleDownloadChoose(entry);
      return;
    }
    closeMenu();
    sftpDownloadDefault(sessionId, entry.path, channelUser)
      .then((result) => notifyDownloaded(entry.name, result))
      .catch((err) => notifyFailed("fileToasts.downloadFailed", entry.name, err));
  };

  return { handleDownloadDefault, handleDownloadChoose };
}
