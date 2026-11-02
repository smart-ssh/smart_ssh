import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Trans, useTranslation } from "react-i18next";
import {
  claimClipboardFiles,
  clipboardHasFiles,
  clipboardWriteText,
  commandErrorCode,
  commandErrorMessage,
  sftpChmod,
  sftpChown,
  sftpDelete,
  sftpDeletePreview,
  sftpDownload,
  sftpDownloadDefault,
  sftpDownloadDir,
  sftpElevationStatus,
  sftpExists,
  sftpStat,
  sftpMkdir,
  sftpCreateFile,
  sftpCancelCopyContent,
  sftpCopyContent,
  sftpCopyFile,
  sftpRename,
  sessionIsRoot,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import {
  onConnectionStatusChanged,
} from "../events";
import { formatBytes } from "../format";
import { sortEntries } from "../fileSort";
import { displayPath, joinPath, localBaseName, parentPath } from "../remotePath";
import { showToast } from "../toastBus";
import type {
  DeletePreviewDto,
  DownloadResultDto,
  RemoteEntryDto,
} from "../types";
import { useLocalEditSession, type UploadOffer } from "../useLocalEditSession";
import {
  previewReadFailureHint,
} from "../previewReadFailure";
import { NoteDiffPreview } from "./NoteDiffPreview";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { useMenuPlacement } from "../hooks/useMenuPlacement";
import { Button, DialogFrame, FIELD_CONTROL_CLASS } from "./ui";
import { LocalEditCopyEditor } from "./LocalEditCopyEditor";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { FileEntryMenu } from "./fileBrowser/FileEntryMenu";
import { FilePropertiesDialog } from "./fileBrowser/FilePropertiesDialog";
import { ColumnsMenu } from "./fileBrowser/ColumnsMenu";
import { ChmodDialog } from "./fileBrowser/ChmodDialog";
import { ChownDialog } from "./fileBrowser/ChownDialog";
import { RenamePrompt } from "./fileBrowser/RenamePrompt";
import { DirectoryMenu } from "./fileBrowser/DirectoryMenu";
import { ElevationFailureDialog } from "./fileBrowser/ElevationFailureDialog";
import { FolderSummaryDialog } from "./fileBrowser/FolderSummaryDialog";
import { LocalEditStatusBar } from "./fileBrowser/LocalEditStatusBar";
import { NarrowEntryList } from "./fileBrowser/NarrowEntryList";
import { StopPromptDialog } from "./fileBrowser/StopPromptDialog";
import { UploadConflictDialog } from "./fileBrowser/UploadConflictDialog";
import { FileTable } from "./fileBrowser/FileTable";
import { FileToolbar } from "./fileBrowser/FileToolbar";
import { ElevationControls } from "./fileBrowser/ElevationControls";
import { useFileColumns } from "./fileBrowser/useFileColumns";
import { useRemoteDirectory } from "./fileBrowser/useRemoteDirectory";
import { useElevation } from "./fileBrowser/useElevation";
import { useSftpTransfers } from "./fileBrowser/useSftpTransfers";
import { TransferList } from "./fileBrowser/TransferList";
import {
  currentCaps,
  focusMenuTrigger,
  formatMb,
  isValidEntryName,
  type MoveKind,
} from "./fileBrowser/shared";

/** Issue #357: ab dieser Größe zeigt "Dateiinhalt kopieren" Fortschritt. */
const COPY_PROGRESS_FROM_BYTES = 5 * 1024 * 1024;

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


export function FileBrowserPanel({
  sessionId,
  isVisible,
  onElevationChange,
}: FileBrowserPanelProps) {
  const caps = currentCaps();
  const {
    fileSort,
    hiddenColumns,
    columnWidths,
    columnsMenu,
    setColumnsMenu,
    tableContainerRef,
    toggleColumn,
    handleColumnDrag,
    handleColumnDragEnd,
    handleSortClick,
  } = useFileColumns();
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
  const [chownTarget, setChownTarget] = useState<RemoteEntryDto | null>(null);
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
  const panelRef = useRef<HTMLDivElement>(null);

  // Spec 0054, Teil 4: "Lokal öffnen -> bearbeiten -> Upload anbieten".
  const localEdit = useLocalEditSession(sessionId);
  const { t } = useTranslation();

  // Reloads the directory over the given channel user; `load` only exists
  // after `useRemoteDirectory`, which needs the channel user of this hook.
  const reloadRef = useRef<(user: string | null) => void>(() => {});
  const {
    elevation,
    elevated,
    elevatedUser,
    channelUser,
    enablingElevation,
    elevationTargetUser,
    setElevationTargetUser,
    elevationMenuOpen,
    setElevationMenuOpen,
    otherUserOpen,
    setOtherUserOpen,
    otherUserDraft,
    setOtherUserDraft,
    elevationFailure,
    setElevationFailure,
    elevationWarning,
    stopPrompt,
    setStopPrompt,
    updateElevation,
    handleToggleElevation,
    confirmStopPrompt,
  } = useElevation(sessionId, {
    onElevationChange,
    reload: (user) => reloadRef.current(user),
  });
  // Issue #544: `id -u` probe result of the session. `undefined` = not yet
  // answered; `null`/failure = unknown (controls shown); `true` = root (hidden).
  const [sessionRoot, setSessionRoot] = useState<boolean | null | undefined>(undefined);
  // Issue #584, Spec 0113: below 640 px the toolbar collapses into an
  // overflow menu and the list into two-line rows; functions stay the same.
  const narrow = useIsNarrow();
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const overflowButtonRef = useRef<HTMLButtonElement>(null);
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
  }, [elevationMenuOpen, otherUserOpen, setElevationMenuOpen, setOtherUserOpen]);
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
  }, [elevationMenuOpen, otherUserOpen, setElevationMenuOpen, setOtherUserOpen]);
  // Hiding the panel or switching the elevation closes both.
  useEffect(() => {
    setElevationMenuOpen(false);
    setOtherUserOpen(false);
    setOverflowOpen(false);
  }, [isVisible, elevation, setElevationMenuOpen, setOtherUserOpen]);
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
  const {
    path,
    pathRef,
    pathInput,
    setPathInput,
    startPath,
    entries: rawEntries,
    loading,
    error,
    setError,
    knownDirModes,
    load,
    reloadAfterReconnect,
  } = useRemoteDirectory(sessionId, channelUser);
  reloadRef.current = (user) => load(path, user);
  const entries = useMemo(() => sortEntries(rawEntries, fileSort), [rawEntries, fileSort]);
  const connectionLostRef = useRef(false);

  // Verbindung weg → erhöhter Kanal ist weg (lebt nur in der Session).
  // Issue #355: nach einem Reconnect das bisherige Verzeichnis neu laden
  // (Home, falls es nicht mehr existiert). Der erhöhte Kanal kommt nicht
  // zurück, daher immer im normalen Modus.
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

  const modeSuffix = elevated ? t("fileToasts.asElevated", { user: elevatedUser }) : "";

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

  const {
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
  } = useSftpTransfers(sessionId, {
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
  });

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
  const [editUploadOffer, setEditUploadOffer] = useState<UploadOffer | null>(null);


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
  // Issue #679: the app's own editor on the edit copy (no external program).
  const [inAppEditorOpen, setInAppEditorOpen] = useState(false);
  const handleOpenLocally = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    localEdit
      .startEditing(entry, elevated ? elevatedUser : null)
      .then(() => {
        if (currentCaps().externalEditor) notifyOk("fileToasts.openedLocally", { name: entry.name });
        else setInAppEditorOpen(true);
      })
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

  const handleChown = (entry: RemoteEntryDto) => {
    setOpenMenu(null);
    setChownTarget(entry);
  };

  // Issue #654: a side the user did not touch arrives as `null` (unchanged).
  const handleConfirmChown = (owner: string | null, group: string | null, recursive: boolean) => {
    if (!chownTarget) return;
    const target = chownTarget;
    setChownTarget(null);
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
            message: t(
              code === "CHOWN_UNKNOWN_USER" ? "fileToasts.chownUnknownUser" : "fileToasts.chownUnknownGroup",
              { name: target.name },
            ) + modeSuffix,
          });
          return;
        }
        notifyFailed("fileToasts.chownFailed", target.name, err);
      });
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
      onChown={handleChown}
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
                className="fixed z-20 flex w-64 flex-col gap-1.5 rounded border border-border-strong bg-surface-raised p-2 shadow-lg narrow:fixed narrow:inset-x-0 narrow:bottom-0 narrow:left-0 narrow:top-auto narrow:mt-0 narrow:w-full narrow:rounded-b-none narrow:p-3"
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
                <p id="file-elevation-target-user-help" className="m-0 text-xs text-fg-muted">
                  {t("fileElevation.targetUserHelp")}
                </p>
                <Button size={narrow ? "md" : "sm"} type="submit" disabled={enablingElevation || !otherUserDraft.trim()}>
                  {t("fileElevation.confirm")}
                </Button>
              </form>
  );

  const canElevate = !elevated && sessionRoot !== undefined && sessionRoot !== true;

  return (
    <div
      ref={panelRef}
      className={`flex h-full flex-col border-2 ${elevated ? "border-warning-strong" : "border-transparent"}`}
      data-elevated={elevated ? "true" : "false"}
    >
      {elevated && (
        <div
          role="alert"
          className="flex items-center gap-2 bg-warning px-2 py-1.5 text-xs font-semibold text-surface-sunken"
        >
          <span className="flex-1">⚠ {t("fileElevation.banner", { user: elevatedUser })}</span>
          <button
            type="button"
            onClick={() => handleToggleElevation()}
            className="rounded border border-surface-sunken/40 px-2 py-0.5 hover:bg-warning-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-surface-sunken"
          >
            {t("fileElevation.toggleOff")}
          </button>
        </div>
      )}
      {elevated && elevationWarning && (
        <div role="alert" className="bg-danger-subtle px-2 py-1.5 text-xs text-danger-fg-softer">
          {t("fileElevation.failure.sftpServerUnsafe", { path: elevationWarning })}
        </div>
      )}
      <FileToolbar
        narrow={narrow}
        path={path}
        startPath={startPath}
        pathInput={pathInput}
        setPathInput={setPathInput}
        load={load}
        overflowRef={overflowRef}
        overflowButtonRef={overflowButtonRef}
        overflowOpen={overflowOpen}
        setOverflowOpen={setOverflowOpen}
        setElevationMenuOpen={setElevationMenuOpen}
        setOtherUserOpen={setOtherUserOpen}
        setOtherUserDraft={setOtherUserDraft}
        otherUserInputRef={otherUserInputRef}
        elevationTargetUser={elevationTargetUser}
        otherUserForm={otherUserForm}
        elevationControls={
          canElevate ? (
            <ElevationControls
              groupRef={elevationGroupRef}
              menuButtonRef={elevationMenuButtonRef}
              menuRef={elevationMenuRef}
              otherUserItemRef={otherUserItemRef}
              otherUserInputRef={otherUserInputRef}
              menuStyle={elevationMenuStyle}
              enablingElevation={enablingElevation}
              elevationMenuOpen={elevationMenuOpen}
              otherUserOpen={otherUserOpen}
              elevationTargetUser={elevationTargetUser}
              otherUserForm={otherUserForm}
              onToggleElevation={handleToggleElevation}
              setElevationMenuOpen={setElevationMenuOpen}
              setOtherUserOpen={setOtherUserOpen}
              setOtherUserDraft={setOtherUserDraft}
            />
          ) : null
        }
        canElevate={canElevate}
        enablingElevation={enablingElevation}
        onToggleElevation={handleToggleElevation}
        folderPickerAvailable={caps.folderPicker}
        onNewFolder={() => {
          setNameError(null);
          setMkdirOpen("");
        }}
        onUpload={handleUploadButton}
        onUploadFolder={handleUploadFolderButton}
        cutEntry={cutEntry}
        onPaste={handlePaste}
        onCancelCut={() => setCutEntry(null)}
      />

      {transfers.length > 0 && (
        <TransferList
          transfers={transfers}
          sessionId={sessionId}
          onDismiss={dismissTransfer}
        />
      )}

      {copyJob && Math.max(copyJob.done, copyJob.total ?? 0) >= COPY_PROGRESS_FROM_BYTES && (
        <output
          className="flex items-center gap-2 border-b border-border px-2 py-1.5 text-xs text-fg-soft"
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
        className={`relative min-h-0 flex-1 overflow-auto ${dragOver ? "bg-accent-subtle/40" : ""}`}
        // Issue #570: Rechtsklick auf leere Fläche (unter den Zeilen oder in
        // einem leeren Verzeichnis). Die Zeilen stoppen das Ereignis selbst.
        onContextMenu={(e) => {
          e.preventDefault();
          setOpenMenu(null);
          setClipboardFiles(false);
          setDirMenu({ x: e.clientX, y: e.clientY });
          if (!cutEntry && caps.fileClipboard) {
            clipboardHasFiles()
              .then(setClipboardFiles)
              .catch(() => setClipboardFiles(false));
          }
        }}
      >
        {loading && <p className="p-3 text-xs text-fg-subtle">{t("fileBrowser.loading")}</p>}
        {error && <p className="p-3 text-xs text-danger-fg">{error}</p>}
        {!loading && !error && entries.length === 0 && (
          <p className="p-3 text-xs text-fg-subtle">
            {(knownDirModes.current.get(path) ?? 0o111) & 0o111
              ? t("fileBrowser.emptyDirectory")
              : t("fileBrowser.noPermissionToOpen")}
          </p>
        )}
        {!loading && !error && narrow && entries.length > 0 && (
          <NarrowEntryList
            entries={entries}
            openMenu={openMenu}
            setOpenMenu={setOpenMenu}
            setDirMenu={setDirMenu}
            load={load}
            renderEntryMenu={renderEntryMenu}
          />
        )}
        {!loading && !error && !narrow && entries.length > 0 && (
          <FileTable
            entries={entries}
            fileSort={fileSort}
            hiddenColumns={hiddenColumns}
            columnWidths={columnWidths}
            openMenu={openMenu}
            setOpenMenu={setOpenMenu}
            setDirMenu={setDirMenu}
            setColumnsMenu={setColumnsMenu}
            load={load}
            onSort={handleSortClick}
            onColumnDrag={handleColumnDrag}
            onColumnDragEnd={handleColumnDragEnd}
            renderEntryMenu={renderEntryMenu}
          />
        )}
        {columnsMenu && (
          <ColumnsMenu
            point={columnsMenu}
            hidden={hiddenColumns}
            onToggle={toggleColumn}
            onClose={() => setColumnsMenu(null)}
          />
        )}
        {dragOver && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center border-2 border-dashed border-accent-strong bg-accent-subtle/30 text-sm text-accent-fg-softer">
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
      <LocalEditStatusBar
        localEdit={localEdit}
        editIdentityMismatch={editIdentityMismatch}
        channelUser={channelUser}
        externalEditor={caps.externalEditor}
        onOfferUpload={handleOfferEditUpload}
        onOpenInAppEditor={() => setInAppEditorOpen(true)}
      />

      {inAppEditorOpen && localEdit.session && !caps.externalEditor && (
        <LocalEditCopyEditor
          sessionId={sessionId}
          localPath={localEdit.session.localPath}
          fileName={localEdit.session.entry.name}
          onClose={() => setInAppEditorOpen(false)}
        />
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
            <p className="mb-3 border border-danger-border-strong/50 bg-danger-subtle/40 px-2 py-1.5 text-xs text-danger-fg-soft">
              {t("fileBrowser.localEdit.remoteChanged")}
            </p>
          )}
          {editUploadOffer.localText !== null && editUploadOffer.remoteText !== null ? (
            <NoteDiffPreview
              previousContent={editUploadOffer.remoteText}
              newContent={editUploadOffer.localText}
            />
          ) : (
            <p className="border border-border bg-surface-sunken px-2 py-1.5 text-xs text-fg-subtle">
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

      {chownTarget && (
        <ChownDialog
          entry={chownTarget}
          modeNote={elevated ? t("fileElevation.chownNote", { user: elevatedUser }) : null}
          onCancel={() => setChownTarget(null)}
          onConfirm={handleConfirmChown}
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
        <UploadConflictDialog
          conflict={uploadConflict}
          elevated={elevated}
          elevatedUser={elevatedUser}
          onCancel={() => setUploadConflict(null)}
          onConfirm={handleConfirmUpload}
        />
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
          <ul className="max-h-48 overflow-y-auto border border-border bg-surface-sunken px-2 py-1.5 font-mono text-xs break-all text-fg-muted">
            {folderConfirm.preview.overwrites.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </DialogFrame>
      )}

      {folderSummary && <FolderSummaryDialog folderSummary={folderSummary} onClose={() => setFolderSummary(null)} />}

      {stopPrompt && (
        <StopPromptDialog
          operations={stopPrompt.operations}
          onCancel={() => setStopPrompt(null)}
          onConfirm={confirmStopPrompt}
        />
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
