// Spec 0069, Teil B: reine, testbare Ollama-Hilfen — getrennt von
// `AiProviderSettings.tsx`, damit sich Konstanten/`effectiveApiKey` ohne
// Komponenten-Render testen lassen (Test 26).

import type { AiProviderConfigInput } from "./types";

/** Spec 0069, Teil B2/B5: `127.0.0.1` statt `localhost`, damit kein
 * IPv6-Umweg nötig ist (s. Spec, Begründung B2). Nur dieser eine feste
 * Endpunkt — kein `OLLAMA_HOST`, kein anderer Port (§2, Nicht-Ziele). */
export const OLLAMA_BASE_URL = "http://127.0.0.1:11434/v1";

/** Spec 0069, Teil B5: Ollama braucht keinen API-Key, das Backend-Formular
 * verlangt aber eines. Kein Geheimnis (geht nie an einen echten Dritten),
 * aber bewusst kein Wort, das in normalem Text vorkommt (nicht "ollama")
 * — Spec 0069 §4.5: wird wie jeder andere Key aus Logs redigiert, ein
 * unauffälliger Wert verhindert, dass die Redaction ausgerechnet dieses
 * Wort aus harmlosem Text entfernt. */
export const OLLAMA_PLACEHOLDER_API_KEY = "ollama-no-key";

/** Spec 0069, Teil B5: für `providerType === "ollama"` mit leerem Feld den
 * Platzhalter einsetzen — jeder andere Providertyp behält einen leeren Key
 * tatsächlich leer (kein Platzhalter für Cloud-Provider, die echte
 * Zugangsdaten brauchen). */
export function effectiveApiKey(
  form: Pick<AiProviderConfigInput, "providerType" | "apiKey">,
): string {
  if (form.providerType === "ollama" && form.apiKey.trim() === "") {
    return OLLAMA_PLACEHOLDER_API_KEY;
  }
  return form.apiKey;
}
