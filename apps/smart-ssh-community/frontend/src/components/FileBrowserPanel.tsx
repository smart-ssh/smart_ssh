import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { sortEntries } from "../lib/fileSort";
import { displayPath } from "../lib/remotePath";
import type { RemoteEntryDto } from "../api/types";
import { FileEntryMenu } from "./fileBrowser/FileEntryMenu";
import { ColumnsMenu } from "./fileBrowser/ColumnsMenu";
import { DirectoryMenu } from "./fileBrowser/DirectoryMenu";
import { NarrowEntryList } from "./fileBrowser/NarrowEntryList";
import { FileTable } from "./fileBrowser/FileTable";
import { FileToolbar } from "./fileBrowser/FileToolbar";
import { ElevationControls } from "./fileBrowser/ElevationControls";
import { ElevationBanner } from "./fileBrowser/ElevationBanner";
import { CopyProgress } from "./fileBrowser/CopyProgress";
import { OtherUserForm } from "./fileBrowser/OtherUserForm";
import { useFileColumns } from "./fileBrowser/useFileColumns";
import { useRemoteDirectory } from "./fileBrowser/useRemoteDirectory";
import { useElevation } from "./fileBrowser/useElevation";
import { useSftpTransfers } from "./fileBrowser/useSftpTransfers";
import { useFileNotify } from "./fileBrowser/useFileNotify";
import { useFileOperations } from "./fileBrowser/useFileOperations";
import { COPY_PROGRESS_FROM_BYTES, useFileClipboard } from "./fileBrowser/useFileClipboard";
import { useFileDownloads } from "./fileBrowser/useFileDownloads";
import { useLocalEditFlow } from "./fileBrowser/useLocalEditFlow";
import { useConnectionLoss } from "./fileBrowser/useConnectionLoss";
import { FileBrowserDialogs } from "./fileBrowser/FileBrowserDialogs";
import { useToolbarMenus } from "./fileBrowser/useToolbarMenus";
import { TransferList } from "./fileBrowser/TransferList";
import { currentCaps } from "./fileBrowser/shared";

interface FileBrowserPanelProps {
  sessionId: string;
  /** Issue #1195: the server of this session, for the one-time elevation notice. */
  serverId?: string;
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

export function FileBrowserPanel({ sessionId, serverId, isVisible, onElevationChange }: FileBrowserPanelProps) {
  const caps = currentCaps();
  const { t } = useTranslation();
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
  const [dirMenu, setDirMenu] = useState<{ x: number; y: number } | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeMenu = () => setOpenMenu(null);

  // Reloads the directory over the given channel user; `load` only exists
  // after `useRemoteDirectory`, which needs the channel user of this hook.
  const reloadRef = useRef<(user: string | null) => void>(() => {});
  const elev = useElevation(sessionId, {
    onElevationChange,
    reload: (user) => reloadRef.current(user),
  });
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
    elevationWarning,
    serverLogging,
    passwordless,
    updateElevation,
    handleToggleElevation,
  } = elev;
  const menus = useToolbarMenus(sessionId, {
    isVisible,
    elevation,
    elevationMenuOpen,
    otherUserOpen,
    setElevationMenuOpen,
    setOtherUserOpen,
  });
  const { narrow } = menus;
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

  useConnectionLoss(sessionId, updateElevation, reloadAfterReconnect);


  const { modeSuffix, notifyOk, notifyFailed } = useFileNotify(sessionId, elevated, elevatedUser, updateElevation);

