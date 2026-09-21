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

## Abschluss

- **ADR** für jede offen gelassene Entscheidung, jede Abweichung von der Spec
  und jeden bewusst nicht behobenen Fund. Nummer steht im Auftrag; fehlt
  sie, `XXXX` als Platzhalter (wird beim Merge vergeben).
- **Changelog**: nicht `CHANGELOG.md` direkt ändern, sondern ein Fragment
  `changelog.d/<spec-nummer>-<thema>.md` (deutsch, nutzerrelevant). Grund:
  parallele Coder würden sonst dieselben Zeilen ändern.
- **Version nie selbst erhöhen.**
- **Bericht**: was umgesetzt ist (in Nutzersprache), Teil-0-Befund, Review-
  Funde behoben / bewusst nicht behoben mit Grund, **manuelle Testabläufe**
  (nummeriert, inkl. nötiger Server-Einrichtung), Commit-Liste.

## Was bei diesem Produkt zählt

- Kernversprechen: volle Transparenz und Kontrolle über jedes Kommando, das
  einen Server erreicht. Die KI ist Copilot, nie autonomer Akteur.
- Sicherheit vor Bequemlichkeit — aber keine Bevormundung bei manuellen
  Aktionen (Dateibrowser, Terminal sind vertrauenswürdige Nutzeraktionen).
- Ehrliche Hinweise statt Verharmlosung.
- Bei langen Aufgaben kurze Zwischenstände statt langer Funkstille.
- Wird ein Befehl per Berechtigung abgelehnt (z. B. `rm`), ihn zum
  Selbst-Ausführen anbieten, nicht umgehen.
