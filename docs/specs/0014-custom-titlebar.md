# Spec: Individuelle Titelleiste

Status: Entwurf
Modul: Erweiterung `crates/app-tauri` (Tauri-Konfiguration/Setup) +
`frontend/` (Header-Komponente)
Abhängigkeiten: keine fachliche Abhängigkeit zu anderen Modulen, rein
UI-Chrome-Ebene

## 1. Ziel

Die native Titelleiste wird durch eine eigene, ins App-Design integrierte
Kopfzeile ersetzt — auf allen drei Plattformen, nicht nur macOS, damit das
Erscheinungsbild konsistent bleibt. Wichtig dabei: **die nativen
Fenster-Controls bleiben erhalten** (macOS-Ampel, Windows-Minimieren/
Maximieren/Schließen inkl. Snap-Layout-Unterstützung, Linux-System-Icons) —
wir bauen keine eigenen Nachbauten dieser Buttons, nur den Rest der
Titelleiste wird zu eigenem Inhalt (App-Name/Logo, perspektivisch eine
Tab-Leiste für offene Server-Sessions).

## 2. Technologie: `tauri-plugin-decoration`

Community-Plugin, das die Overlay-Titelleiste plattformspezifisch verwaltet:
- **macOS**: `create_overlay_titlebar()` + `set_traffic_lights_inset(x, y)`
  positioniert die nativen Ampel-Buttons exakt im eigenen Header-Layout,
  ohne sie selbst nachbauen zu müssen.
- **Windows**: rendert eigene Minimieren/Maximieren/Schließen-Buttons als
  HTML über einer transparenten, draggable Titelleistenfläche, koppelt sich
  aber an die native Snap-Layout-Funktion (Windows 11 Flyout beim Hover über
  Maximieren) — kein manuelles Nachbauen dieses Verhaltens nötig.
- **Linux**: Fenster-Controls nutzen das aktuelle System-Icon-Theme, damit
  sie sich nicht fremd anfühlen.

Begründung für ein Plugin statt Eigenbau: Die in Abschnitt 1 genannten
Verhaltensdetails (Snap Layout, Ampel-Verhalten bei nicht reagierender App,
Dark/Light-Mode-Anpassung der Hover-Farben) sind in der Summe genug
Detailarbeit, dass ein gepflegtes, plattformübergreifendes Plugin dem
Eigenbau vorzuziehen ist — konsistent mit dem Projektprinzip, wo möglich auf
ausgereifte Bibliotheken zu setzen statt plattformspezifische Details selbst
nachzubauen (vgl. Begründung für `russh`/`docx-rs` in Spec 0005/0012).

## 3. Konfiguration

`tauri.conf.json`: `decorations: true` bleibt gesetzt (das Plugin verwaltet
die Decorations selbst pro Plattform — **nicht** `decorations: false`
setzen, sonst greifen die plattformspezifischen Mechanismen des Plugins
nicht). Für macOS zusätzlich `titleBarStyle: "Overlay"` und
`hiddenTitle: true`.

```rust
// app-tauri setup
main_window.create_overlay_titlebar()?;

#[cfg(target_os = "macos")]
main_window.set_traffic_lights_inset(12.0, 16.0)?; // Startwert, siehe Abschnitt 6
```

## 4. Eigener Header-Inhalt und Layout

Der übrige Titelleistenbereich wird zu einer React-Komponente
(`<AppHeader />`), die App-Icon + "Smart SSH"-Schriftzug links zeigt (Platz
für spätere Erweiterung um eine Tab-Leiste offener Sessions). Wichtig für
die Platzierung relativ zu den nativen Controls:

- **macOS**: native Ampel sitzt oben links → eigener Header-Inhalt beginnt
  mit entsprechendem linken Abstand (abgeleitet aus dem
  `set_traffic_lights_inset`-Wert), damit nichts überlappt.
- **Windows/Linux**: native Controls sitzen oben rechts (Konvention dieser
  Plattformen) → eigener Header-Inhalt bekommt stattdessen rechts
  entsprechenden Abstand.

Die Komponente erkennt die Plattform (Tauri liefert das zur Laufzeit) und
wählt das passende Padding, statt ein hartcodiertes Layout für nur eine
Plattform zu bauen.

## 5. Drag-Region und Interaktivität

Der gesamte Header-Bereich (außer den nativen Controls selbst) muss als
ziehbare Fensterfläche markiert werden (`data-tauri-drag-region`-Attribut),
damit der Nutzer das Fenster weiterhin per Ziehen der Titelleiste bewegen
kann — das geht sonst verloren, sobald die native Titelleiste durch eigenen
Inhalt ersetzt wird. Falls künftig interaktive Elemente in den Header
kommen (z. B. Tab-Klicks): diese Elemente müssen gezielt **von** der
Drag-Region ausgenommen werden (`pointer-events`-Handling gemäß
Plugin-Dokumentation), sonst werden Klicks fälschlich als Fenster-Ziehen
interpretiert statt als Interaktion.

## 6. Offene Punkte

- Der `set_traffic_lights_inset(12.0, 16.0)`-Wert ist ein Startwert und
  visuell noch nicht final abgestimmt — das lässt sich nur durch
  tatsächliches Hinschauen auf dem gebauten Fenster feintunen, nicht rein
  aus der Spec heraus festlegen.
- Eine native macOS-Menüleiste (oben am Bildschirmrand, nicht am Fenster) ist
  ein separates Thema und nicht Teil dieser Spec — hier geht es nur um die
  Fenster-Titelleiste selbst.
- Sobald die in Spec 0007 angedachte Mehrfach-Tab-Funktionalität (mehrere
  offene Server-Sessions parallel) ansteht, wird der Header-Bereich um eine
  Tab-Leiste erweitert — diese Spec legt nur das Grundgerüst (Icon, Name,
  Drag-Region, Platzierung relativ zu den nativen Controls), keine Tabs.
