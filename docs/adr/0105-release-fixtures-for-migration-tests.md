# ADR 0105 — Release-Fixtures für die Migrationstests stammen vom Release selbst

Status: akzeptiert
Betrifft: Spec 0059, Spec 0101, ADR 0052, Issue #15

## Problem

Issue #15 verlangt, dass Datenbankmigrationen über Releases hinweg
nachweislich sicher sind: ein Ketten-Test mit einer Fixture je
veröffentlichtem Schema, ein Downgrade-Test durch den normalen Startablauf
und ein Test des wiederholten Starts samt SQLCipher-Umwandlung. Offen ließ
das Issue,

1. wie eine Fixture entsteht und was sie als „Datei dieses Release“
   ausweist,
2. was „Schritt für Schritt“ beim Ketten-Test heißt,
3. wo „der volle Startablauf“ für einen Test endet,
4. wie ein Test an eine Datenbank „mit einer unbekannten künftigen
   Migration“ kommt.

## Entscheidung

1. **Die Fixture schreibt das Release selbst.** Der Generator läuft in
   einem Worktree auf dem Release-Tag, gegen die API dieses Release; die
   Datei und der Generator werden unter
   `crates/persistence-sqlite/tests/fixtures/releases/` eingecheckt
   (`v0.5.2.sqlite3`, `generate_v0.5.2.rs`). Der Generator wird im
   aktuellen Baum nicht kompiliert. Eine Fixture, die ein späterer Build
   mit den alten Migrationen nachbaut, wäre bequemer, belegt aber nicht,
   was das Release tatsächlich geschrieben hat (Store-Code, SQLite-Version,
   Journalmodus). Das Vorgehen beim Release steht in der `README.md`
   daneben.
2. **Ein Schritt ist eine Migration.** Der Ketten-Test wendet die
   Migrationen nach dem Schema der Fixture einzeln an (`sqlx`-Migrator über
   eine Teilmenge der Migrationsdateien) und vergleicht nach jedem Schritt
   ein Abbild von Schema und Inhalt (jede Zeile jeder Tabelle als
   `quote()`-Werte): Keine Tabelle, Spalte oder Zeile darf fehlen oder sich
   ändern. Eine Klartext-Fixture wird vorher umgewandelt, wie beim echten
   Update. Zum Schluss öffnet `connect_encrypted` die Datei und die Stores
   lesen die Daten Feld für Feld zurück.
3. **Der Startablauf im Test ist der Tauri-freie Teil.**
   `open_or_prepare_database` und `migrate_secrets_into_database` — genau
   das, was `app_shell::open_and_assemble` vor dem Zusammenbau des
   App-Zustands ausführt. Der Host-Key-Speicher und der App-Zustand ändern
   die Datenbank nicht und bleiben außen vor.
4. **Die künftige Migration ist eine echte.** Der Downgrade-Test wendet
   über einen `sqlx`-Migrator die Migrationen dieses Builds plus eine
   zusätzliche mit der nächsthöheren Nummer an. `_sqlx_migrations` trägt
   damit eine echte Zeile mit Prüfsumme statt einer von Hand eingetragenen.
5. **Die Helfer liegen hinter `test-support`** (`persistence_sqlite::
   test_support`), wie `connect_plaintext`: Sie öffnen Dateien am
   Startablauf vorbei, und `cargo build --workspace` fängt einen
   Produktivaufruf ab.

## Konsequenzen

- Jedes Release mit neuer Migration braucht eine eigene Fixture; ohne sie
  deckt die Kette den Sprung von diesem Release nicht ab. Ab 0.6.0 werden
  die Fixtures mit SQLCipher geschrieben (Schlüssel aus dem festen
  `RELEASE_FIXTURE_ROOT_KEY`).
- Eine künftige Migration, die Werte bewusst umschreibt (z. B. einen Typ
  von TEXT auf BLOB), lässt den Ketten-Test scheitern. Das ist gewollt:
  Wer so eine Migration schreibt, muss die erwartete Änderung im Test
  ausdrücklich zulassen.
- Beobachtet beim Downgrade-Test: Nach dem gescheiterten Öffnen wird der
  Pool fallen gelassen, ohne das Schließen abzuwarten; `-wal`/`-shm` der
  Verbindung können kurz nach der Rückkehr noch existieren. Die
  Datenbankdatei selbst ist sofort bytegleich. Der Test wartet begrenzt,
  bis die Verbindung die Datei freigibt, und vergleicht dann das ganze
  Verzeichnis.
