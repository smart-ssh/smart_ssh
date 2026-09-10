import { settingsStore } from "./i18n";

/** Spec 0053: UI-Layout-Präferenzen (Spaltenbreiten im Dateimanager,
 * KI-/SSH-Bereichsaufteilung) — derselbe `tauri-plugin-store`-Ablageort
 * wie Sprache (`i18n.ts`) und Risiko-Zweitmeinung (`riskSettings.ts`):
 * reine, nicht sicherheitsrelevante UI-Einstellungen ohne eigenen
 * SQLite-Bedarf. Anders als die Sprache/Risiko-Einstellungen hat keiner
 * dieser Werte einen Rust-seitigen Leser — reines Frontend-Layout, das
 * Backend kennt diese Schlüssel nicht.
 *
 * Bewusst global (nicht pro Server/Session) — die App merkt sich, wie der
 * Nutzer sein UI eingerichtet hat, nicht wie jeder einzelne Server
 * aussehen soll. */
const FILE_MANAGER_COLUMN_WIDTHS_KEY = "fileManagerColumnWidths";
const AI_SSH_SPLIT_WIDTH_KEY = "aiSshSplitWidthPx";

export interface FileManagerColumnWidths {
  size: number;
  permissions: number;
  modified: number;
}

/** Obergrenze rein als Sanity-Check gegen eine grob kaputte
 * `settings.json` (z. B. von Hand editiert) — deutlich über jeder
 * jemals sinnvollen Fensterbreite, aber klein genug, um eine
 * `Number.MAX_VALUE`-artige Eingabe zuverlässig abzufangen, bevor sie
 * (z. B. in `otherColumnsTotal` in `FileBrowserPanel.tsx`) mit einem
 * weiteren extremen Wert zu `Infinity`/`NaN` führen könnte. */
const MAX_PLAUSIBLE_WIDTH = 10_000;

/** Nur numerisch-endliche, positive Werte innerhalb einer plausiblen
 * Obergrenze gelten als gültig — ein von Hand editiertes oder durch eine
 * künftige Formatänderung unpassend gewordenes `settings.json` darf nie
 * zu `NaN`/negativen/absurd großen Spaltenbreiten oder einer kaputten
 * Aufteilung führen. Aufrufer verschmelzen das Ergebnis mit ihren
 * eigenen Default-/Mindestwerten, hier wird sonst bewusst nicht geklemmt
 * (die genauen Mindest-/Maximalwerte hängen vom aktuellen Container ab,
 * s. `FileBrowserPanel.tsx`/`SessionView.tsx`). */
function isValidWidth(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value > 0 &&
    value <= MAX_PLAUSIBLE_WIDTH
  );
}

/** Liefert nur die Felder zurück, die tatsächlich gültig gespeichert sind
 * — fehlende/ungültige Felder lässt der Aufrufer bei seinem Default. */
export async function loadFileManagerColumnWidths(): Promise<Partial<FileManagerColumnWidths>> {
  const store = await settingsStore();
  const stored = await store.get<Partial<Record<keyof FileManagerColumnWidths, unknown>>>(
    FILE_MANAGER_COLUMN_WIDTHS_KEY,
  );
  if (!stored) return {};
  const result: Partial<FileManagerColumnWidths> = {};
  if (isValidWidth(stored.size)) result.size = stored.size;
  if (isValidWidth(stored.permissions)) result.permissions = stored.permissions;
  if (isValidWidth(stored.modified)) result.modified = stored.modified;
  return result;
}

export async function saveFileManagerColumnWidths(widths: FileManagerColumnWidths): Promise<void> {
  const store = await settingsStore();
  await store.set(FILE_MANAGER_COLUMN_WIDTHS_KEY, widths);
  await store.save();
}

/** `null`, wenn nie gespeichert oder der gespeicherte Wert ungültig ist —
 * der Aufrufer fällt dann auf seinen eigenen Default zurück. */
export async function loadAiSshSplitWidthPx(): Promise<number | null> {
  const store = await settingsStore();
  const stored = await store.get<unknown>(AI_SSH_SPLIT_WIDTH_KEY);
  return isValidWidth(stored) ? stored : null;
}

export async function saveAiSshSplitWidthPx(widthPx: number): Promise<void> {
  const store = await settingsStore();
  await store.set(AI_SSH_SPLIT_WIDTH_KEY, widthPx);
  await store.save();
}