  const xfer = useSftpTransfers(sessionId, {
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
  const { transfers, dismissTransfer, dragOver, editUploadingNameRef, uploadMany } = xfer;

  const ops = useFileOperations(sessionId, {
    channelUser,
    path,
    load,
    notifyOk,
    notifyFailed,
    modeSuffix,
    setError,
    closeMenu,
  });
  const { cutEntry } = ops;
  const clipboard = useFileClipboard(sessionId, {
    channelUser,
    path,
    notifyOk,
    notifyFailed,
    modeSuffix,
    closeMenu,
    uploadMany,
  });
  const { handleDownloadDefault, handleDownloadChoose } = useFileDownloads(
    sessionId,
    channelUser,
    notifyOk,
    notifyFailed,
    closeMenu,
  );
  const edit = useLocalEditFlow(sessionId, {
    elevated,
    elevatedUser,
    channelUser,
    path,
    load,
    setError,
    notifyOk,
    notifyFailed,
    closeMenu,
    editUploadingNameRef,
  });


  const handleRefresh = () => {
    setOpenMenu(null);
    load(path);
  };

  const renderEntryMenu = (entry: RemoteEntryDto, className: string, point?: { x: number; y: number }) => (
    <FileEntryMenu
      entry={entry}
      narrow={narrow}
      className={className}
      point={point}
      onClose={closeMenu}
      onOpenLocally={edit.handleOpenLocally}
      onDownloadDefault={handleDownloadDefault}
      onDownloadChoose={handleDownloadChoose}
      onCopyContent={clipboard.handleCopyContent}
      onCopyFile={clipboard.handleCopyFile}
      onCopyPath={clipboard.handleCopyPath}
      onShowProperties={ops.handleShowProperties}
      onRefresh={handleRefresh}
      onRename={ops.handleRename}
      onCut={ops.handleCut}
      onChmod={ops.handleChmod}
      onChown={ops.handleChown}
      onDelete={ops.handleDelete}
    />
  );

  const otherUserForm = otherUserOpen && (
    <OtherUserForm
      formRef={menus.otherUserFormRef}
      inputRef={menus.otherUserInputRef}
      style={menus.otherUserFormStyle}
      narrow={narrow}
      draft={otherUserDraft}
      setDraft={setOtherUserDraft}
      enablingElevation={enablingElevation}
      onSubmit={(user) => {
        setElevationTargetUser(user);
        setOtherUserOpen(false);
        handleToggleElevation(user);
      }}
    />
  );

  const canElevate =
    !elevated &&
    menus.elevationAllowed === true &&
    menus.sessionRoot !== undefined &&
    menus.sessionRoot !== true;

  return (
    <div
      ref={panelRef}
      className={`flex h-full flex-col border-2 ${elevated ? "border-warning-strong" : "border-transparent"}`}
      data-elevated={elevated ? "true" : "false"}
    >
      {elevated && (
        <ElevationBanner
          elevatedUser={elevatedUser}
          warningPath={elevationWarning}
          serverLogging={serverLogging}
          serverId={serverId}
          passwordless={passwordless}
          onToggleOff={() => handleToggleElevation()}
        />
      )}
      <FileToolbar
        narrow={narrow}
        path={path}
        startPath={startPath}
        pathInput={pathInput}
        setPathInput={setPathInput}
        load={load}
        overflowRef={menus.overflowRef}
        overflowButtonRef={menus.overflowButtonRef}
        overflowOpen={menus.overflowOpen}
        setOverflowOpen={menus.setOverflowOpen}
        setElevationMenuOpen={setElevationMenuOpen}
        setOtherUserOpen={setOtherUserOpen}
        setOtherUserDraft={setOtherUserDraft}
        otherUserInputRef={menus.otherUserInputRef}
        elevationTargetUser={elevationTargetUser}
        otherUserForm={otherUserForm}
        elevationControls={
          canElevate ? (
            <ElevationControls
              groupRef={menus.elevationGroupRef}
              menuButtonRef={menus.elevationMenuButtonRef}
              menuRef={menus.elevationMenuRef}
              otherUserItemRef={menus.otherUserItemRef}
              otherUserInputRef={menus.otherUserInputRef}
              menuStyle={menus.elevationMenuStyle}
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
        onNewFolder={ops.openMkdir}
        onUpload={xfer.handleUploadButton}
        onUploadFolder={xfer.handleUploadFolderButton}
        cutEntry={cutEntry}
        onPaste={ops.handlePaste}
        onCancelCut={() => ops.dispatch({ type: "clearCut" })}
      />

      {transfers.length > 0 && (
        <TransferList transfers={transfers} sessionId={sessionId} onDismiss={dismissTransfer} />
      )}

      {clipboard.copyJob &&
        Math.max(clipboard.copyJob.done, clipboard.copyJob.total ?? 0) >= COPY_PROGRESS_FROM_BYTES && (
          <CopyProgress job={clipboard.copyJob} />
        )}
      <div
        ref={tableContainerRef}
        className={`relative min-h-0 flex-1 overflow-auto ${dragOver ? "bg-accent-subtle/40" : ""}`}
        // Issue #570: Rechtsklick auf leere Fläche (unter den Zeilen oder in
        // einem leeren Verzeichnis). Die Zeilen stoppen das Ereignis selbst.
        onContextMenu={(e) => {
          e.preventDefault();
          setOpenMenu(null);
          clipboard.refreshClipboardFiles(!cutEntry && caps.fileClipboard);
          setDirMenu({ x: e.clientX, y: e.clientY });
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
          onClose={() => setDirMenu(null)}
          x={dirMenu.x}
          y={dirMenu.y}
          pasteName={cutEntry?.name ?? null}
          clipboardFiles={!cutEntry && clipboard.clipboardFiles}
          onNewFolder={() => {
            setDirMenu(null);
            ops.openMkdir();
          }}
          onNewFile={() => {
            setDirMenu(null);
            ops.openNewFile();
          }}
          onRefresh={() => {
            setDirMenu(null);
            handleRefresh();
          }}
          onPaste={() => {
            setDirMenu(null);
            if (cutEntry) ops.handlePaste();
            else void clipboard.handlePasteClipboardFiles();
          }}
        />
      )}

      <FileBrowserDialogs
        sessionId={sessionId}
        ops={ops}
        edit={edit}
        elevated={elevated}
        elevatedUser={elevatedUser}
        channelUser={channelUser}
        transfers={xfer}
        elevationState={elev}
      />
    </div>
  );
}
