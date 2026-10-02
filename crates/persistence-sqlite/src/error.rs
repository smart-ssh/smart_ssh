use std::fmt;

/// Fehler beim Aufbau der Datenbankverbindung (`SqliteProfileStore::connect`).
///
/// Getrennt von `ssh_manager_core::profiles::ProfileError`: Verbindungs-/
/// Migrations-Fehler passieren, bevor überhaupt ein funktionsfähiger
/// `ProfileStore` existiert, gehören also nicht in dessen Fehlertyp (der für
/// Zugriffe *auf* einen bestehenden Store gedacht ist). Konkrete
/// `ProfileStore`-Trait-Methoden geben weiterhin `ProfileResult` zurück, mit
/// `sqlx`-Fehlern über `ProfileError::Backend(err.to_string())` abgebildet
/// (kein `From<sqlx::Error> for ProfileError` möglich/gewollt — `core` darf
/// laut Spec 0004 Abschnitt 1 keine `sqlx`-Abhängigkeit bekommen, und Rusts
/// Orphan-Rules verbieten den Impl ohnehin von hier aus).
#[derive(Debug)]
pub enum PersistenceError {
    Connect(sqlx::Error),
    Migrate(sqlx::migrate::MigrateError),
    /// Spec 0101, A4: Die Datei ließ sich mit dem übergebenen
    /// Datenbankschlüssel nicht lesen — Grundlage für Dialog D2.
    ///
    /// **Ohne Nutzlast, und das ist der Punkt:** SQLite liefert hier je nach
    /// Zeitpunkt `code 26 „file is not a database"` oder (wenn die Migration
    /// zuerst läuft) `code 7 „out of memory"`; beide Texte sagen einem
    /// Nutzer das Falsche, und beide sind von „Datei beschädigt" nicht zu
    /// unterscheiden (gemessen, Spec 0101 §1). Der Fehler trägt deshalb
    /// keinen Bibliothekstext — und damit auch keinen, in dem je ein
    /// `PRAGMA key`-Wert stehen könnte (A2).
    NotReadableWithKey,
    /// Spec 0101, A6: Die Umwandlung der Klartext-Datei ist gescheitert. Das
    /// Original ist inhaltsgleich, es liegt keine Zwischendatei mehr, und es
    /// wurde nichts umbenannt — es gibt also **keinen** Weiterlauf im
    /// Klartext, nur einen Startfehler.
    Conversion(crate::encryption::ConversionFailure),
    /// Spec 0101, A2: Ein Fehler aus dem Verbindungsaufbau **mit
    /// Schlüssel** enthielt den Schlüssel in seinem Text und wurde deshalb
    /// vollständig verworfen.
    ///
    /// Gemessen und kein theoretischer Fall: `sqlx` zitiert in einem
    /// Syntaxfehler die Anweisung, die ihn ausgelöst hat — und das ist bei
    /// einem falsch gesetzten Schlüssel-Pragma die Anweisung mit dem
    /// Schlüssel darin (s. `encryption::contains_key_material`). Ein
    /// Fehlertext ohne Schlüssel behält seine Variante; nur dieser Fall
    /// verliert sie.
    RedactedConnect,
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistenceError::Connect(e) => write!(f, "Datenbankverbindung fehlgeschlagen: {e}"),
            PersistenceError::Migrate(e) => write!(f, "Migration fehlgeschlagen: {e}"),
            PersistenceError::NotReadableWithKey => write!(
                f,
                "die Datenbankdatei ist mit dem vorhandenen Schlüssel nicht lesbar"
            ),
            PersistenceError::Conversion(e) => {
                write!(f, "Umwandlung der Datenbank fehlgeschlagen: {e}")
            }
            PersistenceError::RedactedConnect => write!(
                f,
                "Datenbankverbindung fehlgeschlagen (Details ausgelassen, weil sie \
                 Schlüsselmaterial enthielten)"
            ),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersistenceError::Connect(e) => Some(e),
            PersistenceError::Migrate(e) => Some(e),
            PersistenceError::NotReadableWithKey | PersistenceError::RedactedConnect => None,
            PersistenceError::Conversion(e) => Some(e),
        }
    }
}

