import { useCallback, useEffect, useRef, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  closeEditSession,
  commandErrorMessage,
  localFileMtime,
  readLocalTextPreview,
  sftpOpenForEditing,
  sftpReadText,
  sftpStat,
  sftpUpload,
} from "./api";
import { appForFileName, loadFileTypeApps } from "./fileTypeSettings";
import type { RemoteEntryDto } from "./types";

/** Spec 0054, Teil 4: wie oft auf eine lokale Änderung der Bearbeitungskopie
 * geprüft wird. Ein Sekundenwert im niedrigen einstelligen Bereich ist für
 * eine EINE aktiv beobachtete Datei unauffällig — kein Grund, dafür einen
 * nativen Dateisystem-Watcher einzuführen (s. `crate::commands`-
 * Moduldoc-Kommentar zu `sftp_open_for_editing` für die volle Begründung). */
const POLL_INTERVAL_MS = 2000;

export type LocalEditStatus = "editing" | "changed" | "uploading";

export interface LocalEditSessionState {
  entry: RemoteEntryDto;
  localPath: string;
  status: LocalEditStatus;
  error: string | null;
}

export interface UploadOffer {
  localText: string | null;
  localSize: number;
  remoteText: string | null;
  /** `true`, wenn sich die Remote-Datei laut `modified`-Zeitstempel seit
   * dem Download für diese Bearbeitung geändert hat — Spec 0054, Teil 4,
   * Punkt 5: "Warnung vor dem Überschreiben (Datenverlust vermeiden)".
   * Konservativ `true`, wenn einer der beiden Zeitstempel fehlt (nie
   * stillschweigend als "unverändert" werten, wenn es nicht sicher
   * feststellbar ist). */
  remoteChangedSinceDownload: boolean;
}

/**
 * Spec 0054, Teil 4: "Lokal öffnen → bearbeiten → Upload anbieten". Hält
 * genau EINE aktive Bearbeitung gleichzeitig (bewusste Vereinfachung — der
 * Nutzer bearbeitet praktisch nie mehrere Dateien parallel über denselben
 * Flow; ein zweiter "Lokal öffnen"-Klick während eine Bearbeitung bereits
 * läuft beendet die alte zuerst, s. `startEditing`).
 */
