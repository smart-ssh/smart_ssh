# ADR 0083 — Entscheidungen beim Umstieg der CI-Gates auf blockierend (Spec 0090)

Status: akzeptiert · Spec: `docs/specs/0090-ci-gates-blocking.md` · Backlog:
BL-0229, BL-0228, BL-0056, BL-0094, BL-0284, BL-0047

Spec 0090 ließ Design und Aufteilung bewusst offen („Design: Nichts
vorzugeben"). Dieses ADR hält fest, welche Wege bei der Umsetzung gewählt
wurden, welcher Review-Fund wie behoben wurde und was bewusst stehen
geblieben ist.

## 1. `rustls`-Update per Hand statt `cargo update -p rustls`

A1 verlangt, dass der Lockfile-Diff **ausschließlich** `rustls` ändert.
`cargo update -p rustls` (auch mit `--precise 0.23.45`) tut das nicht: Der
Resolver wählt dabei zusätzlich für `tempfile`s `getrandom`-Abhängigkeit
eine andere, bereits im Lockfile vorhandene Version (`0.4.3` → `0.3.4`) —
eine Nebenwirkung, die die im Ist-Stand der Spec dokumentierte
`--dry-run`-Messung nicht zeigte, weil deren Textausgabe nur Pakete nennt,
deren *eigene* Version sich ändert, nicht verschobene Abhängigkeits-Kanten.

Verifiziert per Hand-Edit: `rustls`s Versions- und Prüfsummen-Zeile im
Lockfile direkt auf `0.23.45`/die neue Prüfsumme gesetzt, dann
`cargo check --workspace --locked` — Exit 0, keine weitere Änderung nötig.
Das beweist, dass die Tempfile/getrandom-Verschiebung eine ungezwungene
Vereinfachung des Resolvers ist, keine Voraussetzung des Updates. Der
committete Lockfile-Diff ist deshalb der Hand-Edit, nicht das
`cargo update`-Ergebnis.

## 2. A4-Abgleich: eigener Workflow-Schritt statt `cargo deny check advisories`

Spec 0090 §1 schließt den `cargo-deny`-Weg für A4 explizit aus (falsch
gemeldetes `RUSTSEC-2024-0429` als „nicht erkannt", zweite Ausnahmeliste
nötig). Stattdessen: ein eigener Bash-Schritt, der `cargo audit --json` in
einem Verzeichnis ohne `.cargo/audit.toml` laufen lässt und die gefundenen
RUSTSEC-IDs (`jq`) gegen die in `.cargo/audit.toml` eingetragenen IDs
(`grep`/`sed`) abgleicht.

## 3. Review-Fund Runde 1: errexit hätte den A4-Schritt immer rot gefärbt

Der `spec-reviewer` fand in Runde 1 (`.agent/BL-0229/review-01.md`) einen
kritischen Fehler: GitHub Actions führt `shell: bash`-Schritte mit
`bash -eo pipefail` aus. Die ursprüngliche Zuweisung
`json_output="$(cargo audit --json -f Cargo.lock)"` reicht unter `errexit`
den Exit-Code der Substitution an die Zuweisung selbst weiter — und
`cargo audit` ohne Ausnahmeliste endet bei **jedem** Lauf mit Exit 1
(RUSTSEC-2023-0071 hat keinen Fix), nicht nur bei einer tatsächlich
überholten Ausnahme. Ohne Gegenmaßnahme wäre die Shell an dieser Stelle bei
jedem Lauf sofort abgebrochen, bevor die eigentliche Stale-Prüfung je
lief — der Job wäre dauerhaft rot gewesen, unabhängig vom Zustand der
Ausnahmeliste. Derselbe Fehler steckte ein zweites Mal in der
`found_ids=...`-Zuweisung (`jq`-Aufruf).

Behoben (Commit `291f08f`): beide Zuweisungen laufen jetzt in einem
`if ! ... ; then ... fi`-Kontext, den `errexit` laut POSIX/bash-Semantik
ausnimmt. Verifiziert durch Extraktion des tatsächlich committeten
Skriptkörpers und Ausführung unter `bash -eo pipefail` (nicht nur ohne
`-e`, wie im ersten, dadurch unauffälligen lokalen Test
`.agent/BL-0229/test-a4.sh`): Endstand liefert Exit 0 ohne überholte
Ausnahme (entspricht N5), eine probeweise wieder eingetragene
zurückgezogene ID liefert Exit 1 mit der erwarteten Fehlermeldung
(entspricht N4). Runde 2 (`.agent/BL-0229/review-02.md`) bestätigt den Fix
und dass er nichts anderes lockert.

## 4. Im selben Commit mitbehoben: `ignore`-Block statt ganzer Datei durchsuchen

Ebenfalls Runde 1, als „zurückstellbar" eingestuft, aber beim ohnehin
nötigen Anfassen derselben Zeile mitbehoben: die ID-Extraktion aus
`.cargo/audit.toml` durchsuchte per `grep` die gesamte Datei, nicht nur den
`ignore = [ ... ]`-Block. Eine künftig in einem Kommentar zitierte,
quotierte RUSTSEC-ID (z. B. beim Dokumentieren einer entfernten Ausnahme)
wäre sonst fälschlich als aktive Ausnahme gezählt worden. Fix:
`sed -n '/^ignore = \[/,/^\]/p'` grenzt die Suche auf den Array-Block ein,
verifiziert gegen die tatsächlichen Zeilenanker in Runde 2.

## 5. Bewusst nicht behobene Review-Funde

- **`docs/adr/0028-rsa-marvin-attack-risk-acceptance.md`** beschreibt an
  einer Stelle weiterhin den alten Berichtsmodus-Zustand als offenen
  Folgeschritt. A7 zählt „Workflow, `.cargo/audit.toml` und `deny.toml`"
  auf; diese ADR gehört nicht zur „Berührte Dateien"-Liste von Spec 0090.
  Sachlich ist die Beschreibung jetzt überholt (genau dieser Folgeschritt
  ist mit dieser Spec erfolgt) — Korrektur folgt als eigener, kleiner
  Schritt, nicht Teil dieses Laufs.
- **Textbasierte `sed`/`grep`-Extraktion bleibt strukturell an das
  Zeilenformat von `.cargo/audit.toml` gebunden** (Runde 2, zurückgestellt).
  Verschöbe ein künftiges Tooling `ignore = [` oder die schließende `]` von
  Spaltenposition 0, läse die Prüfung aus A4 still null Zeilen und meldete
  nie eine überholte Ausnahme — ein stiller statt ein roter Ausfall. Ein
  strukturierter TOML-Parser wäre robuster; da die Datei aktuell nur von
  Hand gepflegt wird, ist das Risiko gering. Vorschlag: eigenes,
  eigenständiges Backlog-Item, falls die Datei je automatisiert bearbeitet
  wird.
