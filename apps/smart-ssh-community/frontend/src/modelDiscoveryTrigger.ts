// Issue #326 (Spec 0025, Abschnitt 2): Regel für die automatische
// Modell-Entdeckung beim Verlassen des API-Key- bzw. Base-URL-Felds.
// Reine Logik, ohne React — die Komponente entscheidet damit, ob ein Blur
// eine Anfrage auslöst und ob eine eingetroffene Antwort noch zu den
// aktuellen Eingaben passt.
import { type AiProviderConfigInput, needsBaseUrl, supportsModelDiscovery } from "./types";

/** Die Eingaben, von denen das Ergebnis einer Discovery abhängt. */
export interface DiscoveryInputs {
  providerType: AiProviderConfigInput["providerType"];
  baseUrl: string;
  apiKey: string;
}

export function discoveryInputsOf(
  form: Pick<AiProviderConfigInput, "providerType" | "baseUrl" | "apiKey">,
): DiscoveryInputs {
  return {
    providerType: form.providerType,
    baseUrl: form.baseUrl?.trim() ?? "",
    apiKey: form.apiKey.trim(),
  };
}

export function sameDiscoveryInputs(a: DiscoveryInputs | null, b: DiscoveryInputs): boolean {
  return (
    a !== null &&
    a.providerType === b.providerType &&
    a.baseUrl === b.baseUrl &&
    a.apiKey === b.apiKey
  );
}

/** Löst ein Blur des Key- oder Base-URL-Felds eine Discovery aus?
 *
 * Nur wenn der Typ Discovery unterstützt (Ollama ausgenommen: dort gibt es
 * keinen Key, die eigene Ollama-Suche deckt das ab), ein Key eingegeben
 * ist, eine nötige Base-URL gesetzt ist und sich die Eingaben seit der
 * letzten automatischen oder manuellen Discovery in diesem Formular
 * geändert haben. */
export function shouldAutoDiscover(
  form: Pick<AiProviderConfigInput, "providerType" | "baseUrl" | "apiKey">,
  lastDiscovery: DiscoveryInputs | null,
): boolean {
  if (form.providerType === "ollama" || !supportsModelDiscovery(form.providerType)) {
    return false;
  }
  const inputs = discoveryInputsOf(form);
  if (inputs.apiKey === "") return false;
  if (needsBaseUrl(form.providerType) && inputs.baseUrl === "") return false;
  return !sameDiscoveryInputs(lastDiscovery, inputs);
}
