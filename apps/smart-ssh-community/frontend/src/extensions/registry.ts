/** Erweiterungs-Registry (Spec 0038, Abschnitt 4; aufgeräumt in Spec 0045):
 * registriert Beiträge zu Settings/Dokument-Aktionen, statt sie fest in den
 * jeweiligen App-Komponenten zu verdrahten — die Voraussetzung dafür, dass
 * ein künftiges Official-Binary zusätzliche Beiträge einspeisen kann, ohne
 * die Community-Frontend-Komponenten zu forken (analog zu `Wiring` auf der
 * Rust-Seite, s. `crates/app-shell/src/wiring.rs`).
 *
 * **Spec 0045, Abschnitt 2 — kein Registry-Typ ohne echten, gerenderten
 * Konsumenten.** Diese Registry deklarierte ursprünglich (Spec 0038) auch
 * `registerRoute`/`registerPanel`/`registerCommandPaletteAction` — keiner
 * davon hatte je einen tatsächlichen Renderer (keine Route-/Panel-
 * Rendering-Stelle, keine Command-Palette-UI), totes Vokabular, das ein
 * Feature unsichtbar werden lässt, das sich darauf verlässt (genau das
 * passierte dem privaten Word-Export über `registerCommandPaletteAction`
 * als Notlösung). Entfernt — ein entfernter Typ kann jederzeit wiederkommen,
 * wenn ein Feature ihn braucht, dann aber mit Rendering, nicht als leere
 * Deklaration. Verbleibende Typen: `registerSettingsSection` (gerendert in
 * `SettingsScreen`, Spec 0050 — vor dem dortigen Umbau auf die
 * zweispaltige Struktur in `AiProviderSettings`), `registerDocumentAction`
 * (gerendert in `ChatPanel`s Dokument-Karte, Spec 0045) und
 * `registerFirstRunNoticeExtension` (gerendert in `FirstRunNoticeScreen`,
 * Spec 0031, Abschnitt 6).
 *
 * **Scope-Hinweis:** Spec 0038 Abschnitt 4 skizziert dieses Paket unter
 * `frontend/packages/app` (Repo-Root, außerhalb der konkreten App). Das
 * würde ein echtes npm-Workspace-Setup voraussetzen — `react`/
 * `@tauri-apps/api` müssten aus einem Paket ohne eigenes `node_modules`
 * auflösbar sein, ohne dass eine zweite `react`-Kopie geladen wird (sonst
 * "Invalid hook call"). Das ist ein eigener, deutlich größerer Umbau
 * (Root-`package.json`, Workspace-Hoisting, CI-/Lockfile-Anpassungen), kein
 * Nebeneffekt dieses Schritts — deshalb lebt die Registry stattdessen als
 * gewöhnliches Modul unter `apps/smart-ssh-community/frontend/src/
 * extensions/`, importiert über normale relative Pfade. Funktional
 * identisch, nur ohne eigene Paketgrenze/-versionierung.
 *
 * Bewusst ein einfaches Modul-Singleton (kein React-Context): Beiträge
 * werden als Modul-Nebeneffekt registriert (import-time, s.
 * `registerBuiltinExtensions.ts`), bevor irgendeine Komponente rendert —
 * kein Provider/Consumer-Baum nötig für einen Zustand, der sich nach dem
 * App-Start nicht mehr ändert. */

import type { ComponentType } from "react";

