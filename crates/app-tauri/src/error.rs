//! Fehlertyp für Tauri-Commands. Tauri serialisiert das `Err`-Ergebnis
//! eines `#[tauri::command]` per `serde::Serialize` zurück ans Frontend —
//! ein flacher `{ message: String }` reicht für Teil 1 (die einzelnen
//! Rust-Fehlertypen aus `core`/`persistence-sqlite` unterscheiden sich zu
//! sehr, um sie 1:1 über die IPC-Grenze zu spiegeln; das Frontend braucht
//! ohnehin nur eine anzeigbare Meldung). Ein blanket `From<E: Display>`
//! deckt alle projektinternen Fehlertypen ab (`ProfileError`,
//! `CredentialError`, `persistence_sqlite::AiProviderStoreError`,
//! `keyring`-Fehler über `credentials-keyring`), die allesamt `Display`
//! implementieren — kein separater `From`-Impl pro Fehlertyp nötig.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
    /// Spec 0024, Abschnitt 5: stabiler, sprachunabhängiger Bezeichner fürs
    /// Frontend-Mapping auf Übersetzungs-Keys — `None` für die (weit
    /// überwiegende) Mehrheit der Fehler, die weiterhin nur über den
    /// blanket `From<E: Display>`-Impl unten entstehen (unverändert wie vor
    /// Spec 0024: `message` reicht dafür aus, ein `code` pro Fehlertyp wäre
    /// hier unverhältnismäßiger Aufwand, s. Moduldoc). Gezielt `Some` nur an
    /// den Stellen, die explizit `CommandError::with_code` verwenden — aktuell
    /// die Validierungsfehler aus den Server-/Gruppen-Formularen (Spec 0008,
    /// s. `groups.rs`/`server_credentials.rs`).
    pub code: Option<&'static str>,
}

impl CommandError {
    pub fn with_code(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self {
            message: err.to_string(),
            code: None,
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod code_tests {
    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Validierungsfehler doppelt
    /// vergeben sein. Enumeriert alle aktuell über `CommandError::with_code`
    /// vergebenen Codes (Server-/Gruppen-Formulare, Spec 0008); s.
    /// `groups.rs`/`server_credentials.rs` für die jeweiligen
    /// Einzeltests, die zusätzlich prüfen, dass der *richtige* Code am
    /// jeweiligen Fehlerfall hängt.
    #[test]
    fn test_command_error_with_code_values_are_unique() {
        let codes = [
            "GROUP_SELF_PARENT",
            "GROUP_CYCLE_DETECTED",
            "SERVER_PASSWORD_REQUIRED",
            "SERVER_PRIVATE_KEY_REQUIRED",
            "SERVER_CERTIFICATE_REQUIRED",
            "SERVER_CERTIFICATE_KEY_REQUIRED",
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(codes.len(), unique.len(), "doppelt vergebener CommandError-Code: {codes:?}");
    }
}
