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
- Einträge entstehen während der Entwicklung als einzelne Fragmente im
  Verzeichnis `changelog.d/` (Regeln dort im README) und werden beim
  Release in den neuen Versionsabschnitt übernommen; danach werden die
  Fragmente gelöscht.

## 3. Grenzen

- Die App zeigt den Changelog nicht an.