impl From<crate::encryption::ConversionFailure> for PersistenceError {
    fn from(e: crate::encryption::ConversionFailure) -> Self {
        PersistenceError::Conversion(e)
    }
}

impl From<sqlx::Error> for PersistenceError {
    fn from(e: sqlx::Error) -> Self {
        PersistenceError::Connect(e)
    }
}

impl From<sqlx::migrate::MigrateError> for PersistenceError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        PersistenceError::Migrate(e)
    }
}

pub type PersistenceResult<T> = Result<T, PersistenceError>;

/// Spec 0059 (fataler Startfehler-Dialog): grobe Klassifizierung eines
/// [`PersistenceError`] aus `SqliteProfileStore::connect`, für den
/// Aufrufer (`app-shell`) OHNE eigene `sqlx`-Abhängigkeit — `sqlx`-Typen
/// bleiben vollständig in dieser Crate eingekapselt (dieselbe Grenze wie
/// beim `core`/`sqlx`-Verbot oben, hier nur zwischen `persistence-sqlite`
/// und `app-shell` statt `core`). Absichtlich nur drei grobe Fälle statt
/// aller `sqlx::Error`/`MigrateError`-Varianten einzeln durchzureichen:
/// `app-shell` braucht für den Dialogtext nur "welcher der drei
/// benannten Fälle (1/2/4 aus Spec 0059) ist das", nicht die volle
/// Fehlertaxonomie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectFailureKind {
    /// Spec 0059, Fall 1: die DB trägt eine Migration, die dieses Binary
    /// nicht kennt (`MigrateError::VersionMissing`) — die DB wurde von
    /// einer NEUEREN Programmversion angelegt.
    SchemaTooNew {
        applied_version: i64,
        max_known_version: i64,
    },
    /// Spec 0059, Fall 4: Datenverzeichnis/-datei nicht beschreibbar oder
    /// lesbar (`std::io::ErrorKind::PermissionDenied`).
    PermissionDenied,
    /// Spec 0059, Fall 2 (und Auffangfall für jeden anderen, nicht
    /// spezifisch benannten `PersistenceError`) — "DB korrupt/nicht
    /// lesbar" ist ohnehin die generische, sichere Fehlermeldung für
    /// "irgendetwas beim Öffnen/Migrieren ist schiefgegangen", passt also
    /// auch als Rückfalloption.
    Other,
    /// Spec 0101, D2: Die Datei ist mit dem vorhandenen Schlüssel nicht
    /// lesbar (oder es gibt keinen Schlüssel zu einer verschlüsselten
    /// Datei).
    ///
    /// **Eigener Fall und nicht [`Self::Other`]**, obwohl „beschädigt" auch
    /// hier zutreffen könnte: Der `Other`-Text rät als Erstes zu einer
    /// Backup-Wiederherstellung (s. `app_logic::startup_error_messages`).
    /// Genau das ist bei einem Schlüsselproblem der falsche Rat — das Backup
    /// wäre mit demselben Schlüssel genauso unlesbar. D2 verlangt deshalb
    /// ausdrücklich „**kein** Hinweis auf ‚Backup einspielen‘ als erste
    /// Wahl".
    KeyMismatch,
    /// Spec 0101, A6: Die Umwandlung ist gescheitert; nichts wurde
    /// verändert.
    ConversionFailed,
    /// Spec 0101, A6 (letzter Satz): `smart-ssh.db` ist eine symbolische
    /// Verknüpfung — es wird nicht umgewandelt (T20).
    SymlinkedDatabase,
}

