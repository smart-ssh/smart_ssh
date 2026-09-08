// Spec 0050, Teil 2: sofortiger, rein clientseitiger Offline-Hinweis, falls
// ein eingegebener API-Key nicht zum erwarteten Präfix des gewählten
// Providers passt. Ergänzt den Rand-Trim aus Spec 0049 (der bleibt die
// eigentliche CRLF-Lösung) — hier geht es um ein inhaltliches Format-Netz,
// nicht um Whitespace.
//
// KRITISCH (Spec 0050, Abschnitt "Sicherheits-/Konsistenz-Invarianten"):
// nur ein Hinweis, nie eine Blockade — Provider ändern Key-Formate, eine zu
// strenge Prüfung lehnt irgendwann gültige Keys ab. Dieses Modul liefert
// deshalb ausschließlich einen (optionalen) Hinweistext zurück, nie ein
// Boolean/Ergebnis, das ein Aufrufer versehentlich als "gültig/ungültig"
// missverstehen und zum Blockieren des Speicherns nutzen könnte.
import type { ProviderType } from "./types";

/** `null` bedeutet: für diese Provider-Konfiguration gibt es keine
 * verlässliche Präfix-Erwartung, also keine Prüfung (Spec 0050: "für
 * generische OpenAI-kompatible Provider komplett aus" — kein
 * vorhersagbares Format, sonst blockiert man legitime selbstgehostete
 * Endpunkte). `ollama` läuft ebenfalls meist ohne festen Key (lokal, oft
 * gar kein Secret nötig) und bekommt deshalb ebenfalls keine Prüfung.
 *
 * OpenRouter hat in diesem Formular keinen eigenen `ProviderType`-Wert
 * (Spec 0025: OpenRouter wird als `generic_openai_compatible` mit eigener
 * Base-URL konfiguriert) — die von Spec 0050 explizit genannte
 * `sk-or-`-Prüfung greift deshalb nur, wenn die Base-URL erkennbar auf
 * OpenRouter zeigt, sonst bliebe die "generisch = keine Prüfung"-Regel
 * unerreichbar für den einzigen Fall, den die Spec konkret nennt. Jede
 * andere Base-URL bei `generic_openai_compatible` bleibt ungeprüft.
 */
function expectedKeyPrefix(providerType: ProviderType, baseUrl: string | null): string | null {
  switch (providerType) {
    case "anthropic":
      return "sk-ant-";
    case "openai":
      // "sk-proj-..." ist selbst schon ein `sk-`-Präfix (projektgebundene
      // OpenAI-Keys) — ein einziger Präfix-Check deckt beide Fälle aus der
      // Spec ab, ohne zwei Präfixe verwalten zu müssen.
      return "sk-";
    case "generic_openai_compatible":
      return baseUrl?.includes("openrouter.ai") ? "sk-or-" : null;
    case "ollama":
      return null;
  }
}

/** Liefert einen Hinweistext, falls `apiKey` nicht mit dem für
 * `providerType`/`baseUrl` erwarteten Präfix beginnt — sonst `null` (kein
 * Hinweis: leerer Key, kein vorhersagbares Format für diesen Provider,
 * oder das Präfix passt). Der Aufrufer entscheidet selbst, wie der Hinweis
 * dargestellt wird; diese Funktion trifft keine Entscheidung über
 * Blockieren/Erlauben. */
export function apiKeyFormatWarning(
  providerType: ProviderType,
  baseUrl: string | null,
  apiKey: string,
): { expectedPrefix: string } | null {
  if (apiKey.trim() === "") return null;
  const expectedPrefix = expectedKeyPrefix(providerType, baseUrl);
  if (expectedPrefix === null) return null;
  if (apiKey.startsWith(expectedPrefix)) return null;
  return { expectedPrefix };
}
