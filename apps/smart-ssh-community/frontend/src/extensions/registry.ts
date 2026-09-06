/** Erweiterungs-Registry (Spec 0038, Abschnitt 4): registriert Beiträge zu
 * Route/Panel/Settings/Command-Palette, statt sie fest in den jeweiligen
 * App-Komponenten zu verdrahten — die Voraussetzung dafür, dass ein
 * künftiges Official-Binary zusätzliche Beiträge einspeisen kann, ohne die
 * Community-Frontend-Komponenten zu forken (analog zu `Wiring` auf der
 * Rust-Seite, s. `crates/app-shell/src/wiring.rs`).
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
 * identisch (dieselben vier `register*`-Funktionen, derselbe
 * `useEntitlements`-Hook), nur ohne eigene Paketgrenze/-versionierung.
 *
 * Bewusst ein einfaches Modul-Singleton (kein React-Context): Beiträge
 * werden als Modul-Nebeneffekt registriert (import-time, s.
 * `registerBuiltinExtensions.ts`), bevor irgendeine Komponente rendert —
 * kein Provider/Consumer-Baum nötig für einen Zustand, der sich nach dem
 * App-Start nicht mehr ändert. */

import type { ComponentType } from "react";

export interface RouteContribution {
  id: string;
  path: string;
  component: ComponentType;
}

export interface PanelContribution {
  id: string;
  component: ComponentType;
}

export interface SettingsSectionContribution {
  id: string;
  component: ComponentType;
}

export interface CommandPaletteActionContribution {
  id: string;
  label: string;
  run: () => void;
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
  routes: Map<string, RouteContribution>;
  panels: Map<string, PanelContribution>;
  settingsSections: Map<string, SettingsSectionContribution>;
  commandPaletteActions: Map<string, CommandPaletteActionContribution>;
  documentActions: Map<string, DocumentAction>;
}

const registry: Registry = {
  routes: new Map(),
  panels: new Map(),
  settingsSections: new Map(),
  commandPaletteActions: new Map(),
  documentActions: new Map(),
};

/** Registriert (bzw. ersetzt bei gleicher `id`, z. B. bei einem
 * Hot-Module-Reload) eine Route. */
export function registerRoute(route: RouteContribution): void {
  registry.routes.set(route.id, route);
}

export function registerPanel(panel: PanelContribution): void {
  registry.panels.set(panel.id, panel);
}

export function registerSettingsSection(section: SettingsSectionContribution): void {
  registry.settingsSections.set(section.id, section);
}

export function registerCommandPaletteAction(
  action: CommandPaletteActionContribution,
): void {
  registry.commandPaletteActions.set(action.id, action);
}

/** Registriert (bzw. ersetzt bei gleicher `id`) eine Dokument-Aktion (Spec
 * 0045) — gerendert in `ChatPanel`s Dokument-Karte, neben dem bestehenden
 * Markdown-Export. */
export function registerDocumentAction(action: DocumentAction): void {
  registry.documentActions.set(action.id, action);
}

export function listRoutes(): RouteContribution[] {
  return Array.from(registry.routes.values());
}

export function listPanels(): PanelContribution[] {
  return Array.from(registry.panels.values());
}

export function listSettingsSections(): SettingsSectionContribution[] {
  return Array.from(registry.settingsSections.values());
}

export function listCommandPaletteActions(): CommandPaletteActionContribution[] {
  return Array.from(registry.commandPaletteActions.values());
}

export function listDocumentActions(): DocumentAction[] {
  return Array.from(registry.documentActions.values());
}

/** Nur für Tests: setzt die Registry zwischen Testfällen zurück, damit
 * Registrierungen aus einem Test nicht in den nächsten durchsickern. */
export function resetRegistryForTests(): void {
  registry.routes.clear();
  registry.panels.clear();
  registry.settingsSections.clear();
  registry.commandPaletteActions.clear();
  registry.documentActions.clear();
}
