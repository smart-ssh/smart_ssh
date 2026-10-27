//! The error type Tauri commands (and extension commands) return to the
//! frontend: `{ message, code, feature_locked, connect_log }` (Spec 0024 §5,
//! Spec 0037 §2, Spec 0038). It lives here, Tauri-free, so every crate can
//! use it. The JSON shape is pinned by
//! `tests/fixtures/command_error_shapes.json`, which the frontend parser
//! test reads as well.

use serde::Serialize;

use crate::entitlements::FeatureLocked;
use crate::ssh::ConnectStepRecord;

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
    /// Issue #51: das Schritt-Protokoll eines gescheiterten
    /// Verbindungsaufbaus (`connect`), damit die Oberfläche es unter der
    /// Fehlermeldung zeigen kann. `None` für jeden anderen Fehler.
    ///
    /// Nur für die Oberfläche: Wer einen `CommandError` loggt, loggt
    /// `code`/`message`, nie dieses Feld (Spec 0094).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_log: Option<Vec<ConnectStepRecord>>,
}

impl CommandError {
    /// A plain message without a code (what the blanket `From` produces).
    pub fn message_only(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            feature_locked: None,
            connect_log: None,
        }
    }

    pub fn with_code(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            feature_locked: None,
            connect_log: None,
        }
    }

    /// Issue #51: hängt das Schritt-Protokoll eines Verbindungsversuchs an.
    pub fn with_connect_log(mut self, steps: Vec<ConnectStepRecord>) -> Self {
        self.connect_log = Some(steps);
        self
    }

    /// Spec 0037, Abschnitt 3 (D5): gating convention. There is no
    /// `impl From<FeatureLocked>` (the blanket `From<E: Display>` below
    /// already covers it and a second impl would overlap, E0119), so a gated
    /// command calls `.map_err(CommandError::feature_locked)?` explicitly.
    pub fn feature_locked(err: FeatureLocked) -> Self {
        Self {
            message: err.to_string(),
            code: Some("FEATURE_LOCKED"),
            feature_locked: Some(err),
            connect_log: None,
        }
    }
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self {
            message: err.to_string(),
            code: None,
            feature_locked: None,
            connect_log: None,
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entitlements::{Feature, Tier};

    #[test]
    fn json_shapes_match_fixture() {
        let rec = ConnectStepRecord {
            hop_index: 0,
            hop: "alice@example.org:22".into(),
            step: crate::ssh::ConnectStep::DnsResolution {
                host: "example.org".into(),
                port: 22,
                addresses: vec!["192.0.2.1".into()],
            },
            status: crate::ssh::StepStatus::Failed {
                code: "DNS_FAILED".into(),
            },
            duration_ms: Some(12),
        };
        let actual = serde_json::json!({
            "message_only": CommandError::from("Something failed"),
            "with_code": CommandError::with_code("Group cannot be its own parent", "GROUP_SELF_PARENT"),
            "feature_locked": CommandError::feature_locked(FeatureLocked {
                feature: Feature::new("document_export"),
                tier: Tier::Free,
            }),
            "connect_log": CommandError::with_code("Connection failed", "SSH_CONNECT_FAILED")
                .with_connect_log(vec![rec]),
        });
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/command_error_shapes.json"))
                .unwrap();
        assert_eq!(actual, fixture);
    }

    #[test]
    fn message_only_has_no_code() {
        let e = CommandError::message_only("x");
        assert_eq!(e.message, "x");
        assert!(e.code.is_none());
    }
}
