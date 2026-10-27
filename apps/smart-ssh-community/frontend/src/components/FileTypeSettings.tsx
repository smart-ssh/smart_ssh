import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import {
  loadFileTypeApps,
  normalizeExtension,
  saveFileTypeApps,
  type FileTypeAppSettings,
} from "../fileTypeSettings";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { Button, FIELD_CONTROL_CLASS } from "./ui";

interface Row {
  /** Stabiler React-`key`, unabhängig vom (während der Bearbeitung
   * flüchtigen) Endungs-Text — ohne eigene Id würde jede Endungs-Änderung
   * die Zeilenidentität für React wechseln und z. B. den Eingabefokus
   * verlieren. */
  id: string;
  extension: string;
  appPath: string;
}

let nextRowId = 0;
function newRowId(): string {
  nextRowId += 1;
  return `row-${nextRowId}`;
}

function rowsFromSettings(apps: FileTypeAppSettings): Row[] {
  return Object.entries(apps).map(([extension, appPath]) => ({
    id: newRowId(),
    extension,
    appPath,
  }));
}

/** Spec 0054, Teil 5: pro Dateiendung ein lokales Standardprogramm — rein
 * lokal (kein Server-Bezug), Voraussetzung für den "Lokal öffnen"-Flow
 * (Teil 4), der noch nicht Teil dieses Schritts ist. Eigene Settings-
 * Kategorie statt einer Erweiterung von `DiagnosticsSettings`/
 * `AiProviderSettings` — eine eigenständige, wachsende Liste von
 * Zuordnungen passt thematisch zu keiner der bestehenden Kategorien.
 *
 * Bearbeitung als lokale Zeilen-Liste mit einem einzigen expliziten
 * "Speichern" statt Auto-Save pro Tastenanschlag — bei einer Liste
 * beliebiger Länge mit zwei Freitextfeldern pro Zeile wäre Auto-Save pro
 * Zeichen unnötig geschwätzig (viele Store-Schreibvorgänge während des
 * Tippens einer Endung/eines Pfads). */
export function FileTypeSettings() {
  const { t } = useTranslation();
  const [rows, setRows] = useState<Row[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<"idle" | "saved" | "failed">("idle");

  useEffect(() => {
    loadFileTypeApps()
      .then((apps) => setRows(rowsFromSettings(apps)))
      .catch((err) => {
        console.warn("Konnte Dateityp-Zuordnungen nicht laden:", err);
        setLoadError(t("fileTypes.loadFailed"));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const updateRow = (id: string, patch: Partial<Pick<Row, "extension" | "appPath">>) => {
    setRows((prev) => prev.map((row) => (row.id === id ? { ...row, ...patch } : row)));
  };

  const removeRow = (id: string) => {
    setRows((prev) => prev.filter((row) => row.id !== id));
  };

  const addRow = () => {
    setRows((prev) => [...prev, { id: newRowId(), extension: "", appPath: "" }]);
  };

  const handleBrowse = async (id: string) => {
    const picked = await open({ title: t("fileTypes.pickAppTitle"), multiple: false });
    if (typeof picked === "string") {
      updateRow(id, { appPath: picked });
    }
  };

  /** Leere Zeilen (keine Endung ODER kein Pfad) fallen beim Speichern still
   * weg statt einen Validierungsfehler zu zeigen — eine halb ausgefüllte
   * Zeile ist beim Bearbeiten ein normaler Zwischenzustand, kein Fehler.
   * Doppelte Endungen: die letzte Zeile in der Liste gewinnt (`Object.
   * fromEntries` überschreibt frühere Duplikate in Einfügereihenfolge) —
   * unauffällig genug, um keinen eigenen Kollisions-Dialog zu rechtfertigen
   * (anders als z. B. Umbenennen im Dateibrowser: hier gibt es keine
   * "vorhandene Datei", die verloren gehen könnte). */
  const handleSave = async () => {
    const entries = rows
      .map((row) => [normalizeExtension(row.extension), row.appPath.trim()] as const)
      .filter(([extension, appPath]) => extension !== "" && appPath !== "");
    const apps = Object.fromEntries(entries);
    try {
      await saveFileTypeApps(apps);
      setRows(rowsFromSettings(apps));
      setSaveState("saved");
    } catch (err) {
      console.warn("Konnte Dateityp-Zuordnungen nicht speichern:", err);
      setSaveState("failed");
    } finally {
      setTimeout(() => setSaveState("idle"), 2000);
    }
  };

  return (
    <div>
      <p className="mb-4 text-sm text-fg-subtle">{t("fileTypes.description")}</p>

      {loadError && (
        <p role="alert" className="mb-4 rounded bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">{loadError}</p>
      )}

      <div className="mb-3 space-y-2">
        {rows.map((row) => (
          <div key={row.id} className="flex items-center gap-2">
            <input
              {...TECHNICAL_INPUT_PROPS}
              value={row.extension}
              onChange={(e) => updateRow(row.id, { extension: e.target.value })}
              placeholder=".conf"
              aria-label={t("fileTypes.extensionLabel")}
              className={`${FIELD_CONTROL_CLASS} w-24 font-mono`}
            />
            <input
              {...TECHNICAL_INPUT_PROPS}
              value={row.appPath}
              onChange={(e) => updateRow(row.id, { appPath: e.target.value })}
              placeholder={t("fileTypes.appPathPlaceholder")}
              aria-label={t("fileTypes.appPathPlaceholder")}
              title={row.appPath || undefined}
              className={`${FIELD_CONTROL_CLASS} min-w-0 flex-1 font-mono`}
            />
            <Button onClick={() => handleBrowse(row.id)} className="shrink-0">
              {t("fileTypes.browse")}
            </Button>
            <Button
              variant="ghost"
              onClick={() => removeRow(row.id)}
              title={t("fileTypes.remove")}
              aria-label={t("fileTypes.remove")}
              className="shrink-0"
            >
              ✕
            </Button>
          </div>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={addRow}>{t("fileTypes.addRow")}</Button>
        <Button variant="primary" onClick={handleSave}>
          {t("fileTypes.save")}
        </Button>
        {saveState === "saved" && (
          <span className="text-xs text-success-fg">{t("fileTypes.saved")}</span>
        )}
        {saveState === "failed" && (
          <span className="text-xs text-danger-fg-soft">{t("fileTypes.saveFailed")}</span>
        )}
      </div>
    </div>
  );
}
