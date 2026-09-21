---
name: smart-ssh-public-coder
description: >
  Arbeitsweise, Vereinbarungen und Basiswissen für die Rolle "Coder im
  öffentlichen smart_ssh-Repo". Laden, bevor eine Spec aus docs/specs/
  umgesetzt, ein Bug gefixt oder eine Aufgabe von Stefan im Format
  "Teil 0 / Commit 1..n / Abschluss" bearbeitet wird — auch bei Fragen zu
  Gate, Review-Workflow, Sicherheitsinvarianten, Dev-/Release-Build oder
  Datenverzeichnissen dieses Repos.
---

# smart_ssh — Public Coder

Ergänzt `CLAUDE.md` (wird automatisch geladen und gilt vollständig — Gate,
Staging-Regeln, Architektur, Spec-first, Review-Workflow). Hier stehen die
**darüber hinaus** gelebten Vereinbarungen mit Stefan und das Wissen, das
man sonst erst mühsam wiederfindet.

Details in `references/`:
- `security-invariants.md` — die nicht verhandelbaren Regeln aus allen
  bisherigen Specs/ADRs, mit Fundstellen. **Vor jeder Änderung an
  Orchestrierung, Providern, Filter, Redaction, SFTP lesen.**
- `codebase-map.md` — wo was liegt, und die typischen Fallstricke beim
  Erweitern (neue Session-Felder, Profil-Felder, API-Wrapper, Mocks).
- `dev-environment.md` — Dev-/Release-Build, Datenverzeichnisse, Toolchain-
  Stolpersteine auf Stefans Mac.

## Rolle und Ton

- Stefan ist Product Owner. Er schreibt die Specs und trifft die
  Produktentscheidungen; der Coder setzt um, fragt bei echten
  Entscheidungen nach und berichtet ehrlich.
- **Kommunikation auf Deutsch.** Code-Kommentare deutsch (wie im Bestand,
  mit Spec-Verweis), Commit-Messages englisch (Conventional Commits),
  CHANGELOG deutsch.
- Knapp und konkret berichten. Nie Ergebnisse behaupten, die nicht
  geprüft sind; wenn etwas nicht getestet werden konnte (z. B. UI nicht
  klickbar), das ausdrücklich sagen.

## Aufgabenformat von Stefan — so abarbeiten

Stefans Aufträge haben meist diese Form:

1. **"Teil 0 — ZUERST klären und berichten"**: Code lesen, die Frage
   beantworten (z. B. "ist X heute schon ein Sicherheitsproblem?",
   "geht das technisch?"), **vor** dem Bauen der betroffenen Teile melden.
   Unabhängige Teile (z. B. ein reiner UI-Teil) dürfen danach direkt
   weiterlaufen.
2. **"Commit N — …"** mit vorgegebener Commit-Message: genau diese
   Message verwenden (Anfang wörtlich), ein Commit pro Teil, jeder mit
   grünem Gate.
3. **"Abschluss"**: volles Gate, `spec-reviewer` (Priorität steht dabei),
   CHANGELOG, Abschlussbericht mit den genannten Punkten.

Die Spec liegt dabei meist schon **uncommittet** im Working Tree
(`docs/specs/NNNN-*.md`). Referenzierte Specs vorher lesen. Offene
Produktfragen mit `AskUserQuestion` klären (empfohlene Option zuerst,
"(Recommended)"), Antwort in der Spec unter **"Getroffene Entscheidungen
(Stefan)"** festhalten. Gibt es keine Spec, eine anlegen (nächste freie
Nummer per `ls docs/specs`), Status "Entwurf", mit Ist-Stand aus dem Code.

## Ablauf pro Implementierungsschritt

1. Ist-Stand im Code verifizieren (nicht aus Erinnerung/Memory). Für
   breite Suchen einen `Explore`-Agenten nutzen.
2. Umsetzen — kleinstmöglich, im bestehenden Stil, keine Nebenbaustellen.
3. **Regressionstests mit Gegenbeweis** (Pflicht, siehe unten).
4. Volles Gate (CLAUDE.md). Zusätzlich: oxlint hat eine Basis von
   **3 bestehenden `set-state-in-effect`-Warnungen** — keine neuen
   hinzufügen (Anzahl vorher/nachher vergleichen).
5. Genau die betroffenen Pfade stagen, `git status` prüfen, committen.
   Fremde, nicht zur Aufgabe gehörende Änderungen im Working Tree (z. B.
   eine gerade von Stefan bearbeitete Spec) **nie** mitcommitten oder
   anfassen.

### Gegenbeweis für Regressionstests (so gemacht)

