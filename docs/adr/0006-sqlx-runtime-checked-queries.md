# 0006-sqlx-runtime-checked-queries

## Status
Accepted

## Kontext

`docs/specs/0004-sqlite-persistence.md`, Abschnitt 2, begründet die Wahl von
`sqlx` (statt z. B. `rusqlite`) explizit auch damit:

> **`sqlx`** mit SQLite-Backend ... Begründung: compile-time-geprüfte
> Queries (`sqlx::query!`), nativer async/await-Support ...

`sqlx::query!`/`sqlx::query_as!` (die Makro-Varianten) prüfen SQL-Strings
beim Kompilieren gegen ein echtes Datenbankschema und lehnen z. B. Tippfehler
in Spaltennamen oder falsche Typen als Compile-Fehler ab. Das setzt aber
voraus, dass beim `cargo build`/`cargo check` entweder:

- eine laufende Datenbank unter der Umgebungsvariable `DATABASE_URL`
  erreichbar ist (mit bereits angewendeten Migrationen), oder
- ein vorab über `cargo sqlx prepare` erzeugter, eingecheckter Offline-
  Query-Cache (`.sqlx/`-Verzeichnis) im Repository liegt.

Beides existierte zum Zeitpunkt dieser Implementierung nicht: kein
`sqlx-cli` installiert, keine `DATABASE_URL` in der lokalen/CI-Umgebung
konfiguriert, kein `.sqlx`-Cache im Repo. Beim Implementieren von
`crates/persistence-sqlite/src/store.rs` musste diese Frage also praktisch
beantwortet werden, nicht nur theoretisch.

## Entscheidung

Alle Datenbankzugriffe in `SqliteProfileStore` nutzen die **Runtime-API**
(`sqlx::query(...)`, `sqlx::query_scalar(...)` mit manueller
`.bind()`/`.get()`-Extraktion), nicht die compile-time-geprüften Makros. Das
Zeilen-Mapping (`row_to_group`/`row_to_server` in `store.rs`) erfolgt
manuell statt über `#[derive(sqlx::FromRow)]` oder `query_as!`.

Begründung: Die Runtime-API braucht weder eine laufende DB noch einen
Offline-Cache beim Bauen — `cargo build`/`cargo test` funktionieren ohne
zusätzliche Tooling-Infrastruktur, genau wie jeder andere Rust-Crate in
diesem Workspace. Das passt zum in Spec 0001 Abschnitt 5 festgehaltenen
Entwicklungsprozess ("Test-Gate vor jedem Commit", lokal und in CI ohne
Sonderaufwand) — ein `DATABASE_URL`- oder `.sqlx`-Erfordernis hätte sowohl
diese Session als auch die spätere CI-Pipeline (`ci.yml`, Matrix-Build auf
drei Betriebssystemen) zusätzlich verkompliziert.

## Konsequenzen

**Positiv:**
- `cargo build -p persistence-sqlite` funktioniert ohne jede Vorbereitung
  (keine `sqlx-cli`-Installation, kein `DATABASE_URL`, kein eingecheckter
  Cache), genau wie jedes andere Crate im Workspace.
- Keine Gefahr eines veralteten/inkonsistenten `.sqlx`-Caches, der im
  schlimmsten Fall falsche Sicherheit vortäuscht (Cache zeigt "geprüft OK",
  Schema hat sich seither aber geändert).

**Negativ / Trade-off:**
- **Der von der Spec explizit genannte Vorteil ("compile-time-geprüfte
  Queries") entfällt vollständig.** Ein Tippfehler in einem Spaltennamen
  oder eine falsche Typannahme in `row.get::<T, _>("spalte")` fällt aktuell
  erst zur Laufzeit auf (im schlimmsten Fall erst in einem Test, im
  schlimmsten Fall erst produktiv), nicht beim Kompilieren.
- Das manuelle Zeilen-Mapping (`row_to_group`/`row_to_server`) ist reiner
  Boilerplate-Code, der bei jeder Schema-Änderung von Hand nachgezogen
  werden muss, statt dass der Compiler das erzwingt.
- Sollte künftig `sqlx-cli` und eine feste CI-Konvention für
  `cargo sqlx prepare` eingeführt werden, ist ein Wechsel auf die
  Makro-Varianten sinnvoll — diese ADR hält fest, dass das eine bewusst
  vertagte, keine vergessene Verbesserung ist.
