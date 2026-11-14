import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import "../extensions/registerBuiltinExtensions";
import {
  listSettingsSections,
  listSettingsSectionsForCategory,
  subscribeSettingsSections,
} from "../extensions/registry";
import { resolveSectionLabel } from "../lib/resolveSectionLabel";
import { AboutSettings } from "./AboutSettings";
import { AiProviderSettings } from "./AiProviderSettings";
import { BackupSettings } from "./BackupSettings";
import { DiagnosticsSettings } from "./DiagnosticsSettings";
import { FileTypeSettings } from "./FileTypeSettings";
import { LanguageSettings } from "./LanguageSettings";
import { MasterPasswordSettings } from "./MasterPasswordSettings";
import { VoiceInputSettings } from "./VoiceInputSettings";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { Button, DialogFrame, ErrorBoundary } from "./ui";

interface SettingsScreenProps {
  onClose: () => void;
  /** Element that opened the dialog; focus returns there on close. Falls back to the element focused at mount. */
  returnFocusTo?: HTMLElement | null;
  onProvidersChanged: () => void;
}

const AI_PROVIDER_CATEGORY_ID = "ai-provider";
const DISPLAY_LANGUAGE_CATEGORY_ID = "display-language";
const DIAGNOSTICS_CATEGORY_ID = "diagnostics";
const FILE_TYPES_CATEGORY_ID = "file-types";
/** Spec 0101, A18: Wo der Schlüssel zur verschlüsselten Datenbank liegt. */
const MASTER_PASSWORD_CATEGORY_ID = "master-password";
/** Spec 0109, T1. */
const DATA_BACKUP_CATEGORY_ID = "data-backup";
const VOICE_INPUT_CATEGORY_ID = "voice-input";
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
export function SettingsScreen({ onClose, onProvidersChanged, returnFocusTo }: SettingsScreenProps) {
  const { t, i18n } = useTranslation();

  const builtinCategories: NavCategory[] = [
    { id: AI_PROVIDER_CATEGORY_ID, label: t("settings.categories.aiProvider") },
    { id: DISPLAY_LANGUAGE_CATEGORY_ID, label: t("settings.categories.displayLanguage") },
    { id: DIAGNOSTICS_CATEGORY_ID, label: t("settings.categories.diagnostics") },
    // Spec 0054, Teil 5: "passt zur neuen zweispaltigen Settings-Struktur,
    // 'Dateien'- oder 'Editor'-Sektion" — "Dateien" gewählt, näher am
    // Dateibrowser-Wortlaut ("Dateityp", nicht "Editor-Konfiguration").
    { id: FILE_TYPES_CATEGORY_ID, label: t("settings.categories.fileTypes") },
    // Spec 0101, A18: „Die Einstellungen zeigen den aktiven Modus." Dazu
    // der Wechsel in beide Richtungen (A13/A15).
    { id: MASTER_PASSWORD_CATEGORY_ID, label: t("settings.categories.masterPassword") },
    // Spec 0050, Abschnitt 1.1 schlägt "Über" als letzten Eintrag vor,
    // nach einer möglichen (nur in der Official-Edition registrierten)
    // "Lizenz"-Kategorie — die kommt aber über `registeredCategories`
    // unten, IMMER nach allen `builtinCategories` (s. Zusammenführung
    // weiter unten). "Über" landet deshalb hier nur als letzter
    // eingebauter Eintrag, nicht strikt nach einer eventuellen
    // Lizenz-Sektion; eine vollständige Neuordnung dafür wäre für diese
    // reine Anzeige-Ergänzung unverhältnismäßig.
    { id: DATA_BACKUP_CATEGORY_ID, label: t("settings.categories.dataAndBackup") },
    { id: VOICE_INPUT_CATEGORY_ID, label: t("settings.categories.voiceInput") },
    { id: ABOUT_CATEGORY_ID, label: t("settings.categories.about") },
  ];

  // Spec 0050, Abschnitt 1.2: registrierte Sektionen bekommen einen eigenen
  // Navigationseintrag, exakt wie eine eingebaute Kategorie. `label` ist
  // optional (Spec 0050, Abschnitt 1.3 — Rückwärtskompatibilität für eine
  // bestehende Registrierung ohne `label`, z. B. die private Lizenz-
  // Sektion vor einer Anpassung), fällt in dem Fall auf die `id` zurück.
  //
  // Spec 0055, Teil 4: `label` kann jetzt ZUSÄTZLICH ein
  // Übersetzungsschlüssel sein (s. `resolveSectionLabel`) — das eingebaute
  // "chat-retention"/"mcp-server" nutzen das seit diesem Schritt, damit in
  // der englischen UI nicht die festen deutschen Strings aus
  // `registerBuiltinExtensions.ts` erscheinen.
  const registeredSections = useSyncExternalStore(
    subscribeSettingsSections,
    listSettingsSections,
  );
  // Issue #1311: a section whose `category` matches no built-in category is
  // not lost; it falls back to its own top-level entry (and a warning).
  const builtinIds = new Set(builtinCategories.map((c) => c.id));
  const isTopLevel = (s: { category?: string }) => !s.category || !builtinIds.has(s.category);
  const unknownCategories = registeredSections
    .filter((s) => s.category && isTopLevel(s))
    .map((s) => ({ id: s.id, category: s.category as string }));
  // Stable dependency: warn once per change of the set, not per render.
  const unknownCategoryKey = JSON.stringify(unknownCategories.map((u) => [u.id, u.category]));
  useEffect(() => {
    const pairs = JSON.parse(unknownCategoryKey) as [string, string][];
    for (const [id, category] of pairs) {
      console.warn(
        `Settings section "${id}" names unknown category "${category}"; showing it as a top-level entry.`,
      );
    }
  }, [unknownCategoryKey]);
  const registeredCategories: NavCategory[] = registeredSections
    .filter(isTopLevel)
    .map(({ id, label }) => ({
    id,
    label: resolveSectionLabel(label, id, i18n, t),
  }));

  const categories = [...builtinCategories, ...registeredCategories];

  const [activeId, setActiveId] = useState<string>(categories[0]?.id ?? AI_PROVIDER_CATEGORY_ID);
  const active = categories.find((c) => c.id === activeId) ?? categories[0];

  // Issue #586, Spec 0113 §5d: below 640 px the dialog is full-screen and shows
  // either the section list or one section (list -> detail with a Back button).
  const narrow = useIsNarrow();
  const [narrowView, setNarrowView] = useState<"list" | "detail">("list");
  const showList = !narrow || narrowView === "list";
  const showDetail = !narrow || narrowView === "detail";
  const backRef = useRef<HTMLButtonElement>(null);
  const viewChanged = useRef(false);
  useEffect(() => {
    if (!narrow) return;
    if (!viewChanged.current) {
      viewChanged.current = true;
      return;
    }
    (narrowView === "detail" ? backRef.current : activeNavRef.current)?.focus();
  }, [narrow, narrowView]);

  // Issue #385 (docs/ui-guidelines.md, "Keyboard"): `DialogFrame` handles Escape,
  // Tab wrap and focus return; focus starts on the active category. It yields to
  // the dialogs opened from inside the settings (licenses, master password).
  const titleId = useId();
  const activeNavRef = useRef<HTMLButtonElement>(null);

  return (
    <DialogFrame
      layer="app"
      bare
      yieldToNested
      labelledBy={titleId}
      onClose={onClose}
      returnFocusTo={returnFocusTo}
      initialFocusRef={activeNavRef}
      panelClassName="flex h-[85vh] w-full max-w-3xl overflow-hidden rounded-lg bg-surface-raised shadow-xl narrow:h-full narrow:max-w-none narrow:flex-col narrow:rounded-none"
    >
        {showList && (
        <nav className="flex w-52 shrink-0 flex-col overflow-y-auto border-r border-border bg-surface/40 narrow:min-h-0 narrow:w-full narrow:flex-1 narrow:border-r-0">
          <div className="flex items-center justify-between">
            <h2 id={titleId} className="font-heading px-4 pt-4 pb-2 text-lg font-semibold tracking-wide text-fg">
              {t("settings.title")}
            </h2>
            {narrow && (
              <Button variant="ghost" size="sm" className="mr-2 min-h-11 min-w-11" onClick={onClose} aria-label={t("common.close")}>
                ✕
              </Button>
            )}
          </div>
          <ul className="flex-1">
            {categories.map((category) => (
              <li key={category.id}>
                <Button variant="bare"
                  type="button"
                  ref={category.id === active?.id ? activeNavRef : undefined}
                  onClick={() => {
                    setActiveId(category.id);
                    setNarrowView("detail");
                  }}
                  aria-current={category.id === active?.id ? "page" : undefined}
                  className={`w-full px-4 py-2 text-left text-sm narrow:min-h-11 focus-ring-inset ${
                    category.id === active?.id
                      ? "bg-accent text-surface-sunken"
                      : "text-fg-muted hover:bg-surface-raised"
                  }`}
                >
                  {category.label}
                </Button>
              </li>
            ))}
          </ul>
        </nav>
        )}

        {showDetail && (
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="flex items-center justify-between gap-2 border-b border-border px-6 py-4 narrow:px-2 narrow:py-2">
            {narrow && (
              <>
                <h2 id={titleId} className="sr-only">
                  {t("settings.title")}
                </h2>
                <Button
                  ref={backRef}
                  variant="ghost"
                  size="sm"
                  className="min-h-11 shrink-0"
                  onClick={() => setNarrowView("list")}
                >
                  ← {t("settings.backToList")}
                </Button>
              </>
            )}
            <h3 className="font-heading min-w-0 flex-1 truncate text-base font-semibold tracking-wide text-fg narrow:text-center">
              {active?.label}
            </h3>
            <Button variant="ghost" size="sm" className="narrow:min-h-11 narrow:min-w-11" onClick={onClose} aria-label={t("common.close")}>
              ✕
            </Button>
          </div>
          <div className="min-w-0 flex-1 overflow-y-auto p-6 narrow:p-3">
            {active?.id === AI_PROVIDER_CATEGORY_ID && (
              <AiProviderSettings onProvidersChanged={onProvidersChanged} />
            )}
            {active?.id === DISPLAY_LANGUAGE_CATEGORY_ID && <LanguageSettings />}
            {active?.id === DIAGNOSTICS_CATEGORY_ID && <DiagnosticsSettings />}
            {active?.id === FILE_TYPES_CATEGORY_ID && <FileTypeSettings />}
            {active?.id === MASTER_PASSWORD_CATEGORY_ID && <MasterPasswordSettings />}
            {active?.id === DATA_BACKUP_CATEGORY_ID && <BackupSettings />}
            {active?.id === VOICE_INPUT_CATEGORY_ID && <VoiceInputSettings />}
            {active?.id === ABOUT_CATEGORY_ID && <AboutSettings />}
            {active && builtinIds.has(active.id) &&
              listSettingsSectionsForCategory(active.id).map(({ id, component: Section }) => (
                <div key={id} className="mt-6 min-w-0">
                  <ErrorBoundary id={`settings-section:${id}`}>
                    <Section />
                  </ErrorBoundary>
                </div>
              ))}
            {registeredSections.map(
              ({ id, category, component: Section }) =>
                isTopLevel({ category }) && active?.id === id && (
                  <ErrorBoundary key={id} id={`settings-section:${id}`}>
                    <Section />
                  </ErrorBoundary>
                ),
            )}
          </div>
        </div>
        )}
    </DialogFrame>
  );
}
