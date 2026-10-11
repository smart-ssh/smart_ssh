# Spec 0055 — UI-Politur: Chat-Eingabe, Nachrichten-Aktionen, Settings-Überschriften und -Labels

Status: umgesetzt
Zweck: Vier kleine, unabhängige Verbesserungen des Frontends, die das Produkt fertiger wirken lassen: mehrzeilige Chat-Eingabe, aufgeräumte Nachrichten-Aktionen, keine doppelten Überschriften in den Einstellungen, übersetzte Namen registrierter Einstellungs-Kategorien. Dazu die Kontrast-Zusage für alle Texte.
Bezüge: Spec 0012 (Dokumente), Spec 0038 (Erweiterungs-Registry), Spec 0050 (Einstellungen), Spec 0065 (abgeschnittene Antworten), Spec 0080, ADR 0046.

Nichts davon berührt Server-Zugriff oder Filter-Engine.

## Teil 1 — Mehrzeilige Eingabe im Chat

- **Enter** sendet die Nachricht, **Shift+Enter** fügt einen Zeilenumbruch ein.
- Während einer IME-Komposition (z. B. Pinyin, Tot-Tasten für Akzente)
  bestätigt Enter nur die Komposition und sendet nicht.
- Das Feld wächst mit dem Inhalt bis zu einer Maximalhöhe von etwa acht Zeilen
  und scrollt darüber hinaus intern; es schrumpft wieder, wenn der Text kürzer
  wird.

## Teil 2 — Nachrichten-Aktionen dezent

„In Notiz übernehmen" und der Markdown-Export hängen an Antworten des
Assistenten, aber nicht dauerhaft sichtbar: die Aktionsleiste blendet erst bei
Hover oder Tastaturfokus auf einem ihrer Buttons ein. Triviale Antworten wirken so
nicht überladen.

- Die Aktionen bleiben an jeder Antwort mit Text erreichbar, auch an kurzen
  Antworten und per Tastatur; keine Funktion entfällt.
- Läuft eine Aktion, hat sie fehlgeschlagen oder zeigt sie eine Bestätigung,
  bleibt die Leiste sichtbar, damit der Nutzer das Ergebnis sieht.
- Ohne Text (z. B. eine leere, abgeschnittene Antwort) gibt es keine Leiste
  (Spec 0080).
- Generierte Dokumente haben ihre eigene Darstellung (Spec 0012) und tragen
  diese Leiste nicht.
- Der Hinweis auf eine abgeschnittene Antwort samt „Fortsetzen" ist nicht
  Teil der Hover-Leiste, sondern dauerhaft sichtbar (Spec 0065).

## Teil 3 — Keine doppelten Überschriften in den Einstellungen

Den Titel einer Einstellungs-Kategorie zeigt der Einstellungs-Bildschirm
selbst. Die Kategorien darunter wiederholen ihn nicht noch einmal als eigene
Überschrift.

## Teil 4 — Namen registrierter Kategorien folgen der Sprache

Einstellungs-Kategorien, die sich über die Erweiterungs-Registry eintragen
(z. B. Chat-Aufbewahrung, MCP-Server), erscheinen in der Navigation in der
eingestellten UI-Sprache.

- Der Name einer Kategorie kann ein Übersetzungsschlüssel oder ein fester
  Text sein. Existiert ein Übersetzungsschlüssel mit genau diesem Namen, wird
  übersetzt; sonst wird der Text unverändert angezeigt. Registrierungen mit
  festem Text (auch von außen oder aus einer anderen Edition) bleiben
  funktionsfähig; ohne Namen gilt die Kennung der Kategorie (ADR 0046).
- Die eingebauten registrierten Kategorien sind in Deutsch und Englisch
  übersetzt.
- Eine Kategorie, die sich erst nach dem ersten Aufbau der Einstellungen
  registriert, erscheint ohne Neuladen in der Navigation.

## Einheitliche Bedienelemente

Schaltflächen, Dialoge und Formularfelder folgen einem gemeinsamen Muster
(Leitfaden `docs/ui-guidelines.md`), damit jeder Bildschirm gleich aussieht und
sich gleich bedient:

- Schaltflächen gibt es als Primär-, Sekundär-, Gefahr- und Link-Variante. Pro
  Dialog oder Formular gibt es höchstens eine Primäraktion.
- Dialoge zeigen Titel, scrollenden Inhalt und eine Aktionsleiste rechts unten;
  Abbrechen steht links von der Primäraktion. Escape schließt den Dialog, Tab
  bleibt im Dialog, und der Fokus kehrt beim Schließen zurück.
- In einem Formular löst Enter die Primäraktion aus.
- Formularfelder haben eine sichtbare Beschriftung; Hilfetext und Fehlermeldung
  stehen darunter und sind dem Feld für Screenreader zugeordnet.
- Der Tastaturfokus ist an jedem Bedienelement als Rahmen sichtbar.

## Kontrast und Lesbarkeit

Jeder Text erreicht gegen seinen tatsächlichen Hintergrund mindestens das
WCAG-2.1-AA-Verhältnis von 4,5:1 (große Schrift 3:1). Das gilt auch für
abgesetzte Texte (Sekundärbeschriftungen, inaktive Tabs, Hinweise) und für
Text auf Akzentflächen (Primärbuttons, aktive Auswahl). Abgesetzte Texte
bleiben durch ihre Abstufung vom Haupttext unterscheidbar. Deaktivierte
Bedienelemente sind nach WCAG ausgenommen. Die automatischen
Barrierefreiheits-Scans der Browser-Tests prüfen das ohne Ausnahme für das
Hauptfenster, jede Einstellungskategorie und die Befehlsbestätigung.

## Grenzen

- Es gibt keine automatische Unterscheidung, wann eine Antwort ein Dokument
  ist.
