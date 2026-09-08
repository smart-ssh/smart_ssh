# 0041-settings-categories-scoped-to-what-already-lives-in-settings

## Status
Akzeptiert

## Kontext

Spec 0050, Abschnitt 1.1 schlägt eine Beispiel-Gruppierung für die linke
Navigation der neuen zweispaltigen Settings-Struktur vor, darunter
**"Filter & Regeln"** (Regel-Verwaltung, Test-Panel) und **"Über /
Erststart-Info"** — ausdrücklich als Beispiel markiert ("z. B."), mit der
Erlaubnis, die konkrete Zuordnung sinnvoll selbst zu wählen, aber der
Auflage, die Gruppierung vor dem Festzurren zu beschreiben.

Vor der Umsetzung wurde dem Nutzer per Rückfrage (`AskUserQuestion`) eine
konkrete Kategorien-Liste vorgeschlagen, basierend auf einer Bestandsaufnahme
des tatsächlichen Settings-Screens: `AiProviderSettings.tsx` enthielt vor
dem Umbau Provider-Liste/-Formular, Modell-Discovery,
Zweitmeinungs-Provider-Auswahl, Sprachumschalter, den "Log-Verzeichnis
öffnen"-Button und die beiden bereits registrierten Sektionen
(`chat-retention`, `mcp-server`). **Filter-Regeln und Server-/
Gruppenverwaltung leben dagegen in eigenen Top-Level-Tabs (`App.tsx`s
`Tab = "connect" | "manage" | "rules"`), nicht im Settings-Modal** — die
Spec-Beispielliste geht an dieser Stelle von einer Struktur aus, die es in
der App nicht gibt. Der Nutzer hat den vorgeschlagenen, auf den
tatsächlichen Settings-Inhalt begrenzten Kategorien-Vorschlag bestätigt.

## Entscheidung

Die zweispaltige Navigation bekommt genau die Kategorien, die vorher schon
im Settings-Screen existierten (KI-Provider, Anzeige & Sprache, Diagnose als
neue eingebaute Kategorien; Sitzungen & Daten/MCP-Server als bereits
registrierte Sektionen, jetzt mit eigenem Nav-Eintrag statt Inline-Rendering)
— **keine** "Filter & Regeln"- oder "Über / Erststart-Info"-Kategorie.

Begründung:

- Filter-Regeln/Server-Verwaltung aus ihren bestehenden Top-Level-Tabs in
  das Settings-Modal zu verschieben wäre eine **größere strukturelle
  Verschiebung** der App-Navigation, kein reiner Umbau des
  Settings-Screens selbst — das würde den Rahmen dieses Schritts (Spec
  0050 bündelt drei zusammengehörige Frontend-Themen, explizit NORMAL
  priorisiert) sprengen und Verhalten außerhalb des Settings-Screens
  ändern, das die Spec nicht anspricht.
- Für "Über / Erststart-Info" existiert im aktuellen Settings-Screen kein
  Inhalt, der dorthin verschoben werden könnte (keine Versionsanzeige, kein
  erneuter Zugriff auf den Erststart-Hinweis) — eine leere Kategorie nur
  um die Beispielliste vollständig abzubilden, wäre Schein-Vollständigkeit
  ohne Nutzen.

## Konsequenzen

- Ein Nutzer, der Filter-Regeln oder Server-Verwaltung in den
  Einstellungen sucht, findet sie weiterhin nur über die separaten
  Top-Level-Tabs — das ist unverändertes Verhalten (kein Rückschritt),
  aber auch keine Vereinheitlichung, die die Spec-Beispielliste suggeriert
  haben könnte.
- Sollte künftig eine "Über"-Kategorie mit echtem Inhalt gewünscht sein
  (App-Version, Lizenzhinweise, erneuter Zugriff auf den
  Erststart-Hinweis), lässt sie sich als weitere eingebaute Kategorie in
  `SettingsScreen.tsx` ergänzen, ohne die hier getroffene Struktur
  aufzubrechen.
