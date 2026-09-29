# Spec 0083 — app-shell: `orchestration.rs` und `commands.rs` in Module aufteilen

Status: **freigegeben** (2026-09-27, Auftrag „geht in die Umsetzung“) · Backlog: BL-0268 · Gate: release-1.0/E
Repo: **öffentlich** `smart-ssh` — `crates/app-shell/src/` (nur Umstrukturierung)
Review-Priorität: **NORMAL** (reine Verschiebung; die elementweise
Verschiebungs-Prüfung §6 T2 sichert den Ausführungspfad ab)
Zweck: Die beiden größten Dateien des Workspace werden in Module mit je
höchstens 2 500 Zeilen aufgeteilt, ohne dass sich Verhalten, Tests oder
öffentliche API ändern.

## 1. Ausgangslage (gemessen, Stand `a938763`)

- `crates/app-shell/src/orchestration.rs`: 14 955 Zeilen. Produktivcode bis
  zum `#[cfg(test)] mod tests` (beginnt nach `propose_note_from_chat_content`),
  danach rund 11 000 Zeilen Tests in **einem** Modul mit 186 Test-Attributen.
- `crates/app-shell/src/commands.rs`: 6 439 Zeilen, 91 `#[tauri::command]`
  (fast alle `pub`), registriert in `run` (`lib.rs`) über
  `tauri::generate_handler![commands::…]`.
- Keine andere Datei unter `crates/` und `apps/` hat mehr als 2 500 Zeilen
  (größte: `crates/core/src/filter/tests.rs`, 2 012).
- Alle Module von `app-shell` sind privat (`mod …;` in `lib.rs`). Nach
  außen sichtbar sind nur `pub fn run` und `pub use wiring::{Edition, Wiring}`.
  `AppState` und `CommandError` sind **nicht** öffentlich und werden es auch
  nicht.
- `#[tauri::command]` erzeugt im Modul der Funktion Hilfsmakros
  (`__cmd__<name>`, `__tauri_command_name_<name>`) samt einem `use` mit der
  Sichtbarkeit der Funktion (tauri-macros 2.6.3, `command/wrapper.rs`). Ein
  benannter Re-Export `pub use sub::connect` bringt diese Hilfsmakros nicht
  mit; ein Glob-Re-Export `pub use sub::*` oder ein geänderter Pfad in
  `generate_handler!` schon (nachgelesen, nicht kompiliert; A3.4 lässt
  beides zu).
- Das Repo nutzt für Untermodule bereits die Form `x.rs` + `x/`
  (`ssh_config_apply`, `ssh_config_export`, `ssh_config_import`).

## 2. Ziel und Nicht-Ziele

Ziel: Code und Tests der beiden Dateien liegen in thematisch geschnittenen
Modulen; jeder Leser kann einen Themenbereich laden, ohne 15 000 Zeilen zu
lesen.

Nicht-Ziele:
- **Keine** Verhaltensänderung, keine Umbenennung von Funktionen, Typen,
  Tests oder Tauri-Befehlen, keine Signaturänderung, keine „Aufräumarbeiten
  nebenbei“ (Vereinfachungen, Kommentar-Überarbeitungen, Clippy-Nachbesserungen
  über das Nötige hinaus).
- Kein neuer Crate, keine Verschiebung zwischen Crates (das ist BL-0269).
- Keine Änderungen außerhalb von `crates/app-shell/`; `apps/` bleibt
  unverändert, ein Changelog-Fragment entfällt (§7).
- Andere Dateien von `app-shell` werden nicht mit aufgeteilt, auch wenn sie
  groß sind.

## 3. Anforderungen

**A1 — Dateigröße.** Nach der Umsetzung hat keine `.rs`-Datei unter
`crates/app-shell/src/` mehr als 2 500 Zeilen. Das gilt auch für
Testdateien.

**A2 — Schnitt nach Thema.** Produktivcode und Tests werden nach
Themenbereichen geschnitten, nicht nach Zeilenzahl. Tests liegen im selben
Themenmodul wie der Code, den sie prüfen, oder in einem Testmodul mit
passendem Namen. Den Schnitt wählt der Coder; er beschreibt ihn in einer
Tabelle im Bericht (Modul → enthaltene Funktionen bzw. Tests, Zeilen).

