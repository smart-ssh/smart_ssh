# Spec: UI-Politur-Runde

Status: Umgesetzt
Repo: **öffentlich** `smart_ssh`, Frontend
Abhängigkeiten: Chat-UI, Settings-Neustruktur (0050), Extension-Registry (0038)

> Mehrere kleine, unabhängige UI-Verbesserungen, die zusammen den „rau →
> fertig"-Unterschied Richtung 1.0 machen. Alle **öffentliches Frontend,
> risikoarm, Priorität NORMAL**. Kein server-verändernder Zugriff, keine
> Filter-Engine-Berührung. Jeder Teil ein eigener Commit — sie sind nur
> gebündelt, weil alle klein und UI-nah.

## Teil 1: Multiline-Eingabe im Chat-Input (Shift+Enter)

Das Chat-Eingabefeld erlaubt aktuell keine mehrzeilige Eingabe wie gewohnt.
**Standard-Verhalten herstellen:**
- **Enter** sendet die Nachricht.
- **Shift+Enter** fügt einen **Zeilenumbruch** ein (kein Senden).
- Das Feld **wächst** mit dem Inhalt (auto-grow, bis zu einer sinnvollen
  Maximalhöhe, danach scrollt es intern).
- Auf Mobile/Touch (falls relevant) das plattformübliche Verhalten.
Das ist das Verhalten, das Nutzer aus jedem Chat-Tool kennen — mehrzeilige
Kommandos/Erklärungen eingeben ist sonst umständlich.

## Teil 2: Nachrichten-Aktionen entrümpeln

Aktuell hängen an **jeder** KI-Chat-Nachricht „Export to Markdown" und „Take
into Note" — auch an trivialen Antworten („ok, verstanden"). Das verwässert
die Bedeutung dieser Aktionen und wirkt unaufgeräumt.

**Aufräumen** (wähle den saubersten Weg, beschreibe ihn mir):
- Entweder die Aktionen nur an **substanziellen** Antworten zeigen (ab einer
  Mindestlänge / bei tatsächlichem Inhalt), oder
- die Aktionen dezenter darstellen (z. B. erst bei Hover sichtbar, statt
  permanent), oder
- klar zwischen „normale Antwort" und „generiertes Dokument" (DocumentCard,
  Spec 0012) unterscheiden — Export/Notiz primär an Dokumenten, nicht an
  jeder Plauderei.
Ziel: Das UI wirkt aufgeräumter, die Aktionen erscheinen dort, wo sie Sinn
ergeben. **Keine** bestehende Funktion entfernen — nur die Darstellung/
Platzierung verbessern.

## Teil 3: Doppelte Überschrift in Settings-Sektionen (aus 0050-Review)

Nach der Settings-Neustruktur (0050) rendert `SettingsScreen` den
Sektions-Titel als Überschrift (`{active?.label}`) — aber die einzelnen
Sektionen rendern **zusätzlich** ihre eigene `<h3>` (z. B.
`ChatRetentionSettings`, `McpServerSettings`). Ergebnis: **doppelte
Überschrift**.

**Fix:** Die Sektionen rendern ihre eigene Titelzeile **nicht** mehr — der
`SettingsScreen` übernimmt den Titel. Alle eingebauten Sektionen durchgehen
und die redundante `<h3>` entfernen. (Die private Lizenz-Sektion zieht im
selben Muster nach — das ist ein separater privater Folgeschritt, hier nur
die öffentlichen Sektionen.)

## Teil 4: i18n-Labels der registrierten Settings-Sektionen (aus 0050-Review)

Die über `registerSettingsSection` registrierten Sektionen (`chat-retention`,
`mcp-server`) tragen **feste deutsche `label`-Strings** — in der **englischen
UI** stehen dadurch deutsche Nav-Einträge zwischen englischen.

**Fix:** Das `label`-Feld der Registry so nutzen, dass es einen
**Übersetzungs-Schlüssel** aufnimmt (statt eines festen Anzeigetexts), den
das Frontend beim Rendern über den bestehenden i18n-Mechanismus auflöst.
- Die Registry-API bleibt rückwärtskompatibel (ein fester String ohne
  passenden Übersetzungs-Schlüssel wird weiterhin direkt angezeigt — kein
  Bruch für Registrierungen, die keinen Key nutzen).
- Die eingebauten Sektionen auf Übersetzungs-Schlüssel umstellen (DE+EN).
- Beschreibe mir, wie du den Schlüssel-vs-Text-Fall unterscheidest (z. B. ein
  Konventions-Präfix, oder ein separates Feld).

## Nicht Teil dieser Spec

- Settings-Registry-**Notify-Mechanismus** (spät registrierte Sektion nach
  Async-Render) — eigener, größerer Punkt, bleibt im Backlog.
- Die **private** Lizenz-Sektion (`<h3>` entfernen) — separater privater
  Folgeschritt zu Teil 3.
- „Wann ist etwas ein Dokument"/Dokument-Erkennung — eigenes größeres Thema.

## Design/Konventionen

- **frontend-design-Skill** beachten (Tokens, keine Ad-hoc-Styles).
- Teil 1 (auto-grow) und Teil 2 (Hover-Aktionen) sollen sich natürlich ins
  bestehende Chat-Layout einfügen.

## Testbarkeit

- Teil 1: Enter sendet; Shift+Enter fügt Zeilenumbruch; Feld wächst bis Max,
  dann intern scrollbar.
- Teil 2: Aktionen erscheinen nach der gewählten Regel (substanziell/Hover/
  Dokument), bestehende Funktion (Export/Notiz) weiter erreichbar.
- Teil 3: Keine Sektion zeigt ihren Titel doppelt; der SettingsScreen-Titel
  bleibt.
- Teil 4: Registrierte Sektionen erscheinen in der UI-Sprache übersetzt;
  eine Registrierung mit festem String ohne Key bricht nicht.

## Reihenfolge

1. Multiline-Input (Teil 1) — abgegrenzt, hoher Nutzen.
2. Doppelte Überschrift (Teil 3) — kleiner Aufräumer.
3. i18n-Labels (Teil 4) — Registry-nah.
4. Nachrichten-Aktionen (Teil 2) — etwas mehr Design-Spielraum, zuletzt.
