import { detectFallbackPlatform, platformCapabilities } from "../../lib/platform";
import type { HideableFileColumn } from "../../settings/layoutSettings";
import type { RateState } from "../../lib/transferRate";
import type { PreviewReadFailure } from "../../lib/previewReadFailure";
import type { LocalFilePreviewDto, RemoteEntryDto } from "../../api/types";

/** Issue #653: display order of the hideable columns (Name comes first). */
export const COLUMN_ORDER: readonly HideableFileColumn[] = ["size", "permissions", "owner", "modified"];

export const currentCaps = () => platformCapabilities(detectFallbackPlatform());

export type MoveKind = "rename" | "move";

/** Focuses the ⋮ button of the entry with this path (no-op for the context menu). */
export function findMenuTrigger(path: string): HTMLElement | null {
  for (const el of document.querySelectorAll<HTMLElement>("[data-menu-trigger]")) {
    if (el.dataset.menuTrigger === path) return el;
  }
  return null;
}

export function focusMenuTrigger(path: string): void {
  findMenuTrigger(path)?.focus();
}

export function formatMb(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}

/** Issue #570: Namen für neue Einträge — nicht leer, kein `/`, nicht `.`
 * oder `..` (wie bei Umbenennen). */
export function isValidEntryName(name: string): boolean {
  return name !== "" && !name.includes("/") && name !== "." && name !== "..";
}

/** Fixed, non-resizable actions column (⋮ menu) of the entry table, in px. */
export const ACTIONS_COLUMN_WIDTH = 36;

/** Minimum width the name column keeps when other columns are dragged wider, in px. */
export const NAME_MIN_WIDTH = 120;

// Header cells (not the thead) are sticky with their own opaque background and
// the table uses `border-separate`: a sticky thead under `border-collapse`
// leaves a seam in WebKit through which scrolled rows showed (#650).
export const STICKY_TH = "sticky top-0 z-10 bg-surface";

/** The open per-entry menu: anchored to the row's ⋮ button or at a point (context menu). */
export type OpenEntryMenu = {
  entry: RemoteEntryDto;
  anchor: "row" | { x: number; y: number };
};

export interface Transfer {
  id: string;
  kind: "upload" | "download" | "delete" | "chmod" | "chown";
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
  /** Issue #658: recursive delete/chmod — entries done / total (`null` while counting). */
  entries?: { done: number; total: number | null };
  /** Issue #658: entries changed when the row ended (cancelled / stopped / failed). */
  entriesDone?: number;
  /** Issue #622: a stopped upload left this incomplete remote file behind. */
  incompleteRemotePath?: string;
}

/** Upload-overwrite diff preview (Spec 0054, Teil 3): `remoteText` is `null` when the
 * remote file was not readable as text (binary / too large) — then the dialog only
 * shows a size comparison. */
export interface UploadConflict {
  localPath: string;
  remotePath: string;
  localPreview: LocalFilePreviewDto;
  remoteText: string | null;
  /** Issue #655: why `remoteText` is `null`. */
  remoteReadFailure: PreviewReadFailure | null;
  remoteSize: number;
}