export interface SettingsSectionContribution {
  id: string;
  component: ComponentType;
  /** Spec 0050, Abschnitt 1.2: Anzeige-Text für den linken Navigations-
   * Eintrag der zweispaltigen Settings-Struktur. Optional statt
   * Pflichtfeld — eine bestehende Registrierung (z. B. die private
   * Lizenz-Sektion der Official Edition) kompiliert dadurch unverändert
   * weiter; ohne `label` erscheint im Nav vorerst die rohe `id` als
   * Text, statt dass die Sektion komplett aus der Navigation verschwindet
   * (die Spec-0050-Invariante, die dieser Umbau nicht brechen darf). Ein
   * sprechendes `label` nachzutragen ist trotzdem empfehlenswert.
   *
   * Spec 0055, Teil 4: kann seitdem ENTWEDER ein fester Anzeigetext ODER
   * ein i18next-Übersetzungsschlüssel sein — diese Registry bleibt dabei
   * bewusst komplett i18n-unabhängig (reines Modul-Singleton, kein Zugriff
   * auf `i18next`/`react-i18next` hier), die Unterscheidung und Auflösung
   * passiert erst beim Rendern in `SettingsScreen.tsx`s
   * `resolveSectionLabel` (dortiger Doc-Kommentar erklärt das Wie: über
   * `i18next.exists(label)`, kein Präfix, kein zweites Feld). Für einen
   * Registrierenden ändert sich dadurch nichts an der Aufrufform — ob
   * `label` ein Schlüssel ist oder nicht, entscheidet einzig, ob er im
   * geladenen Sprachpaket existiert.
   */
  label?: string;
}

/** Kontext, den `ChatPanel`s Dokument-Karte (Spec 0012) einer registrierten
 * [`DocumentAction`] beim Klick übergibt — Markdown-Inhalt + Titel, analog
 * zum bestehenden Markdown-Export-Button (Spec 0045, Abschnitt 3). */
export interface DocumentContext {
  contentMarkdown: string;
  title: string;
}

/** Andockpunkt für Aktionen an einem KI-generierten Dokument (Spec 0045),
 * gerendert neben dem bestehenden Markdown-Export-Button in `ChatPanel`s
 * Dokument-Karte. Bewusst entitlement-agnostisch — die Registry kennt kein
 * `Feature`/`Entitlements` (das wäre Pro-Wissen im öffentlichen Repo); ob
 * eine Aktion gesperrt ist, entscheidet der Registrierende über `disabled`/
 * `disabledReason`. */
export interface DocumentAction {
  id: string;
  label: string;
  onInvoke: (doc: DocumentContext) => void;
  disabled?: boolean;
  disabledReason?: string;
}

/** Handler, den eine Erststart-Hinweis-Erweiterung über `onContinue`
 * anmeldet. Darf synchron oder asynchron sein; ein Wurf bzw. eine
 * Ablehnung wird protokolliert und blockiert nichts (Spec 0031,
 * Abschnitt 6). */
export type FirstRunNoticeContinueHandler = () => void | Promise<void>;

/** Kontext, den `FirstRunNoticeScreen` jeder registrierten Erweiterung
 * übergibt (Spec 0031, Abschnitt 6). Bewusst nur `onContinue`: Eine
 * Erweiterung sieht weder den Hinweistext noch die Pflicht-Checkbox noch,
 * ob "Weiter" aktiv ist, und kann nichts davon ändern. */
export interface FirstRunNoticeExtensionContext {
  /** Meldet einen Handler an, der genau einmal aufgerufen wird, nachdem
   * die Pflicht-Bestätigung gespeichert ist. Je Erweiterung gilt ein
   * Handler — ein erneuter Aufruf (z. B. bei jedem Render) ersetzt den
   * vorherigen, statt einen zweiten anzuhängen. */
  onContinue: (handler: FirstRunNoticeContinueHandler) => void;
}

/** Andockpunkt für optionale Inhalte im Erststart-Hinweis (Spec 0031,
 * Abschnitt 6; Registry-Regeln aus Spec 0045). Gerendert in
 * `FirstRunNoticeScreen`, je Erweiterung in einem eigenen, abgesetzten
 * Bereich unter der Pflicht-Bestätigung, aufsteigend nach `order`.
 *
 * **Regel für Autoren:** Jedes optionale Element (Checkbox, Feld, …) ist
 * beim Anzeigen aus bzw. leer. Nichts darf vorausgewählt sein — der
 * Nutzer entscheidet sich aktiv dafür, wie bei der Pflicht-Checkbox. */
