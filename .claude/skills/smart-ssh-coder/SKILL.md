---
name: smart-ssh-coder
description: >
  Arbeitsweise für die Umsetzung von Specs, Bugfixes und Aufträgen im
  öffentlichen smart_ssh-Repo. Laden, bevor eine Spec aus docs/specs/
  umgesetzt oder ein Auftrag im Format "Teil 0 / Commit 1..n / Abschluss"
  bearbeitet wird — auch bei Fragen zu Regressionstests, Review-Workflow,
  ADRs, CHANGELOG oder Abschlussberichten.
---

# smart_ssh — Coder-Arbeitsweise

Ergänzt `CLAUDE.md` (Gate, Architektur, Spec-first, Review-Pflicht gelten
dort vollständig und werden hier nicht wiederholt).

Vor Änderungen an Orchestrierung, Providern, Filter-Engine, Redaction oder
SFTP: `references/security-invariants.md` lesen. Für das Finden von
Erweiterungspunkten: `references/codebase-map.md`.

## Rollen

Der **Product Owner** (PO) schreibt die Specs und trifft Produkt-
entscheidungen. Der Coder setzt um, legt echte Entscheidungen vor statt sie
selbst zu treffen, und berichtet ehrlich — nie ein Ergebnis behaupten, das
nicht geprüft ist. Was nicht getestet werden konnte (z. B. UI nicht klickbar),
ausdrücklich sagen.

Sprache: Bericht und Code-Kommentare deutsch (mit Spec-Verweis),
Commit-Messages englisch (Conventional Commits).

## Auftragsformat

- **Teil 0 — zuerst klären und berichten:** Ist-Stand im Code verifizieren
  (nicht aus Erinnerung), die Frage beantworten, *vor* dem Bauen der
  betroffenen Teile berichten. Unabhängige Teile dürfen weiterlaufen.
- **Commit N:** ein Commit pro Teil, vorgegebene Commit-Message wörtlich
  übernehmen, jeder Commit mit grünem Gate.
- **Abschluss:** Gate, Review, ADR, Changelog-Fragment, Bericht (siehe unten).

Offene Produktfragen: dem PO vorlegen, mit empfohlener Option zuerst. Wenn
keine Rückfrage möglich ist (Subagent), **anhalten und berichten** statt zu
raten. Die Antwort in der Spec unter „Getroffene Entscheidungen" festhalten.

## Regressionstests mit Gegenbeweis

Ein Regressionstest zählt erst, wenn er gegen den ungefixten Stand
fehlschlägt. Deshalb: Fix vorübergehend entfernen, Test rot sehen, Fix
wiederherstellen, Test grün. Im Bericht erwähnen. Grund: Mehrere frühere
Tests prüften weniger, als sie behaupteten.

- Async-Tests, die im Fehlerfall hängen könnten, mit
  `tokio::time::timeout` umschließen — sonst hängt der Gegenbeweis, statt
  sauber zu scheitern.
- Bei Zeit- und Nebenläufigkeitstests echte Signale (Notify, Gate) statt
  Sleeps.

## Sicherheitsänderungen: verschärfen, nie lockern

Bei Filter-, Redaction- und Eskalationslogik darf keine Änderung einen
bestehenden Schutz schwächen — auch nicht als Nebenwirkung eines Fixes.
Bewährtes Muster: die alte Prüfung wörtlich behalten und mit der neuen
ODER-verknüpfen; dann kann die neue Fassung per Konstruktion nicht weniger
erkennen. Keine stillen Rückfälle: lieber sichtbar scheitern als unbemerkt
etwas anderes tun.

## Review

`spec-reviewer`-Agent mit der Priorität aus dem Auftrag, bei ERHÖHT mit
ausformulierten Angriffswegen. Jeden Fund triagieren:
- **beheben** — eigener Commit, mit Test und Gegenbeweis, oder
- **bewusst nicht beheben** — mit Begründung in der ADR und im Bericht.
Nichts stillschweigend fallen lassen. Hat eine Nachbesserung selbst etwas
gelockert, eine weitere Runde.

## Die App starten — während der Arbeit nicht, am Ende einmal

**Während du arbeitest, startest du keine sichtbare Instanz der App.**
Bauen, Testen und Linten brauchen kein Fenster. Jede Instanz, die du
zwischendurch hochziehst, nimmt Stefan den Bildschirm und hinterlässt
Fenster, die niemand zuordnen kann.

**Ausnahme, und die gilt wirklich:** Lässt sich etwas anders nicht prüfen —
ein Layout, ein Dialogablauf, ein Zustand, den kein Test abbildet —, dann
starte sie, sieh nach, und **schließe sie danach wieder**. Sag im Bericht,
warum es nötig war.

**Genau eine Instanz am Ende**, wenn das Gate grün ist und du fertig
meldest, damit Stefan prüfen kann. Schreib in den Bericht dazu, **was er
sich ansehen soll** — welcher Ablauf, welcher Bildschirm, worauf zu achten
ist. Eine laufende App ohne diesen Hinweis nützt ihm nichts.

Auf macOS immer `./scripts/tauri-dev.sh`, nie `cargo tauri dev` direkt
(Signatur-Eigenheit, s. `docs/adr/0022-stable-dev-code-signature.md`).

## Abschluss

- **ADR** für jede offen gelassene Entscheidung, jede Abweichung von der Spec
  und jeden bewusst nicht behobenen Fund. Nummer steht im Auftrag; fehlt
  sie, `XXXX` als Platzhalter (wird beim Merge vergeben).
- **Changelog**: nicht `CHANGELOG.md` direkt ändern, sondern ein Fragment
  `changelog.d/<spec-nummer>-<thema>.md` (deutsch, nutzerrelevant). Grund:
  parallele Coder würden sonst dieselben Zeilen ändern.
- **Version nie selbst erhöhen.**
- **Bericht**: `.agent/report.md` — was umgesetzt ist (in Nutzersprache),
  Teil-0-Befund, Review-Funde behoben / bewusst nicht behoben mit Grund,
  **manuelle Testabläufe** (nummeriert, inkl. nötiger Server-Einrichtung),
  Commit-Liste.
- **Zusammenfassung**: zusätzlich `.agent/summary.md`, **höchstens 40
  Zeilen**, in dieser Reihenfolge:
  1. Status und ob das Gate grün ist,
  2. was entschieden werden muss (je Punkt eine Zeile, mit Verweis auf den
     Abschnitt im Bericht),
  3. offene Annahmen (`ANNAHME A-n`) und wodurch sie aufgelöst werden,
  4. Kosten des Laufs.
  Grund: Der Architekt liest zuerst diese Datei und öffnet den langen
  Bericht nur, wenn sie ihn auf eine Stelle zeigt. Ein 370-Zeilen-Bericht
  landet sonst vollständig in seinem Kontext. Die Zusammenfassung ersetzt
  den Bericht nicht — sie verweist auf ihn.

## Was bei diesem Produkt zählt

- Kernversprechen: volle Transparenz und Kontrolle über jedes Kommando, das
  einen Server erreicht. Die KI ist Copilot, nie autonomer Akteur.
- Sicherheit vor Bequemlichkeit — aber keine Bevormundung bei manuellen
  Aktionen (Dateibrowser, Terminal sind vertrauenswürdige Nutzeraktionen).
- Ehrliche Hinweise statt Verharmlosung.
- Bei langen Aufgaben kurze Zwischenstände statt langer Funkstille.
- Wird ein Befehl per Berechtigung abgelehnt (z. B. `rm`), ihn zum
  Selbst-Ausführen anbieten, nicht umgehen.
