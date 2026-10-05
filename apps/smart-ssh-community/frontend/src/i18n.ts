import { locale as systemLocale } from "@tauri-apps/plugin-os";
import { load, type Store } from "@tauri-apps/plugin-store";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import de from "./locales/de/common.json";
import en from "./locales/en/common.json";

// Spec 0024, Abschnitt 3/4: `react-i18next` mit `de`/`en` als
// mitgebundene JSON-Ressourcen (kein Nachladen über HTTP nötig — die App
// läuft ohnehin nur lokal) und einem einzigen `common`-Namensraum, solange
// der nicht zu unübersichtlich wird (Spec nennt `servers.json` etc. als
// spätere Option, aktuell nicht nötig).

export const SUPPORTED_LANGUAGES = ["de", "en"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];
const DEFAULT_LANGUAGE: SupportedLanguage = "en";

function isSupportedLanguage(value: string | null | undefined): value is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(value as SupportedLanguage);
}

/** Spec 0024, Abschnitt 4: eigene Store-Datei statt der SQLite-Datenbank —
 * eine einzelne, nicht sicherheitsrelevante UI-Einstellung braucht keine
 * eigene Migration. Auch künftige weitere reine UI-Einstellungen (Theme
 * o. Ä.) gehören laut Spec hierher. `autoSave: false`: wir rufen `save()`
 * selbst genau dann auf, wenn sich die Sprache tatsächlich ändert — kein
 * Bedarf für automatisches Debounce-Speichern bei jedem `.set()`. */
const STORE_FILE = "settings.json";
const LANGUAGE_KEY = "language";

let storePromise: Promise<Store> | null = null;
/** Exportiert (Spec 0026, Abschnitt 4, Punkt 1: `risk_classifier_enabled`/
 * `risk_classifier_provider_id` gehören in denselben Store wie die
 * Sprache — s. `riskSettings.ts`), damit nicht zwei unabhängige
 * `load(STORE_FILE, ...)`-Aufrufe an verschiedenen Stellen entstehen. */
export function settingsStore(): Promise<Store> {
  if (!storePromise) {
    // Spec 0101, A16/Klarstellung 9: Im Passwort-Modus ist das
    // `store`-Plugin vor der Entsperrung **nicht registriert** — `load()`
    // scheitert dann mit „plugin store not found". Ein gescheitertes
    // Versprechen hier zwischengespeichert zu lassen hieße, dass der Store
    // für den ganzen Programmlauf kaputt bleibt: Nach dem Entsperren wären
    // Sprachwahl (Spec 0024) und Risiko-Einstellungen (Spec 0026) dauerhaft
    // nicht speicherbar, obwohl das Plugin längst da ist.
    storePromise = load(STORE_FILE, { autoSave: false }).catch((err: unknown) => {
      storePromise = null;
      throw err;
    });
  }
  return storePromise;
}

/** Erster Teil der Spec-0024-Abschnitt-4-Ermittlung: gespeicherte
 * Nutzerwahl hat Vorrang vor der System-Locale — einmal explizit gewählt,
 * soll sich die Sprache nicht bei jedem Start wieder an das System
 * anpassen. */
async function savedLanguage(): Promise<SupportedLanguage | null> {
  try {
    const store = await settingsStore();
    const value = await store.get<string>(LANGUAGE_KEY);
    return isSupportedLanguage(value) ? value : null;
  } catch (err) {
    console.error("Gespeicherte Sprache konnte nicht gelesen werden:", err);
    return null;
  }
}

/** Spec 0024, Abschnitt 4: "System-Locale wird ermittelt (Tauri-API), bei
 * Übereinstimmung mit einer unterstützten Sprache wird diese automatisch
 * gewählt, sonst Fallback auf Englisch." `locale()` liefert Werte wie
 * `"de-DE"`/`"en-US"`/`null` — nur der Sprachteil vor dem Trenner zählt. */
async function detectedSystemLanguage(): Promise<SupportedLanguage> {
  try {
    const raw = await systemLocale();
    const lang = raw?.split(/[-_]/)[0]?.toLowerCase();
    return isSupportedLanguage(lang) ? lang : DEFAULT_LANGUAGE;
  } catch (err) {
    console.error("System-Locale konnte nicht ermittelt werden:", err);
    return DEFAULT_LANGUAGE;
  }
}

async function resolveInitialLanguage(): Promise<SupportedLanguage> {
  const saved = await savedLanguage();
  if (saved) return saved;
  return detectedSystemLanguage();
}

/** Muss vor dem ersten Render abgeschlossen sein (s. `main.tsx`, awaitet
 * dies vor `createRoot(...).render(...)`) — vermeidet ein sichtbares
 * Umschalten der Sprache kurz nach dem Start.
 *
 * `forced` setzt die Sprache **ohne** `store` und `os` zu fragen: Spec 0101,
 * ADR 0095 §8 — im Passwort-Modus sind beide Plugins vor der Entsperrung
 * nicht registriert, und Klarstellung 9 verlangt genau das
 * („Einstellungsdateien sind vor der Entsperrung nicht lesbar"). Die
 * Startmasken nehmen stattdessen die Sprache aus `StartupStateDto`, die das
 * Backend einmal aus der Umgebung bestimmt hat (Spec 0071, A11a/A11b: EINE
 * Sprachwahl für alle Startdialoge eines Programmlaufs). Nach der
 * Entsperrung holt [`applyStoredLanguage`] die gespeicherte Wahl nach. */
export async function initI18n(forced?: SupportedLanguage): Promise<void> {
  const language = forced ?? (await resolveInitialLanguage());
  await i18next.use(initReactI18next).init({
    resources: {
      de: { common: de },
      en: { common: en },
    },
    lng: language,
    fallbackLng: DEFAULT_LANGUAGE,
    defaultNS: "common",
    interpolation: { escapeValue: false }, // React escaped bereits selbst
  });
}

/** Spec 0024, Abschnitt 4: "Auswahl in den Einstellungen jederzeit
 * änderbar, Wirkung sofort ohne Neustart" — `i18next.changeLanguage`
 * löst automatisch ein Re-Render aller `useTranslation`-Verbraucher aus. */
/** Spec 0101, ADR 0095 §8: **Nach** der Entsperrung gilt wieder die
 * gespeicherte Wahl aus Spec 0024.
 *
 * Wird genau dann gebraucht, wenn [`initI18n`] mit `forced` gelaufen ist —
 * dann steht die Sprache der Startmasken, aber nicht die des Nutzers. Ein
 * Fehler hier ist kein Grund, die App nicht zu zeigen: Es bleibt bei der
 * Sprache der Startmaske, und die ist aus derselben Umgebung bestimmt. */
export async function applyStoredLanguage(): Promise<void> {
  const language = await resolveInitialLanguage();
  if (language !== i18next.language) {
    await i18next.changeLanguage(language);
  }
}

export async function setLanguage(language: SupportedLanguage): Promise<void> {
  await i18next.changeLanguage(language);
  const store = await settingsStore();
  await store.set(LANGUAGE_KEY, language);
  await store.save();
}
