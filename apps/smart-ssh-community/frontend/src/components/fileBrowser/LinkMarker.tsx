import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../api/types";
import { Icon, type IconName } from "../ui";

const MAX_TARGET_TITLE = 200;

/** Issue #1291: hover text of an entry — the name, plus `-> <target>` for a
 * symbolic link whose target could be read. The target is server-controlled
 * text: it is only ever used as plain text and truncated here. */
export function entryTitle(entry: RemoteEntryDto): string {
  const target = truncatedTarget(entry);
  return target === null ? entry.name : `${entry.name} -> ${target}`;
}

/** The link target, cut to `MAX_TARGET_TITLE` characters; `null` for a non-link or an unreadable target. */
function truncatedTarget(entry: RemoteEntryDto): string | null {
  if (!entry.isSymlink || entry.linkTarget === null) return null;
  return entry.linkTarget.length > MAX_TARGET_TITLE ? `${entry.linkTarget.slice(0, MAX_TARGET_TITLE)}…` : entry.linkTarget;
}

/** Marker after the name of a symbolic link: the target inline as faint mono
 * "→ target" (truncated by CSS, full value in `title`), with an accessible
 * label. The target is server-controlled text and only rendered as text. */
export function LinkMarker({ entry }: { entry: RemoteEntryDto }) {
  const { t } = useTranslation();
  if (!entry.isSymlink) return null;
  return (
    <span title={truncatedTarget(entry) ?? undefined} data-link-marker className="ml-2 min-w-0 shrink truncate font-mono text-xs text-fg-faint">
      <span className="sr-only">{" "}{t("fileBrowser.linkMarker")}</span>
      {entry.linkTarget !== null && <span aria-hidden="true" data-link-target>{`→ ${entry.linkTarget}`}</span>}
    </span>
  );
}

/** Broken links are shown de-emphasised. */
export function isBrokenLink(entry: RemoteEntryDto): boolean {
  return entry.isSymlink && entry.linkKind === "broken";
}

/** Icon of a directory-listing entry: folder / folder-link for directories,
 * file / symlink for files, file-x for a broken link (still described by the
 * entry title and the link marker's text). */
export function entryIconName(entry: RemoteEntryDto): IconName {
  if (isBrokenLink(entry)) return "file-x";
  if (entry.isDir) return entry.isSymlink ? "folder-link" : "folder";
  return entry.isSymlink ? "symlink" : "file";
}

export function EntryIcon({ entry, size }: { entry: RemoteEntryDto; size?: 16 | 20 }) {
  return <Icon name={entryIconName(entry)} size={size} className={`mr-1.5 inline shrink-0 align-text-bottom ${entry.isDir ? "text-accent-fg" : "text-fg-subtle"}`} />;
}
