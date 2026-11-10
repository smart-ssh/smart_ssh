import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../lib/formatDateTime";
import { formatBytes } from "../../lib/format";
import { type RemoteEntryDto } from "../../api/types";
import { Button, DialogFrame } from "../ui";

/** Spec 0054, Teil 2: "Eigenschaften" — reine Anzeige der bereits geladenen
 * `RemoteEntryDto` (kein eigener Backend-Aufruf: `sftp_list` liefert
 * Rechte/Besitzer/Gruppe/Datum schon mit, ein erneutes `stat` wäre
 * redundant). Rechte numerisch (`permissionsOctal`, 3-stellig oktal) UND
 * symbolisch (`permissions`, bereits fertig formatiert) nebeneinander, wie
 * von der Spec verlangt. */
export function FilePropertiesDialog({
  entry,
  onClose,
}: {
  entry: RemoteEntryDto;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation();
  const octal = entry.permissionsOctal.toString(8).padStart(4, "0");
  const owner = entry.owner ?? (entry.uid !== null ? String(entry.uid) : "—");
  const group = entry.group ?? (entry.gid !== null ? String(entry.gid) : "—");

  const row = (label: string, value: string) => (
    <div className="flex justify-between gap-4 py-1">
      <span className="shrink-0 text-fg-subtle">{label}</span>
      <span className="min-w-0 text-right font-mono text-fg break-all">{value}</span>
    </div>
  );

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={t("fileBrowser.properties.title")}
      onClose={onClose}
      actions={<Button onClick={onClose}>{t("fileBrowser.properties.close")}</Button>}
    >
      <div className="divide-y divide-surface-raised/70 text-sm">
        {row(t("fileBrowser.properties.name"), entry.name)}
        {row(t("fileBrowser.properties.path"), entry.path)}
        {row(
          t("fileBrowser.properties.type"),
          entry.isSymlink
            ? t(
                entry.linkKind === "dir"
                  ? "fileBrowser.properties.typeLinkFolder"
                  : entry.linkKind === "file"
                    ? "fileBrowser.properties.typeLinkFile"
                    : "fileBrowser.properties.typeLinkBroken",
              )
            : entry.isDir
              ? t("fileBrowser.properties.typeFolder")
              : t("fileBrowser.properties.typeFile"),
        )}
        {entry.isSymlink && entry.linkTarget !== null && row(t("fileBrowser.properties.linkTarget"), entry.linkTarget)}
        {row(t("fileBrowser.properties.size"), entry.isDir ? "—" : formatBytes(entry.size))}
        {row(t("fileBrowser.properties.permissionsSymbolic"), entry.permissions)}
        {row(t("fileBrowser.properties.permissionsNumeric"), octal)}
        {row(t("fileBrowser.properties.owner"), owner)}
        {row(t("fileBrowser.properties.group"), group)}
        {row(
          t("fileBrowser.properties.modified"),
          entry.modified ? formatDateTime(entry.modified, i18n.language) : "—",
        )}
      </div>
    </DialogFrame>
  );
}