**A3 — Nichts ändert sich nach außen.**
1. Die öffentliche Schnittstelle des Crates bleibt identisch: dieselben
   Namen unter denselben Pfaden (`app_shell::run`, `app_shell::Wiring`,
   `app_shell::Edition`), kein weiteres Element wird nach außen
   sichtbar. Neue Module sind privat.
2. Sichtbarkeiten dürfen **innerhalb des Crates** erweitert werden, soweit
   die Aufteilung es verlangt (`pub(super)`, `pub(crate)`), nie auf `pub`
   für ein bisher nicht-`pub` Element.
3. Jeder Test existiert danach mit identischem Namen; nur sein Modulpfad darf
   sich ändern. Kein Test entfällt, keiner kommt hinzu, keiner wird
   `#[ignore]`.
4. Die Menge der registrierten Tauri-Befehlsnamen (die Namen, unter denen
   das Frontend `invoke` aufruft) bleibt identisch. Die Pfade in
   `generate_handler!` dürfen sich ändern, wenn der Compiler es verlangt.
5. `#[cfg(test)]`-Hilfen bleiben `#[cfg(test)]`; kein Testcode wandert in
   den Produktivbau.

**A4 — Verschieben, nicht abschreiben.** Funktions- und Testkörper werden
mechanisch verschoben (Ausschneiden nach Zeilenbereichen), nicht neu
getippt. Die Reihenfolge der Anweisungen innerhalb eines Elements bleibt
unverändert. Geändert werden dürfen nur: `use`-Zeilen, `mod`-Deklarationen
samt ihren Attributen (`#[cfg(test)]`), Modulklammern (`mod tests {` und
die schließende `}`), Sichtbarkeitsmodifikatoren, Pfadpräfixe bei Aufrufen
(`super::`, `crate::…`), Modul-Doku-Kommentare am Dateianfang und reine
Umbrüche bzw. Einrückung durch `cargo fmt`. Jede andere Änderung ist ein
Befund (T2).

**A5 — Gate grün**, wie in `CLAUDE.md` des Repos beschrieben (Rust und
Frontend). Es kommen keine neuen `#[allow(…)]`-Attribute hinzu.

## 4. Design

- Modulform wie im Repo üblich: `orchestration.rs` bleibt als Modulwurzel
  bestehen (Re-Exporte, gemeinsame Konstanten) und bekommt ein Verzeichnis
  `orchestration/`; gleiches für `commands`. Aufrufer innerhalb des Crates
  sollen möglichst unverändert bleiben — dafür re-exportiert die Wurzel, was
  andere Module von `app-shell` bisher unter `orchestration::…` bzw.
  `commands::…` erreichen.
- Das Test-Riesenmodul wird in mehrere Testmodule zerlegt. Gemeinsame
  Test-Hilfen (Fixtures, Mock-Aufbauten) kommen in ein gemeinsames
  `#[cfg(test)]`-Modul, nicht dupliziert.
- Ein Commit je Datei (s. §7), damit ein Fehlschlag sich auf eine Hälfte
  eingrenzen lässt.

Verworfen: nur die Tests auslagern (lässt `commands.rs` bei 6 400 Zeilen);
Aufteilen nach Zeilenzahl (Leser müssten weiterhin mehrere Dateien für ein
Thema laden).

## 5. Sicherheits-Invarianten

Keine Invariante wird inhaltlich berührt, aber `orchestration` enthält den
Ausführungspfad (Bestätigung, AutoExec, Schreibzugriffe, Redaktion vor dem
Senden). Deshalb gilt A4 streng, und T2 prüft, dass außer den dort
erlaubten Zeilenarten nichts anders ist. Eine Abweichung im Ausführungspfad,
die T2 findet, ist ein Blocker, kein Kleinbefund.

## 6. Tests

Es entstehen **keine neuen Tests** (A3.3). Die Abnahme sind diese Prüfungen;
der Coder fährt sie und legt die Ausgaben im Bericht ab.

