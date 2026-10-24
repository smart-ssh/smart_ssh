import type { RemoteEntryDto } from "./types";

/** Spec 0053, Teil 3: sortable columns of the file list. */
export type FileSortColumn = "name" | "size" | "permissions" | "modified";
export type FileSortDirection = "asc" | "desc";

export interface FileSort {
  column: FileSortColumn;
  direction: FileSortDirection;
}

export const DEFAULT_FILE_SORT: FileSort = { column: "name", direction: "asc" };

export const FILE_SORT_COLUMNS: readonly FileSortColumn[] = ["name", "size", "permissions", "modified"];

const nameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function compareNames(a: RemoteEntryDto, b: RemoteEntryDto): number {
  return nameCollator.compare(a.name, b.name) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
}

/** Ascending comparison inside one group (folders or files). */
function compareByColumn(column: FileSortColumn, a: RemoteEntryDto, b: RemoteEntryDto): number {
  switch (column) {
    case "name":
      return compareNames(a, b);
    case "size":
      // Folders carry no meaningful size: they are ordered by name.
      if (a.isDir && b.isDir) return compareNames(a, b);
      return a.size - b.size;
    case "permissions":
      return a.permissions < b.permissions ? -1 : a.permissions > b.permissions ? 1 : 0;
    case "modified": {
      // Entries without a timestamp sort before dated ones (ascending).
      const ta = a.modified ? Date.parse(a.modified) : Number.NEGATIVE_INFINITY;
      const tb = b.modified ? Date.parse(b.modified) : Number.NEGATIVE_INFINITY;
      const na = Number.isNaN(ta) ? Number.NEGATIVE_INFINITY : ta;
      const nb = Number.isNaN(tb) ? Number.NEGATIVE_INFINITY : tb;
      return na === nb ? 0 : na < nb ? -1 : 1;
    }
  }
}

/** Folders always stay on top; both groups are sorted by the chosen
 * column. Ties fall back to name ascending (also when reversed), so the
 * order is deterministic. Returns a new array. */
export function sortEntries(entries: readonly RemoteEntryDto[], sort: FileSort): RemoteEntryDto[] {
  const sign = sort.direction === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    const primary = compareByColumn(sort.column, a, b);
    if (primary !== 0) return primary * sign;
    return compareNames(a, b);
  });
}

/** Header click: same column reverses, another column starts ascending. */
export function nextFileSort(current: FileSort, column: FileSortColumn): FileSort {
  if (current.column === column) {
    return { column, direction: current.direction === "asc" ? "desc" : "asc" };
  }
  return { column, direction: "asc" };
}
