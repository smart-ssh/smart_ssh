import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../api/types";

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

/** Small glyph after the name of a symbolic link, with an accessible label. */
export function LinkMarker({ entry }: { entry: RemoteEntryDto }) {
  const { t } = useTranslation();
  if (!entry.isSymlink) return null;
  return (
    <span title={entryTitle(entry)} data-link-marker>
      <span aria-hidden="true"> ↪</span>
      <span className="sr-only">{" "}{t("fileBrowser.linkMarker")}</span>
    </span>
  );
}

/** Broken links are shown de-emphasised. */
export function isBrokenLink(entry: RemoteEntryDto): boolean {
  return entry.isSymlink && entry.linkKind === "broken";
}
