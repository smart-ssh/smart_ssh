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
 * werden üblicherweise als Modul-Nebeneffekt registriert (import-time, s.
 * `registerBuiltinExtensions.ts`), bevor irgendeine Komponente rendert.
 * Eine Registrierung darf aber auch später erfolgen (z. B. nach einer
 * asynchronen Prüfung): Für Settings-Abschnitte gibt es dafür
 * `subscribeSettingsSections` (kompatibel mit `useSyncExternalStore`), damit
 * ein geöffnetes Einstellungsfenster den neuen Abschnitt ohne erneutes
 * Öffnen zeigt. */

import type { ComponentType, ReactNode } from "react";

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
 * `Feature`/`Entitlements`, damit sie reiner Andockpunkt bleibt; ob eine
 * Aktion gesperrt ist, entscheidet der Registrierende über `disabled`/
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

/** Spec 0038, Abschnitt 4: Kontext für einen Abschnitt im Server-Formular.
 * `serverId` ist immer gesetzt — für einen neuen, ungespeicherten Server
 * wird kein Abschnitt gerendert. `dirty` ist `true`, solange das Formular
 * ungespeicherte Änderungen hat. */
export interface ServerFormSectionContext {
  serverId: string;
  dirty: boolean;
}

/** Built-in tabs of the server settings (issue #608). */
export type ServerFormTabId = "general" | "connection" | "security" | "notes";

export interface ServerFormSectionContribution {
  id: string;
  /** Built-in tab the section is rendered at the bottom of. Default
   * `"general"`. */
  tab?: ServerFormTabId;
  component: ComponentType<ServerFormSectionContext>;
}

/** Issue #608 (Spec 0038, Abschnitt 4): an extension's own tab in the
 * settings of a saved server. `label` is plain text or an i18n key (like
 * settings sections). The component only gets the server id. */
export interface ServerSettingsTabContribution {
  id: string;
  label: string;
  order?: number;
  component: ComponentType<{ serverId: string }>;
}

/** Issue #611 (Spec 0038, Abschnitt 4): a small item in the header of a
 * connected (non-MCP) session, between the status and the reconnect /
 * Disconnect controls. The component gets ids only (no credentials, no
 * message content). `chatSessionId` is `null` for the local pseudo-server
 * and no-persist sessions. */
export interface SessionHeaderItemContribution {
  id: string;
  order?: number;
  component: ComponentType<{
    runtimeSessionId: string;
    serverId: string;
    chatSessionId: string | null;
  }>;
}

/** Kontext für einen Abschnitt im Gruppen-Formular (nur bestehende Gruppen). */
export interface GroupFormSectionContext {
  groupId: string;
  dirty: boolean;
}

export interface GroupFormSectionContribution {
  id: string;
  component: ComponentType<GroupFormSectionContext>;
}

/** Eigene Ansicht im Verwalten-Tab: erscheint als zusätzlicher Eintrag
 * unterhalb des Server-Baums in der Seitenleiste. `label` ist ein fester
 * Text oder ein i18next-Schlüssel (wie bei Settings-Abschnitten), `icon`
 * ein beliebiges React-Element (dekorativ). */
export interface ManageViewContribution {
  id: string;
  label: string;
  icon?: ReactNode;
  component: ComponentType;
}

/** Issue #471 (Spec 0038, Spec 0110): ein ausstehender Anhang im Eingabefeld.
 * Nur eine undurchsichtige `id` — nie ein Dateipfad im Webview. */
export interface ComposerAttachment {
  id: string;
  name: string;
  size: number;
  kind: "text" | "image" | "pdf";
  /** Zeigt am Chip den Hinweis „unredigiert gesendet". */
  unredactedHint?: boolean;
}

/** Was eine Composer-Aktion/ein Drop-Handler beim Aufruf bekommt. */
export interface ComposerContext {
  sessionId: string;
  addAttachment: (attachment: ComposerAttachment) => void;
  removeAttachment: (id: string) => void;
}

/** Andockpunkt neben dem Senden-Button des Chat-Eingabefelds (Issue #471).
 * Wie `DocumentAction` entitlement-agnostisch: Sperrzustand über
 * `disabled`/`disabledReason`. `icon` ist ein beliebiges React-Element. */
export interface ComposerAction {
  id: string;
  label: string;
  icon: ReactNode;
  onInvoke: (ctx: ComposerContext) => void | Promise<void>;
  disabled?: boolean;
  disabledReason?: string;
}

/** Wird bei einem nativen Ablegen von Dateien auf den Chat aufgerufen. Das
 * Webview reicht keine Pfade weiter; der Handler holt sie im Backend ab
 * (ADR 0113). */
export type ComposerDropHandler = (ctx: ComposerContext) => void | Promise<void>;

