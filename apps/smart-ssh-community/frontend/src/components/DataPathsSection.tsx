import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, getDataPaths, openDataPathFolder } from "../api";
import type { DataPathEntryDto } from "../types";
import { Button } from "./ui";

/** Issue #16: Wo diese Instanz ihre Daten ablegt — Datenbank, Logs,
 * Host-Keys, Schlüsseldatei des Master-Passworts, MCP-Einstellungen und
 * gegebenenfalls Einträge einer Edition. Nur Anzeige: jeder Pfad lässt sich
 * kopieren, der Ordner öffnen. Sitzungen haben keine eigene Datei, sie
 * liegen in der Datenbank. */
export function DataPathsSection() {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<DataPathEntryDto[] | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    getDataPaths()
      .then(setEntries)
      .catch(() => setLoadFailed(true));
  }, []);

  /** Eingebaute Einträge haben `label === null` und werden übersetzt; ein
   * unbekannter Bezeichner ohne Beschriftung zeigt den Bezeichner selbst,
   * nie eine leere Zeile. */
  const labelFor = (entry: DataPathEntryDto) => {
    if (entry.label) return entry.label;
    const key = `diagnostics.dataPaths.${entry.id}`;
    const text = t(key);
    return text === key ? entry.id : text;
  };

  const handleCopy = async (entry: DataPathEntryDto) => {
    try {
      await navigator.clipboard.writeText(entry.path);
      setMessage({ text: t("diagnostics.dataPaths.copied"), error: false });
    } catch (err) {
      setMessage({
        text: `${t("diagnostics.dataPaths.copyFailed")} ${String(err)}`,
        error: true,
      });
    }
  };

  const handleOpenFolder = async (entry: DataPathEntryDto) => {
    setMessage(null);
    try {
      await openDataPathFolder(entry.id);
    } catch (err) {
      setMessage({
        text: `${t("diagnostics.dataPaths.openFolderFailed")} ${commandErrorMessage(err)}`,
        error: true,
      });
    }
  };

  return (
    <div
      className="space-y-2 rounded border border-border px-3 py-2 text-sm text-fg-muted"
      data-testid="data-paths"
    >
      <p className="text-fg-subtle">{t("diagnostics.dataPaths.title")}</p>
      <p className="text-xs text-fg-subtle">{t("diagnostics.dataPaths.hint")}</p>
      {loadFailed && <p className="text-xs text-danger-fg-soft">{t("diagnostics.dataPaths.loadFailed")}</p>}
      {message && (
        <output className={`block text-xs ${message.error ? "text-danger-fg-soft" : "text-fg-subtle"}`}>
          {message.text}
        </output>
      )}
      <ul className="space-y-2">
        {entries?.map((entry) => (
          <li key={entry.id} data-testid={`data-path-${entry.id}`}>
            <div className="text-xs text-fg-subtle">{labelFor(entry)}</div>
            <div className="flex items-center gap-2">
              <code
                title={entry.path}
                className="min-w-0 flex-1 select-text break-all font-mono text-xs text-fg"
              >
                {entry.path}
              </code>
              <Button
                size="sm"
                onClick={() => void handleCopy(entry)}
                aria-label={`${t("diagnostics.dataPaths.copy")}: ${labelFor(entry)}`}
                className="shrink-0"
              >
                {t("diagnostics.dataPaths.copy")}
              </Button>
              <Button
                size="sm"
                onClick={() => void handleOpenFolder(entry)}
                aria-label={`${t("diagnostics.dataPaths.openFolder")}: ${labelFor(entry)}`}
                className="shrink-0"
              >
                {t("diagnostics.dataPaths.openFolder")}
              </Button>
            </div>
          </li>
        ))}
        <li data-testid="data-path-sessions">
          <div className="text-xs text-fg-subtle">{t("diagnostics.dataPaths.sessions")}</div>
          <div className="text-xs text-fg">{t("diagnostics.dataPaths.sessionsInDatabase")}</div>
        </li>
      </ul>
    </div>
  );
}