export interface FirstRunNoticeExtension {
  id: string;
  order: number;
  Component: ComponentType<FirstRunNoticeExtensionContext>;
}

interface Registry {
  settingsSections: Map<string, SettingsSectionContribution>;
  documentActions: Map<string, DocumentAction>;
  firstRunNoticeExtensions: Map<string, FirstRunNoticeExtension>;
}

const registry: Registry = {
  settingsSections: new Map(),
  documentActions: new Map(),
  firstRunNoticeExtensions: new Map(),
};

/** Registriert (bzw. ersetzt bei gleicher `id`, z. B. bei einem
 * Hot-Module-Reload) einen Settings-Abschnitt. */
export function registerSettingsSection(section: SettingsSectionContribution): void {
  registry.settingsSections.set(section.id, section);
}

/** Registriert (bzw. ersetzt bei gleicher `id`) eine Dokument-Aktion (Spec
 * 0045) — gerendert in `ChatPanel`s Dokument-Karte, neben dem bestehenden
 * Markdown-Export. */
export function registerDocumentAction(action: DocumentAction): void {
  registry.documentActions.set(action.id, action);
}

export function listSettingsSections(): SettingsSectionContribution[] {
  return Array.from(registry.settingsSections.values());
}

/** spec-reviewer-Fund (Review dieses Schritts): rein statisch — kein
 * Subscription-/Change-Notification-Mechanismus. `ChatPanel` liest diese
 * Liste bei jedem Render neu, reagiert aber nicht selbst auf eine
 * Registrierung/ein `disabled`-Umschalten, das NACH dem ersten Render der
 * jeweiligen Dokument-Karte passiert — ein solcher Aufrufer muss also vor
 * dem ersten Render registriert haben (wie `registerBuiltinExtensions.ts`
 * es für `registerSettingsSection` tut) bzw. selbst für einen Re-Render
 * sorgen (z. B. über ein von außen beobachtetes Entitlement-Update, das
 * die aufrufende Komponente ohnehin neu rendert). Kein Verstoß gegen Spec
 * 0045 (die keinen Notify-Mechanismus verlangt), aber genau die Art
 * "sieht aus wie ein Andockpunkt, verhält sich aber überraschend" von
 * Falle, die diese Spec eigentlich vermeiden will — deshalb hier explizit
 * festgehalten statt stillschweigend vorausgesetzt. */
export function listDocumentActions(): DocumentAction[] {
  return Array.from(registry.documentActions.values());
}

/** Registriert (bzw. ersetzt bei gleicher `id`) eine Erweiterung des
 * Erststart-Hinweises (Spec 0031, Abschnitt 6). Wie die übrigen Typen vor
 * dem ersten Render zu registrieren — der Hinweis liest die Liste beim
 * Öffnen. */
export function registerFirstRunNoticeExtension(extension: FirstRunNoticeExtension): void {
  registry.firstRunNoticeExtensions.set(extension.id, extension);
}

/** Registrierte Erststart-Hinweis-Erweiterungen, aufsteigend nach `order`;
 * bei gleichem `order` nach `id`, damit die Reihenfolge nicht von der
 * Import-Reihenfolge abhängt. */
export function listFirstRunNoticeExtensions(): FirstRunNoticeExtension[] {
  return Array.from(registry.firstRunNoticeExtensions.values()).sort(
    (a, b) => a.order - b.order || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

/** Nur für Tests: setzt die Registry zwischen Testfällen zurück, damit
 * Registrierungen aus einem Test nicht in den nächsten durchsickern. */
export function resetRegistryForTests(): void {
  registry.settingsSections.clear();
  registry.documentActions.clear();
  registry.firstRunNoticeExtensions.clear();
}
