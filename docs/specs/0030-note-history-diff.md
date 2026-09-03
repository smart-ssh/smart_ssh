# Spec: Diff-Anzeige in der Notiz-Historie

Status: Entwurf
Modul: `frontend/` (Notiz-Historie-Ansicht)
Abhängigkeiten: Notiz-Historie (Spec 0003, Abschnitt 5.3; Spec 0008,
Abschnitt 6), bestehende Diff-Komponente (Spec 0019, Abschnitt 4)

## 1. Problem

Die Notiz-Historie (Spec 0008, Abschnitt 6) zeigt pro Revision bisher nur
Zeitpunkt, Editor (Nutzer/KI) und einen "Wiederherstellen"-Button — nicht,
**was sich inhaltlich geändert hat**. Um das nachzuvollziehen, müsste man
aktuell zwei Revisionen manuell nebeneinanderhalten und selbst vergleichen.

## 2. Ziel

Jeder Eintrag in der Notiz-Historie zeigt zusätzlich, was sich gegenüber der
**unmittelbar vorherigen** Revision (chronologisch) geändert hat — über
dieselbe zeilenbasierte Diff-Komponente, die bereits für Notiz-
Änderungsvorschläge existiert (Spec 0019, Abschnitt 4), nicht über eine
zweite Implementierung.

## 3. Verhalten

- Standardmäßig **eingeklappt** — die Historie-Liste bleibt bei vielen
  Einträgen übersichtlich (Zeitpunkt, Editor, Wiederherstellen-Button wie
  bisher). Klick auf einen Eintrag klappt den Diff gegenüber der
  vorherigen Revision auf.
- Für die **älteste** Revision (kein Vorgänger vorhanden): kein Diff, da
  nichts zum Vergleichen existiert — stattdessen der volle Inhalt als
  "Ursprüngliche Version", klar mit einer eigenen Beschriftung erkennbar.
- Diff-Berechnung passiert clientseitig aus den bereits über
  `list_note_revisions` (Spec 0008, Abschnitt 5) geladenen Daten — kein
  neuer Backend-Command nötig, alle Revisionsinhalte liegen dem Frontend
  ohnehin schon vor.
- Farbschema identisch zur bestehenden Diff-Darstellung (Spec 0019,
  Abschnitt 4): hinzugefügte Zeilen grün, entfernte Zeilen
  rot/durchgestrichen.

## 4. Nicht-Ziele

- Kein Diff über mehr als zwei benachbarte Revisionen hinweg (z. B. "zeige
  mir alle Änderungen der letzten 5 Versionen auf einmal") — jede Revision
  wird nur gegen ihren direkten Vorgänger verglichen, das reicht für den
  Anwendungsfall "nachvollziehen, was bei diesem einen Schritt passiert
  ist".

## 5. Test

Historie mit mindestens drei Revisionen: mittlere Revision zeigt beim
Aufklappen korrekt den Diff gegenüber der direkt vorherigen (nicht gegenüber
der ältesten oder der aktuellen); älteste Revision zeigt "Ursprüngliche
Version" ohne Diff-Darstellung.