interface Registry {
  composerActions: Map<string, ComposerAction>;
  composerDropHandlers: Map<string, ComposerDropHandler>;
  serverFormSections: Map<string, ServerFormSectionContribution>;
  serverSettingsTabs: Map<string, ServerSettingsTabContribution>;
  sessionHeaderItems: Map<string, SessionHeaderItemContribution>;
  groupFormSections: Map<string, GroupFormSectionContribution>;
  manageViews: Map<string, ManageViewContribution>;
  settingsSections: Map<string, SettingsSectionContribution>;
  documentActions: Map<string, DocumentAction>;
  firstRunNoticeExtensions: Map<string, FirstRunNoticeExtension>;
}

const registry: Registry = {
  composerActions: new Map(),
  composerDropHandlers: new Map(),
  serverFormSections: new Map(),
  serverSettingsTabs: new Map(),
  sessionHeaderItems: new Map(),
  groupFormSections: new Map(),
  manageViews: new Map(),
  settingsSections: new Map(),
  documentActions: new Map(),
  firstRunNoticeExtensions: new Map(),
};

/** Registriert (bzw. ersetzt bei gleicher `id`, z. B. bei einem
 * Hot-Module-Reload) einen Settings-Abschnitt. */
export function registerSettingsSection(section: SettingsSectionContribution): void {
  registry.settingsSections.set(section.id, section);
  notifySettingsSectionsChanged();
}

type SettingsSectionsListener = () => void;

const settingsSectionListeners = new Set<SettingsSectionsListener>();

/** Stabiler Schnappschuss: ändert sich nur bei einer Änderung der Liste,
 * damit `listSettingsSections()` als `getSnapshot` taugt. */
let settingsSectionsSnapshot: SettingsSectionContribution[] = [];

function notifySettingsSectionsChanged(): void {
  settingsSectionsSnapshot = Array.from(registry.settingsSections.values());
  // Kopie, damit ein Listener sich während des Aufrufs abmelden darf.
  for (const listener of Array.from(settingsSectionListeners)) {
    listener();
  }
}

/** Meldet einen Listener an, der nach jeder Änderung der Settings-Abschnitte
 * (neue oder ersetzte `id`) aufgerufen wird. Gibt die Abmelde-Funktion
 * zurück. */
export function subscribeSettingsSections(listener: () => void): () => void {
  settingsSectionListeners.add(listener);
  return () => {
    settingsSectionListeners.delete(listener);
  };
}

/** Registriert (bzw. ersetzt bei gleicher `id`) eine Dokument-Aktion (Spec
 * 0045) — gerendert in `ChatPanel`s Dokument-Karte, neben dem bestehenden
 * Markdown-Export. */
export function registerDocumentAction(action: DocumentAction): void {
  registry.documentActions.set(action.id, action);
}

/** Registrierte Settings-Abschnitte. Zwischen zwei Änderungen dieselbe
 * Array-Referenz (nicht verändern). */
