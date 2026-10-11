// Eigene, minimale `i18next`-Instanz für Komponententests — `../i18n.ts`
// selbst lässt sich in Tests nicht importieren, da es beim Import
// `@tauri-apps/plugin-os`/`@tauri-apps/plugin-store` anspricht (kein
// `__TAURI_INTERNALS__` außerhalb einer echten Tauri-Webview vorhanden).
// Nutzt dieselben Ressourcen der Sprach-Registry (`languages.ts`)-Ressourcen wie die echte App, damit
// Tests reale Übersetzungstexte statt roher Keys sehen.
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { LANGUAGE_REGISTRY } from "./languages";

export const testI18n = i18next.createInstance();
void testI18n.use(initReactI18next).init({
  lng: "de",
  fallbackLng: "en",
  ns: ["common"],
  defaultNS: "common",
  resources: Object.fromEntries(LANGUAGE_REGISTRY.map((l) => [l.code, { common: l.resources }])),
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
});