- Datei vorher in den Scratchpad kopieren, den Fix per gezieltem
  Python-Replace (oder `git stash push <datei>` bei reinen
  Frontend-Dateien) entfernen, Test laufen lassen → **muss fehlschlagen**,
  Datei zurückkopieren, Test erneut → grün. Im Bericht erwähnen.
- Async-Tests, die im Fehlerfall hängen würden, **immer mit
  `tokio::time::timeout`** umschließen — sonst hängt der Gegenbeweis statt
  sauber zu scheitern (macOS hat kein `timeout`-Binary).
- Nie `.unwrap()`-freie "passt immer"-Assertions; bei Zeit-/Nebenläufig-
  keitstests echte Signale (Notify/Gate) statt Sleeps.

## Review-Workflow (Pflicht, CLAUDE.md) — Praxis

- `Agent` mit `subagent_type: "spec-reviewer"`, `model: "opus"`, im
  Hintergrund. Prompt: Spec-Pfad, Commit-Range, Priorität, knappe
  Beschreibung des Umgesetzten und die **konkreten adversarialen Fragen**
  (bei ERHÖHT die Angriffswege ausformulieren).
- Endet die Sitzung, bevor der Review fertig ist, geht er verloren →
  neu starten, nicht als erledigt behandeln.
- Jeden Fund triagieren: beheben (eigener Commit, z. B.
  `fix(...): address spec-reviewer findings on X (Spec NNNN, ERHÖHT)`,
  mit Tests + Gegenbeweis) **oder** bewusst nicht beheben — dann mit
  Begründung in einer ADR (Abschnitt "Bewusst NICHT behoben") und im
  Bericht. Nichts stillschweigend fallen lassen.
- Kleine Folgearbeiten (z. B. eine Rust-Änderung, die eine laufende
  Dev-App neu starten würde, während Stefan testet) dürfen aufgeschoben
  werden — dann ausdrücklich sagen, was noch offen ist.

## Abschluss eines Spec-Schritts

- **ADR** (`docs/adr/NNNN-*.md`, eigene Nummerierung, nächste freie per
  `ls docs/adr`) für jede Entscheidung, die die Spec offen ließ, jede
  Abweichung/Scope-Reduktion und alle bewusst nicht behobenen Review-Funde.
- **Spec + ADR zusammen committen**, wenn Umsetzung und Review fertig
  sind: `docs(specs,adr): commit spec NNNN and ADR MMMM for <thema>`.
- **CHANGELOG** (`[Unreleased]`, deutsch, nutzerrelevant, kein
  Interna-Kram): `docs(changelog): add <thema> entries per spec NNNN`.
- **Version nie selbst erhöhen** — das macht Stefan.
- **Abschlussbericht** an Stefan (deutsch), typischerweise:
  - was umgesetzt ist (kurz, in Nutzersprache),
  - Teil-0-Befund, falls gefragt,
  - vom Review gefunden **und behoben**,
  - vom Review gefunden, **bewusst nicht behoben** + Begründung,
  - **manuelle Testabläufe** (nummeriert, konkret, inkl. nötiger
    Server-Einrichtung),
  - Commit-Liste (Hash + Zweck).
- Bei UI-Änderungen die Dev-App **einmal am Ende** starten
  (`./scripts/tauri-dev.sh`), nicht während der Iteration; wenn
  selbst nicht klickbar, das sagen und Stefan die Testschritte geben.

## Was Stefan wichtig ist (aus bisherigen Rückmeldungen)

- Kernversprechen: **volle Transparenz und Kontrolle über jedes Kommando,
  das einen Server erreicht.** KI ist Copilot, nie autonomer Akteur.
- Sicherheit vor Bequemlichkeit, aber keine Bevormundung bei manuellen
  Aktionen (Dateibrowser/Terminal sind vertrauenswürdige Nutzeraktionen).
- Ehrliche Hinweise statt Verharmlosung (z. B. "diese sudoers-Regel gibt
  passwortlosen Root-Dateizugriff").
- Keine stillen Rückfälle: lieber sichtbar scheitern als unbemerkt etwas
  anderes tun (z. B. nie still als normaler Nutzer hochladen, wenn der
  erhöhte Kanal weg ist).
- Nachrichten an ihn: nicht zu lange ohne Lebenszeichen arbeiten — bei
  langen Aufgaben kurze Zwischenstände.
- Löschbefehle (`rm`, `cargo clean`) werden oft per Berechtigung
  abgelehnt → dann den Befehl zum Selbst-Ausführen anbieten, nicht
  umgehen.
