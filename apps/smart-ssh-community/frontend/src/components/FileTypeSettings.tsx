import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import {
  loadFileTypeApps,
  normalizeExtension,
  saveFileTypeApps,
  type FileTypeAppSettings,
} from "../fileTypeSettings";

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
      <p className="mb-4 text-sm text-slate-400">{t("fileTypes.description")}</p>

      {loadError && (
        <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{loadError}</p>
      )}

      <div className="mb-3 space-y-2">
        {rows.map((row) => (
          <div key={row.id} className="flex items-center gap-2">
            <input
              value={row.extension}
              onChange={(e) => updateRow(row.id, { extension: e.target.value })}
              placeholder=".conf"
              className="w-24 rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-sm text-slate-100 focus:outline-none"
            />
            <input
              value={row.appPath}
              onChange={(e) => updateRow(row.id, { appPath: e.target.value })}
              placeholder={t("fileTypes.appPathPlaceholder")}
              className="min-w-0 flex-1 rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-sm text-slate-100 focus:outline-none"
            />
            <button
              type="button"
              onClick={() => handleBrowse(row.id)}
              className="rounded border border-slate-600 px-2 py-1.5 text-xs text-slate-300 hover:bg-slate-700"
            >
              {t("fileTypes.browse")}
            </button>
            <button
              type="button"
              onClick={() => removeRow(row.id)}
              title={t("fileTypes.remove")}
              className="px-2 text-slate-400 hover:text-red-400"
            >
              ✕
            </button>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={addRow}
          className="rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
        >
          {t("fileTypes.addRow")}
        </button>
        <button
          type="button"
          onClick={handleSave}
          className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-semibold text-slate-950 hover:bg-indigo-500"
        >
          {t("fileTypes.save")}
        </button>
        {saveState === "saved" && (
          <span className="text-xs text-emerald-400">{t("fileTypes.saved")}</span>
        )}
        {saveState === "failed" && (
          <span className="text-xs text-red-300">{t("fileTypes.saveFailed")}</span>
        )}
      </div>
    </div>
  );
}