impl PersistenceError {
    pub fn classify(&self) -> ConnectFailureKind {
        match self {
            PersistenceError::Migrate(sqlx::migrate::MigrateError::VersionMissing(applied)) => {
                ConnectFailureKind::SchemaTooNew {
                    applied_version: *applied,
                    max_known_version:
                        crate::store::SqliteProfileStore::max_known_migration_version(),
                }
            }
            PersistenceError::Connect(sqlx::Error::Io(io_err))
                if io_err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                ConnectFailureKind::PermissionDenied
            }
            PersistenceError::NotReadableWithKey => ConnectFailureKind::KeyMismatch,
            PersistenceError::Conversion(crate::encryption::ConversionFailure::Symlink) => {
                ConnectFailureKind::SymlinkedDatabase
            }
            PersistenceError::Conversion(crate::encryption::ConversionFailure::Io(io_err))
                if io_err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                ConnectFailureKind::PermissionDenied
            }
            PersistenceError::Conversion(_) => ConnectFailureKind::ConversionFailed,
            _ => ConnectFailureKind::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// spec-0059-Fund: empirisch gegen eine echte, künstlich "aus der
    /// Zukunft" präparierte DB verifiziert (nicht nur aus der `sqlx`-Doku
    /// vermutet), über einen inzwischen wieder entfernten Diagnose-Test —
    /// `SqliteProfileStore::connect` liefert für diesen Fall exakt
    /// `Migrate(VersionMissing(n))`. Hier nur die reine, unabhängig davon
    /// dauerhaft bestehende Klassifizierungs-Logik; kein laufender
    /// End-zu-Ende-Test dieses exakten `sqlx`-Fehlerpfads mehr in
    /// `store.rs` (spec-reviewer-Fund: dieser Kommentar behauptete
    /// fälschlich einen noch existierenden Beweis dort).
    #[test]
    fn test_classify_recognizes_schema_too_new() {
        let err = PersistenceError::Migrate(sqlx::migrate::MigrateError::VersionMissing(999_999));
        assert_eq!(
            err.classify(),
            ConnectFailureKind::SchemaTooNew {
                applied_version: 999_999,
                max_known_version: crate::store::SqliteProfileStore::max_known_migration_version(),
            }
        );
    }

    /// spec-0059-Fund: reine Klassifizierungs-Logik für den
    /// `Io(PermissionDenied)`-Fall, den `SqliteProfileStore::connect`s
    /// aktiver Schreib-Probe (`store::probe_directory_writable`, s. dort)
    /// jetzt zuverlässig erzeugt, statt sich auf einen mehrdeutigen
    /// `sqlx`-Fehlercode aus dem eigentlichen Connect-Versuch zu verlassen
    /// (spec-reviewer-Fund: `create_dir_all` allein erkennt ein bereits
    /// existierendes, aber schreibgeschütztes Verzeichnis NICHT).
    #[test]
    fn test_classify_recognizes_permission_denied() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let err = PersistenceError::Connect(sqlx::Error::Io(io_err));
        assert_eq!(err.classify(), ConnectFailureKind::PermissionDenied);
    }

    /// spec-0059-Fund: empirisch verifiziert (eine mit Müll überschriebene
    /// DB-Datei) — der Fehler kommt tatsächlich über den MIGRATE-Schritt
    /// zurück (`Migrate(Execute(Database(..., code: 26, "file is not a
    /// database")))`), nicht über `Connect` — `SqlitePoolOptions::
    /// connect_with` selbst öffnet die Datei nur, liest ihren Inhalt aber
    /// erst beim ersten echten Query (hier: der Migrations-Lauf). Fällt
    /// hier korrekt auf `Other` zurück (kein eigener Fall dafür nötig, s.
    /// `ConnectFailureKind::Other`-Doc-Kommentar).
    #[test]
    fn test_classify_falls_back_to_other_for_a_corrupt_database_file() {
        let db_err = sqlx::Error::Database(Box::new(TestDatabaseError));
        let err = PersistenceError::Migrate(sqlx::migrate::MigrateError::Execute(db_err));
        assert_eq!(err.classify(), ConnectFailureKind::Other);
    }

    #[derive(Debug)]
    struct TestDatabaseError;

    impl std::fmt::Display for TestDatabaseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "file is not a database")
        }
    }

    impl std::error::Error for TestDatabaseError {}

    impl sqlx::error::DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "file is not a database"
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    /// Kein spezifischer Fall (Spec 0059 nennt nur drei DB-bezogene Fälle
    /// namentlich) — bestätigt trotzdem den generischen Auffangfall für
    /// jeden anderen `Connect`-Fehler.
    #[test]
    fn test_classify_falls_back_to_other_for_an_unrecognized_connect_error() {
        let err = PersistenceError::Connect(sqlx::Error::PoolClosed);
        assert_eq!(err.classify(), ConnectFailureKind::Other);
    }
}
