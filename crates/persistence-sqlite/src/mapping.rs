use chrono::{DateTime, Utc};
use ssh_manager_core::profiles::{AuthMethod, ProfileError};
use uuid::Uuid;

/// `auth_method`-Spalte (Spec 0004 Abschnitt 4): JSON-serialisiertes
/// [`AuthMethod`]. Enthält ausschließlich `CredentialRef`-Strings, nie
/// Secrets (s. Spec 0003 Abschnitt 4) — unbedenklich als Klartext-JSON in
/// der DB.
pub(crate) fn auth_method_to_json(auth: &AuthMethod) -> Result<String, ProfileError> {
    serde_json::to_string(auth).map_err(|e| {
        ProfileError::Backend(format!("AuthMethod-Serialisierung fehlgeschlagen: {e}"))
    })
}

pub(crate) fn auth_method_from_json(json: &str) -> Result<AuthMethod, ProfileError> {
    serde_json::from_str(json).map_err(|e| {
        ProfileError::Backend(format!("AuthMethod-Deserialisierung fehlgeschlagen: {e}"))
    })
}

/// Alle `id`/`*_id`-Spalten sind `TEXT` (hyphenierte UUID-Strings, siehe
/// Migration) — bewusst manuell geparst statt sqlx' `uuid`-Feature direkt
/// gegen die Spalte zu binden. Grund: sqlx codiert `Uuid` gegen SQLite
/// typischerweise als 16-Byte-BLOB, nicht als lesbaren Text; das würde dem
/// in der Spec explizit genannten Ziel widersprechen, die DB beim manuellen
/// Debuggen lesbar zu halten (Abschnitt 4, "Designentscheidungen"). Manuelles
/// Parsen/Formatieren über `to_string()`/`parse_str()` garantiert echte
/// Text-Repräsentation.
pub(crate) fn parse_uuid(raw: &str, column: &str) -> Result<Uuid, ProfileError> {
    Uuid::parse_str(raw)
        .map_err(|e| ProfileError::Backend(format!("ungültige UUID in Spalte {column}: {e}")))
}

/// Timestamps sind `TEXT` im ISO-8601/RFC-3339-Format (Migrations-Kommentar:
/// "für bessere Lesbarkeit... und verlustfreies Round-tripping mit
/// `chrono::DateTime<Utc>`"). `to_rfc3339()`/`parse_from_rfc3339()` bilden
/// das direkt ab.
pub(crate) fn parse_timestamp(raw: &str, column: &str) -> Result<DateTime<Utc>, ProfileError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            ProfileError::Backend(format!("ungültiger Zeitstempel in Spalte {column}: {e}"))
        })
}
