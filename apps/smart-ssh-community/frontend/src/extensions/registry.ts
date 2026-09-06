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
 * `AiProviderSettings`) und `registerDocumentAction` (gerendert in
 * `ChatPanel`s Dokument-Karte, Spec 0045).
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

interface Registry {
  settingsSections: Map<string, SettingsSectionContribution>;
  documentActions: Map<string, DocumentAction>;
}

const registry: Registry = {
  settingsSections: new Map(),
  documentActions: new Map(),
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

/** Nur für Tests: setzt die Registry zwischen Testfällen zurück, damit
 * Registrierungen aus einem Test nicht in den nächsten durchsickern. */
export function resetRegistryForTests(): void {
  registry.settingsSections.clear();
  registry.documentActions.clear();
}
