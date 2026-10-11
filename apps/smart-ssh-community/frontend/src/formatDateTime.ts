/** Alle sichtbaren Datums-/Zeitangaben folgen der UI-Sprache (Spec 0024),
 * nicht der Webview-Locale. Aufrufer übergeben `i18n.language` aus
 * `useTranslation()`, damit sich die Anzeige beim Sprachwechsel mitändert. */
export function formatDateTime(value: string | number | Date, language: string): string {
  const date = new Date(value);
  try {
    return date.toLocaleString(language);
  } catch {
    // Ungültiger Sprachcode: lieber Webview-Format als ein Absturz.
    return date.toLocaleString();
  }
}
