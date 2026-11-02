import { useCallback, useEffect, useRef, useState } from "react";
import {
  loadFileManagerColumnWidths,
  loadFileManagerSort,
  loadHiddenFileColumns,
  saveFileManagerColumnWidths,
  saveFileManagerSort,
  saveHiddenFileColumns,
  type FileManagerColumnWidths,
  type HideableFileColumn,
} from "../../layoutSettings";
import {
  DEFAULT_FILE_SORT,
  nextFileSort,
  type FileSort,
  type FileSortColumn,
} from "../../fileSort";
import { ACTIONS_COLUMN_WIDTH, NAME_MIN_WIDTH } from "./shared";

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
  owner: 130,
};
const MIN_COLUMN_WIDTHS: FileManagerColumnWidths = {
  size: 56,
  permissions: 64,
  modified: 90,
  owner: 80,
};

/**
 * Sort order, hidden columns and column widths of the file table (Spec 0053,
 * issue #653). All three are app-wide settings, loaded once on mount and
 * persisted through `layoutSettings.ts`.
 */
export function useFileColumns() {
  const [fileSort, setFileSort] = useState<FileSort>(DEFAULT_FILE_SORT);
  // Issue #653: hidden columns (app-wide) and the header's right-click menu.
  const [hiddenColumns, setHiddenColumns] = useState<HideableFileColumn[]>([]);
  const [columnsMenu, setColumnsMenu] = useState<{
    x: number;
    y: number;
  } | null>(null);
  const [columnWidths, setColumnWidths] = useState<FileManagerColumnWidths>(
    DEFAULT_COLUMN_WIDTHS,
  );
  const tableContainerRef = useRef<HTMLDivElement>(null);
  const hiddenColumnsRef = useRef(hiddenColumns);
  hiddenColumnsRef.current = hiddenColumns;
  const columnWidthsRef = useRef(columnWidths);
  useEffect(() => {
    columnWidthsRef.current = columnWidths;
  }, [columnWidths]);

  useEffect(() => {
    loadFileManagerColumnWidths()
      .then((stored) => setColumnWidths((prev) => ({ ...prev, ...stored })))
      .catch((err) => console.warn("Konnte Spaltenbreiten nicht laden:", err));
  }, []);

  useEffect(() => {
    loadHiddenFileColumns()
      .then(setHiddenColumns)
      .catch((err) => console.warn("Could not load hidden columns:", err));
  }, []);

  useEffect(() => {
    loadFileManagerSort()
      .then((stored) => {
        if (stored) setFileSort(stored);
      })
      .catch((err) => console.warn("Konnte Sortierung nicht laden:", err));
  }, []);

  const toggleColumn = useCallback((column: HideableFileColumn) => {
    setHiddenColumns((prev) => {
      const next = prev.includes(column)
        ? prev.filter((c) => c !== column)
        : [...prev, column];
      saveHiddenFileColumns(next).catch((err) =>
        console.warn("Could not save hidden columns:", err),
      );
      return next;
    });
  }, []);

  /** Clamps a proposed width for `column` to its minimum AND so that the
   * Name column (which takes the remaining width) never drops below
   * `NAME_MIN_WIDTH`, based on the container's actual available width. */
  const clampColumnWidth = useCallback(
    (
      column: keyof FileManagerColumnWidths,
      proposed: number,
      widths: FileManagerColumnWidths,
    ) => {
      const hidden = hiddenColumnsRef.current;
      const min = MIN_COLUMN_WIDTHS[column];
      const containerWidth = tableContainerRef.current?.clientWidth ?? Infinity;
      const otherColumnsTotal =
        ACTIONS_COLUMN_WIDTH +
        (Object.keys(widths) as (keyof FileManagerColumnWidths)[])
          .filter((key) => key !== column && !hidden.includes(key))
          .reduce((sum, key) => sum + widths[key], 0);
      const maxForThisColumn =
        containerWidth - otherColumnsTotal - NAME_MIN_WIDTH;
      return Math.min(Math.max(proposed, min), Math.max(min, maxForThisColumn));
    },
    [],
  );

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

  // Persisting reads the width from a ref instead of a state updater, so it
  // is not run twice under StrictMode.
  const handleColumnDragEnd = useCallback(() => {
    saveFileManagerColumnWidths(columnWidthsRef.current).catch((err) =>
      console.warn("Konnte Spaltenbreiten nicht speichern:", err),
    );
  }, []);

  const handleSortClick = (column: FileSortColumn) => {
    const next = nextFileSort(fileSort, column);
    setFileSort(next);
    saveFileManagerSort(next).catch((err) =>
      console.warn("Konnte Sortierung nicht speichern:", err),
    );
  };

  return {
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
  };
}
