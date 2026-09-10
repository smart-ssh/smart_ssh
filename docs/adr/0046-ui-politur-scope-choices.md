# 0046-ui-politur-scope-choices

## Status
Akzeptiert

## Kontext

Spec 0055 (UI-Politur-Runde) ließ an zwei Stellen den konkreten
Umsetzungsweg bewusst offen und bat ausdrücklich um eine Beschreibung der
getroffenen Wahl (Teil 2: "wähle den saubersten Weg, beschreibe ihn";
Teil 4: "beschreibe mir, wie du Schlüssel vs. festen Text
unterscheidest"). Diese ADR hält die während der Umsetzung getroffenen
Entscheidungen fest, inklusive einer Korrektur, die der `spec-reviewer`
im Review des Gesamtpakets veranlasst hat.

## Entscheidungen

**1. Teil 2 (Nachrichten-Aktionen entrümpeln): nur Hover-/Fokus-Dezenz,
keine längenbasierte Entfernung.**

Die erste Umsetzung blendete die Aktionsleiste ("Als Markdown
exportieren"/"In Notiz übernehmen") unterhalb einer Mindestlänge
(`MIN_SUBSTANTIAL_REPLY_LENGTH = 40` Zeichen, getrimmt) komplett aus dem
DOM aus. Der `spec-reviewer` wies das mit einem konkreten Gegenbeispiel
zurück: eine Antwort wie `"rm -rf / löscht das ganze System"` ist mit 32
Zeichen kürzer als die Schwelle, aber eindeutig notizwürdig — die Aktion
wäre für solche Antworten unerreichbar geworden, auch nicht per Tastatur.
Das verletzt Spec 0055 Zeile 40 wörtlich: "Keine bestehende Funktion
entfernen — nur die Darstellung/Platzierung verbessern."

Entscheidung: die längenbasierte Entfernung wurde vollständig verworfen.
Die Aktionsleiste wird jetzt für **jede** Antwort unverändert ins DOM
gerendert; die alleinige Milderung des ursprünglichen Problems
("permanenter visueller Ballast bei trivialen Antworten") ist die
bestehende Tailwind-Hover-/Fokus-Reveal-Technik
(`opacity-0 transition-opacity duration-150 group-hover:opacity-100
focus-within:opacity-100` am `.group`-Elternelement) — das Element bleibt
im Accessibility-Tree und per Tab erreichbar, ist aber standardmäßig
visuell zurückgenommen und blendet erst bei Hover/Tastaturfokus ein.

Ergänzend deckte derselbe Review eine WKWebView-Eigenheit auf: ein
Maus-Klick auf einen `<button>` verleiht ihm in WKWebView (macOS/Tauri)
keinen DOM-Fokus — `focus-within` triggert also nie durch einen reinen
Maus-Klick auf "In Notiz übernehmen". Bewegt der Nutzer danach die Maus
weg, würde die gesamte Zeile (inklusive einer etwaigen Fehlermeldung oder
des Erfolgs-Häkchens) wieder auf `opacity-0` fallen, bevor der Nutzer sie
überhaupt gelesen hat — ein fehlgeschlagenes Speichern sähe dann optisch
nicht anders aus als ein erfolgreiches. Behoben durch einen neuen
`onStateChange`-Callback an `TakeIntoNoteButton`, der den Zustand
("idle"/"sending"/"error") an `AssistantMessageView` zurückmeldet; ein
`forceVisible`-Flag (wahr, solange irgendeine Aktion läuft, fehlgeschlagen
ist, oder gerade erfolgreich war — `exporting`, `savedFormat` oder
`noteState !== "idle"`) überschreibt die Opacity unabhängig von
Hover/Fokus auf `100`.

**2. Teil 4 (i18n-Labels der registrierten Sektionen):
`i18next.exists(label)` zur Laufzeit statt Präfix-Konvention oder
zweitem Feld.**

Die Spec verlangte Rückwärtskompatibilität: ein registriertes
`SettingsSectionContribution.label`, das kein Übersetzungsschlüssel ist
(z. B. aus einer künftigen privaten/Pro-Erweiterung, die feste Strings
liefert), muss weiterhin unverändert angezeigt werden. Zwei Alternativen
wurden verworfen:

- Ein Präfix (`"i18n:settings.categories.mcpServer"`) hätte jeden
  existierenden und künftigen Aufrufer von `registerSettingsSection`
  gezwungen, das Präfix zu kennen und korrekt zu setzen — leicht zu
  vergessen, keine Fehlermeldung bei falscher Schreibweise.
- Ein zweites Feld (`labelKey?: string`) hätte die
  `SettingsSectionContribution`-Schnittstelle erweitert und zwei
  konkurrierende Quellen für denselben Anzeigetext geschaffen.

Stattdessen prüft `resolveSectionLabel(label, id, i18n, t)` (neu:
`src/resolveSectionLabel.ts`) zur Render-Zeit in `SettingsScreen.tsx` per
`i18n.exists(label)`, ob der String als Pfad in den geladenen
Sprachdateien existiert — wenn ja, wird er übersetzt (`t(label)`), wenn
nein, unverändert als Text gerendert. Kein neues Feld, keine Konvention,
die Aufrufer einhalten müssten; ein fester String ohne passenden Schlüssel
bricht nichts.

Bekanntes Restrisiko (in `resolveSectionLabel.ts` selbst dokumentiert):
ein fest gewählter String, der zufällig exakt einem existierenden
i18next-Pfad entspricht (oder auf einen reinen Objekt-Knoten ohne Blatt
zeigt, wofür `exists()` `true` liefert, `t()` aber den rohen Schlüssel
zurückgibt), würde fehlerhaft behandelt. Für kurze, für UI-Kategorien
typische Label-Strings als vernachlässigbar eingestuft — die Registry
selbst bleibt bewusst frei von jeder `i18next`-Abhängigkeit
(Architekturregel aus Spec 0038/0050), die Auflösung passiert
ausschließlich beim Rendern.

**3. Die Registry selbst (`extensions/registry.ts`) bleibt
i18n-unwissend.**

Weder `SettingsSectionContribution` noch `registerSettingsSection`
importieren `i18next`/`react-i18next` — der Übersetzungsschlüssel ist aus
Sicht der Registry ein beliebiger String, identisch zum bisherigen
Verhalten. Das erhält die in Spec 0038 festgelegte Trennung (Registry als
reiner, UI-Framework-unabhängiger Kontributionspunkt) und lässt eine
private/Pro-Erweiterung weiterhin feste Strings ohne jede Abhängigkeit
von der öffentlichen i18n-Konfiguration liefern.