export function listSettingsSections(): SettingsSectionContribution[] {
  return settingsSectionsSnapshot;
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

// Server-/Gruppen-Formular-Abschnitte und Verwalten-Ansichten teilen sich
// einen Benachrichtigungskanal (`subscribeFormSlots`, kompatibel mit
// `useSyncExternalStore`), damit eine spätere Registrierung ohne Neuladen
// erscheint. Die Schnappschüsse ändern sich nur bei einer Änderung.
const formSlotListeners = new Set<() => void>();
let serverFormSectionsSnapshot: ServerFormSectionContribution[] = [];
let serverSettingsTabsSnapshot: ServerSettingsTabContribution[] = [];
let sessionHeaderItemsSnapshot: SessionHeaderItemContribution[] = [];
let groupFormSectionsSnapshot: GroupFormSectionContribution[] = [];
let manageViewsSnapshot: ManageViewContribution[] = [];

function notifyFormSlotsChanged(): void {
  serverFormSectionsSnapshot = Array.from(registry.serverFormSections.values());
  // Map insertion order is the registration order; a replace keeps its slot.
  // Array.prototype.sort is stable, so equal `order` keeps that order.
  serverSettingsTabsSnapshot = Array.from(registry.serverSettingsTabs.values()).sort(
    (a, b) => (a.order ?? 0) - (b.order ?? 0),
  );
  sessionHeaderItemsSnapshot = Array.from(registry.sessionHeaderItems.values()).sort(
    (a, b) => (a.order ?? 0) - (b.order ?? 0),
  );
  groupFormSectionsSnapshot = Array.from(registry.groupFormSections.values());
  manageViewsSnapshot = Array.from(registry.manageViews.values());
  for (const listener of Array.from(formSlotListeners)) listener();
}

export function subscribeFormSlots(listener: () => void): () => void {
  formSlotListeners.add(listener);
  return () => {
    formSlotListeners.delete(listener);
  };
}

/** Registriert (bzw. ersetzt bei gleicher `id`) einen Abschnitt im
 * Server-Formular (normale Server und lokaler Pseudo-Server). Ob er
 * gesperrt ist, entscheidet der Registrierende in der Komponente (die
 * Registry kennt keine Entitlements, s. `DocumentAction`). */
export function registerServerFormSection(section: ServerFormSectionContribution): void {
  registry.serverFormSections.set(section.id, section);
  notifyFormSlotsChanged();
}

/** Registers (or replaces, same `id`) a tab in the server settings. */
export function registerServerSettingsTab(tab: ServerSettingsTabContribution): void {
  registry.serverSettingsTabs.set(tab.id, tab);
  notifyFormSlotsChanged();
}

export function unregisterServerSettingsTab(id: string): void {
  if (registry.serverSettingsTabs.delete(id)) notifyFormSlotsChanged();
}

/** Registers (or replaces, same `id`) an item in the session header. */
export function registerSessionHeaderItem(item: SessionHeaderItemContribution): void {
  registry.sessionHeaderItems.set(item.id, item);
  notifyFormSlotsChanged();
}

export function unregisterSessionHeaderItem(id: string): void {
  if (registry.sessionHeaderItems.delete(id)) notifyFormSlotsChanged();
}

export function registerGroupFormSection(section: GroupFormSectionContribution): void {
  registry.groupFormSections.set(section.id, section);
  notifyFormSlotsChanged();
}

export function registerManageView(view: ManageViewContribution): void {
  registry.manageViews.set(view.id, view);
  notifyFormSlotsChanged();
}

/** Zwischen zwei Änderungen dieselbe Array-Referenz (nicht verändern). */
export function listServerFormSections(): ServerFormSectionContribution[] {
  return serverFormSectionsSnapshot;
}
export function listServerSettingsTabs(): ServerSettingsTabContribution[] {
  return serverSettingsTabsSnapshot;
}
export function listSessionHeaderItems(): SessionHeaderItemContribution[] {
  return sessionHeaderItemsSnapshot;
}
export function listGroupFormSections(): GroupFormSectionContribution[] {
  return groupFormSectionsSnapshot;
}
export function listManageViews(): ManageViewContribution[] {
  return manageViewsSnapshot;
}

/** Nur für Tests: setzt die Registry zwischen Testfällen zurück, damit
 * Registrierungen aus einem Test nicht in den nächsten durchsickern. */
export function resetRegistryForTests(): void {
  registry.settingsSections.clear();
  notifySettingsSectionsChanged();
  registry.documentActions.clear();
  registry.firstRunNoticeExtensions.clear();
  registry.serverFormSections.clear();
  registry.serverSettingsTabs.clear();
  registry.sessionHeaderItems.clear();
  registry.groupFormSections.clear();
  registry.manageViews.clear();
  notifyFormSlotsChanged();
}

type ComposerListener = () => void;
const composerListeners = new Set<ComposerListener>();
let composerActionsSnapshot: ComposerAction[] = [];
let composerDropSnapshot: ComposerDropHandler[] = [];

function notifyComposerChanged(): void {
  composerActionsSnapshot = Array.from(registry.composerActions.values());
  composerDropSnapshot = Array.from(registry.composerDropHandlers.values());
  for (const listener of Array.from(composerListeners)) listener();
}

/** Registriert (bzw. ersetzt bei gleicher `id`) eine Composer-Aktion
 * (Issue #471). Die Community Edition registriert keine. */
export function registerComposerAction(action: ComposerAction): void {
  registry.composerActions.set(action.id, action);
  notifyComposerChanged();
}

/** Registriert (bzw. ersetzt bei gleicher `id`) einen Drop-Handler für
 * Dateien, die auf den Chat gezogen werden. */
export function registerComposerDropHandler(id: string, handler: ComposerDropHandler): void {
  registry.composerDropHandlers.set(id, handler);
  notifyComposerChanged();
}

/** Entfernt einen Drop-Handler und benachrichtigt Abonnenten. Eine unbekannte
 * `id` ist ein No-op. */
export function unregisterComposerDropHandler(id: string): void {
  if (!registry.composerDropHandlers.delete(id)) return;
  notifyComposerChanged();
}

/** Composer-Aktionen in Registrierungsreihenfolge; zwischen zwei Änderungen
 * dieselbe Array-Referenz (taugt als `getSnapshot`). */
export function listComposerActions(): ComposerAction[] {
  return composerActionsSnapshot;
}

/** Registrierte Drop-Handler; stabile Referenz wie oben. */
export function listComposerDropHandlers(): ComposerDropHandler[] {
  return composerDropSnapshot;
}

export function subscribeComposer(listener: () => void): () => void {
  composerListeners.add(listener);
  return () => {
    composerListeners.delete(listener);
  };
}

/** Nur für Tests. */
export function resetComposerRegistryForTests(): void {
  registry.composerActions.clear();
  registry.composerDropHandlers.clear();
  notifyComposerChanged();
}
