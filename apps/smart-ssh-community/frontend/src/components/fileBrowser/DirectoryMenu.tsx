import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useMenuPlacement } from "../../hooks/useMenuPlacement";

/** Issue #570: Kontextmenü auf leerer Fläche des Dateibrowsers — bewusst
 * eine eigene kleine Komponente, damit `FileEntryMenu` nur Eintrags-
 * Aktionen behält. Schließen per Klick außerhalb/Escape macht der Aufrufer. */
export function DirectoryMenu({
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
  const itemClass = `block w-full px-3 ${narrow ? "min-h-11 py-2 text-sm" : "py-1.5"} text-left text-fg-soft hover:bg-accent/14 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-fg`;
  return (
    <fieldset
      ref={menuRef}
      className={
        narrow
          ? "fixed inset-x-0 bottom-0 z-30 m-0 min-w-0 rounded-t border border-border bg-surface px-0 py-1 text-left text-xs shadow-lg"
          : "fixed z-20 m-0 w-52 min-w-0 border border-border bg-surface px-0 py-1 text-left text-xs shadow-lg"
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
