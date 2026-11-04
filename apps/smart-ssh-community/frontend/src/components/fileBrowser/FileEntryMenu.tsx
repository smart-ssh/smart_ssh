import { useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../types";
import { useMenuPlacement } from "../../hooks/useMenuPlacement";
import { currentCaps, findMenuTrigger, focusMenuTrigger } from "./shared";

/** Spec 0054, Teil 1: die EINE Aktionsliste, die sowohl vom Drei-Punkte-
 * Menü als auch vom Kontextmenü (Rechtsklick) gerendert wird — "beide Wege
 * zeigen dieselben Aktionen". `className`/`style` bestimmen nur die
 * Positionierung, die Aktionsliste selbst ist davon unabhängig. Reihenfolge
 * folgt Spec 0054: lesend/lokal zuerst (Herunterladen, Kopieren,
 * Eigenschaften, Aktualisieren), dann — durch einen Trenner abgesetzt — die
 * server-verändernden Aktionen (Umbenennen, Löschen; chmod/Verschieben
 * folgen in Teil 3). */
export function FileEntryMenu({
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
  onChown,
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
  onChown: (entry: RemoteEntryDto) => void;
  onDelete: (entry: RemoteEntryDto) => void;
}) {
  const caps = currentCaps();
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
  const itemClass = `block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-fg-soft hover:bg-accent/14 focus-ring-inset`;
  return (
    <div
      ref={menuRef}
      className={
        narrow
          ? "fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-border bg-surface py-1 text-left text-xs shadow-lg"
          : `${className} w-52 border border-border bg-surface py-1 text-left text-xs shadow-lg`
      }
      style={narrow ? undefined : style}
    >
      <button type="button" onClick={act(() => onDownloadDefault(entry))} className={itemClass}>
        {t("fileBrowser.menu.download")}
      </button>
      {(caps.folderPicker || !entry.isDir) && (
        <button type="button" onClick={act(() => onDownloadChoose(entry))} className={itemClass}>
          {t("fileBrowser.menu.downloadTo")}
        </button>
      )}
      {!entry.isDir && (
        <button type="button" onClick={act(() => onCopyContent(entry))} className={itemClass}>
          {t("fileBrowser.menu.copyContent")}
        </button>
      )}
      {!entry.isDir && caps.fileClipboard && (
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
      <div className="my-1 border-t border-surface-raised" />
      <button type="button" onClick={act(() => onChmod(entry))} className={itemClass}>
        {t("fileBrowser.menu.chmod")}
      </button>
      <button type="button" onClick={act(() => onChown(entry))} className={itemClass}>
        {t("fileBrowser.menu.chown")}
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
        className={`block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-danger-fg hover:bg-danger/12 focus-ring-inset`}
      >
        {t("fileBrowser.menu.delete")}
      </button>
    </div>
  );
}
