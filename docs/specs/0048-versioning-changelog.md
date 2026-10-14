# Spec 0048 — Versionierung und Changelog

Status: umgesetzt
Zweck: Das Produkt hat genau eine Versionsnummer, und ein Changelog
beschreibt, was sich zwischen den Versionen für den Nutzer geändert hat.
Bezüge: Spec 0052 (Anzeige von Version und Build in der App), Spec 0090
(Release-Gate).

## 1. Eine Version

- Dieses Repository ist die maßgebliche Quelle der Produktversion. Jeder
  Build des Produkts trägt diese Version; die App zeigt sie an (Spec 0052).
- Alle Stellen, die die Version tragen (App-Konfiguration, Rust-Workspace
  samt den internen Abhängigkeiten der Crates untereinander,
  Frontend-Manifest und dessen Lockfile), nennen dieselbe Version.
- Die Version folgt Semantic Versioning: neue Funktionen erhöhen die
  Minor-, reine Fehlerbehebungen die Patch-Stelle.
- Die Version wird bewusst erhöht, für ein Release, nicht automatisch und
  nicht je Änderung. Ein Release bekommt ein Git-Tag `vX.Y.Z`.

## 2. Changelog

- `CHANGELOG.md` im Wurzelverzeichnis folgt „Keep a Changelog":
  umgekehrt chronologisch, ein Abschnitt je Version mit Datum, darin die
  Kategorien Added, Changed, Fixed und Security. Oben steht ein Abschnitt
  `[Unreleased]`.
- Der Changelog enthält nur, was für den Nutzer der App relevant ist.
  Interne Umbauten, Tests und CI stehen nicht darin.
- Er deckt den vollen Produktumfang ab. Was nur in einer kostenpflichtigen
  Edition verfügbar ist, ist mit **(Pro)** markiert.
- Jeder Versionsabschnitt ab Version 0.5.3 beginnt mit der Überschrift
  `### Highlights`, vor den Kategorien. Sie enthält 3 bis 6 Listenpunkte,
  jeder genau eine Zeile, aus Nutzersicht und ohne Interna. Sie
  werden als Kurzfassung der Release-Notes veröffentlicht.
  `[Unreleased]` hat keine Highlights; sie entstehen beim Anlegen des
  Versionsabschnitts. Ältere Abschnitte sind ausgenommen. Eine
  automatische Prüfung schlägt fehl, wenn ein Abschnitt ab 0.5.3 die
  Highlights nicht oder nicht in dieser Form hat.
- Zu jeder Version ab 0.5.3 (und rückwirkend zu 0.4.5 bis 0.5.2) gibt es
  eine Datei `release-notes/X.Y.Z.json` im
  Wurzelverzeichnis mit den Highlights in Englisch, Deutsch, Französisch
  und Spanisch. Sie enthält genau die Schlüssel `version` und `highlights`;
  `highlights` hat genau die Schlüssel `en`, `de`, `fr` und `es`. Jede Liste
  hat 3 bis 6 Einträge, alle Listen gleich viele, und Eintrag *n* bedeutet
  in jeder Sprache dasselbe. Die Einträge sind einzeilige Klartexte ohne
  Markdown. `en` ist Eintrag für Eintrag identisch mit den
  `### Highlights` der Version im Changelog. Die Download-Seite und der
  Update-Dialog ausgelieferter Builds holen die Datei einer Version über
  ihren Namen und zeigen die Highlights in der Sprache des Lesers. Die
  automatische Prüfung schlägt fehl, wenn eine Datei diesem Format nicht
  entspricht, `en` abweicht, die Version keinen Changelog-Abschnitt hat oder
  die Datei für eine Version ab 0.5.3 fehlt.
- Der Changelog und die Fragmente sind englisch.
- Ein Eintrag (Fragment) hat höchstens zwei Zeilen, beschreibt die sichtbare
  Wirkung statt des Mechanismus und enthält nichts Internes.
- Einträge entstehen während der Entwicklung als einzelne Fragmente im
  Verzeichnis `changelog.d/` (Regeln dort im README) und werden beim
  Release in den neuen Versionsabschnitt übernommen; danach werden die
  Fragmente gelöscht.

## 3. Grenzen

- Die App zeigt den Changelog nicht an.
