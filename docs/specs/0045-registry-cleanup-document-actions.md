# Spec: Registry aufräumen — Dokument-Aktions-Andockpunkt, tote Typen entfernen

Status: Entwurf
Modul: **öffentliches Repo** `smart_ssh` — `src/extensions/registry.ts`,
`ChatPanel.tsx`, ggf. weitere Konsumenten
Abhängigkeiten: Extension-Registry (0038), KI-Dokumente/Markdown-Export
(0012)

> Dies ist eine **öffentliche** Spec (Apache-2.0). Sie schafft einen
> generischen Andockpunkt, den *jedes* Frontend (Community wie Official)
> nutzen kann — sie weiß nichts von Pro/Word-Export. Der private Word-Export
> dockt in einem separaten privaten Schritt daran an.

## 1. Ausgangslage

Der Aufbau des Official-Frontends (privates Repo) hat einen Fund im
öffentlichen Repo aufgedeckt: Die Extension-Registry aus 0038 deklariert
vier Beitragsarten (`registerRoute`, `registerPanel`,
`registerSettingsSection`, `registerCommandPaletteAction`), aber **nur
`registerSettingsSection` wird tatsächlich irgendwo gerendert.**
`listRoutes()`, `listPanels()` und `listCommandPaletteActions()` werden
nirgends konsumiert — es gibt keine Command-Palette-UI, keine
Route-/Panel-Rendering-Stelle. Diese drei Typen sind **totes Vokabular**:
Sie behaupten "hier kannst du andocken", rendern aber nichts. Ein Feature,
das sie nutzt (wie zunächst der Word-Export über `registerCommandPaletteAction`),
wird dadurch unsichtbar — genau die Falle, in die der Word-Export gelaufen
ist.

## 2. Prinzip

**Kein Registry-Typ ohne echten, gerenderten Konsumenten.** Ein Andockpunkt,
der nichts rendert, ist schlimmer als keiner — er lockt in die
Unsichtbarkeits-Falle. Diese Spec macht genau **einen** neuen Typ real (den
akut gebrauchten), und **entfernt** die drei toten. Ein entfernter Typ kann
jederzeit wiederkommen, wenn ein Feature ihn braucht — dann aber **mit**
Rendering, nicht als leere Deklaration.

## 3. Neuer Typ: `registerDocumentAction`

Ein generischer Andockpunkt für Aktionen an einem KI-generierten Dokument
(dort, wo im Dokument-/Chat-Bereich der Markdown-Export sitzt, 0012).

```ts
export interface DocumentAction {
  id: string;
  label: string;                       // z. B. "Als Word speichern"
  // Aufruf mit dem aktuellen Dokument-Inhalt/Kontext, den der Renderer
  // bereitstellt (Markdown-Inhalt + Titel, analog zum bestehenden
  // Markdown-Export)
  onInvoke: (doc: DocumentContext) => void;
  // Optional: darstellungssteuernd, damit ein gegatetes Feature sich als
  // gesperrt zeigen kann, ohne dass die Registry etwas von Entitlements weiß
  disabled?: boolean;
  disabledReason?: string;             // Tooltip/Hinweis bei disabled
}

export function registerDocumentAction(action: DocumentAction): void;
export function listDocumentActions(): DocumentAction[];
```

**Wichtig — die Registry bleibt entitlement-agnostisch.** Sie kennt keine
`Feature`/`Entitlements` (das wäre Pro-Wissen im öffentlichen Repo). Ob eine
Aktion gesperrt ist, entscheidet der *Registrierende* (im Pro-Fall das
private Modul) und setzt `disabled`/`disabledReason` — die Registry und
`ChatPanel` rendern nur, was ihnen gegeben wird.

## 4. Rendering in `ChatPanel`

`ChatPanel.tsx` (bzw. die Dokument-Karten-Komponente aus 0012) rendert die
über `listDocumentActions()` registrierten Aktionen **neben** dem
bestehenden Markdown-Export-Button. Eine `disabled`-Aktion wird sichtbar,
aber ausgegraut/gesperrt dargestellt, mit `disabledReason` als Tooltip.
Klick auf eine aktive Aktion ruft ihr `onInvoke` mit dem Dokument-Kontext
auf.

Der bestehende Markdown-Export bleibt unverändert an seinem Platz — die
registrierten Dokument-Aktionen kommen daneben, nicht statt.

## 5. Entfernen der toten Typen

`registerRoute`/`listRoutes`, `registerPanel`/`listPanels`,
`registerCommandPaletteAction`/`listCommandPaletteActions` werden aus der
Registry **entfernt**, samt zugehöriger Typen. Prüfe vor dem Entfernen:
- Gibt es *irgendeinen* tatsächlichen Aufruf im öffentlichen Repo? (Erwartung
  laut Fund: nein, außer der Registrierung selbst.) Falls doch ein echter
  Konsument auftaucht, **melden** statt blind entfernen.
- Der private Word-Export nutzt aktuell `registerCommandPaletteAction` als
  Notlösung — der wird im **privaten** Repo auf `registerDocumentAction`
  umgestellt (separater privater Schritt). Das Entfernen hier macht den
  privaten Notlösungs-Code kaputt, **das ist beabsichtigt und wird im
  privaten Schritt behoben**; koordiniere über den Submodule-Pin (der private
  Schritt bumpt auf den öffentlichen Commit mit dieser Spec).

## 6. Sicherheits-/Konsistenz-Invarianten

- Die Registry bleibt entitlement-agnostisch (kein `Feature`-Wissen im
  öffentlichen Repo).
- `disabled` an einer Dokument-Aktion ist **nur** die freundliche Vorderseite
  — die harte Durchsetzung eines gegateten Features bleibt die
  `require(...)`-Prüfung im jeweiligen Command (privat). Ein Umgehen der
  UI-Sperre führt weiterhin zu `FeatureLocked`.
- Nach dem Entfernen der drei Typen gibt es **kein** totes Registry-Vokabular
  mehr — jeder verbleibende Typ (`registerSettingsSection`,
  `registerDocumentAction`) hat einen echten Renderer.

## 7. Testbarkeit

- `registerDocumentAction`/`listDocumentActions`: Registrierung und Auflistung
  (Unit).
- `ChatPanel` rendert eine registrierte aktive Aktion (klickbar, `onInvoke`
  wird aufgerufen) und eine `disabled`-Aktion (sichtbar, gesperrt, Tooltip) —
  Komponententest.
- Nach dem Entfernen der toten Typen kompiliert das öffentliche Repo und
  alle bestehenden Tests bleiben grün (die drei Typen hatten ja keine
  Konsumenten).

## 8. Offene Punkte / Koordination

- Der private Word-Export-Umbau (von `registerCommandPaletteAction` auf
  `registerDocumentAction`) ist ein **separater privater Schritt**, der auf
  den öffentlichen Commit dieser Spec aufsetzt (Submodule-Pin bumpen).
- Falls später ein Pro-Feature doch eine eigene Route/ein Panel braucht,
  wird der entsprechende Typ **neu und mit Renderer** hinzugefügt — nicht
  aus einer leeren Deklaration reaktiviert.
