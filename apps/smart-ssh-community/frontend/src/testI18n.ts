// Eigene, minimale `i18next`-Instanz für Komponententests — `../i18n.ts`
// selbst lässt sich in Tests nicht importieren, da es beim Import
// `@tauri-apps/plugin-os`/`@tauri-apps/plugin-store` anspricht (kein
// `__TAURI_INTERNALS__` außerhalb einer echten Tauri-Webview vorhanden).
// Nutzt dieselben `de`/`en`-JSON-Ressourcen wie die echte App, damit
// Tests reale Übersetzungstexte statt roher Keys sehen.
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import de from "./locales/de/common.json";
import en from "./locales/en/common.json";

export const testI18n = i18next.createInstance();
void testI18n.use(initReactI18next).init({
  lng: "de",
  fallbackLng: "en",
  ns: ["common"],
  defaultNS: "common",
  resources: {
    de: { common: de },
    en: { common: en },
  },
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
});