- **T1 — Testnamen.** `cargo test --workspace -- --list --format terse`
  vor und nach der Umsetzung. Ausgangswert: 1 345 Einträge, davon 186 unter
  `orchestration::` und 66 unter `commands::`. Nach Entfernen des
  Modulpfads (alles bis zum letzten `::`) sind die sortierten Listen
  einschließlich Duplikaten identisch, und die Gesamtzahl bleibt 1 345. Scheitert, wenn ein Test verloren geht, doppelt
  entsteht oder umbenannt wird.
- **T2 — Verschiebung ohne Inhaltsänderung, je Element.** Für jedes
  Element der alten Datei (Funktion, Test, `const`, `static`, `type`,
  `struct`, `enum`, `trait`, `impl`-Block, samt Attributen und
  Doku-Kommentaren) wird sein Text mit dem Text desselben Elements in den
  neuen Dateien verglichen, **als geordnete Folge** und nach Entfernen allen
  Leerraums (so fallen `cargo fmt`-Umbrüche und Einrückung heraus, eine
  Umstellung von Anweisungen aber nicht; Leerraum in String-Literalen wird
  bewusst mit entfernt, weil mechanisches Ausschneiden ihn nicht berührt).
  „Dasselbe Element“: Schlüssel ist Art plus Name bzw. `impl`-Kopf; bei
  mehreren gleichen Schlüsseln werden die Elemente paarweise zugeordnet.
  Inline-Module (`mod tests { … }`, `mod …_tests { … }`) sind Behälter und
  werden nicht selbst verglichen, ihr Inhalt schon. Ergebnis im Bericht:
  (a) Zahl der Elemente alt und in den neuen Dateien zusammen, gleich;
  (b) jedes Element mit Abweichung samt Abweichung. **Innerhalb eines
  Elements** zulässig sind nur Sichtbarkeit und Pfadpräfixe (A4); jedes
  geänderte Pfadpräfix wird einzeln mit „löst auf dasselbe Element auf wie
  vorher“ begründet. Scheitert, wenn beim Verschieben eine Zeile verändert,
  umgestellt, verloren oder erfunden wird. Ein Prüfskript dafür liefert der
  Auftrag mit; es wird nicht committet.
- **T2b — `use`-Zeilen je neuer Datei.** `use`-Zeilen stehen auf
  Modulebene und haben kein altes Gegenstück. Der Bericht listet sie je
  neuer Datei; jede Glob-Zeile (`use super::*`) nennt ihr Zielmodul, und
  jeder ausdrückliche Import, der einen über einen Glob erreichbaren Namen
  verdeckt, wird begründet. Scheitert, wenn ein Test oder eine Funktion
  still ein gleichnamiges anderes Element einbindet.
- **T3 — Dateigröße.** Zeilenzahl aller `.rs`-Dateien unter
  `crates/app-shell/src/`; Maximum ≤ 2 500 (A1).
- **T4 — Öffentliche Schnittstelle.** `git diff` auf `lib.rs` zeigt keine
  geänderte `pub`-Zeile außer ggf. geänderten Pfaden in
  `generate_handler!`; `git diff --stat -- apps/` ist leer. Zusätzlich:
  kein neues `pub mod` in `lib.rs`.
- **T5 — Befehlsnamen.** Die letzten Pfadsegmente der Einträge in
  `generate_handler!` sind vor und nach der Umsetzung als sortierte Liste
  identisch (A3.4).
- **T6 — Gate** vollständig grün (A5).

## 7. Umsetzungsreihenfolge

1. Ausgangswerte festhalten: T1-Liste, T5-Liste, Zeilenzahlen.
2. `orchestration.rs` aufteilen (Produktivcode und Tests), Gate, T1–T4 (mit T2b),
   Commit `refactor(app-shell): split orchestration into modules [BL-0268]`.
3. `commands.rs` aufteilen, Gate, T1–T5 (mit T2b), Commit
   `refactor(app-shell): split commands into modules [BL-0268]`.
4. **Kein** Changelog-Fragment: `changelog.d/` nimmt nur nutzerrelevante
   Änderungen auf, und diese Umstrukturierung hat keine Nutzerwirkung.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)
