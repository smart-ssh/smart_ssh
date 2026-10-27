---
name: smart-ssh-coder
description: >
  Interactive working method for implementing issues, bug fixes and specs in
  the public smart_ssh repo: regression tests with counter-proof, security
  changes only tighten, review triage, repo-research for broad searches. Load
  before implementing a GitHub issue or a spec from docs/specs/.
---

# smart_ssh — Coder-Arbeitsweise

> Optional helper for interactive Claude Code sessions. Project rules live in `AGENTS.md`; on conflict, `AGENTS.md` and the issue win.

Ergänzt `AGENTS.md` (Gate, Architektur, Spec-Workflow, Changelog- und
Testkonventionen gelten dort vollständig und werden hier nicht wiederholt).
Der Ablauf einer Sitzung steht im Agenten `smart-ssh-coder`.

Vor Änderungen an Orchestrierung, Providern, Filter-Engine, Redaction oder
SFTP: `references/security-invariants.md` lesen. Für das Finden von
Erweiterungspunkten: `docs/codemap.md` (`references/codebase-map.md` verweist darauf).

## Auftrag

Auftrag ist ein **GitHub-Issue** (Goal, Requirements, Acceptance Criteria,
Spec impact) oder eine Spec. Die Spec beschreibt das Soll-Verhalten
(`docs/specs/README.md`) und wird im selben Pull Request nach dem
Spec-Impact nachgezogen (`AGENTS.md`, „Spec workflow").

**Code gegen das Issue prüfen, Widerspruch melden.** Sieht der Code anders
aus, als das Issue ihn beschreibt (eine Funktion fehlt, ein Verhalten ist
schon behoben), hältst du an und meldest das der Person, die die Sitzung
führt — du passt die Umsetzung nicht stillschweigend an.

Offene Produktfragen: mit empfohlener Option zuerst vorlegen, nicht raten.
Die Antwort im Issue festhalten.

## Breite Suchen: `repo-research`

Für Fragen wie „wo wird X verwendet", „welche Stellen konstruieren Y",
„was hat sich seit Commit Z geändert" startest du den Subagenten
**`repo-research`** (Agent-Werkzeug) statt selbst große Dateien oder lange
Trefferlisten zu lesen. Er läuft auf Sonnet, liest nur und antwortet in
höchstens 40 Zeilen mit Fundstellen — die langen Ausgaben bleiben in seinem
Kontext, nicht in deinem. Mehrere unabhängige Fragen in einer Nachricht,
dann laufen sie gleichzeitig.

Seine Grenze: Er liefert Fundstellen, keine Urteile. Was den Kern einer
Sicherheitsänderung trägt, liest und prüfst du selbst.

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

Nach jedem Implementierungsschritt mit eigenem Commit rufst du den
`spec-reviewer` ausdrücklich auf (Aufruf und Priorität: Agent
`smart-ssh-coder`, Schritt 5). Den Bericht vollständig lesen, dann jeden
Fund triagieren:
- **beheben** — Gate wieder grün, eigener Commit mit Test und Gegenbeweis, oder
- **bewusst nicht beheben** (Fehlalarm, außerhalb dieses Schritts,
  Designentscheidung) — mit Begründung im PR und ggf. in der ADR.
Nichts stillschweigend fallen lassen; beide Teile (behoben / bewusst nicht
behoben) gehören in den PR-Text. Hat eine Nachbesserung selbst etwas
gelockert, eine weitere Runde.

**Runde 2 prüft nur das Delta:** die Funde der ersten Runde mit deiner
Triage, die Commit-Range seit Runde 1 und den Auftrag zu prüfen, ob die Funde
behoben sind und ob die Nachbesserungen etwas gelockert haben. Eine volle
Runde nur, wenn die Nachbesserung den Kern umbaut.

## Keine Personennamen

In Specs, ADRs, Code-Kommentaren, Changelog und Commit-Messages steht kein
Personenname. Eine Freigabe heißt nur „freigegeben“, eine Entscheidung nur
„entschieden“ — ohne Namen und ohne Datum. Ausgenommen ist nur ein
Lizenz- oder Copyright-Text, der wörtlich zitiert wird.

## Abschluss

Gate, Spec, ADR und Changelog-Fragment nach `AGENTS.md` („Before every
commit", „Spec workflow", „Versioning & changelog", Details in
`changelog.d/README.md`); ADR-Nummer vor dem Push gegen den Basis-Branch
prüfen. Der PR-Text nennt je Acceptance Criterion den Test, der es prüft,
die Gegenbeweise, Annahmen und nummerierte manuelle Testschritte für alles,
was nur in der Oberfläche prüfbar ist. Was nicht getestet werden konnte,
sagst du ausdrücklich.

## Was bei diesem Produkt zählt

- Kernversprechen: volle Transparenz und Kontrolle über jedes Kommando, das
  einen Server erreicht. Die KI ist Copilot, nie autonomer Akteur.
- Sicherheit vor Bequemlichkeit — aber keine Bevormundung bei manuellen
  Aktionen (Dateibrowser, Terminal sind vertrauenswürdige Nutzeraktionen).
- Ehrliche Hinweise statt Verharmlosung.
- Wird ein Befehl per Berechtigung abgelehnt (z. B. `rm`), ihn zum
  Selbst-Ausführen anbieten, nicht umgehen.
