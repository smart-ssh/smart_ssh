import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../api/types";
import { Icon, type IconName } from "../ui";

const MAX_TARGET_TITLE = 200;

/** Issue #1291: hover text of an entry — the name, plus `-> <target>` for a
 * symbolic link whose target could be read. The target is server-controlled
 * text: it is only ever used as plain text and truncated here. */
export function entryTitle(entry: RemoteEntryDto): string {
  if (!entry.isSymlink || entry.linkTarget === null) return entry.name;
  const target =
    entry.linkTarget.length > MAX_TARGET_TITLE ? `${entry.linkTarget.slice(0, MAX_TARGET_TITLE)}…` : entry.linkTarget;
  return `${entry.name} -> ${target}`;
}

/** Marker after the name (the entry icon shows the link; this adds the text) of a symbolic link, with an accessible label. */
export function LinkMarker({ entry }: { entry: RemoteEntryDto }) {
  const { t } = useTranslation();
  if (!entry.isSymlink) return null;
  return (
    <span title={entryTitle(entry)} data-link-marker>
      <span className="sr-only">{" "}{t("fileBrowser.linkMarker")}</span>
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

export function EntryIcon({ entry }: { entry: RemoteEntryDto }) {
  return <Icon name={entryIconName(entry)} className="mr-1 inline align-text-bottom" />;
}
