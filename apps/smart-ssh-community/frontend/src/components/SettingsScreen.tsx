import { useState } from "react";
import { useTranslation } from "react-i18next";
import "../extensions/registerBuiltinExtensions";
import { listSettingsSections } from "../extensions/registry";
import { AboutSettings } from "./AboutSettings";
import { AiProviderSettings } from "./AiProviderSettings";
import { DiagnosticsSettings } from "./DiagnosticsSettings";
import { LanguageSettings } from "./LanguageSettings";

interface SettingsScreenProps {
  onClose: () => void;
  onProvidersChanged: () => void;
}

const AI_PROVIDER_CATEGORY_ID = "ai-provider";
const DISPLAY_LANGUAGE_CATEGORY_ID = "display-language";
const DIAGNOSTICS_CATEGORY_ID = "diagnostics";
const ABOUT_CATEGORY_ID = "about";

interface NavCategory {
  id: string;
  label: string;
}

/**
 * Spec 0050, Teil 1: zweispaltige Settings-Struktur (Navigation links,
 * Inhalt rechts) statt der bisherigen ungegliederten langen Liste in
 * `AiProviderSettings`.
 *
 * **Spec 0050, Abschnitt 1.2 — kritisch**: über `registerSettingsSection`
 * (Spec 0038) registrierte Sektionen (der einzige tatsächlich gerenderte
 * Registry-Kontributionspunkt — die Official Edition klinkt darüber ihre
 * Lizenz-Sektion ein) erscheinen hier gleichwertig zu den eingebauten
 * Kategorien: derselbe Navigations-Eintrag links, derselbe Inhaltsbereich
 * rechts. Ein hartcodierter `switch` nur über die eingebauten IDs hätte
 * genau das kaputt gemacht, was diese Spec ausdrücklich verbietet.
 *
 * Reihenfolge: eingebaute Kategorien zuerst (feste, sinnvolle Abfolge),
 * danach registrierte Sektionen in Registrierungsreihenfolge (die Registry
 * kennt keine Priorität, s. `extensions/registry.ts`) — "MCP-Server" und
 * "Sitzungen & Daten" sind seit Spec 0038 selbst schon registrierte
 * Sektionen (`registerBuiltinExtensions.ts`), keine neuen eingebauten
 * Kategorien.
 */
export function SettingsScreen({ onClose, onProvidersChanged }: SettingsScreenProps) {
  const { t } = useTranslation();

  const builtinCategories: NavCategory[] = [
    { id: AI_PROVIDER_CATEGORY_ID, label: t("settings.categories.aiProvider") },
    { id: DISPLAY_LANGUAGE_CATEGORY_ID, label: t("settings.categories.displayLanguage") },
    { id: DIAGNOSTICS_CATEGORY_ID, label: t("settings.categories.diagnostics") },
    // Spec 0050, Abschnitt 1.1 schlägt "Über" als letzten Eintrag vor,
    // nach einer möglichen (nur in der Official-Edition registrierten)
    // "Lizenz"-Kategorie — die kommt aber über `registeredCategories`
    // unten, IMMER nach allen `builtinCategories` (s. Zusammenführung
    // weiter unten). "Über" landet deshalb hier nur als letzter
    // eingebauter Eintrag, nicht strikt nach einer eventuellen
    // Lizenz-Sektion; eine vollständige Neuordnung dafür wäre für diese
    // reine Anzeige-Ergänzung unverhältnismäßig.
    { id: ABOUT_CATEGORY_ID, label: t("settings.categories.about") },
  ];

  // Spec 0050, Abschnitt 1.2: registrierte Sektionen bekommen einen eigenen
  // Navigationseintrag, exakt wie eine eingebaute Kategorie. `label` ist
  // optional (Spec 0050, Abschnitt 1.3 — Rückwärtskompatibilität für eine
  // bestehende Registrierung ohne `label`, z. B. die private Lizenz-
  // Sektion vor einer Anpassung), fällt in dem Fall auf die `id` zurück.
  const registeredSections = listSettingsSections();
  const registeredCategories: NavCategory[] = registeredSections.map(({ id, label }) => ({
    id,
    label: label ?? id,
  }));

  const categories = [...builtinCategories, ...registeredCategories];

  const [activeId, setActiveId] = useState<string>(categories[0]?.id ?? AI_PROVIDER_CATEGORY_ID);
  const active = categories.find((c) => c.id === activeId) ?? categories[0];

  return (
    <div className="fixed inset-0 flex items-center justify-center bg-black/50 p-4">
      <div className="flex h-[85vh] w-full max-w-3xl overflow-hidden rounded-lg bg-slate-800 shadow-xl">
        <nav className="flex w-52 shrink-0 flex-col overflow-y-auto border-r border-slate-700 bg-slate-900/40">
          <h2 className="font-heading px-4 pt-4 pb-2 text-lg font-semibold tracking-wide text-slate-100">
            {t("settings.title")}
          </h2>
          <ul className="flex-1">
            {categories.map((category) => (
              <li key={category.id}>
                <button
                  type="button"
                  onClick={() => setActiveId(category.id)}
                  aria-current={category.id === active?.id ? "page" : undefined}
                  className={`w-full px-4 py-2 text-left text-sm ${
                    category.id === active?.id
                      ? "bg-indigo-600 text-white"
                      : "text-slate-300 hover:bg-slate-800"
                  }`}
                >
                  {category.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex items-center justify-between border-b border-slate-700 px-6 py-4">
            <h3 className="font-heading text-base font-semibold tracking-wide text-slate-100">
              {active?.label}
            </h3>
            <button
              type="button"
              onClick={onClose}
              className="text-slate-400 hover:text-slate-100"
              aria-label={t("common.close")}
            >
              ✕
            </button>
          </div>
          <div className="flex-1 overflow-y-auto p-6">
            {active?.id === AI_PROVIDER_CATEGORY_ID && (
              <AiProviderSettings onProvidersChanged={onProvidersChanged} />
            )}
            {active?.id === DISPLAY_LANGUAGE_CATEGORY_ID && <LanguageSettings />}
            {active?.id === DIAGNOSTICS_CATEGORY_ID && <DiagnosticsSettings />}
            {active?.id === ABOUT_CATEGORY_ID && <AboutSettings />}
            {registeredSections.map(
              ({ id, component: Section }) => active?.id === id && <Section key={id} />,
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
