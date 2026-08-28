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
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistenceError::Connect(e) => write!(f, "Datenbankverbindung fehlgeschlagen: {e}"),
            PersistenceError::Migrate(e) => write!(f, "Migration fehlgeschlagen: {e}"),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersistenceError::Connect(e) => Some(e),
            PersistenceError::Migrate(e) => Some(e),
        }
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
