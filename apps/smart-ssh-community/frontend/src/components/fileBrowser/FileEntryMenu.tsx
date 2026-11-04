import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../types";
import { Menu, MenuItem, MenuSeparator } from "../ui";
import { currentCaps, findMenuTrigger, focusMenuTrigger } from "./shared";

/** Spec 0054, Teil 1: die EINE Aktionsliste, die sowohl vom Drei-Punkte-
 * Menü als auch vom Kontextmenü (Rechtsklick) gerendert wird — "beide Wege
 * zeigen dieselben Aktionen". `className`/`style` bestimmen nur die
 * Positionierung, die Aktionsliste selbst ist davon unabhängig. Reihenfolge
 * folgt Spec 0054: lesend/lokal zuerst (Herunterladen, Kopieren,
 * Eigenschaften, Aktualisieren), dann — durch einen Trenner abgesetzt — die
 * server-verändernden Aktionen (Umbenennen, Löschen; chmod/Verschieben
 * folgen in Teil 3). */
type MenuKey =
  | "download" | "downloadTo" | "copyContent" | "copyFile" | "openLocally" | "copyPath"
  | "properties" | "refresh" | "chmod" | "chown" | "rename" | "cut" | "delete";

export function FileEntryMenu({
  entry,
  narrow,
  className,
  point,
  onClose,
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
  onClose: () => void;
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
  // The menu unmounts when an action runs. Focus the ⋮ button first, so a
  // dialog opened by the action remembers it and gives focus back on close.
  const act = (fn: () => void) => () => {
    focusMenuTrigger(entry.path);
    fn();
  };
  const item = (key: MenuKey, onClick: () => void, danger = false) => (
    <MenuItem narrow={narrow} danger={danger} onClick={onClick}>
      {t(`fileBrowser.menu.${key}`)}
    </MenuItem>
  );
  return (
    <Menu
      label={entry.name}
      anchor={() => point ?? findMenuTrigger(entry.path)}
      onClose={onClose}
      returnFocusTo={point ? undefined : () => findMenuTrigger(entry.path)}
      narrow={narrow}
      align={point ? "start" : "end"}
      gap={point ? 0 : 4}
      className={`${className.replace(/^fixed\s*/, "")} w-52`.trim()}
    >
      {item("download", act(() => onDownloadDefault(entry)))}
      {(caps.folderPicker || !entry.isDir) && item("downloadTo", act(() => onDownloadChoose(entry)))}
      {!entry.isDir && item("copyContent", act(() => onCopyContent(entry)))}
      {!entry.isDir && caps.fileClipboard && item("copyFile", act(() => onCopyFile(entry)))}
      {!entry.isDir && item("openLocally", act(() => onOpenLocally(entry)))}
      {item("copyPath", act(() => onCopyPath(entry)))}
      {item("properties", act(() => onShowProperties(entry)))}
      {item("refresh", act(onRefresh))}
      <MenuSeparator />
      {item("chmod", act(() => onChmod(entry)))}
      {item("chown", act(() => onChown(entry)))}
      {item("rename", act(() => onRename(entry)))}
      {item("cut", act(() => onCut(entry)))}
      {item("delete", act(() => onDelete(entry)), true)}
    </Menu>
  );
}
