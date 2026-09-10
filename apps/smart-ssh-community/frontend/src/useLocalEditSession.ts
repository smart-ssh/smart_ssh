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
        closeEditSession(sessionId, session.localPath).catch(() => {});
      }

      // Der Fehler selbst wird weitergereicht (Sache des Aufrufers,
      // `FileBrowserPanel`s bestehende `setError`-Anzeige — dieser Hook
      // bleibt UI-frei, s. Moduldoc-Kommentar zu `buildUploadOffer`), aber
      // NACH einem erfolgreichen Download muss ein späterer Fehlschlag
      // (z. B. `openPath`: "Programm nicht gefunden") die bereits
      // heruntergeladene Temp-Kopie wieder aufräumen — sonst bliebe sie
      // für immer liegen, weil ohne gesetzten Session-State kein Weg mehr
      // existiert, `close_edit_session` später dafür auszulösen
      // (Spec-Reviewer-Fund, Spec 0054, Review des Gesamtpakets: "Temp-
      // Datei-Leichen im Fehlerpfad").
      const editSession = await sftpOpenForEditing(sessionId, entry.path);
      try {
        remoteModifiedAtDownloadRef.current = editSession.remoteModified;

        const apps = await loadFileTypeApps().catch(() => ({}));
        const appPath = appForFileName(apps, entry.name) ?? undefined;
        await openPath(editSession.localPath, appPath);

        lastSeenMtimeRef.current = await localFileMtime(editSession.localPath);

        setSession({ entry, localPath: editSession.localPath, status: "editing", error: null });
        stopPolling();
        pollTimerRef.current = setInterval(() => poll(editSession.localPath), POLL_INTERVAL_MS);
      } catch (err) {
        closeEditSession(sessionId, editSession.localPath).catch(() => {});
        throw err;
      }
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
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): die
    // Doc-Kommentar-Zusage an `UploadOffer.remoteChangedSinceDownload`
    // ("konservativ true, wenn einer der beiden Zeitstempel fehlt") war
    // hier nicht umgesetzt — ein simples `!==` liefert `false`, wenn BEIDE
    // Seiten `null` sind (Server ohne `modified`-Unterstützung, oder
    // `sftpStat` ist oben fehlgeschlagen und landete im `.catch(() =>
    // null)`) — genau der Fall, in dem gar nichts geprüft werden konnte,
    // nie stillschweigend als "unverändert" gelten darf (Spec 0054, Teil
    // 4, Punkt 5: Datenverlust durch stilles Überschreiben vermeiden).
    const remoteChangedSinceDownload =
      baseline === null || remoteModifiedNow === null || remoteModifiedNow !== baseline;
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
    closeEditSession(sessionId, session.localPath).catch((err) =>
      console.warn("Konnte Temp-Datei nicht aufräumen:", err),
    );
    setSession(null);
  }, [session, sessionId, stopPolling]);

  return { session, startEditing, buildUploadOffer, confirmUpload, dismissChange, endSession };
}
