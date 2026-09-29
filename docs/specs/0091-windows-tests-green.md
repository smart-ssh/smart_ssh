# Spec 0091 — Tests green on Windows

Status: Vorschlag (Architekt) · Backlog: BL-0285, BL-0281 · Gate: —
Zweck: `cargo test` kommt im CI-Job `Test (windows-latest)` durch
`app-logic` hindurch (weitere Crates: R1); dabei
wird ein echter Fehler behoben: Der ssh_config-Import folgt unter Windows
keinem `Include`.
Review-Priorität: NORMAL (in `key_files` ändern sich nur Tests, kein
Produktpfad, §9; die Import-Grenzen bleiben und sind durch T3 und
bestehende Tests abgesichert, §6)

## 1. Ist-Stand (Stand `fe539eb`)

**CI-Lauf `36537624639`** (erster Lauf nach Spec 0089): Clippy unter
Windows grün. `cargo test` scheitert in `app-logic` (Lib-Tests) mit
14 von 507 Tests; `cargo test` bricht danach ab, die übrigen Crates liefen
unter Windows nicht.

**F1 — Include wird nie gefolgt (12 Tests in `ssh_config_import::tests`).**
Aus dem Code hergeleitet, zum Protokoll passend, nicht unter Windows
ausgeführt (`crates/app-logic/src/ssh_config_import.rs`): `Walker::visit` kanonisiert jeden Pfad; unter Windows liefert
`std::fs::canonicalize` Pfade mit dem Präfix `\\?\` (im Protokoll:
`\\?\C:\Users\runneradmin\…\unten\config`). `resolve_include` setzt den
Include-Wert mit diesem Verzeichnis zusammen und prüft `has_wildcard`
(`*`, `?`, `[`) über den **ganzen** Pfad. Das `?` des Präfixes macht jeden
Include zum Muster; weil es im Verzeichnisteil steht, endet die Auflösung
als `IncludeNoMatch`. Jeder Include wird verworfen, die eingebundenen
Dateien erscheinen weder als gelesen noch als unlesbar.
Dieselbe Ursache trifft auf allen Plattformen ein Verzeichnis, dessen Name
`*`, `?` oder `[` enthält (hergeleitet, nicht gemessen).

**F2 — `key_files::tests::test_directory_is_rejected`.** Unter Windows
liefert das Lesen eines Verzeichnisses `NotReadable` statt
`NotARegularFile` (Protokoll). Vermutlich scheitert dort schon das
Öffnen oder das Lesen der Metadaten, bevor die Prüfung auf eine reguläre
Datei läuft; das Protokoll zeigt nur `NotReadable`.

**F4 — Pfadvergleich als Zeichenkette.** `t_6_2_4` prüft den angezeigten
Pfad mit `ends_with("unten/tief.conf")` auf dem String; unter Windows
trennt der kanonische Pfad mit `\`. Der Test bliebe nach F1 rot. Die
übrigen Pfadvergleiche der Import-Tests prüfen nur Dateinamen oder
`is_absolute` und sind unkritisch.

**F3 — `key_files::tests::test_path_containing_a_nul_byte_fails_cleanly`.**
Der Test benutzt den Pfad `/tmp/key\0extra`; der ist unter Windows nicht
absolut, das Ergebnis ist `PathNotAbsolute` statt eines Lesefehlers. Der
Test prüft damit unter Windows nicht, was er prüfen soll.

## 2. Teil 0

Teil 0: entfällt. F1 ist hergeleitet; auf Unix belegen T1 und T2 die
Ursache (sie würden heute scheitern, hergeleitet aus `resolve_include`). Unter Windows ist nichts gemessen: Ein
Testlauf unter wine ließ sich in dieser Umgebung nicht aufsetzen (zwei
Versuche, wine startete nicht). Unbelegt bleibt damit, ob nach A1 und A3a
unter Windows noch Tests aus F1 scheitern, etwa weil ein Include-Wert mit
`/` oder `..` auf einem `\\?\`-Pfad anders zusammengesetzt wird. Das zeigt
der CI-Lauf nach dem Push (R1), dank A4 vollständig.

## 3. Ziel und Nicht-Ziele

Ziel: A1–A5 (mit A3a).

Nicht-Ziele:
- Keine Änderung an der Verarbeitung von Schlüsseldateien (F2 wird im Test
  festgehalten, nicht im Code geändert — Klarstellung §9).
- Keine weiteren Windows-Anpassungen ohne Befund aus der CI (R1).

## 4. Anforderungen

- **A1 MUSS (F1):** Ob ein `Include` ein Muster ist, entscheidet allein der
  Wert, den die Datei nennt — nicht das Verzeichnis, in dem die
  einbindende Datei liegt, und kein Präfix der Plattform. Auf allen
  Plattformen gilt:
  - Ein Include ohne `*`/`?`/`[` im Wert wird als einzelne Datei gefolgt,
    auch wenn der kanonische Pfad der einbindenden Datei solche Zeichen
    enthält.
  - Platzhalter im letzten Teil des Werts werden wie bisher aufgelöst,
    auch in einem Verzeichnis, dessen Pfad solche Zeichen enthält.
  - Platzhalter in einem Verzeichnisteil **des Werts** werden wie bisher
    abgelehnt.
- **A2 MUSS:** Alle übrigen Regeln des Imports bleiben unverändert
  (Tiefe, Besuchtmenge, Größen- und Zeilengrenzen, Reihenfolge).
- **A3 MUSS (F2, F3):** Die beiden `key_files`-Tests prüfen auf jeder
  Plattform, was sie prüfen sollen: F2 erwartet unter Windows
  `NotReadable`, sonst weiter `NotARegularFile`; F3 benutzt einen Pfad,
  der auf der jeweiligen Plattform absolut ist, und erwartet weiter einen
  sauberen Lesefehler.
- **A3a MUSS (F4):** Tests des Imports, die Pfade vergleichen, vergleichen
  Pfadbestandteile, nicht Zeichenketten mit einem bestimmten Trenner, und
  prüfen dabei weiter dasselbe.
- **A4 MUSS:** Der CI-Schritt `cargo test` läuft mit `--no-fail-fast`,
  damit ein Lauf alle scheiternden Test-Binaries zeigt und nicht nur das
  erste. Sonst keine Änderung am Workflow.
- **A5 MUSS:** Ein Changelog-Fragment `changelog.d/0091-windows-include.md`
  (Kategorie „Behoben", Verfahren nach `changelog.d/README.md`, nicht
  `CHANGELOG.md` direkt) nennt die Korrektur des Imports unter Windows.

## 5. Design

Nichts vorzugeben.

## 6. Sicherheits-Invarianten

Die Grenzen des Imports gegen große oder zyklische Konfigurationen
(Tiefe, Besuchtmenge über kanonische Pfade, Größen- und Zeilengrenzen)
bleiben unverändert und greifen jetzt auch unter Windows für eingebundene
Dateien. Die Regel, dass Platzhalter nur im letzten Teil des Include-Werts
stehen dürfen (Annahme A-2 aus Spec 0075, Schutz gegen das Ablaufen fremder
Verzeichnisbäume), bleibt; A1 verschiebt nur, **worauf** sie geprüft wird.
Abgesichert durch T3 und den bestehenden Test zu A-2. Schlüsseldateien:
kein Produktcode geändert.

## 7. Tests

- **T1 (A1, Unix, scheitert heute):** Einbindende Datei liegt in einem
  Verzeichnis, dessen Name `?` enthält; `Include` ohne Platzhalter →
  die eingebundene Datei wird gelesen.
- **T2 (A1, Unix, scheitert heute):** dasselbe mit `[` im
  Verzeichnisnamen und einem `Include *.conf` → alle passenden Dateien
  werden gelesen.
- **T3 (A1, Unix, Wächter, heute grün):** Die einbindende Datei liegt in
  einem Verzeichnis mit `?` im Namen; im selben Verzeichnis wie die
  einbindende Datei existiert ein Verzeichnis, das wörtlich `sub*` heißt,
  mit `x.conf` darin. `Include sub*/x.conf` →
  `IncludeNoMatch`, `x.conf` wird nicht gelesen. Fängt einen Fix, der
  Platzhalter nur noch im letzten Teil prüft.
- **T4 (A1, A2, A3a):** Die 12 Tests aus F1 laufen unter Unix grün wie
  vorher; geändert ist nur der Pfadvergleich in `t_6_2_4` (A3a).
- **T5 (A3):** Die beiden Tests aus F2/F3 laufen auf Unix unverändert
  grün; der Windows-Zweig ist im Diff sichtbar.
- Nachweis: T1 und T2 scheitern gegen den Stand vor der Änderung
  (Repo-`CLAUDE.md`, Abschnitt Testing); `cargo test --workspace`
  `passed` steigt um die Zahl der neuen Tests, `ignored` bleibt gleich.

## 8. Offene Punkte

Keine.

**R1 (bewusst offen):** Die übrigen Crates liefen unter Windows noch nie
durch `cargo test`. Weitere Befunde zeigt erst der CI-Lauf nach dem Push
(dank A4 in einem Lauf vollständig); sie folgen als eigene Runde.

## 9. Klarstellungen

- 2026-09-29 (Architekt, K2): F2 wird im Test als Plattformverhalten
  festgehalten, nicht im Code angeglichen. Ein Verzeichnis als Schlüssel
  wählt kaum jemand; eine Angleichung hieße, die Reihenfolge aus Öffnen
  und Prüfen im Credential-Pfad anzufassen.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `test(app-logic): cover include resolution below directories with glob characters [BL-0285]` — T1, T2 rot; T3 als Wächter.
2. `fix(app-logic): decide include wildcards on the written value only [BL-0285]` — A1, A2.
3. `test(app-logic): make path and key file tests platform-correct [BL-0281, BL-0285]` — A3, A3a.
4. `ci: run cargo test without stopping at the first failing binary [BL-0281]` — A4.

A5 gehört in den Commit von Schritt 2.

**Priorität:** NORMAL.

**Aufteilung:** ein Lauf, Sonnet.

**Berührte Module:** `crates/app-logic/src/ssh_config_import.rs` samt
Tests, `crates/app-logic/src/key_files.rs` (nur Tests),
`.github/workflows/community.yml`, `changelog.d/0091-windows-include.md`.

**Melde zurück:** Nachweis, dass T1/T2 vorher scheitern; `passed`/`ignored`
vor/nach.
