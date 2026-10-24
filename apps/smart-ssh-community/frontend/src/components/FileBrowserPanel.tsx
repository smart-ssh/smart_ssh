import { formatDuration, formatSpeed, nextRate, type RateState } from "../transferRate";
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Trans, useTranslation } from "react-i18next";
import { formatDateTime } from "../formatDateTime";
import {
  claimClipboardFiles,
  claimDroppedPaths,
  clipboardHasFiles,
  clipboardWriteText,
  commandErrorCode,
  commandErrorMessage,
  pickUploadFiles,
  pickUploadFolder,
  sftpLocalPathsAreFolders,
  sftpUploadFolder,
  sftpUploadFolderPreview,
  readLocalTextPreview,
  sftpChmod,
  sftpDelete,
  sftpDeletePreview,
  sftpDownload,
  sftpDownloadDefault,
  sftpDownloadDir,
  sftpCancelTransfer,
  sftpElevationDisable,
  sftpElevationEnable,
  sftpElevationStatus,
  sftpExists,
  sftpStat,
  sftpList,
  sftpMkdir,
  sftpCreateFile,
  sftpCancelCopyContent,
  sftpCopyContent,
  sftpCopyFile,
  sftpReadText,
  sftpRename,
  sftpStartDirectory,
  sessionIsRoot,
  sftpUpload,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import {
  onConnectionStatusChanged,
  onSftpTransferFinished,
  onSftpTransferProgress,
  onSftpTransferStarted,
} from "../events";
import { isPointerInside } from "../dropZone";
import { formatBytes } from "../format";
import {
  loadFileManagerColumnWidths,
  saveFileManagerColumnWidths,
  type FileManagerColumnWidths,
} from "../layoutSettings";
import { displayPath, joinPath, localBaseName, parentPath } from "../remotePath";
import { notifyMissingStartDirectory } from "../startDirectory";
import { showToast } from "../toastBus";
import type {
  DeletePreviewDto,
  DownloadResultDto,
  ElevationResultDto,
  RunningOperationDto,
  LocalFilePreviewDto,
  FolderUploadPreviewDto,
  FolderUploadSummaryDto,
  RemoteEntryDto,
} from "../types";
import { useDragResize } from "../useDragResize";
import { useLocalEditSession, type UploadOffer } from "../useLocalEditSession";
import {
  classifyPreviewReadFailure,
  previewReadFailureHint,
  type PreviewReadFailure,
} from "../previewReadFailure";
import { NoteDiffPreview } from "./NoteDiffPreview";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { useMenuPlacement } from "../hooks/useMenuPlacement";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "./ui";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";

/** Spec 0053, Teil 1: Standard-/Mindestbreiten der verstellbaren Spalten
 * (alles in px). Die Name-Spalte hat bewusst keinen eigenen Eintrag hier
 * — sie bekommt nie eine explizite Breite (s. `<colgroup>` unten), nimmt
 * unter `table-layout: fixed` also automatisch den nach den anderen
 * Spalten verbleibenden Rest ein. `NAME_MIN_WIDTH` fließt stattdessen in
 * `clampColumnWidth` ein, damit eine sehr breit gezogene Nachbarspalte
 * die Name-Spalte nie unter ihre eigene Mindestbreite drückt. */
const DEFAULT_COLUMN_WIDTHS: FileManagerColumnWidths = {
  size: 90,
  permissions: 90,
  modified: 150,
};
const MIN_COLUMN_WIDTHS: FileManagerColumnWidths = {
  size: 56,
  permissions: 64,
  modified: 90,
};
const NAME_MIN_WIDTH = 120;

/** Issue #357: ab dieser Größe zeigt "Dateiinhalt kopieren" Fortschritt. */
const COPY_PROGRESS_FROM_BYTES = 5 * 1024 * 1024;

function formatMb(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}
/** Feste, nicht verstellbare Aktionsspalte (⋮-Menü) — kein Label, immer
 * gleich schmal, kein Drag-Handle. */
const ACTIONS_COLUMN_WIDTH = 36;

/** Verzeichnis-Anteil eines lokalen Pfads (POSIX oder Windows). */
function localDirName(localPath: string): string {
  const cut = Math.max(localPath.lastIndexOf("/"), localPath.lastIndexOf("\\"));
  return cut > 0 ? localPath.slice(0, cut) : localPath;
}

/** „Im Finder/Explorer zeigen“ — Beschriftung passend zum Betriebssystem. */
function revealLabelKey(): string {
  const platform = navigator.platform.toLowerCase();
  if (platform.includes("mac")) return "fileToasts.revealFinder";
  if (platform.includes("win")) return "fileToasts.revealExplorer";
  return "fileToasts.revealFileManager";
}

type MoveKind = "rename" | "move";

interface Transfer {
  id: string;
  kind: "upload" | "download";
  fileName: string;
  totalBytes: number | null;
  error: string | null;
  /** Issue #621: the transfer started while elevated mode was on. */
  startedElevated?: boolean;
  /** Issue #621: finished successfully, but elevated mode ended meanwhile —
   * the row stays (as done) with a note. */
  doneElevationEnded?: boolean;
  /** Issue #622: bytes transferred so far (progress events, throttled). */
  bytesDone?: number;
  /** Issue #623: moving-average speed and remaining time. */
  rate?: RateState;
  /** Issue #622: folder transfers — files done / total (known once the walk is done). */
  folder?: { filesDone: number; filesTotal: number };
  /** Issue #622: ended without error — `cancelled` (cancel button) or `stopped`
   * (elevated rights ended). */
  stopped?: "cancelled" | "stopped";
  /** Issue #622: a stopped upload left this incomplete remote file behind. */
  incompleteRemotePath?: string;
}

/** Issue #622: asked before ending elevated rights (or changing the target
 * user) while elevated operations still run. */
interface StopPrompt {
  operations: RunningOperationDto[];
  /** What to do after the user confirmed. */
  action: { kind: "disable" } | { kind: "switch"; targetUser: string };
}

interface FileBrowserPanelProps {
  sessionId: string;
  /** Spec 0020, Abschnitt 5.4/Spec 0017: nur sichtbar (und damit für
   * Drag-and-Drop aktiv), wenn dies der aktive Tab UND die "Dateien"-Ansicht
   * gerade gewählt ist — bleibt sonst wie `TerminalView` gemountet (eigener
   * Navigationszustand pro Tab bleibt erhalten), nur per CSS ausgeblendet. */
  isVisible: boolean;
  /** Spec 0067, A5: meldet den Ziel-Nutzer des erhöhten Modus (oder `null`)
   * nach oben, damit er auch bei verborgener Dateien-Ansicht sichtbar ist. */
  onElevationChange?: (targetUser: string | null) => void;
}

/**
 * Manueller SFTP-Dateibrowser (Spec 0020, Abschnitt 5). Läuft komplett ohne
 * Filter-Engine-Prüfung — direkte Nutzeraktionen, wie im interaktiven
 * Terminal. Eigener, pro Tab getrennter Navigationszustand (Abschnitt 5.4):
 * diese Komponente wird einmal pro Session-Tab instanziiert und bleibt beim
 * Tab-Wechsel gemountet (s. `SessionView`), ihr `path`-State lebt daher
 * automatisch pro Tab getrennt, ohne zusätzliche Buchführung.
 */
// Header cells (not the thead) are sticky with their own opaque background and
// the table uses `border-separate`: a sticky thead under `border-collapse`
// leaves a seam in WebKit through which scrolled rows showed (#650).
const STICKY_TH = "sticky top-0 z-10 bg-slate-900";

export function FileBrowserPanel({
  sessionId,
  isVisible,
  onElevationChange,
}: FileBrowserPanelProps) {
  const [path, setPath] = useState(".");
  const [pathInput, setPathInput] = useState(".");
  // Spec 0102: Startpfad dieser Sitzung (`"."` = Home), Ziel von „Zum
  // Startverzeichnis". Kommt einmalig vom Backend (`sftp_start_directory`).
  const [startPath, setStartPath] = useState(".");
  const [entries, setEntries] = useState<RemoteEntryDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Spec 0054, Teil 1: EIN gemeinsamer State für Drei-Punkte-Menü UND
  // Kontextmenü (Rechtsklick) statt zweier getrennter — beide zeigen
  // dieselben Aktionen (`FileEntryMenu` unten), der einzige Unterschied ist
  // die Positionierung (`anchor`). `"row"` rendert das Menü innerhalb der
  // Aktionsspalte des jeweiligen Eintrags (wie das bisherige Drei-Punkte-
  // Menü), `{ x, y }` rendert es `fixed` an der Klickposition
  // (Kontextmenü) — ein zweiter, praktisch identischer State (und eine
  // zweite "Klick-außerhalb-schließt"-Logik) dafür wäre unnötige Dopplung.
  const [openMenu, setOpenMenu] = useState<{
    entry: RemoteEntryDto;
    anchor: "row" | { x: number; y: number };
  } | null>(null);
  /** Laufendes "Dateiinhalt kopieren" (Issue #357). */
  const [copyJob, setCopyJob] = useState<{
    requestId: string;
    name: string;
    done: number;
    total: number | null;
  } | null>(null);
  const [properties, setProperties] = useState<RemoteEntryDto | null>(null);
  const [renaming, setRenaming] = useState<{ entry: RemoteEntryDto; value: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<RemoteEntryDto | null>(null);
  const [deletePreview, setDeletePreview] = useState<DeletePreviewDto | null>(null);
  const [mkdirOpen, setMkdirOpen] = useState<string | null>(null);
  /** Issue #570: "Neue Datei" (Namensabfrage) und Kontextmenü auf leerer
   * Fläche (Position, `null` = zu). `nameError` gilt für beide Abfragen. */
  const [newFileOpen, setNewFileOpen] = useState<string | null>(null);
  const [dirMenu, setDirMenu] = useState<{ x: number; y: number } | null>(null);
  // Issue #571: whether the OS clipboard holds local files, checked each time
  // the empty-space menu opens (no paths, only a flag).
  const [clipboardFiles, setClipboardFiles] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const [chmodTarget, setChmodTarget] = useState<RemoteEntryDto | null>(null);
  // Spec 0054, Teil 3: "Verschieben" per Ausschneiden/Einfügen (die Spec
  // erlaubt ausdrücklich Cut/Paste ODER Drag-and-Drop als Alternativen —
  // Cut/Paste deckt die Anforderung bereits vollständig ab, ein
  // zusätzlicher Drag-Mechanismus zwischen Zeilen wäre redundanter Aufwand,
  // s. ADR). `null` = nichts ausgeschnitten.
  const [cutEntry, setCutEntry] = useState<RemoteEntryDto | null>(null);
  // Kollisionsprüfung bei Umbenennen/Verschieben (Spec 0054, Teil 3): beide
  // laufen über denselben `performMove`-Pfad, da beide backend-seitig
  // dieselbe `sftp_rename` sind (s. dortiger Kommentar).
  const [moveCollision, setMoveCollision] = useState<{
    from: string;
    to: string;
    kind: MoveKind;
    /** Issue #625: ein Ordner als Ziel lässt sich nicht ersetzen. */
    targetIsDir: boolean;
  } | null>(null);
  // Upload-Überschreib-Diff-Vorschau (Spec 0054, Teil 3): `remoteText` ist
  // `null`, wenn die Remote-Datei nicht als Text lesbar war (Binärdatei/zu
  // groß) — dann zeigt der Dialog nur einen Größenvergleich.
  const [uploadConflict, setUploadConflict] = useState<{
    localPath: string;
    remotePath: string;
    localPreview: LocalFilePreviewDto;
    remoteText: string | null;
    /** Issue #655: why `remoteText` is `null`. */
    remoteReadFailure: PreviewReadFailure | null;
    remoteSize: number;
  } | null>(null);
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
  const [columnWidths, setColumnWidths] = useState<FileManagerColumnWidths>(DEFAULT_COLUMN_WIDTHS);
  const tableContainerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // Spec 0054, Teil 4: "Lokal öffnen -> bearbeiten -> Upload anbieten".
  const localEdit = useLocalEditSession(sessionId);
  const { t, i18n } = useTranslation();

  // Spec 0067, A5: erhöhter Modus pro Browser-Ansicht — nie Default, nicht
  // persistiert. `null` = normaler Modus.
  const [elevation, setElevation] = useState<{ targetUser: string } | null>(null);
  const [enablingElevation, setEnablingElevation] = useState(false);
  // Spec 0067, A4: Ziel-Nutzer der Rechteerhöhung, Default root.
  const [elevationTargetUser, setElevationTargetUser] = useState("root");
  // Issue #544: `id -u` probe result of the session. `undefined` = not yet
  // answered; `null`/failure = unknown (controls shown); `true` = root (hidden).
  const [sessionRoot, setSessionRoot] = useState<boolean | null | undefined>(undefined);
  // Issue #584, Spec 0113: below 640 px the toolbar collapses into an
  // overflow menu and the list into two-line rows; functions stay the same.
  const narrow = useIsNarrow();
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const overflowButtonRef = useRef<HTMLButtonElement>(null);
  const [elevationMenuOpen, setElevationMenuOpen] = useState(false);
  const [otherUserOpen, setOtherUserOpen] = useState(false);
  const [otherUserDraft, setOtherUserDraft] = useState("root");
  const elevationMenuButtonRef = useRef<HTMLButtonElement>(null);
  const otherUserInputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let cancelled = false;
    sessionIsRoot(sessionId)
      .then((value) => {
        if (!cancelled) setSessionRoot(value);
      })
      .catch(() => {
        if (!cancelled) setSessionRoot(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);
  const otherUserItemRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!elevationMenuOpen && !otherUserOpen) return;
    if (elevationMenuOpen) otherUserItemRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setElevationMenuOpen(false);
      setOtherUserOpen(false);
      (elevationMenuButtonRef.current ?? overflowButtonRef.current)?.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [elevationMenuOpen, otherUserOpen]);
  const elevationGroupRef = useRef<HTMLFieldSetElement>(null);
  // Issue #627: the elevation menu and the target-user form stay inside the window.
  const elevationMenuRef = useRef<HTMLDivElement>(null);
  const otherUserFormRef = useRef<HTMLFormElement>(null);
  const getElevationAnchor = useCallback(() => elevationGroupRef.current, []);
  const elevationMenuStyle = useMenuPlacement(elevationMenuRef, getElevationAnchor, elevationMenuOpen, { gap: 4 });
  const otherUserFormStyle = useMenuPlacement(otherUserFormRef, getElevationAnchor, otherUserOpen, { gap: 4 });
  // Issue #569: a click outside the elevation group closes the menu and the
  // target user form. Clicks inside (arrow button, menu, form) are ignored, so
  // the arrow button's own toggle is not undone.
  useEffect(() => {
    if (!elevationMenuOpen && !otherUserOpen) return;
    const onClick = (e: MouseEvent) => {
      // composedPath, not target: a clicked menu item may already be detached.
      const path = e.composedPath();
      const group = elevationGroupRef.current;
      if (group && path.includes(group)) return;
      const overflow = overflowRef.current;
      if (overflow && path.includes(overflow)) return;
      setElevationMenuOpen(false);
      setOtherUserOpen(false);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, [elevationMenuOpen, otherUserOpen]);
  // Hiding the panel or switching the elevation closes both.
  useEffect(() => {
    setElevationMenuOpen(false);
    setOtherUserOpen(false);
    setOverflowOpen(false);
  }, [isVisible, elevation]);
  // Leaving the narrow layout drops the overflow menu.
  useEffect(() => {
    if (!narrow) setOverflowOpen(false);
  }, [narrow]);
  // Overflow menu: outside click and Escape close it (Escape returns focus).
  useEffect(() => {
    if (!overflowOpen) return;
    const onClick = (e: MouseEvent) => {
      const overflow = overflowRef.current;
      if (overflow && e.composedPath().includes(overflow)) return;
      setOverflowOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setOverflowOpen(false);
      overflowButtonRef.current?.focus();
    };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [overflowOpen]);
  const [elevationFailure, setElevationFailure] = useState<ElevationResultDto | null>(null);
  const [elevationWarning, setElevationWarning] = useState<string | null>(null);
  const elevated = elevation !== null;
  const elevatedUser = elevation?.targetUser ?? "";
  // Spec 0067, A5: an jeden Aufruf übergeben — `null` = normaler Kanal,
  // sonst der erwartete Ziel-Nutzer (Backend prüft ihn gegen den Kanal).
  const channelUser = elevation?.targetUser ?? null;
  const modeSuffix = elevated ? t("fileToasts.asElevated", { user: elevatedUser }) : "";
  // Jede Modusänderung erhöht die Generation — eine Freischaltung, die erst
  // NACH einem Ausschalten/Verbindungsverlust zurückkommt, wird verworfen.
  const elevationGenerationRef = useRef(0);
  const onElevationChangeRef = useRef(onElevationChange);
  onElevationChangeRef.current = onElevationChange;
  /** Einzige Stelle, die den Modus ändert: hält Elternansicht und (beim
   * Beenden) das Backend synchron — "Backend aktiv ⇔ UI zeigt erhöht". */
  const updateElevation = (next: { targetUser: string } | null, backendDone = false) => {
    elevationGenerationRef.current += 1;
    setElevation(next);
    if (next === null) setElevationWarning(null);
    onElevationChangeRef.current?.(next?.targetUser ?? null);
    if (next === null && !backendDone) {
      sftpElevationDisable(sessionId).catch((err) =>
        console.warn("Konnte erhöhten Modus nicht beenden:", err),
      );
    }
  };

  // Ein evtl. noch offener erhöhter Kanal (z. B. aus einer früheren
  // Ansicht dieser Session) wird beim Öffnen geschlossen — der Browser
  // startet immer normal, der erhöhte Modus bleibt nie unbemerkt aktiv.
  useEffect(() => {
    sftpElevationDisable(sessionId).catch((err) =>
      console.warn("Konnte erhöhten Modus nicht zurücksetzen:", err),
    );
  }, [sessionId]);

  // Verbindung weg → erhöhter Kanal ist weg (lebt nur in der Session).
  // Issue #355: nach einem Reconnect das bisherige Verzeichnis neu laden
  // (Home, falls es nicht mehr existiert). Der erhöhte Kanal kommt nicht
  // zurück, daher immer im normalen Modus.
  const pathRef = useRef(path);
  pathRef.current = path;
  const connectionLostRef = useRef(false);
  const reloadAfterReconnect = () => {
    setLoading(true);
    setError(null);
    const show = (target: string) => (result: RemoteEntryDto[]) => {
      setEntries(result);
      setPath(target);
      setPathInput(target);
    };
    const previous = pathRef.current;
    sftpList(sessionId, previous, null)
      .then(show(previous), () => sftpList(sessionId, ".", null).then(show(".")))
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    const unlisten = onConnectionStatusChanged((event) => {
      if (event.sessionId !== sessionId) return;
      if (event.status !== "connected") {
        connectionLostRef.current = true;
        updateElevation(null);
      } else if (connectionLostRef.current) {
        connectionLostRef.current = false;
        reloadAfterReconnect();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // `updateElevation` liest nur Refs/State-Setter und `sessionId`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Spec 0067, Teil B: jede Aktion meldet ihr Ergebnis als Toast. Nur
  // Namen/Pfade, nie Dateiinhalte.
  const notifyOk = (key: string, options: Record<string, unknown>, action?: () => void) =>
    showToast({
      kind: "success",
      message: t(key, options) + modeSuffix,
      action: action ? { label: t(revealLabelKey()), onClick: action } : undefined,
    });
  const notifyFailed = (key: string, name: string, err: unknown) => {
    // Issue #622: a stopped operation is not an error — its row shows the state.
    if (commandErrorCode(err) === "FILE_OPERATION_STOPPED") return;
    showToast({
      kind: "error",
      message: t(key, { name, reason: commandErrorMessage(err) }) + modeSuffix,
    });
    // Spec 0067, A5: ist der erhöhte Kanal inzwischen weg (z. B. nach einem
    // Verbindungsabbruch), wird der Modus sichtbar beendet statt weiter
    // "aktiv" anzuzeigen.
    if (elevated) {
      sftpElevationStatus(sessionId)
        .then((user) => {
          if (user === null) {
            updateElevation(null);
            showToast({ kind: "error", message: t("fileElevation.channelLost") });
          }
        })
        .catch(() => {});
    }
  };

  const handleToggleElevation = async (targetUser: string = "root") => {
    setElevationMenuOpen(false);
    setOtherUserOpen(false);
    if (elevated) {
      // Issue #622: with running elevated operations the backend changes
      // nothing and lists them; the user confirms (or cancels) first.
      try {
        const result = await sftpElevationDisable(sessionId, false);
        if (!result.disabled) {
          setStopPrompt({ operations: result.runningOperations, action: { kind: "disable" } });
          return;
        }
      } catch (err) {
        console.warn("Konnte erhöhten Modus nicht beenden:", err);
      }
      updateElevation(null, true);
      load(path, null);
      return;
    }
    await enableElevation(targetUser);
  };

  const enableElevation = async (targetUser: string, confirmStop = false) => {
    setEnablingElevation(true);
    const generation = elevationGenerationRef.current;
    try {
      const result = await sftpElevationEnable(sessionId, targetUser.trim() || null, confirmStop);
      if (result.runningOperations && result.runningOperations.length > 0) {
        setStopPrompt({
          operations: result.runningOperations,
          action: { kind: "switch", targetUser },
        });
        return;
      }
      if (generation !== elevationGenerationRef.current) {
        // Inzwischen ausgeschaltet/Verbindung weg — nicht wieder "erhöht"
        // anzeigen, Backend-Kanal schließen.
        if (result.active) updateElevation(null);
        return;
      }
      if (result.active) {
        updateElevation({ targetUser: result.targetUser });
        setElevationWarning(result.warning?.path ?? null);
        load(path, result.targetUser);
      } else {
        setElevationFailure(result);
      }
    } catch (err) {
      setElevationFailure({
        active: false,
        targetUser: "root",
        sftpServerPath: null,
        failure: { kind: "checkFailed", sudoersLine: null, detail: commandErrorMessage(err) },
      });
    } finally {
      setEnablingElevation(false);
    }
  };

  const [stopPrompt, setStopPrompt] = useState<StopPrompt | null>(null);
  /** Issue #622: "End elevated rights" in the dialog — stops the listed
   * operations, then closes the channel. */
  const confirmStopPrompt = async () => {
    const prompt = stopPrompt;
    setStopPrompt(null);
    if (!prompt) return;
    if (prompt.action.kind === "disable") {
      try {
        await sftpElevationDisable(sessionId, true);
      } catch (err) {
        console.warn("Konnte erhöhten Modus nicht beenden:", err);
      }
      updateElevation(null, true);
      load(path, null);
    } else {
      await enableElevation(prompt.action.targetUser, true);
    }
  };

  const notifyDownloaded = (entryName: string, result: DownloadResultDto) => {
    const reveal = () => {
      revealItemInDir(result.localPath).catch((err) =>
        console.warn("Konnte Download nicht im Dateimanager zeigen:", err),
      );
    };
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
  const [editUploadOffer, setEditUploadOffer] = useState<UploadOffer | null>(null);

  // Spec 0053, Teil 1: einmalig beim Mounten geladen (diese Komponente lebt
  // pro Tab, s. Doc-Kommentar oben) — ungültige/fehlende Felder bleiben
  // beim eingebauten Default (`loadFileManagerColumnWidths` liefert nur
  // die tatsächlich gültigen Felder zurück, s. dortiger Kommentar).
  useEffect(() => {
    loadFileManagerColumnWidths()
      .then((stored) => setColumnWidths((prev) => ({ ...prev, ...stored })))
      .catch((err) => console.warn("Konnte Spaltenbreiten nicht laden:", err));
  }, []);

  /** Klemmt eine vorgeschlagene Breite für `column` auf ihre Mindestbreite
   * UND darauf, dass die Name-Spalte (die den Rest der verfügbaren Breite
   * einnimmt, s. `<colgroup>` unten) nie unter `NAME_MIN_WIDTH` fällt —
   * abhängig von der aktuell tatsächlich verfügbaren Breite des
   * Tabellen-Containers, nicht von einem festen Annahmewert. */
  const clampColumnWidth = useCallback(
    (column: keyof FileManagerColumnWidths, proposed: number, widths: FileManagerColumnWidths) => {
      const min = MIN_COLUMN_WIDTHS[column];
      const containerWidth = tableContainerRef.current?.clientWidth ?? Infinity;
      const otherColumnsTotal =
        ACTIONS_COLUMN_WIDTH +
        (Object.keys(widths) as (keyof FileManagerColumnWidths)[])
          .filter((key) => key !== column)
          .reduce((sum, key) => sum + widths[key], 0);
      const maxForThisColumn = containerWidth - otherColumnsTotal - NAME_MIN_WIDTH;
      return Math.min(Math.max(proposed, min), Math.max(min, maxForThisColumn));
    },
    [],
  );

  // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): der
  // Persistenz-Aufruf lief vorher als Seiteneffekt *innerhalb* eines
  // `setState`-Updaters (`setColumnWidths((current) => { save(current);
  // return current; })`) — unter `StrictMode` (aktiv in `main.tsx`) ruft
  // React Updater im Dev-Modus doppelt auf, was `store.save()` doppelt
  // ausgelöst hätte (harmlos, da idempotent, aber ein vermeidbares
  // Anti-Pattern). `columnWidthsRef` hält den aktuellen Wert stattdessen
  // synchron zum State nach, `handleColumnDragEnd` liest ihn direkt statt
  // über einen Updater.
  const columnWidthsRef = useRef(columnWidths);
  useEffect(() => {
    columnWidthsRef.current = columnWidths;
  }, [columnWidths]);

  const handleColumnDrag = useCallback(
    (column: keyof FileManagerColumnWidths) => (deltaX: number) => {
      setColumnWidths((prev) => {
        const clamped = clampColumnWidth(column, prev[column] + deltaX, prev);
        if (clamped === prev[column]) return prev;
        return { ...prev, [column]: clamped };
      });
    },
    [clampColumnWidth],
  );

  const handleColumnDragEnd = useCallback(() => {
    saveFileManagerColumnWidths(columnWidthsRef.current).catch((err) =>
      console.warn("Konnte Spaltenbreiten nicht speichern:", err),
    );
  }, []);

  // `elevatedOverride`: beim Umschalten ist der neue Modus im State noch
  // nicht angekommen — der Aufrufer übergibt ihn dann explizit.
  // Issue #624: the mode of every listed entry, so an empty listing of a
  // folder can be explained by the folder's own (missing) execute bit.
  const knownDirModes = useRef(new Map<string, number>());

  const load = useCallback((targetPath: string, userOverride?: string | null) => {
    setLoading(true);
    setError(null);
    sftpList(sessionId, targetPath, userOverride === undefined ? channelUser : userOverride)
      .then((result) => {
        for (const e of result) knownDirModes.current.set(e.path, e.permissionsOctal);
        setEntries(result);
        setPath(targetPath);
        setPathInput(targetPath);
      })
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, channelUser]);

  useEffect(() => {
    // Spec 0102: erst den Startpfad erfragen (ein fehlendes oder nicht
    // erreichbares Startverzeichnis liefert `"."` und ggf. den einmaligen
    // Hinweis); scheitert schon die Abfrage, wie bisher im Home starten.
    let cancelled = false;
    const queryStart = async () => {
      try {
        return await sftpStartDirectory(sessionId);
      } catch {
        return null;
      }
    };
    void queryStart().then((start) => {
      if (cancelled) return;
      const target = start?.path ?? ".";
      notifyMissingStartDirectory(t, start?.missingDirectory ?? null);
      setStartPath(target);
      load(target);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Schließt das offene Menü (Drei-Punkte oder Kontextmenü) bei einem Klick
  // irgendwo sonst hin. Die Trigger (⋮-Button, Rechtsklick) rufen jeweils
  // `stopPropagation()` auf (s. dortige Kommentare, Spec 0054 Teil 0) —
  // dieser Listener sieht also nie den Klick, der ein Menü gerade erst
  // geöffnet/gewechselt hat, nur einen tatsächlich "von außen" kommenden.
  useEffect(() => {
    if (openMenu === null) return;
    const handler = () => setOpenMenu(null);
    // Escape closes the menu too; focus goes back to the ⋮ button that
    // opened it (docs/ui-guidelines.md, Keyboard).
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const closing = openMenu;
      setOpenMenu(null);
      if (closing.anchor === "row") {
        focusMenuTrigger(closing.entry.path);
      }
    };
    document.addEventListener("click", handler);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("click", handler);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [openMenu]);

  // Issue #570: Kontextmenü auf leerer Fläche — Klick außerhalb und Escape
  // schließen es wie das Eintragsmenü.
  useEffect(() => {
    if (dirMenu === null) return;
    const handler = () => setDirMenu(null);
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setDirMenu(null);
    };
    document.addEventListener("click", handler);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("click", handler);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [dirMenu]);

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
                ? { ...row, stopped, incompleteRemotePath: event.incompleteRemotePath ?? undefined }
                : row,
            ),
          );
        } else if (event.error) {
          setTransfers((prev) =>
            prev.map((t) => (t.id === event.transferId ? { ...t, error: event.error } : t)),
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
  }, [sessionId]);

  // Spec 0020, Abschnitt 5.1: Upload per Drag-and-Drop aus dem Betriebssystem.
  // Nur aktiv, während dieser Tab UND die Dateien-Ansicht sichtbar sind (s.
  // `isVisible`-Doc-Kommentar oben) — `onDragDropEvent` ist global für das
  // gesamte Fenster, ohne dieses Gate würden gleichzeitig mehrere
  // (unsichtbar) gemountete Dateibrowser-Instanzen anderer Tabs denselben
  // Drop ebenfalls als Upload in ihr jeweiliges Verzeichnis interpretieren.
  useEffect(() => {
    if (!isVisible) return;
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

  /** Spec 0054, Teil 2: "Herunterladen" — direkt ins Standard-
   * Downloadverzeichnis, ohne Dialog. Datei oder Ordner (rekursiv, Backend
   * entscheidet anhand `entry.isDir`, s. `sftp_download_default`). */
  const handleDownloadDefault = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    sftpDownloadDefault(sessionId, entry.path, channelUser)
      .then((result) => notifyDownloaded(entry.name, result))
      .catch((err) => notifyFailed("fileToasts.downloadFailed", entry.name, err));
  };

  /** Spec 0054, Teil 2: "Herunterladen nach…" — Dialog für einen präzisen
   * Zielpfad. Für Dateien der bestehende Speichern-Dialog (exakter
   * Dateiname wählbar), für Ordner ein Zielordner-Dialog (ein Ordner hat
   * keinen einzelnen Dateinamen zum Speichern). */
  const handleDownloadChoose = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
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

  /** Spec 0054, Teil 2: "Pfad kopieren" — reine Zwischenablage-Aktion, kein
   * Backend-Aufruf nötig. */
  const handleCopyPath = async (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    try {
      await navigator.clipboard.writeText(entry.path);
      notifyOk("fileToasts.pathCopied", {});
    } catch (err) {
      console.warn("Konnte Pfad nicht in die Zwischenablage kopieren:", err);
      notifyFailed("fileToasts.copyFailed", entry.name, err);
    }
  };

  /** Spec 0054, Teil 2 / Issue #357 (Spec 0086 A1.1, A1.5): "Dateiinhalt
   * kopieren" — liest den Inhalt begrenzt bis 50 MB (das Backend lehnt zu
   * große und nicht-Text-Dateien ab, s. `sftp_copy_content`) und kopiert ihn
   * in die Zwischenablage. Ab 5 MB zeigt das Panel Fortschritt und einen
   * Abbrechen-Knopf. */
  const handleCopyContent = async (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    const requestId = crypto.randomUUID();
    setCopyJob({ requestId, name: entry.name, done: 0, total: entry.size });
    try {
      const content = await sftpCopyContent(sessionId, entry.path, channelUser, requestId, (p) =>
        setCopyJob((job) =>
          job?.requestId === requestId ? { ...job, done: p.done, total: p.total ?? job.total } : job,
        ),
      );
      await clipboardWriteText(content);
      notifyOk("fileToasts.contentCopied", { name: entry.name });
    } catch (err) {
      // Ein Abbruch durch den Nutzer ist keine Störung: nichts anzeigen.
      if (commandErrorCode(err) === "SFTP_COPY_CANCELLED") return;
      const code = commandErrorCode(err);
      notifyFailed(
        "fileToasts.copyFailed",
        entry.name,
        code === "SFTP_CONTENT_TOO_LARGE"
          ? { message: translateErrorCode(t, code, commandErrorMessage(err)) }
          : err,
      );
    } finally {
      setCopyJob((job) => (job?.requestId === requestId ? null : job));
    }
  };

  /** Issue #358: "Datei kopieren" — legt die Datei (bis 50 MB) als
   * Dateiverweis in die Zwischenablage; Fortschritt und Abbrechen wie bei
   * "Dateiinhalt kopieren". Bei Abbruch bleibt die Zwischenablage unverändert. */
  const handleCopyFile = async (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    const requestId = crypto.randomUUID();
    setCopyJob({ requestId, name: entry.name, done: 0, total: entry.size });
    try {
      await sftpCopyFile(sessionId, entry.path, channelUser, requestId, (p) =>
        setCopyJob((job) =>
          job?.requestId === requestId ? { ...job, done: p.done, total: p.total ?? job.total } : job,
        ),
      );
      notifyOk("fileToasts.fileCopied", { name: entry.name });
    } catch (err) {
      const code = commandErrorCode(err);
      if (code === "SFTP_COPY_CANCELLED") return;
      notifyFailed(
        "fileToasts.copyFailed",
        entry.name,
        code === "SFTP_CONTENT_TOO_LARGE" || code === "CLIPBOARD_FILE_FAILED"
          ? { message: translateErrorCode(t, code, commandErrorMessage(err)) }
          : err,
      );
    } finally {
      setCopyJob((job) => (job?.requestId === requestId ? null : job));
    }
  };

  const handleShowProperties = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    setProperties(entry);
  };

  const handleRefresh = () => {
    setOpenMenu(null);
    load(path);
  };

  /** Spec 0054, Teil 4, Punkt 1/2: Download in den Editier-Temp-Ordner +
   * Öffnen mit dem konfigurierten (oder OS-Standard-)Programm. Fehler
   * (Download gescheitert, Programm nicht gefunden) landen in der
   * bestehenden `error`-Anzeige — der Hook selbst zeigt keine UI. */
  const handleOpenLocally = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    localEdit
      .startEditing(entry, elevated ? elevatedUser : null)
      .then(() => notifyOk("fileToasts.openedLocally", { name: entry.name }))
      .catch((err) => notifyFailed("fileToasts.openLocallyFailed", entry.name, err));
  };

  /** Spec 0054, Teil 4, Punkt 5: baut die Diff-/Konflikt-Daten und öffnet
   * den Bestätigungsdialog — der eigentliche Upload passiert erst nach
   * Bestätigung in `handleConfirmEditUpload`. */
  // Issue #656: the identity of the edit session differs from the active one
  // (other target user, or elevated mode switched on/off). Plain "Upload" is
  // not offered then; the explicit "Upload as <user>" click is the only way
  // to send the current identity.
  const editIdentityMismatch =
    localEdit.session !== null && localEdit.session.elevatedUser !== channelUser;
  // Identity chosen by that click, held until the offer dialog is confirmed.
  const [editUploadAs, setEditUploadAs] = useState<{ user: string | null } | null>(null);
  const handleOfferEditUpload = (asOther = false) => {
    const target = asOther ? { user: channelUser } : null;
    setEditUploadAs(target);
    localEdit
      .buildUploadOffer(target ? target.user : undefined)
      .then((offer) => setEditUploadOffer(offer))
      .catch((err) => setError(commandErrorMessage(err)));
  };

  const handleConfirmEditUpload = () => {
    setEditUploadOffer(null);
    const name = localEdit.session?.entry.name ?? "";
    // Spec-Reviewer-Fund: der Modus-Zusatz kommt von der Bearbeitung (so
    // wurde hochgeladen), nicht vom aktuellen Modus der Ansicht.
    const editUser = editUploadAs ? editUploadAs.user : (localEdit.session?.elevatedUser ?? null);
    const asOther = editUploadAs;
    setEditUploadAs(null);
    const editSuffix = editUser ? t("fileToasts.asElevated", { user: editUser }) : "";
    editUploadingNameRef.current = name;
    localEdit.confirmUpload(asOther ? asOther.user : undefined).then((error) => {
      editUploadingNameRef.current = null;
      // Issue #655: a failure is shown only in the edit footer (no toast).
      if (error === null) {
        showToast({ kind: "success", message: t("fileToasts.editUploaded", { name }) + editSuffix });
      }
      load(path);
    });
  };

  // Spec 0054, Teil 3: "Löschen ... bei Ordnern mit Hinweis auf rekursives
  // Löschen und Anzahl" — Vorschau nachladen, sobald ein Ordner als
  // Lösch-Ziel gewählt wird (bei einer Datei ist die Zählung trivial, kein
  // Backend-Aufruf nötig).
  useEffect(() => {
    if (!deleteTarget || !deleteTarget.isDir) {
      setDeletePreview(null);
      return;
    }
    let cancelled = false;
    sftpDeletePreview(sessionId, deleteTarget.path, channelUser)
      .then((preview) => {
        if (!cancelled) setDeletePreview(preview);
      })
      .catch((err) => {
        if (!cancelled) setError(commandErrorMessage(err));
      });
    return () => {
      cancelled = true;
    };
    // Spec-Reviewer-Fund: die Vorschau muss im selben Modus zählen, in dem
    // danach gelöscht wird.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, deleteTarget, channelUser]);

  const handleConfirmDelete = () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    const preview = deletePreview;
    setDeleteTarget(null);
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
        setMoveCollision({ from, to, kind, targetIsDir });
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
    if (!moveCollision) return;
    const { from, to, kind } = moveCollision;
    setMoveCollision(null);
    sftpRename(sessionId, from, to, channelUser, true)
      .then(() => {
        notifyMoved(from, to, kind);
        load(path);
      })
      .catch((err) => notifyFailed("fileToasts.moveFailed", localBaseName(from), err));
  };

  const handleConfirmRename = () => {
    if (!renaming) return;
    const trimmed = renaming.value.trim();
    if (!trimmed || trimmed === renaming.entry.name) {
      setRenaming(null);
      return;
    }
    const newPath = joinPath(path, trimmed);
    const from = renaming.entry.path;
    setRenaming(null);
    performMove(from, newPath, "rename");
  };

  /** Issue #570: ein Fehler aus "Neuer Ordner"/"Neue Datei" — "Name
   * existiert" und "Zugriff verweigert" mit übersetztem Grund. */
  const notifyCreateFailed = (key: string, name: string, err: unknown) => {
    const code = commandErrorCode(err);
    notifyFailed(
      key,
      name,
      code === "SSH_SFTP_ALREADY_EXISTS" || code === "SSH_SFTP_PERMISSION_DENIED"
        ? { message: translateErrorCode(t, code, commandErrorMessage(err)) }
        : err,
    );
  };

  const handleConfirmMkdir = () => {
    const name = mkdirOpen?.trim() ?? "";
    if (!isValidEntryName(name)) {
      setNameError(t("fileBrowser.renamePrompt.invalidName"));
      return;
    }
    setMkdirOpen(null);
    setNameError(null);
    sftpMkdir(sessionId, joinPath(path, name), channelUser)
      .then(() => {
        notifyOk("fileToasts.mkdirDone", { name });
        load(path);
      })
      .catch((err) => notifyCreateFailed("fileToasts.mkdirFailed", name, err));
  };

  const handleConfirmNewFile = () => {
    const name = newFileOpen?.trim() ?? "";
    if (!isValidEntryName(name)) {
      setNameError(t("fileBrowser.renamePrompt.invalidName"));
      return;
    }
    setNewFileOpen(null);
    setNameError(null);
    sftpCreateFile(sessionId, joinPath(path, name), channelUser)
      .then(() => {
        notifyOk("fileToasts.createFileDone", { name });
        load(path);
      })
      .catch((err) => notifyCreateFailed("fileToasts.createFileFailed", name, err));
  };

  const handleCut = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    setCutEntry(entry);
  };

  /** Spec 0054, Teil 3: "Einfügen" — verschiebt den ausgeschnittenen
   * Eintrag in das aktuell angezeigte Verzeichnis. */
  const handlePaste = () => {
    if (!cutEntry) return;
    const target = joinPath(path, cutEntry.name);
    setCutEntry(null);
    if (target === cutEntry.path) return; // bereits hier, kein no-op-Fehler
    performMove(cutEntry.path, target, "move");
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

  const handleChmod = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    setChmodTarget(entry);
  };

  const handleConfirmChmod = (mode: number, recursive: boolean, applySpecialBits: boolean) => {
    if (!chmodTarget) return;
    const target = chmodTarget;
    setChmodTarget(null);
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

  const renderEntryMenu = (entry: RemoteEntryDto, className: string, point?: { x: number; y: number }) => (
    <FileEntryMenu
      entry={entry}
      narrow={narrow}
      className={className}
      point={point}
      onOpenLocally={handleOpenLocally}
      onDownloadDefault={handleDownloadDefault}
      onDownloadChoose={handleDownloadChoose}
      onCopyContent={handleCopyContent}
      onCopyFile={handleCopyFile}
      onCopyPath={handleCopyPath}
      onShowProperties={handleShowProperties}
      onRefresh={handleRefresh}
      onRename={(e) => {
        setOpenMenu(null);
        setRenaming({ entry: e, value: e.name });
      }}
      onCut={handleCut}
      onChmod={handleChmod}
      onDelete={(e) => {
        setOpenMenu(null);
        setDeleteTarget(e);
      }}
    />
  );

  const otherUserForm = otherUserOpen && (
              <form
                ref={otherUserFormRef}
                style={otherUserFormStyle}
                className="fixed z-20 flex w-64 flex-col gap-1.5 rounded border border-slate-600 bg-slate-800 p-2 shadow-lg narrow:fixed narrow:inset-x-0 narrow:bottom-0 narrow:left-0 narrow:top-auto narrow:mt-0 narrow:w-full narrow:rounded-b-none narrow:p-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  const user = otherUserDraft.trim();
                  if (!user) return;
                  setElevationTargetUser(user);
                  setOtherUserOpen(false);
                  handleToggleElevation(user);
                }}
              >
                <label className="text-xs" htmlFor="file-elevation-target-user">
                  {t("fileElevation.targetUser")}
                </label>
                <input
                  {...TECHNICAL_INPUT_PROPS}
                  id="file-elevation-target-user"
                  ref={otherUserInputRef}
                  value={otherUserDraft}
                  onChange={(e) => setOtherUserDraft(e.target.value)}
                  aria-describedby="file-elevation-target-user-help"
                  className={`${FIELD_CONTROL_CLASS} px-1.5 py-1 font-mono text-xs`}
                />
                <p id="file-elevation-target-user-help" className="m-0 text-xs text-slate-300">
                  {t("fileElevation.targetUserHelp")}
                </p>
                <Button size={narrow ? "md" : "sm"} type="submit" disabled={enablingElevation || !otherUserDraft.trim()}>
                  {t("fileElevation.confirm")}
                </Button>
              </form>
  );

  return (
    <div
      ref={panelRef}
      className={`flex h-full flex-col border-2 ${elevated ? "border-amber-500" : "border-transparent"}`}
      data-elevated={elevated ? "true" : "false"}
    >
      {elevated && (
        <div
          role="alert"
          className="flex items-center gap-2 bg-amber-600 px-2 py-1.5 text-xs font-semibold text-slate-950"
        >
          <span className="flex-1">⚠ {t("fileElevation.banner", { user: elevatedUser })}</span>
          <button
            type="button"
            onClick={() => handleToggleElevation()}
            className="rounded border border-slate-950/40 px-2 py-0.5 hover:bg-amber-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-950"
          >
            {t("fileElevation.toggleOff")}
          </button>
        </div>
      )}
      {elevated && elevationWarning && (
        <div role="alert" className="bg-red-950 px-2 py-1.5 text-xs text-red-200">
          {t("fileElevation.failure.sftpServerUnsafe", { path: elevationWarning })}
        </div>
      )}
      {narrow && (
        <div className="flex items-center gap-1.5 border-b border-slate-800 px-2 py-1.5">
          <Button
            className="min-h-11 min-w-11"
            onClick={() => load(startPath)}
            title={t("fileBrowser.toStartDirectory")}
            aria-label={t("fileBrowser.toStartDirectory")}
          >
            ⌂
          </Button>
          <Button
            className="min-h-11 min-w-11"
            onClick={() => load(parentPath(path))}
            disabled={path === "."}
            title={t("fileBrowser.parentDirectory")}
            aria-label={t("fileBrowser.parentDirectory")}
          >
            ↑
          </Button>
          <form
            className="min-w-0 flex-1"
            onSubmit={(e) => {
              e.preventDefault();
              load(pathInput.trim() || ".");
            }}
          >
            <input
              {...TECHNICAL_INPUT_PROPS}
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              aria-label={t("fileBrowser.properties.path")}
              title={pathInput}
              className={`${FIELD_CONTROL_CLASS} min-h-11 w-full px-2 py-1 font-mono text-xs`}
            />
          </form>
          <div ref={overflowRef} className="shrink-0">
            <Button
              ref={overflowButtonRef}
              className="min-h-11 min-w-11"
              onClick={() => {
                setElevationMenuOpen(false);
                setOtherUserOpen(false);
                setOverflowOpen((open) => !open);
              }}
              aria-haspopup="menu"
              aria-expanded={overflowOpen}
              aria-label={t("fileBrowser.fileActionsGroup")}
              title={t("fileBrowser.fileActionsGroup")}
            >
              ⋯
            </Button>
            {overflowOpen && (
              <div
                role="menu"
                aria-label={t("fileBrowser.fileActionsGroup")}
                className="fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-slate-600 bg-slate-800 py-1 shadow-lg"
              >
                {[
                  {
                    key: "mkdir",
                    label: t("fileBrowser.newFolderButton"),
                    run: () => {
                      setNameError(null);
                      setMkdirOpen("");
                    },
                  },
                  { key: "upload", label: t("fileBrowser.upload"), run: handleUploadButton },
                  { key: "uploadFolder", label: t("fileBrowser.uploadFolder"), run: handleUploadFolderButton },
                  ...(cutEntry
                    ? [
                        { key: "paste", label: t("fileBrowser.paste"), run: handlePaste },
                        { key: "cancelCut", label: t("fileBrowser.cancelCut"), run: () => setCutEntry(null) },
                      ]
                    : []),
                  ...(!elevated && sessionRoot !== undefined && sessionRoot !== true
                    ? [
                        {
                          key: "elevate",
                          label: enablingElevation
                            ? t("fileElevation.enabling")
                            : `🔓 ${t("fileElevation.toggle")}`,
                          run: () => handleToggleElevation("root"),
                          disabled: enablingElevation,
                        },
                        {
                          key: "otherUser",
                          label: t("fileElevation.otherUser"),
                          run: () => {
                            setOtherUserDraft(elevationTargetUser);
                            setOtherUserOpen(true);
                            setTimeout(() => otherUserInputRef.current?.focus(), 0);
                          },
                          disabled: enablingElevation,
                        },
                      ]
                    : []),
                ].map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    role="menuitem"
                    disabled={"disabled" in item ? item.disabled : false}
                    className="block min-h-11 w-full px-4 py-2 text-left text-sm text-slate-200 hover:bg-slate-700 focus-visible:bg-slate-700 focus-visible:outline-none disabled:opacity-50"
                    onClick={() => {
                      setOverflowOpen(false);
                      item.run();
                    }}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            )}
            {otherUserForm}
          </div>
        </div>
      )}
      {!narrow && (
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-slate-800 px-2 py-1.5">
        <div className="flex min-w-[12rem] flex-1 items-center gap-1.5">
        <Button
          size="sm"
          onClick={() => load(startPath)}
          title={t("fileBrowser.toStartDirectory")}
          aria-label={t("fileBrowser.toStartDirectory")}
        >
          ⌂
        </Button>
        <Button
          size="sm"
          onClick={() => load(parentPath(path))}
          disabled={path === "."}
          title={t("fileBrowser.parentDirectory")}
          aria-label={t("fileBrowser.parentDirectory")}
        >
          ↑
        </Button>
        <form
          className="min-w-32 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            load(pathInput.trim() || ".");
          }}
        >
          <input
            {...TECHNICAL_INPUT_PROPS}
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            aria-label={t("fileBrowser.properties.path")}
            title={pathInput}
            className={`${FIELD_CONTROL_CLASS} w-full px-2 py-1 font-mono text-xs`}
          />
        </form>
        </div>
        <fieldset
          aria-label={t("fileBrowser.fileActionsGroup")}
          className="m-0 flex min-w-0 items-center gap-1.5 border-0 p-0"
        >
        <Button
          size="sm"
          onClick={() => {
            setNameError(null);
            setMkdirOpen("");
          }}
        >
          {t("fileBrowser.newFolderButton")}
        </Button>
        <Button size="sm" onClick={handleUploadButton}>
          {t("fileBrowser.upload")}
        </Button>
        <Button size="sm" onClick={handleUploadFolderButton}>
          {t("fileBrowser.uploadFolder")}
        </Button>
        </fieldset>
        {!elevated && sessionRoot !== undefined && sessionRoot !== true && (
          <fieldset
            ref={elevationGroupRef}
            aria-label={t("fileElevation.group")}
            className="relative m-0 flex min-w-0 items-center gap-0.5 border-0 border-l border-slate-700 p-0 pl-2"
          >
            <Button
              size="sm"
              onClick={() => handleToggleElevation("root")}
              disabled={enablingElevation}
              title={t("fileElevation.toggleTitle")}
            >
              {enablingElevation ? t("fileElevation.enabling") : `🔓 ${t("fileElevation.toggle")}`}
            </Button>
            <Button
              size="sm"
              ref={elevationMenuButtonRef}
              onClick={() => {
                setOtherUserOpen(false);
                setElevationMenuOpen((open) => !open);
              }}
              disabled={enablingElevation}
              aria-haspopup="menu"
              aria-expanded={elevationMenuOpen || otherUserOpen}
              aria-label={t("fileElevation.moreOptions")}
              title={t("fileElevation.moreOptions")}
            >
              ▾
            </Button>
            {elevationMenuOpen && (
              <div
                role="menu"
                ref={elevationMenuRef}
                style={elevationMenuStyle}
                className="fixed z-20 rounded border border-slate-600 bg-slate-800 p-1 shadow-lg"
              >
                <button
                  type="button"
                  role="menuitem"
                  ref={otherUserItemRef}
                  className="w-full whitespace-nowrap rounded px-2 py-1 text-left text-xs hover:bg-slate-700 focus-visible:bg-slate-700"
                  onClick={() => {
                    setElevationMenuOpen(false);
                    setOtherUserDraft(elevationTargetUser);
                    setOtherUserOpen(true);
                    setTimeout(() => otherUserInputRef.current?.focus(), 0);
                  }}
                >
                  {t("fileElevation.otherUser")}
                </button>
              </div>
            )}
            {otherUserForm}
          </fieldset>
        )}
        {cutEntry && (
          <>
            <Button
              size="sm"
              onClick={handlePaste}
              title={t("fileBrowser.pasteTitle", { name: cutEntry.name })}
            >
              {t("fileBrowser.paste")}
            </Button>
            <Button
              size="sm"
              onClick={() => setCutEntry(null)}
              title={t("fileBrowser.cancelCut")}
              aria-label={t("fileBrowser.cancelCut")}
            >
              ✕
            </Button>
          </>
        )}
      </div>
      )}

      {transfers.length > 0 && (
        <div className="space-y-1 border-b border-slate-800 bg-slate-950/60 px-2 py-1.5">
          {transfers.map((transfer) => (
            <div key={transfer.id} className="flex items-center gap-2 text-xs">
              {transfer.doneElevationEnded ? (
                <>
                  <span className="text-emerald-400">✓</span>
                  <span className="min-w-0 flex-1 truncate text-slate-300">
                    {t("fileBrowser.transferDone", { name: transfer.fileName })}{" "}
                    <span className="text-slate-400">{t("fileBrowser.transferElevationEnded")}</span>
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setTransfers((prev) => prev.filter((x) => x.id !== transfer.id))}
                    aria-label={t("common.close")}
                    title={t("common.close")}
                  >
                    ✕
                  </Button>
                </>
              ) : transfer.stopped ? (
                <>
                  <span className="text-amber-400">■</span>
                  <span className="min-w-0 flex-1 truncate text-slate-300">
                    {t(
                      transfer.stopped === "cancelled"
                        ? "fileBrowser.transferCancelled"
                        : "fileBrowser.transferStopped",
                      { name: transfer.fileName },
                    )}
                    {transfer.incompleteRemotePath && (
                      <span className="text-amber-300">
                        {" "}
                        {t("fileBrowser.transferIncomplete", { path: transfer.incompleteRemotePath })}
                      </span>
                    )}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setTransfers((prev) => prev.filter((x) => x.id !== transfer.id))}
                    aria-label={t("common.close")}
                    title={t("common.close")}
                  >
                    ✕
                  </Button>
                </>
              ) : transfer.error ? (
                <>
                  <span className="text-red-400">✗</span>
                  <span className="min-w-0 flex-1 truncate text-red-300">
                    {transfer.fileName}: {transfer.error}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setTransfers((prev) => prev.filter((x) => x.id !== transfer.id))}
                    aria-label={t("common.close")}
                    title={t("common.close")}
                  >
                    ✕
                  </Button>
                </>
              ) : (
                <>
                  <span className="inline-block h-2 w-2 shrink-0 animate-pulse rounded-full bg-indigo-400" />
                  <span className="min-w-0 flex-1 truncate text-slate-300">
                    {t(
                      transfer.kind === "upload"
                        ? "fileBrowser.transferUpload"
                        : "fileBrowser.transferDownload",
                      { name: transfer.fileName },
                    )}
                    {transfer.totalBytes !== null && ` (${formatBytes(transfer.totalBytes)})`}
                    {transfer.folder &&
                      ` — ${t("fileBrowser.transferFiles", {
                        done: transfer.folder.filesDone,
                        total: transfer.folder.filesTotal,
                      })}`}
                  </span>
                  {transfer.totalBytes !== null && transfer.totalBytes > 0 ? (
                    <progress
                      className="h-1.5 w-24 shrink-0"
                      max={transfer.totalBytes}
                      value={Math.min(transfer.bytesDone ?? 0, transfer.totalBytes)}
                      aria-label={transfer.fileName}
                    />
                  ) : null}
                  <span className="shrink-0 whitespace-nowrap text-right tabular-nums text-slate-400">
                    {formatBytes(transfer.bytesDone ?? 0)}
                    {transfer.totalBytes !== null && ` / ${formatBytes(transfer.totalBytes)}`}
                    {" · "}
                    <span className="inline-block min-w-[5.5rem]">{formatSpeed(transfer.rate?.speed ?? null)}</span>
                    {" · "}
                    <span className="inline-block min-w-[6.5rem]">
                      {transfer.rate?.etaSeconds != null
                        ? t("fileBrowser.transferRemaining", { time: formatDuration(transfer.rate.etaSeconds) })
                        : formatDuration(null)}
                    </span>
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      sftpCancelTransfer(sessionId, transfer.id).catch((err) =>
                        console.warn("Konnte Transfer nicht abbrechen:", err),
                      )
                    }
                    aria-label={t("fileBrowser.transferCancel")}
                    title={t("fileBrowser.transferCancel")}
                  >
                    ✕
                  </Button>
                </>
              )}
            </div>
          ))}
        </div>
      )}

      {copyJob && Math.max(copyJob.done, copyJob.total ?? 0) >= COPY_PROGRESS_FROM_BYTES && (
        <output
          className="flex items-center gap-2 border-b border-slate-700 px-2 py-1.5 text-xs text-slate-200"
        >
          <span className="min-w-0 flex-1 truncate">
            {copyJob.total !== null
              ? t("fileBrowser.copyProgress", {
                  done: formatMb(copyJob.done),
                  total: formatMb(copyJob.total),
                })
              : t("fileBrowser.copyProgressUnknownTotal", { done: formatMb(copyJob.done) })}
          </span>
          <Button
            size="sm"
            onClick={() => {
              void sftpCancelCopyContent(copyJob.requestId).catch(() => {});
            }}
          >
            {t("common.cancel")}
          </Button>
        </output>
      )}
      <div
        ref={tableContainerRef}
        className={`relative min-h-0 flex-1 overflow-auto ${dragOver ? "bg-indigo-950/40" : ""}`}
        // Issue #570: Rechtsklick auf leere Fläche (unter den Zeilen oder in
        // einem leeren Verzeichnis). Die Zeilen stoppen das Ereignis selbst.
        onContextMenu={(e) => {
          e.preventDefault();
          setOpenMenu(null);
          setClipboardFiles(false);
          setDirMenu({ x: e.clientX, y: e.clientY });
          if (!cutEntry) {
            clipboardHasFiles()
              .then(setClipboardFiles)
              .catch(() => setClipboardFiles(false));
          }
        }}
      >
        {loading && <p className="p-3 text-xs text-slate-400">{t("fileBrowser.loading")}</p>}
        {error && <p className="p-3 text-xs text-red-400">{error}</p>}
        {!loading && !error && entries.length === 0 && (
          <p className="p-3 text-xs text-slate-400">
            {(knownDirModes.current.get(path) ?? 0o111) & 0o111
              ? t("fileBrowser.emptyDirectory")
              : t("fileBrowser.noPermissionToOpen")}
          </p>
        )}
        {!loading && !error && narrow && entries.length > 0 && (
          <ul className="m-0 list-none p-0 text-xs">
            {entries.map((entry) => {
              const rowMenuOpen = openMenu?.anchor === "row" && openMenu.entry.path === entry.path;
              const details = [
                entry.isDir ? null : formatBytes(entry.size),
                entry.modified ? formatDateTime(entry.modified, i18n.language) : null,
              ]
                .filter((part): part is string => part !== null)
                .join(" · ");
              return (
                // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- mouse-only shortcut to the ⋯ menu, same as the table row
                <li
                  key={entry.path}
                  className="flex min-h-11 items-center border-t border-slate-800/70 pl-2"
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    setDirMenu(null);
                    setOpenMenu({ entry, anchor: "row" });
                  }}
                >
                  <div className="min-w-0 flex-1 py-1">
                    {entry.isDir ? (
                      <button
                        type="button"
                        onClick={() => load(entry.path)}
                        className="block max-w-full truncate rounded text-left text-sm text-indigo-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                        title={entry.name}
                      >
                        📁 {entry.name}
                      </button>
                    ) : (
                      <span className="block truncate text-sm text-slate-200" title={entry.name}>
                        📄 {entry.name}
                      </span>
                    )}
                    <span className="block truncate text-slate-400">{details || "—"}</span>
                  </div>
                  <button
                    type="button"
                    onClick={(e) => {
                      // Same reasoning as the wide row button: keep the click away from the
                      // document-wide "click outside closes" listener.
                      e.stopPropagation();
                      setOpenMenu(rowMenuOpen ? null : { entry, anchor: "row" });
                    }}
                    data-menu-trigger={entry.path}
                    aria-label={`${t("fileBrowser.actionsColumn")}: ${entry.name}`}
                    aria-haspopup="true"
                    aria-expanded={rowMenuOpen}
                    className="flex min-h-11 min-w-11 shrink-0 items-center justify-center rounded text-lg text-slate-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                  >
                    ⋯
                  </button>
                  {rowMenuOpen && renderEntryMenu(entry, "")}
                </li>
              );
            })}
          </ul>
        )}
        {!loading && !error && !narrow && entries.length > 0 && (
          <table
            className="w-full table-fixed border-separate border-spacing-0 text-left text-xs"
            style={{
              // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts):
              // ohne diese `min-width` konnte die Name-Spalte unter
              // `table-fixed` auf tatsächlich 0px kollabieren, sobald der
              // SSH-/SFTP-Bereich (Teil 2, per Divider verstellbar) schmaler
              // war als die Summe der anderen Spalten — Dateinamen
              // verschwanden komplett statt nur abgeschnitten zu werden,
              // und `clampColumnWidth`s `NAME_MIN_WIDTH` griff dabei gar
              // nicht (das klemmt nur beim Spalten-Drag selbst, nicht beim
              // Schmaler-Werden des ganzen Panels). Mit dieser Mindestbreite
              // wächst die Tabelle stattdessen über den Container hinaus,
              // und der umgebende `overflow-auto`-Container (s. oben)
              // scrollt horizontal statt die Name-Spalte zu verschlucken.
              minWidth:
                columnWidths.size +
                columnWidths.permissions +
                columnWidths.modified +
                ACTIONS_COLUMN_WIDTH +
                NAME_MIN_WIDTH,
            }}
          >
            {/* `table-fixed` + `<colgroup>` statt des vorherigen
                automatischen Spaltenlayouts — nur so lassen sich
                Spaltenbreiten gezielt per Drag setzen. Die Name-Spalte hat
                bewusst keine `width` (kein `<col>`-Attribut dafür) — unter
                `table-fixed` nimmt eine Spalte ohne explizite Breite den
                nach den anderen verbleibenden Rest ein, genau das von der
                Spec verlangte "flexibler Rest"-Verhalten. */}
            <colgroup>
              <col />
              <col style={{ width: columnWidths.size }} />
              <col style={{ width: columnWidths.permissions }} />
              <col style={{ width: columnWidths.modified }} />
              <col style={{ width: ACTIONS_COLUMN_WIDTH }} />
            </colgroup>
            <thead className="text-slate-400">
              <tr className="[&>th]:px-2 [&>th]:py-1 [&>th]:font-normal">
                <th className={STICKY_TH}>{t("fileBrowser.column.name")}</th>
                <th className={STICKY_TH}>
                  {t("fileBrowser.column.size")}
                  <ColumnResizeHandle
                    testId="column-resize-size"
                    onDrag={handleColumnDrag("size")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th className={STICKY_TH}>
                  {t("fileBrowser.column.permissions")}
                  <ColumnResizeHandle
                    testId="column-resize-permissions"
                    onDrag={handleColumnDrag("permissions")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th className={STICKY_TH}>
                  {t("fileBrowser.column.modified")}
                  <ColumnResizeHandle
                    testId="column-resize-modified"
                    onDrag={handleColumnDrag("modified")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th className={STICKY_TH}>
                  <span className="sr-only">{t("fileBrowser.actionsColumn")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr
                  key={entry.path}
                  className="hover:bg-slate-800/40 [&>td]:border-t [&>td]:border-slate-800/70"
                  // Spec 0054, Teil 1: Rechtsklick öffnet dasselbe Menü wie
                  // das Drei-Punkte-Symbol, nur an der Klickposition statt
                  // in der Aktionsspalte verankert (s. `openMenu`-Doc-
                  // Kommentar oben). `preventDefault` unterdrückt das
                  // native Browser-/OS-Kontextmenü.
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    setDirMenu(null);
                    setOpenMenu({ entry, anchor: { x: e.clientX, y: e.clientY } });
                  }}
                >
                  <td className="max-w-0 px-2 py-1">
                    {entry.isDir ? (
                      <button
                        type="button"
                        onClick={() => load(entry.path)}
                        className="block max-w-full truncate rounded text-left text-indigo-300 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                        title={entry.name}
                      >
                        📁 {entry.name}
                      </button>
                    ) : (
                      <span className="block truncate text-slate-200" title={entry.name}>
                        📄 {entry.name}
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap text-slate-400">
                    {entry.isDir ? "—" : formatBytes(entry.size)}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap font-mono text-slate-400">
                    {entry.permissions}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap text-slate-400">
                    {entry.modified ? formatDateTime(entry.modified, i18n.language) : "—"}
                  </td>
                  <td className="relative px-2 py-1 text-right">
                    <button
                      type="button"
                      onClick={(e) => {
                        // Bug-Fix (Spec 0054, Teil 0): das "Klick-außerhalb
                        // schließt das Menü"-Effekt weiter unten hängt einen
                        // rohen `document.addEventListener("click", ...)`
                        // ein, der bei JEDEM Klick unbedingt `setMenuFor(null)`
                        // aufruft. Ohne `stopPropagation` hier lief dieser
                        // Klick nach dem synthetischen `onClick` (das den
                        // State auf den NEUEN Eintrag setzt) noch nativ bis
                        // zu `document` weiter und traf dort den ALTEN,
                        // noch nicht abgeräumten Listener (Cleanup des
                        // vorherigen Effekt-Laufs passiert erst nach dem
                        // Commit, also nach diesem Klick) — dessen
                        // `setMenuFor(null)` überschrieb den gerade gesetzten
                        // Wert innerhalb desselben Batches. Effekt: das Menü
                        // eines anderen Eintrags ließ sich nie öffnen, ein
                        // bereits offenes Menü verschwand bei jedem weiteren
                        // Drei-Punkte-Klick sofort wieder, statt zu wechseln.
                        // `stopPropagation` verhindert, dass dieser Klick den
                        // dokumentweiten Listener überhaupt erreicht (React
                        // ruft dabei auch `nativeEvent.stopPropagation()`
                        // auf) — ein echtes Klicken außerhalb von Button und
                        // Menü ist davon unberührt und schließt weiterhin.
                        e.stopPropagation();
                        setOpenMenu(
                          openMenu?.anchor === "row" && openMenu.entry.path === entry.path
                            ? null
                            : { entry, anchor: "row" },
                        );
                      }}
                      data-menu-trigger={entry.path}
                      aria-label={`${t("fileBrowser.actionsColumn")}: ${entry.name}`}
                      aria-haspopup="true"
                      aria-expanded={openMenu?.anchor === "row" && openMenu.entry.path === entry.path}
                      className="rounded px-1.5 text-slate-400 hover:text-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                    >
                      ⋮
                    </button>
                    {openMenu?.anchor === "row" && openMenu.entry.path === entry.path &&
                      renderEntryMenu(entry, "fixed z-10")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {dragOver && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center border-2 border-dashed border-indigo-500 bg-indigo-950/30 text-sm text-indigo-200">
            {t("fileBrowser.dropHint", { path: displayPath(path) })}
          </div>
        )}
      </div>

      {/* Spec 0054, Teil 1: Kontextmenü (Rechtsklick) — dieselbe
          `FileEntryMenu`-Instanz wie das Drei-Punkte-Menü, nur `fixed` an
          der Klickposition statt innerhalb der Aktionsspalte verankert.
          `position: fixed` ignoriert das `overflow-auto` des umgebenden
          Tabellen-Containers (kein Portal nötig), solange kein Vorfahre
          `transform`/`filter`/`perspective` setzt — hier nicht der Fall. */}
      {openMenu &&
        openMenu.anchor !== "row" &&
        renderEntryMenu(openMenu.entry, "fixed z-20", openMenu.anchor)}

      {dirMenu && (
        <DirectoryMenu
          narrow={narrow}
          x={dirMenu.x}
          y={dirMenu.y}
          pasteName={cutEntry?.name ?? null}
          clipboardFiles={!cutEntry && clipboardFiles}
          onNewFolder={() => {
            setDirMenu(null);
            setNameError(null);
            setMkdirOpen("");
          }}
          onNewFile={() => {
            setDirMenu(null);
            setNameError(null);
            setNewFileOpen("");
          }}
          onRefresh={() => {
            setDirMenu(null);
            handleRefresh();
          }}
          onPaste={() => {
            setDirMenu(null);
            if (cutEntry) handlePaste();
            else void handlePasteClipboardFiles();
          }}
        />
      )}

      {/* Spec 0054, Teil 4: Statusleiste für den "Lokal öffnen"-Flow —
          zeigt an, solange eine Bearbeitung aktiv ist (`editing`), bietet
          bei einer erkannten lokalen Änderung (`changed`) den Upload an,
          und macht das "Bearbeitung beenden" jederzeit erreichbar (Spec:
          "Watcher stoppt, wenn der Nutzer den Flow beendet"). */}
      {localEdit.session && (
        <div className="flex items-center gap-2 border-t border-indigo-700/50 bg-indigo-950/40 px-2 py-1.5 text-xs">
          {localEdit.session.status === "uploading" ? (
            <span className="text-slate-300">
              {t("fileBrowser.localEdit.uploading", { name: localEdit.session.entry.name })}
            </span>
          ) : localEdit.session.status === "changed" ? (
            <>
              <span className="flex-1 text-amber-300">
                {t("fileBrowser.localEdit.changed", { name: localEdit.session.entry.name })}
              </span>
              {editIdentityMismatch ? (
                <Button size="sm" variant="primary" onClick={() => handleOfferEditUpload(true)}>
                  {channelUser
                    ? t("fileElevation.editUploadAs", { user: channelUser })
                    : t("fileElevation.editUploadAsLogin")}
                </Button>
              ) : (
                <Button size="sm" variant="primary" onClick={() => handleOfferEditUpload()}>
                  {t("fileBrowser.upload")}
                </Button>
              )}
              <Button size="sm" onClick={localEdit.dismissChange}>
                {t("fileBrowser.localEdit.later")}
              </Button>
            </>
          ) : (
            <span className="flex-1 text-slate-300">
              {t("fileBrowser.localEdit.editing", { name: localEdit.session.entry.name })}
              {localEdit.session.elevatedUser && (
                <span className="ml-1 text-amber-300">
                  {t("fileElevation.editOpenedAs", { user: localEdit.session.elevatedUser })}
                </span>
              )}
            </span>
          )}
          <Button size="sm" onClick={localEdit.endSession}>
            {t("fileBrowser.localEdit.end")}
          </Button>
          {editIdentityMismatch && (
            <span className="w-full text-amber-300">
              {localEdit.session.elevatedUser
                ? channelUser
                  ? t("fileElevation.editIdentityChanged", {
                      from: localEdit.session.elevatedUser,
                      user: channelUser,
                    })
                  : t("fileElevation.editIdentityElevatedOff", {
                      from: localEdit.session.elevatedUser,
                    })
                : t("fileElevation.editIdentityElevatedOn", { user: channelUser })}
            </span>
          )}
          {localEdit.session.error && (
            <span className="w-full text-red-300">{localEdit.session.error}</span>
          )}
        </div>
      )}

      {editUploadOffer && (
        <DialogFrame
          layer="sessionTab"
          title={
            (editUploadAs ? editUploadAs.user : localEdit.session?.elevatedUser)
              ? t("fileElevation.editUploadTitle", {
                  user: editUploadAs ? editUploadAs.user : localEdit.session?.elevatedUser,
                })
              : t("fileBrowser.localEdit.uploadTitle")
          }
          onClose={() => { setEditUploadOffer(null); setEditUploadAs(null); }}
          actions={
            <>
              <Button onClick={() => { setEditUploadOffer(null); setEditUploadAs(null); }}>{t("fileBrowser.cancel")}</Button>
              <Button variant="primary" onClick={handleConfirmEditUpload}>
                {t("fileBrowser.upload")}
              </Button>
            </>
          }
        >
          {editUploadOffer.remoteChangedSinceDownload && (
            <p className="mb-3 border border-red-700/50 bg-red-950/40 px-2 py-1.5 text-xs text-red-300">
              {t("fileBrowser.localEdit.remoteChanged")}
            </p>
          )}
          {editUploadOffer.localText !== null && editUploadOffer.remoteText !== null ? (
            <NoteDiffPreview
              previousContent={editUploadOffer.remoteText}
              newContent={editUploadOffer.localText}
            />
          ) : (
            <p className="border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-400">
              {editUploadOffer.remoteReadFailure
                ? t(
                    previewReadFailureHint(editUploadOffer.remoteReadFailure).key,
                    previewReadFailureHint(editUploadOffer.remoteReadFailure).options,
                  )
                : t("fileBrowser.localEdit.noDiff", { size: formatBytes(editUploadOffer.localSize) })}
            </p>
          )}
        </DialogFrame>
      )}

      {properties && (
        <FilePropertiesDialog entry={properties} onClose={() => setProperties(null)} />
      )}

      {renaming && (
        <RenamePrompt
          initialValue={renaming.value}
          onChange={(value) => setRenaming({ entry: renaming.entry, value })}
          onCancel={() => setRenaming(null)}
          onConfirm={handleConfirmRename}
        />
      )}

      {mkdirOpen !== null && (
        <RenamePrompt
          title={t("fileBrowser.renamePrompt.newFolderTitle")}
          initialValue={mkdirOpen}
          error={nameError}
          onChange={(value) => {
            setNameError(null);
            setMkdirOpen(value);
          }}
          onCancel={() => setMkdirOpen(null)}
          onConfirm={handleConfirmMkdir}
        />
      )}

      {newFileOpen !== null && (
        <RenamePrompt
          title={t("fileBrowser.renamePrompt.newFileTitle")}
          initialValue={newFileOpen}
          error={nameError}
          onChange={(value) => {
            setNameError(null);
            setNewFileOpen(value);
          }}
          onCancel={() => setNewFileOpen(null)}
          onConfirm={handleConfirmNewFile}
        />
      )}

      {deleteTarget && (
        <DialogFrame
          layer="sessionTab"
          size="sm"
          title={
            elevated
              ? t(deleteTarget.isDir ? "fileElevation.deleteDirTitle" : "fileElevation.deleteFileTitle", {
                  user: elevatedUser,
                })
              : deleteTarget.isDir
                ? t("fileBrowser.deleteDialog.dirTitle")
                : t("fileBrowser.deleteDialog.fileTitle")
          }
          onClose={() => setDeleteTarget(null)}
          actions={
            <>
              <Button onClick={() => setDeleteTarget(null)}>{t("fileBrowser.cancel")}</Button>
              <Button variant="danger" onClick={handleConfirmDelete}>
                {t("fileBrowser.delete")}
              </Button>
            </>
          }
        >
          <p>
            <span className="font-mono text-xs break-all">{deleteTarget.path}</span>{" "}
            {t("fileBrowser.deleteDialog.body")}
            {deleteTarget.isDir &&
              (deletePreview ? (
                <>
                  {" "}
                  <Trans
                    t={t}
                    i18nKey="fileBrowser.deleteDialog.contents"
                    values={{
                      fileCount: deletePreview.fileCount,
                      dirCount: deletePreview.dirCount,
                      files: t("fileBrowser.deleteDialog.files", { count: deletePreview.fileCount }),
                      dirs: t("fileBrowser.deleteDialog.dirs", { count: deletePreview.dirCount }),
                    }}
                    components={{ strong: <strong /> }}
                  />
                </>
              ) : (
                ` ${t("fileBrowser.deleteDialog.counting")}`
              ))}
          </p>
        </DialogFrame>
      )}

      {chmodTarget && (
        <ChmodDialog
          entry={chmodTarget}
          modeNote={elevated ? t("fileElevation.chmodNote", { user: elevatedUser }) : null}
          onCancel={() => setChmodTarget(null)}
          onConfirm={handleConfirmChmod}
        />
      )}

      {moveCollision && (
        <DialogFrame
          layer="sessionTab"
          size="sm"
          title={
            elevated && !moveCollision.targetIsDir
              ? t("fileElevation.moveCollisionTitle", { user: elevatedUser })
              : t("fileBrowser.moveCollision.title")
          }
          onClose={() => setMoveCollision(null)}
          actions={
            <>
              <Button onClick={() => setMoveCollision(null)}>
                {t("fileBrowser.cancel")}
              </Button>
              {!moveCollision.targetIsDir && (
                <Button variant="danger" onClick={handleConfirmMoveCollision}>
                  {t("fileBrowser.overwrite")}
                </Button>
              )}
            </>
          }
        >
          <p>
            <span className="font-mono text-xs break-all">{moveCollision.to}</span>{" "}
            {moveCollision.targetIsDir
              ? t("fileBrowser.moveCollision.folderBody")
              : t("fileBrowser.moveCollision.body")}
          </p>
        </DialogFrame>
      )}

      {uploadConflict && (
        <DialogFrame
          layer="sessionTab"
          title={elevated ? t("fileElevation.overwriteTitle", { user: elevatedUser }) : t("fileBrowser.uploadConflict.title")}
          onClose={() => setUploadConflict(null)}
          actions={
            <>
              <Button onClick={() => setUploadConflict(null)}>{t("fileBrowser.cancel")}</Button>
              <Button variant="danger" onClick={handleConfirmUpload}>
                {t("fileBrowser.overwrite")}
              </Button>
            </>
          }
        >
          <p className="mb-3">
            <span className="font-mono text-xs break-all">{uploadConflict.remotePath}</span>{" "}
            {t("fileBrowser.uploadConflict.body")}
          </p>
          {uploadConflict.localPreview.text !== null && uploadConflict.remoteText !== null ? (
            <NoteDiffPreview
              previousContent={uploadConflict.remoteText}
              newContent={uploadConflict.localPreview.text}
            />
          ) : (
            <p className="border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-400">
              {uploadConflict.remoteReadFailure
                ? t(
                    previewReadFailureHint(uploadConflict.remoteReadFailure).key,
                    previewReadFailureHint(uploadConflict.remoteReadFailure).options,
                  )
                : t("fileBrowser.uploadConflict.noDiff", {
                    remoteSize: formatBytes(uploadConflict.remoteSize),
                    localSize: formatBytes(uploadConflict.localPreview.size),
                  })}
            </p>
          )}
        </DialogFrame>
      )}

      {folderConfirm && (
        <DialogFrame
          layer="sessionTab"
          title={t("fileBrowser.folderUpload.conflictTitle")}
          onClose={() => setFolderConfirm(null)}
          actions={
            <>
              <Button onClick={() => setFolderConfirm(null)}>{t("fileBrowser.cancel")}</Button>
              <Button variant="danger" onClick={handleConfirmFolderUpload}>
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
          <ul className="max-h-48 overflow-y-auto border border-slate-700 bg-slate-950 px-2 py-1.5 font-mono text-xs break-all text-slate-300">
            {folderConfirm.preview.overwrites.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </DialogFrame>
      )}

      {folderSummary && (
        <DialogFrame
          layer="sessionTab"
          title={t("fileBrowser.folderUpload.summaryTitle", { name: folderSummary.folderName })}
          onClose={() => setFolderSummary(null)}
          actions={
            <Button variant="primary" onClick={() => setFolderSummary(null)}>
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
            <p className="mb-3 text-amber-300">
              {t("fileBrowser.folderUpload.notAttempted", { count: folderSummary.notAttempted })}
            </p>
          )}
          <ul className="max-h-60 overflow-y-auto border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-300">
            {folderSummary.failed.map((f) => (
              <li key={"f:" + f.path} className="break-all">
                <span className="text-red-400">{t("fileBrowser.folderUpload.failed")}:</span>{" "}
                <span className="font-mono">{f.path}</span> — {f.error}
              </li>
            ))}
            {folderSummary.skipped.map((e) => (
              <li key={"s:" + e.path} className="break-all">
                <span className="text-amber-300">{t("fileBrowser.folderUpload.skipped")}:</span>{" "}
                <span className="font-mono">{e.path}</span> —{" "}
                {t("fileBrowser.folderUpload.reason." + e.reason)}
              </li>
            ))}
          </ul>
        </DialogFrame>
      )}

      {stopPrompt && (
        <DialogFrame
          layer="sessionTab"
          size="sm"
          title={t("fileElevation.stopTitle")}
          onClose={() => setStopPrompt(null)}
          actions={
            <>
              <Button onClick={() => setStopPrompt(null)}>{t("fileBrowser.cancel")}</Button>
              <Button variant="danger" onClick={confirmStopPrompt}>
                {t("fileElevation.toggleOff")}
              </Button>
            </>
          }
        >
          <p>{t("fileElevation.stopBody")}</p>
          <ul className="mt-2 space-y-1 text-xs">
            {stopPrompt.operations.map((op) => (
              <li key={op.id} className="break-all">
                <span className="font-semibold">
                  {t(
                    {
                      download: "fileElevation.opDownload",
                      upload: "fileElevation.opUpload",
                      delete: "fileElevation.opDelete",
                      chmod: "fileElevation.opChmod",
                    }[op.kind],
                  )}
                </span>{" "}
                <span className="font-mono">{op.path}</span>
                {op.bytesTotal !== null && op.bytesDone !== null && (
                  <span className="text-slate-400">
                    {" "}
                    ({formatBytes(op.bytesDone)} / {formatBytes(op.bytesTotal)}
                    {op.filesTotal !== null &&
                      op.filesDone !== null &&
                      ` — ${t("fileBrowser.transferFiles", { done: op.filesDone, total: op.filesTotal })}`}
                    )
                  </span>
                )}
              </li>
            ))}
          </ul>
        </DialogFrame>
      )}

      {elevationFailure && (
        <ElevationFailureDialog
          result={elevationFailure}
          onClose={() => setElevationFailure(null)}
        />
      )}
    </div>
  );
}

/** Focuses the ⋮ button of the entry with this path (no-op for the context menu). */
function findMenuTrigger(path: string): HTMLElement | null {
  for (const el of document.querySelectorAll<HTMLElement>("[data-menu-trigger]")) {
    if (el.dataset.menuTrigger === path) return el;
  }
  return null;
}

function focusMenuTrigger(path: string): void {
  findMenuTrigger(path)?.focus();
}

/** Spec 0054, Teil 1: die EINE Aktionsliste, die sowohl vom Drei-Punkte-
 * Menü als auch vom Kontextmenü (Rechtsklick) gerendert wird — "beide Wege
 * zeigen dieselben Aktionen". `className`/`style` bestimmen nur die
 * Positionierung, die Aktionsliste selbst ist davon unabhängig. Reihenfolge
 * folgt Spec 0054: lesend/lokal zuerst (Herunterladen, Kopieren,
 * Eigenschaften, Aktualisieren), dann — durch einen Trenner abgesetzt — die
 * server-verändernden Aktionen (Umbenennen, Löschen; chmod/Verschieben
 * folgen in Teil 3). */
function FileEntryMenu({
  entry,
  narrow,
  className,
  point,
  onOpenLocally,
  onDownloadDefault,
  onDownloadChoose,
  onCopyContent,
  onCopyFile,
  onCopyPath,
  onShowProperties,
  onRefresh,
  onRename,
  onCut,
  onChmod,
  onDelete,
}: {
  entry: RemoteEntryDto;
  /** Issue #584: render as a bottom sheet across the viewport width. */
  narrow: boolean;
  className: string;
  /** Context menu: opens at this viewport point; otherwise anchored to the ⋮ button. */
  point?: { x: number; y: number };
  onOpenLocally: (entry: RemoteEntryDto) => void;
  onDownloadDefault: (entry: RemoteEntryDto) => void;
  onDownloadChoose: (entry: RemoteEntryDto) => void;
  onCopyContent: (entry: RemoteEntryDto) => void;
  onCopyFile: (entry: RemoteEntryDto) => void;
  onCopyPath: (entry: RemoteEntryDto) => void;
  onShowProperties: (entry: RemoteEntryDto) => void;
  onRefresh: () => void;
  onRename: (entry: RemoteEntryDto) => void;
  onCut: (entry: RemoteEntryDto) => void;
  onChmod: (entry: RemoteEntryDto) => void;
  onDelete: (entry: RemoteEntryDto) => void;
}) {
  const { t } = useTranslation();
  // Issue #627: always fully inside the window (flips/shifts at the edges).
  const menuRef = useRef<HTMLDivElement>(null);
  const getAnchor = useCallback(
    () => point ?? findMenuTrigger(entry.path),
    [point?.x, point?.y, entry.path], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const style = useMenuPlacement(menuRef, getAnchor, true, { align: point ? "start" : "end", gap: point ? 0 : 4 });
  // The menu unmounts when an action runs. Focus the ⋮ button first, so a
  // dialog opened by the action remembers it and gives focus back on close.
  const act = (fn: () => void) => () => {
    focusMenuTrigger(entry.path);
    fn();
  };
  const itemClass = `block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-slate-200 hover:bg-indigo-600/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400`;
  return (
    <div
      ref={menuRef}
      className={
        narrow
          ? "fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-slate-700 bg-slate-900 py-1 text-left text-xs shadow-lg"
          : `${className} w-52 border border-slate-700 bg-slate-900 py-1 text-left text-xs shadow-lg`
      }
      style={narrow ? undefined : style}
    >
      <button type="button" onClick={act(() => onDownloadDefault(entry))} className={itemClass}>
        {t("fileBrowser.menu.download")}
      </button>
      <button type="button" onClick={act(() => onDownloadChoose(entry))} className={itemClass}>
        {t("fileBrowser.menu.downloadTo")}
      </button>
      {!entry.isDir && (
        <button type="button" onClick={act(() => onCopyContent(entry))} className={itemClass}>
          {t("fileBrowser.menu.copyContent")}
        </button>
      )}
      {!entry.isDir && (
        <button type="button" onClick={act(() => onCopyFile(entry))} className={itemClass}>
          {t("fileBrowser.menu.copyFile")}
        </button>
      )}
      {!entry.isDir && (
        <button type="button" onClick={act(() => onOpenLocally(entry))} className={itemClass}>
          {t("fileBrowser.menu.openLocally")}
        </button>
      )}
      <button type="button" onClick={act(() => onCopyPath(entry))} className={itemClass}>
        {t("fileBrowser.menu.copyPath")}
      </button>
      <button type="button" onClick={act(() => onShowProperties(entry))} className={itemClass}>
        {t("fileBrowser.menu.properties")}
      </button>
      <button type="button" onClick={act(onRefresh)} className={itemClass}>
        {t("fileBrowser.menu.refresh")}
      </button>
      <div className="my-1 border-t border-slate-800" />
      <button type="button" onClick={act(() => onChmod(entry))} className={itemClass}>
        {t("fileBrowser.menu.chmod")}
      </button>
      <button type="button" onClick={act(() => onRename(entry))} className={itemClass}>
        {t("fileBrowser.menu.rename")}
      </button>
      <button type="button" onClick={act(() => onCut(entry))} className={itemClass}>
        {t("fileBrowser.menu.cut")}
      </button>
      <button
        type="button"
        onClick={act(() => onDelete(entry))}
        className={`block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-red-400 hover:bg-red-600/12 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400`}
      >
        {t("fileBrowser.menu.delete")}
      </button>
    </div>
  );
}

/** Spec 0054, Teil 2: "Eigenschaften" — reine Anzeige der bereits geladenen
 * `RemoteEntryDto` (kein eigener Backend-Aufruf: `sftp_list` liefert
 * Rechte/Besitzer/Gruppe/Datum schon mit, ein erneutes `stat` wäre
 * redundant). Rechte numerisch (`permissionsOctal`, 3-stellig oktal) UND
 * symbolisch (`permissions`, bereits fertig formatiert) nebeneinander, wie
 * von der Spec verlangt. */
function FilePropertiesDialog({
  entry,
  onClose,
}: {
  entry: RemoteEntryDto;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation();
  const octal = entry.permissionsOctal.toString(8).padStart(4, "0");
  const owner = entry.owner ?? (entry.uid !== null ? String(entry.uid) : "—");
  const group = entry.group ?? (entry.gid !== null ? String(entry.gid) : "—");

  const row = (label: string, value: string) => (
    <div className="flex justify-between gap-4 py-1">
      <span className="shrink-0 text-slate-400">{label}</span>
      <span className="min-w-0 text-right font-mono text-slate-100 break-all">{value}</span>
    </div>
  );

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={t("fileBrowser.properties.title")}
      onClose={onClose}
      actions={<Button onClick={onClose}>{t("fileBrowser.properties.close")}</Button>}
    >
      <div className="divide-y divide-slate-800/70 text-sm">
        {row(t("fileBrowser.properties.name"), entry.name)}
        {row(t("fileBrowser.properties.path"), entry.path)}
        {row(
          t("fileBrowser.properties.type"),
          entry.isDir ? t("fileBrowser.properties.typeFolder") : t("fileBrowser.properties.typeFile"),
        )}
        {row(t("fileBrowser.properties.size"), entry.isDir ? "—" : formatBytes(entry.size))}
        {row(t("fileBrowser.properties.permissionsSymbolic"), entry.permissions)}
        {row(t("fileBrowser.properties.permissionsNumeric"), octal)}
        {row(t("fileBrowser.properties.owner"), owner)}
        {row(t("fileBrowser.properties.group"), group)}
        {row(
          t("fileBrowser.properties.modified"),
          entry.modified ? formatDateTime(entry.modified, i18n.language) : "—",
        )}
      </div>
    </DialogFrame>
  );
}

/** Reihen-Reihenfolge der chmod-Checkbox-Matrix: owner/group/other × je
 * einer Bit-Maske pro Spalte (r/w/x), spiegelt exakt `RemoteEntry::
 * permissions` (Spec 0020: "reine Unix-Rechte-Bits, `0o755`-Stil"). */
const CHMOD_ROWS: { key: "owner" | "group" | "other"; read: number; write: number; execute: number }[] = [
  { key: "owner", read: 0o400, write: 0o200, execute: 0o100 },
  { key: "group", read: 0o040, write: 0o020, execute: 0o010 },
  { key: "other", read: 0o004, write: 0o002, execute: 0o001 },
];

/** Spec 0054, Teil 3: chmod-Dialog — Checkbox-Matrix (owner/group/other ×
 * r/w/x) UND eine numerische Eingabe, bidirektional synchron über
 * gemeinsamen `mode`-State (jede Checkbox toggelt ein einzelnes Bit per
 * XOR, die numerische Eingabe parst die ganze dreistellige Oktalzahl neu —
 * beide schreiben in denselben State, es gibt keine zwei Quellen der
 * Wahrheit). "Optional rekursiv... mit deutlicher Kennzeichnung, weil
 * mächtig" (Spec) nur bei Ordnern sichtbar, mit auffälliger Warnfarbe. */
function ChmodDialog({
  entry,
  modeNote,
  onCancel,
  onConfirm,
}: {
  entry: RemoteEntryDto;
  /** Spec 0067, A5: im erhöhten Modus, z. B. „Die Rechte werden als root
   * gesetzt.“ */
  modeNote: string | null;
  onCancel: () => void;
  onConfirm: (mode: number, recursive: boolean, applySpecialBits: boolean) => void;
}) {
  const { t } = useTranslation();
  const [mode, setMode] = useState(entry.permissionsOctal);
  // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): mit nur 3
  // Ziffern (`maxLength={3}`, `/^[0-7]{1,3}$/`) ließ sich eine Datei mit
  // gesetztem setuid/setgid/sticky-Bit (`permissionsOctal` trägt `&
  // 0o7777`, also auch diese "vierte Ziffer") über die numerische Eingabe
  // nicht mehr KORREKT bearbeiten: eine erneute Eingabe von z. B. "755" auf
  // eine zuvor `4755` (setuid) gesetzte Datei löschte das setuid-Bit still
  // — die Checkbox-Matrix (togglet einzelne Bits per XOR) war davon nicht
  // betroffen, zwei inkonsistente Pfade zum selben `mode`. Durchgehend
  // 4-stellig (führende Null bei "gewöhnlichen" Rechten, z. B. "0644")
  // macht die vierte Ziffer immer sichtbar und bearbeitbar, statt sie
  // implizit wegzulassen.
  const [numericInput, setNumericInput] = useState(mode.toString(8).padStart(4, "0"));
  const [recursive, setRecursive] = useState(false);

  const applyMode = (next: number) => {
    setMode(next);
    setNumericInput(next.toString(8).padStart(4, "0"));
  };

  const toggleBit = (bit: number) => applyMode(mode ^ bit);

  // Issue #624: a recursive apply only touches folders' special bits when
  // the user changed them here.
  const specialChanged = (mode & 0o7000) !== (entry.permissionsOctal & 0o7000);
  const fileBits = mode & 0o777;
  const folderBits = fileBits | ((fileBits & 0o444) >> 2);

  const handleNumericChange = (value: string) => {
    setNumericInput(value);
    if (/^[0-7]{1,4}$/.test(value)) {
      setMode(Number.parseInt(value, 8));
    }
  };

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={t("fileBrowser.chmod.title")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        onConfirm(mode, recursive, specialChanged);
      }}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button type="submit" variant="primary">
            {t("fileBrowser.apply")}
          </Button>
        </>
      }
    >
      <p className="mb-3 font-mono text-xs break-all text-slate-400">{entry.path}</p>
      {modeNote && (
        <p className="mb-3 border border-amber-600/60 bg-amber-950/40 px-2 py-1 text-xs text-amber-300">
          ⚠ {modeNote}
        </p>
      )}

      <table className="mb-3 w-full text-sm text-slate-300">
        <thead>
          <tr className="text-slate-400">
            <th className="text-left font-normal">
              <span className="sr-only">{t("fileBrowser.chmodClassColumn")}</span>
            </th>
            <th className="font-normal">{t("fileBrowser.chmod.read")}</th>
            <th className="font-normal">{t("fileBrowser.chmod.write")}</th>
            <th className="font-normal">{t("fileBrowser.chmod.execute")}</th>
          </tr>
        </thead>
        <tbody>
          {CHMOD_ROWS.map((row) => (
            <tr key={row.key}>
              <td>{t(`fileBrowser.chmod.${row.key}`)}</td>
              {(
                [
                  [row.read, "read"],
                  [row.write, "write"],
                  [row.execute, "execute"],
                ] as const
              ).map(([bit, key]) => (
                <td key={bit} className="text-center">
                  <input
                    type="checkbox"
                    checked={(mode & bit) !== 0}
                    onChange={() => toggleBit(bit)}
                    aria-label={`${t(`fileBrowser.chmod.${row.key}`)}: ${t(`fileBrowser.chmod.${key}`)}`}
                    className="focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                  />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>

      <fieldset className="mb-3 text-sm text-slate-300">
        <legend className="sr-only">{t("fileBrowser.chmod.special")}</legend>
        <div className="flex flex-wrap gap-x-4 gap-y-1">
          {(
            [
              [0o4000, "setuid"],
              [0o2000, "setgid"],
              [0o1000, "sticky"],
            ] as const
          ).map(([bit, key]) => (
            <label key={key} className="flex items-center gap-1">
              <input
                type="checkbox"
                checked={(mode & bit) !== 0}
                onChange={() => toggleBit(bit)}
                className="focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
              />
              {t(`fileBrowser.chmod.${key}`)}
            </label>
          ))}
        </div>
        <p className="mt-1 text-xs text-slate-400">{t("fileBrowser.chmod.specialHint")}</p>
      </fieldset>

      <Field label={t("fileBrowser.chmod.numeric")} className="mb-3">
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            value={numericInput}
            onChange={(e) => handleNumericChange(e.target.value)}
            maxLength={4}
            className={`${FIELD_CONTROL_CLASS} w-24 font-mono`}
          />
        )}
      </Field>

      {entry.isDir && (
        <label className="flex items-start gap-2 text-sm text-amber-300">
          <input
            type="checkbox"
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            className="mt-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
          />
          <span>
            <strong>{t("fileBrowser.chmod.recursive")}</strong>{" "}
            {t("fileBrowser.chmod.recursiveWarning")}
          </span>
        </label>
      )}
      {entry.isDir && recursive && (
        <p className="mt-2 text-xs text-slate-300">
          {t("fileBrowser.chmod.recursiveSummary", {
            folders: folderBits.toString(8).padStart(3, "0"),
            files: fileBits.toString(8).padStart(3, "0"),
          })}
        </p>
      )}
    </DialogFrame>
  );
}

/** Gemeinsame kleine Eingabeaufforderung für "Umbenennen" und "Neuer
 * Ordner" — beide brauchen nur einen einzelnen Namen mit Bestätigen/
 * Abbrechen, ein voller Modal-Dialog (wie beim Löschen) wäre hier
 * überdimensioniert. */
function RenamePrompt({
  title,
  initialValue,
  error,
  onChange,
  onCancel,
  onConfirm,
}: {
  title?: string;
  initialValue: string;
  /** Issue #570: Hinweis zu einem ungültigen Namen (ohne Backend-Aufruf). */
  error?: string | null;
  onChange: (value: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={title ?? t("fileBrowser.renamePrompt.renameTitle")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        onConfirm();
      }}
      initialFocusRef={inputRef}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button type="submit" variant="primary">
            {t("fileBrowser.apply")}
          </Button>
        </>
      }
    >
      <Field label={t("fileBrowser.properties.name")}>
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            ref={inputRef}
            value={initialValue}
            onChange={(e) => onChange(e.target.value)}
            className={`${FIELD_CONTROL_CLASS} w-full font-mono`}
          />
        )}
      </Field>
      {error && (
        <p role="alert" className="mt-2 text-xs text-red-400">
          {error}
        </p>
      )}
    </DialogFrame>
  );
}

/** Issue #570: Namen für neue Einträge — nicht leer, kein `/`, nicht `.`
 * oder `..` (wie bei Umbenennen). */
function isValidEntryName(name: string): boolean {
  return name !== "" && !name.includes("/") && name !== "." && name !== "..";
}

/** Issue #570: Kontextmenü auf leerer Fläche des Dateibrowsers — bewusst
 * eine eigene kleine Komponente, damit `FileEntryMenu` nur Eintrags-
 * Aktionen behält. Schließen per Klick außerhalb/Escape macht der Aufrufer. */
function DirectoryMenu({
  narrow,
  x,
  y,
  pasteName,
  clipboardFiles,
  onNewFolder,
  onNewFile,
  onRefresh,
  onPaste,
}: {
  narrow: boolean;
  x: number;
  y: number;
  pasteName: string | null;
  clipboardFiles: boolean;
  onNewFolder: () => void;
  onNewFile: () => void;
  onRefresh: () => void;
  onPaste: () => void;
}) {
  const { t } = useTranslation();
  const firstRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    firstRef.current?.focus();
  }, []);
  // Issue #627: always fully inside the window (flips/shifts at the edges).
  const menuRef = useRef<HTMLFieldSetElement>(null);
  const getAnchor = useCallback(() => ({ x, y }), [x, y]);
  const placed = useMenuPlacement(menuRef, getAnchor, true);
  const itemClass = `block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-slate-200 hover:bg-indigo-600/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-400`;
  return (
    <fieldset
      ref={menuRef}
      className={
        narrow
          ? "fixed inset-x-0 bottom-0 z-30 m-0 min-w-0 rounded-t border border-slate-700 bg-slate-900 px-0 py-1 text-left text-xs shadow-lg"
          : "fixed z-20 m-0 w-52 min-w-0 border border-slate-700 bg-slate-900 px-0 py-1 text-left text-xs shadow-lg"
      }
      style={narrow ? undefined : placed}
    >
      <legend className="sr-only">{t("fileBrowser.directoryMenuLabel")}</legend>
      <button ref={firstRef} type="button" onClick={onNewFolder} className={itemClass}>
        {t("fileBrowser.menu.newFolder")}
      </button>
      <button type="button" onClick={onNewFile} className={itemClass}>
        {t("fileBrowser.menu.newFile")}
      </button>
      <button type="button" onClick={onRefresh} className={itemClass}>
        {t("fileBrowser.menu.refresh")}
      </button>
      {pasteName !== null && (
        <button
          type="button"
          onClick={onPaste}
          title={t("fileBrowser.pasteTitle", { name: pasteName })}
          className={itemClass}
        >
          {t("fileBrowser.paste")}
        </button>
      )}
      {pasteName === null && clipboardFiles && (
        <button
          type="button"
          onClick={onPaste}
          title={t("fileBrowser.clipboardPaste.menuTitle")}
          className={itemClass}
        >
          {t("fileBrowser.paste")}
        </button>
      )}
    </fieldset>
  );
}

/** Spec 0053, Teil 1: Drag-Handle am rechten Rand eines Spaltenkopfs — die
 * Trefferfläche (`w-2`, per `-right-1` über die Spaltengrenze zentriert)
 * ist bewusst breiter als die sichtbare Trennlinie (`w-px`), damit sie
 * auch ohne Pixel-genaues Zielen zu treffen ist. `cursor-col-resize` /
 * `touch-none` (kein Scroll-Gestenkonflikt beim Ziehen) als der von der
 * Spec verlangte Hover-/Cursor-Hinweis.
 *
 * Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): kein
 * `role="separator"`/`aria-label` mehr — ein per Tastatur nicht
 * bedienbares, rein visuelles Maus-Element (die Spec verlangt
 * Tastaturbedienbarkeit ausdrücklich nicht) würde Screenreadern beim
 * Durchlaufen der Tabellenkopfzeile als benanntes, aber funktionsloses
 * Element angesagt — irreführend. `aria-hidden` ist hier die ehrlichere
 * Wahl; `data-testid` bleibt als reiner Test-Anker. */
function ColumnResizeHandle({
  testId,
  onDrag,
  onDragEnd,
}: {
  testId: string;
  onDrag: (deltaX: number) => void;
  onDragEnd: () => void;
}) {
  const handlers = useDragResize(onDrag, onDragEnd);
  return (
    <span
      aria-hidden="true"
      data-testid={testId}
      className="group absolute -right-1 top-0 bottom-0 z-10 flex w-2 cursor-col-resize touch-none select-none justify-center"
      {...handlers}
    >
      <span className="h-full w-px bg-slate-700 group-hover:bg-indigo-500" />
    </span>
  );
}

/** Spec 0067, A3: verständliche Erklärung, warum der erhöhte Modus nicht
 * geht — bei fehlender sudo-Regel mit der zugeschnittenen sudoers-Zeile zum
 * Kopieren und dem ehrlichen Hinweis, was diese Regel bedeutet. */
function ElevationFailureDialog({
  result,
  onClose,
}: {
  result: ElevationResultDto;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const failure = result.failure;
  if (!failure) return null;

  const copy = async () => {
    if (!failure.sudoersLine) return;
    try {
      await clipboardWriteText(failure.sudoersLine);
      setCopied(true);
    } catch (err) {
      console.warn("Konnte sudoers-Zeile nicht kopieren:", err);
    }
  };

  return (
    <DialogFrame
      layer="sessionTab"
      title={t("fileElevation.failureTitle")}
      onClose={onClose}
      size="md"
      actions={<Button onClick={onClose}>{t("fileElevation.close")}</Button>}
    >
      <div className="space-y-3">
        <p>{t(`fileElevation.failure.${failure.kind}`, { path: failure.detail ?? "" })}</p>
        {failure.sudoersLine && (
          <>
            <p>{t("fileElevation.sudoersIntro")}</p>
            <div className="flex items-center gap-2">
              <code className="min-w-0 flex-1 select-text break-all rounded border border-slate-600 bg-slate-950 px-2 py-1.5 font-mono text-xs text-slate-100">
                {failure.sudoersLine}
              </code>
              <Button size="sm" onClick={copy} className="shrink-0">
                {copied ? t("fileElevation.copied") : t("fileElevation.copy")}
              </Button>
            </div>
            <p className="text-xs text-slate-400">{t("fileElevation.sudoersHowTo")}</p>
            <p className="border border-red-700/50 bg-red-950/40 px-2 py-1.5 text-xs text-red-300">
              {t("fileElevation.sudoersWarning", { user: result.targetUser })}
            </p>
          </>
        )}
        {failure.detail && failure.kind !== "sftpServerUnsafe" && (
          <p className="font-mono text-xs break-all text-slate-400">
            {t("fileElevation.detail", { detail: failure.detail })}
          </p>
        )}
      </div>
    </DialogFrame>
  );
}
