import { detectFallbackPlatform, platformCapabilities } from "../../titlebarLayout";
import type { HideableFileColumn } from "../../layoutSettings";

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
