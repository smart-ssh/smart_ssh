import { settingsStore } from "./i18n";

/** Spec 0054, Teil 5: pro Dateiendung ein lokales Standardprogramm — rein
 * lokal, kein Server-Bezug, keine Sicherheitsfrage (Spec-Wortlaut). Reiht
 * sich in denselben `tauri-plugin-store`-Ablageort wie Sprache (`i18n.ts`),
 * Risiko-Zweitmeinung (`riskSettings.ts`) und Layout-Präferenzen
 * (`layoutSettings.ts`) ein — dieselbe Klasse Einstellung (reine
 * UI-/Lokal-Präferenz ohne Rust-seitigen Leser), kein neuer Ablageort
 * nötig.
 *
 * Schlüssel im Store sind bereits normalisierte Endungen (klein
 * geschrieben, mit führendem Punkt, z. B. `".conf"`) auf einen lokalen
 * Programmpfad. Global (nicht pro Server/Session) — eine reine
 * Nutzerpräferenz für "wie öffne ICH diesen Dateityp lokal", unabhängig
 * davon, von welchem Server die Datei kam. */
const FILE_TYPE_DEFAULT_APPS_KEY = "fileTypeDefaultApps";

export interface FileTypeAppSettings {
  [extension: string]: string;
}

/** Normalisiert eine vom Nutzer eingegebene Endung auf das im Store
 * verwendete Format (klein geschrieben, mit führendem Punkt) — sowohl
 * `"conf"` als auch `".CONF"` sollen auf denselben Schlüssel treffen. */
export function normalizeExtension(extension: string): string {
  const trimmed = extension.trim().toLowerCase();
  if (trimmed === "") return "";
  return trimmed.startsWith(".") ? trimmed : `.${trimmed}`;
}

/** Nur String-Werte mit einer gültigen, nicht-leeren Endung als Schlüssel
 * gelten — ein von Hand editiertes oder durch eine künftige Formatänderung
 * unpassend gewordenes `settings.json` darf nie zu kaputten Einträgen
 * (leere Endung, `null`/Objekt als "Programmpfad") führen. */
export async function loadFileTypeApps(): Promise<FileTypeAppSettings> {
  const store = await settingsStore();
  const stored = await store.get<unknown>(FILE_TYPE_DEFAULT_APPS_KEY);
  // `typeof [] === "object"` in JS — ohne den expliziten `Array.isArray`-
  // Ausschluss würde ein gespeichertes Array über `Object.entries` seine
  // numerischen Indizes ("0", "1", …) als vermeintliche Endungen liefern.
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
  const result: FileTypeAppSettings = {};
  for (const [key, value] of Object.entries(stored as Record<string, unknown>)) {
    const extension = normalizeExtension(key);
    if (extension !== "" && typeof value === "string" && value.trim() !== "") {
      result[extension] = value;
    }
  }
  return result;
}

export async function saveFileTypeApps(apps: FileTypeAppSettings): Promise<void> {
  const store = await settingsStore();
  await store.set(FILE_TYPE_DEFAULT_APPS_KEY, apps);
  await store.save();
}

/** Spec 0054, Teil 4: welches lokale Programm (falls eines festgelegt ist)
 * soll `fileName` öffnen? `null` bedeutet "kein Eintrag — OS-Standard
 * verwenden" (Spec: "Fallback: OS-Standard"). Eine Datei ohne Endung oder
 * eine reine Punktdatei ohne folgenden Namensteil (".bashrc" hat keine
 * "Endung" im üblichen Sinn, nur einen führenden Punkt) hat ebenfalls
 * keinen Eintrag. */
export function appForFileName(apps: FileTypeAppSettings, fileName: string): string | null {
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex <= 0) return null;
  const extension = fileName.slice(dotIndex).toLowerCase();
  return apps[extension] ?? null;
}
