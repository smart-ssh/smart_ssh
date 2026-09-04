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

use ssh_manager_core::entitlements::FeatureLocked;

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
    /// Spec 0037, Abschnitt 2: strukturiert eingebettet (nicht nur als
    /// `message`-String), damit das Frontend einen gesperrten Feature-Fehler
    /// eindeutig von einem fachlichen Fehler unterscheiden kann — inkl.
    /// `feature`/`tier`, nicht nur "irgendein Fehler ist aufgetreten".
    /// `None` für jeden anderen Fehler (die weit überwiegende Mehrheit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_locked: Option<FeatureLocked>,
}

impl CommandError {
    pub fn with_code(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            feature_locked: None,
        }
    }

    /// Spec 0037, Abschnitt 3 (D5): Gating-Konvention. Kein `impl
    /// From<FeatureLocked> for CommandError` (und damit kein bloßes `?` wie
    /// in der Spec-Skizze): `FeatureLocked` implementiert `Display` (über
    /// `thiserror`), der blanket `impl<E: Display> From<E>` unten deckt es
    /// also bereits ab — ein zweiter, spezifischerer `From`-Impl für exakt
    /// diesen einen `Display`-Typ wäre eine von Rusts Kohärenzregeln
    /// verbotene überlappende Impl (E0119). Ein gegatetes Command ruft
    /// deshalb explizit `.map_err(CommandError::feature_locked)?` statt nur
    /// `?` auf `require(...)`.
    ///
    /// `#[allow(dead_code)]`: noch kein einziger tatsächlich gegateter
    /// Command in diesem Schritt (s. `AppState::entitlements`-Doc-
    /// Kommentar) — bleibt bis zum ersten echten `require(...)`-Aufruf
    /// unvermeidlich ungenutzt.
    #[allow(dead_code)]
    pub fn feature_locked(err: FeatureLocked) -> Self {
        Self {
            message: err.to_string(),
            code: Some("FEATURE_LOCKED"),
            feature_locked: Some(err),
        }
    }
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self {
            message: err.to_string(),
            code: None,
            feature_locked: None,
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
            "FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED",
            "SERVER_JUMP_HOST_LOCAL",
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener CommandError-Code: {codes:?}"
        );
    }
}

#[cfg(test)]
mod feature_locked_tests {
    use super::*;
    use ssh_manager_core::entitlements::{Feature, Tier};

    /// Spec 0037, Abschnitt 2: `FeatureLocked` muss strukturiert (nicht nur
    /// als generischer `message`-String über den blanket `From<E: Display>`)
    /// im `CommandError` landen, damit das Frontend ihn eindeutig von
    /// fachlichen Fehlern unterscheiden kann.
    #[test]
    fn test_command_error_feature_locked_carries_structured_feature_and_tier() {
        let err = ssh_manager_core::entitlements::FeatureLocked {
            feature: Feature::DocumentExport,
            tier: Tier::Free,
        };

        let command_error = CommandError::feature_locked(err);

        assert_eq!(command_error.code, Some("FEATURE_LOCKED"));
        let feature_locked = command_error
            .feature_locked
            .expect("feature_locked muss gesetzt sein");
        assert_eq!(feature_locked.feature, Feature::DocumentExport);
        assert_eq!(feature_locked.tier, Tier::Free);
    }

    /// Gegentest: ein gewöhnlicher Fehler (über den blanket `From<E:
    /// Display>`) darf `feature_locked` nicht setzen — sonst könnte das
    /// Frontend jeden Fehler fälschlich als gesperrtes Feature behandeln.
    #[test]
    fn test_ordinary_error_leaves_feature_locked_none() {
        let command_error: CommandError = "irgendein fachlicher Fehler".into();

        assert!(command_error.feature_locked.is_none());
        assert_eq!(command_error.code, None);
    }
}
