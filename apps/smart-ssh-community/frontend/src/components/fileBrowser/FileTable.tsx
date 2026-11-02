import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../formatDateTime";
import { formatOwner } from "../../fileOwner";
import { formatBytes } from "../../format";
import type { FileSort, FileSortColumn } from "../../fileSort";
import type { FileManagerColumnWidths, HideableFileColumn } from "../../layoutSettings";
import type { RemoteEntryDto } from "../../types";
import { ColumnResizeHandle } from "./ColumnResizeHandle";
import { ACTIONS_COLUMN_WIDTH, COLUMN_ORDER, NAME_MIN_WIDTH, STICKY_TH, type OpenEntryMenu } from "./shared";

interface FileTableProps {
  entries: RemoteEntryDto[];
  fileSort: FileSort;
  hiddenColumns: HideableFileColumn[];
  columnWidths: FileManagerColumnWidths;
  openMenu: OpenEntryMenu | null;
  setOpenMenu: (menu: OpenEntryMenu | null) => void;
  setDirMenu: (point: { x: number; y: number } | null) => void;
  setColumnsMenu: (point: { x: number; y: number } | null) => void;
  load: (path: string) => void;
  onSort: (column: FileSortColumn) => void;
  onColumnDrag: (column: keyof FileManagerColumnWidths) => (deltaX: number) => void;
  onColumnDragEnd: () => void;
  renderEntryMenu: (entry: RemoteEntryDto, className: string) => React.ReactNode;
}

/** The wide-layout entry table: column group, sortable/resizable header, rows. */
export function FileTable({
  entries,
  fileSort,
  hiddenColumns,
  columnWidths,
  openMenu,
  setOpenMenu,
  setDirMenu,
  setColumnsMenu,
  load,
  onSort,
  onColumnDrag,
  onColumnDragEnd,
  renderEntryMenu,
}: FileTableProps) {
  const { t, i18n } = useTranslation();
  const language = i18n.language;
  const sortAria = (column: FileSortColumn): "ascending" | "descending" | "none" =>
    fileSort.column !== column ? "none" : fileSort.direction === "asc" ? "ascending" : "descending";
  const sortButton = (column: FileSortColumn) => (
    <button
      type="button"
      onClick={() => onSort(column)}
      data-testid={`column-sort-${column}`}
      className="inline-flex max-w-full items-center gap-1 rounded font-normal hover:text-fg-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
    >
      <span className="truncate">{t(`fileBrowser.column.${column}`)}</span>
      {fileSort.column === column && (
        <span aria-hidden="true">{fileSort.direction === "asc" ? "▲" : "▼"}</span>
      )}
    </button>
  );

  return (
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
          COLUMN_ORDER.filter((c) => !hiddenColumns.includes(c)).reduce(
            (sum, c) => sum + columnWidths[c],
            0,
          ) +
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
        {COLUMN_ORDER.filter((c) => !hiddenColumns.includes(c)).map((c) => (
          <col key={c} style={{ width: columnWidths[c] }} />
        ))}
        <col style={{ width: ACTIONS_COLUMN_WIDTH }} />
      </colgroup>
      <thead className="text-fg-subtle">
        <tr
          className="[&>th]:px-2 [&>th]:py-1 [&>th]:font-normal"
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setOpenMenu(null);
            setDirMenu(null);
            setColumnsMenu({ x: e.clientX, y: e.clientY });
          }}
        >
          <th className={STICKY_TH} aria-sort={sortAria("name")}>{sortButton("name")}</th>
          {COLUMN_ORDER.filter((c) => !hiddenColumns.includes(c)).map((c) => (
            <th
              key={c}
              className={`relative ${STICKY_TH}`}
              aria-sort={c === "owner" ? undefined : sortAria(c)}
            >
              {c === "owner" ? t("fileBrowser.column.owner") : sortButton(c)}
              <ColumnResizeHandle
                testId={`column-resize-${c}`}
                onDrag={onColumnDrag(c)}
                onDragEnd={onColumnDragEnd}
              />
            </th>
          ))}
          <th className={STICKY_TH}>
            <span className="sr-only">{t("fileBrowser.actionsColumn")}</span>
          </th>
        </tr>
      </thead>
      <tbody>
        {entries.map((entry) => (
          <tr
            key={entry.path}
            data-active={openMenu?.entry.path === entry.path ? "true" : undefined}
            className="group hover:bg-border/60 focus-within:bg-border/80 data-[active=true]:bg-border/80 [&>td]:border-t [&>td]:border-surface-raised/70"
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
                  className="block max-w-full truncate rounded text-left text-accent-fg-soft hover:underline group-hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  title={entry.name}
                >
                  📁 {entry.name}
                </button>
              ) : (
                <span className="block truncate text-fg-soft group-hover:underline" title={entry.name}>
                  📄 {entry.name}
                </span>
              )}
            </td>
            {COLUMN_ORDER.filter((c) => !hiddenColumns.includes(c)).map((c) => (
              <td
                key={c}
                className={`px-2 py-1 whitespace-nowrap text-fg-subtle${c === "permissions" ? " font-mono" : ""}${c === "owner" ? " truncate" : ""}`}
                title={c === "owner" ? formatOwner(entry) : undefined}
              >
                {c === "size" && (entry.isDir ? "—" : formatBytes(entry.size))}
                {c === "permissions" && entry.permissions}
                {c === "owner" && formatOwner(entry)}
                {c === "modified" &&
                  (entry.modified ? formatDateTime(entry.modified, language) : "—")}
              </td>
            ))}
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
                className="rounded px-1.5 text-fg-subtle hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
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
  );
}
