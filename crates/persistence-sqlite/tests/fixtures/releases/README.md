# Release-Datenbank-Fixtures

Eine Datenbankdatei je veröffentlichtem Schema, jede **vom veröffentlichten
Build selbst geschrieben**. `src/tests_release_chain.rs` hebt jede Datei hier
Migration für Migration auf den aktuellen Build und prüft nach jedem
Schritt, dass keine Tabelle, Spalte oder Zeile fehlt oder sich verändert
hat; am Ende öffnet es die Datei über
`SqliteProfileStore::connect_encrypted` und liest die Daten über die Stores
zurück. `app-logic` nimmt die neueste Fixture für den Test des
wiederholten Starts (`database_startup/tests_release_upgrade.rs`). ADR 0105
begründet das Vorgehen.

| Datei | Release | Schema (höchste Migration) | Verschlüsselung |
|---|---|---|---|
| `v0.5.2.sqlite3` | 0.5.2 | 14 | Klartext (vor SQLCipher) |

Das Verzeichnis der Fixtures ist `RELEASE_FIXTURES` in
`src/test_support.rs`; `test_release_fixtures_are_registered_consistently`
prüft, dass jeder Eintrag zu seiner Datei passt.

Alle Fixtures nutzen denselben festen, nicht geheimen Wurzelschlüssel
`RELEASE_FIXTURE_ROOT_KEY` (`src/test_support.rs`) für ihren feldweise
verschlüsselten Inhalt (Chat, Prompt-Historie, Ledger) und ab SQLCipher als
den K, aus dem ihr Datenbankschlüssel abgeleitet ist.

Releases ab Issue #113 schreiben Chat, Prompt-Historie, Ledger und
Zusammenfassung nicht mehr feldweise verschlüsselt. Die Generatoren älterer
Releases (z. B. `generate_v0.5.2.rs`) nutzen die damalige API und laufen nur
gegen ihren Release-Tag, nicht in diesem Workspace.

## Beim Release eine Fixture hinzufügen

Für jedes Release, dessen Schema von dem der neuesten Fixture abweicht (das
also eine Migration mitbringt, die noch keine Fixture trägt). Releases ohne
neue Migration brauchen keine Fixture.

1. Den Release-Tag in einem eigenen Worktree auschecken, z. B.
   `git worktree add --detach target/fixture-vX.Y.Z vX.Y.Z`.
2. Dort den neuesten `generate_v*.rs` aus diesem Verzeichnis nach
   `crates/persistence-sqlite/src/gen_release_fixture.rs` kopieren und in
   `src/lib.rs` als `#[cfg(test)] mod gen_release_fixture;` eintragen.
3. Den Generator an die API des Release anpassen und **alle vorhandenen
   Marker behalten** (`*-r052`-Werte, Zeilenzahlen), damit die Prüfungen
   des Ketten-Tests weiter gelten. Für Tabellen oder Spalten, die das
   Release neu hat, Zeilen mit neuen Markern der Form `*-rXYZ` schreiben.
   - Ab SQLCipher (0.6.0 und später) die Datei so öffnen, wie das Release
     es tut: `SqliteProfileStore::connect_encrypted(&out,
     &DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY))` statt des
     Klartext-`connect`.
   - Secrets, die das Release in der Datenbank hält (Tabelle `secrets`),
     bekommen ebenfalls Marker-Werte.
4. Einmal im Worktree ausführen:
   `FIXTURE_OUT=<absoluter Pfad>/vX.Y.Z.sqlite3 cargo test -p persistence-sqlite --lib -- --ignored generate_release_fixture`
5. Die Datei als `vX.Y.Z.sqlite3` hierher kopieren, den Generator als
   `generate_vX.Y.Z.rs` (mit dem Kopf „in diesem Baum nicht kompiliert“).
6. In `RELEASE_FIXTURES` (`src/test_support.rs`) einen Eintrag anhängen
   (Release, Dateiname, höchste Migration des Release,
   `Plaintext`/`Sqlcipher`), die Tabelle oben ergänzen und Prüfungen für
   die neuen Marker in `assert_release_data_readable`
   (`src/tests_release_chain.rs`) aufnehmen.
7. `cargo test -p persistence-sqlite --lib release` laufen lassen, den
   Worktree entfernen.

Eine vorhandene Fixture nie mit einem späteren Build neu erzeugen: Der Sinn
der Datei ist, dass das Release sie geschrieben hat. Die Datei auch nicht
direkt öffnen (z. B. mit `sqlite3`) — schon das Öffnen legt `-wal`/`-shm`
daneben an; Tests arbeiten immer an einer Kopie.