export function useLocalEditSession(sessionId: string) {
  const [session, setSession] = useState<LocalEditSessionState | null>(null);
  const lastSeenMtimeRef = useRef<string | null>(null);
  const remoteModifiedAtDownloadRef = useRef<string | null>(null);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopPolling = useCallback(() => {
    if (pollTimerRef.current !== null) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  }, []);

  // Sauberes Ende beim Unmount (Spec 0054, Teil 4, Punkt 6) — falls der
  // Nutzer den Dateibrowser-Tab schließt, statt den Flow selbst zu
  // beenden. Zusätzliches Netz: `disconnect()` im Backend räumt beim
  // Trennen der Verbindung ohnehin den ganzen Editier-Temp-Ordner dieser
  // Session auf (s. dortiger Kommentar) — dieser Aufräumvorgang hier ist
  // der schnellere, gezielte Weg für genau diese eine Datei.
  useEffect(() => stopPolling, [stopPolling]);

  const poll = useCallback(
    (localPath: string) => {
      localFileMtime(localPath)
        .then((mtime) => {
          if (mtime !== null && mtime !== lastSeenMtimeRef.current) {
            setSession((prev) => (prev ? { ...prev, status: "changed" } : prev));
          }
        })
        .catch((err) => console.warn("Konnte lokale Änderungszeit nicht abfragen:", err));
    },
    [],
  );

  const startEditing = useCallback(
    async (entry: RemoteEntryDto) => {
      // Ein bereits laufender Flow wird zuerst sauber beendet (s.
      // Moduldoc-Kommentar — nur eine aktive Bearbeitung gleichzeitig).
      if (session) {
        stopPolling();
        closeEditSession(session.localPath).catch(() => {});
      }

      // Absichtlich KEIN try/catch hier: ein Fehlschlag beim Download oder
      // beim Start des lokalen Programms ("Programm nicht gefunden") ist
      // Sache des Aufrufers (`FileBrowserPanel`s bestehende `setError`-
      // Anzeige) — dieser Hook bleibt UI-frei, s. Moduldoc-Kommentar zu
      // `buildUploadOffer`. Kein halb-initialisierter Session-State bei
      // einem Fehlschlag.
      const editSession = await sftpOpenForEditing(sessionId, entry.path);
      remoteModifiedAtDownloadRef.current = editSession.remoteModified;

      const apps = await loadFileTypeApps().catch(() => ({}));
      const appPath = appForFileName(apps, entry.name) ?? undefined;
      await openPath(editSession.localPath, appPath);

      lastSeenMtimeRef.current = await localFileMtime(editSession.localPath);

      setSession({ entry, localPath: editSession.localPath, status: "editing", error: null });
      stopPolling();
      pollTimerRef.current = setInterval(() => poll(editSession.localPath), POLL_INTERVAL_MS);
    },
    [poll, session, sessionId, stopPolling],
  );

  /** Spec 0054, Teil 4, Punkt 5: baut die Upload-Angebot-Daten (lokaler +
   * Remote-Inhalt für die Diff-Vorschau, plus die
   * Remote-Änderungs-Konflikt-Prüfung) — zeigt selbst keinen Dialog, das
   * übernimmt der Aufrufer (`FileBrowserPanel`), damit dieser Hook UI-frei
   * bleibt. */
  const buildUploadOffer = useCallback(async (): Promise<UploadOffer | null> => {
    if (!session) return null;
    const [localPreview, remoteEntry, remoteText] = await Promise.all([
      readLocalTextPreview(session.localPath),
      sftpStat(sessionId, session.entry.path).catch(() => null),
      sftpReadText(sessionId, session.entry.path).catch(() => null),
    ]);
    const remoteModifiedNow = remoteEntry?.modified ?? null;
    const baseline = remoteModifiedAtDownloadRef.current;
    const remoteChangedSinceDownload = remoteModifiedNow !== baseline;
    return {
      localText: localPreview.text,
      localSize: localPreview.size,
      remoteText,
      remoteChangedSinceDownload,
    };
  }, [session, sessionId]);

  const confirmUpload = useCallback(async () => {
    if (!session) return;
    setSession((prev) => (prev ? { ...prev, status: "uploading" } : prev));
    try {
      await sftpUpload(sessionId, session.localPath, session.entry.path);
      const uploadedMtime = await localFileMtime(session.localPath);
      lastSeenMtimeRef.current = uploadedMtime;
      remoteModifiedAtDownloadRef.current = await sftpStat(sessionId, session.entry.path)
        .then((e) => e.modified)
        .catch(() => remoteModifiedAtDownloadRef.current);
      setSession((prev) => (prev ? { ...prev, status: "editing", error: null } : prev));
    } catch (err) {
      setSession((prev) => (prev ? { ...prev, status: "changed", error: commandErrorMessage(err) } : prev));
    }
  }, [session, sessionId]);

  /** "Später" — die Änderung wird nicht jetzt hochgeladen, der Watcher
   * beobachtet weiter (nächste Änderung fragt erneut). */
  const dismissChange = useCallback(() => {
    if (!session) return;
    localFileMtime(session.localPath).then((mtime) => {
      lastSeenMtimeRef.current = mtime;
    });
    setSession((prev) => (prev ? { ...prev, status: "editing", error: null } : prev));
  }, [session]);

  const endSession = useCallback(() => {
    if (!session) return;
    stopPolling();
    closeEditSession(session.localPath).catch((err) =>
      console.warn("Konnte Temp-Datei nicht aufräumen:", err),
    );
    setSession(null);
  }, [session, stopPolling]);

  return { session, startEditing, buildUploadOffer, confirmUpload, dismissChange, endSession };
}
