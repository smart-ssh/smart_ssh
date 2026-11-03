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
    /// Spec 0024, section 5: stable, language-independent identifier the
    /// frontend maps to a translation. Every conversion sets it (via
    /// [`ErrorCode`] or [`CommandError::with_code`]); it is an `Option` only
    /// to keep the pinned JSON shape (`null`) readable for old payloads.
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

    /// Spec 0037, Abschnitt 3 (D5): gating convention. A gated command uses
    /// `?` on the `FeatureLocked` (the `From` impl calls this).
    pub fn feature_locked(err: FeatureLocked) -> Self {
        Self {
            message: err.to_string(),
            code: Some("FEATURE_LOCKED"),
            feature_locked: Some(err),
            connect_log: None,
        }
    }
}

/// An error type that can be returned to the frontend through `?`. The
/// `code` is the stable, language-independent identifier the frontend maps to
/// a translation (`errorCodes.ts`); a code without a translation falls back to
/// the message.
///
/// Every error type that reaches a command result implements this (there is no
/// blanket `From<E: Display>`, so a conversion that would drop the code does
/// not compile). One code per variant where the UI can act on it, otherwise
/// one per type. List the codes in a `const CODES: &[&str]` of the type and
/// register that in the uniqueness test. An extension crate adds its own
/// errors by implementing this trait on them; the `From` below then applies.
pub trait ErrorCode {
    /// The stable code of this error value.
    fn code(&self) -> &'static str;
}

impl<E: ErrorCode + std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self::with_code(err.to_string(), err.code())
    }
}

/// A filesystem error that reaches a command through `?` without a more
/// specific mapping: one code for the type (Spec 0024, section 5). The
/// message is the OS error text, as before.
impl ErrorCode for std::io::Error {
    fn code(&self) -> &'static str {
        IO_FAILED
    }
}

/// Code of a plain [`std::io::Error`] converted through `?`.
pub const IO_FAILED: &str = "IO_FAILED";

impl From<FeatureLocked> for CommandError {
    fn from(err: FeatureLocked) -> Self {
        Self::feature_locked(err)
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entitlements::{Feature, TierId};

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
            "with_code": CommandError::with_code("Group cannot be its own parent", "GROUP_SELF_PARENT"),
            "feature_locked": CommandError::feature_locked(FeatureLocked {
                feature: Feature::new("document_export"),
                tier: TierId::FREE,
            }),
            "connect_log": CommandError::with_code("Connection failed", "SSH_CONNECT_FAILED")
                .with_connect_log(vec![rec]),
        });
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/command_error_shapes.json"))
                .unwrap();
        assert_eq!(actual, fixture);
    }

    struct Probe;
    impl std::fmt::Display for Probe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("probe failed")
        }
    }
    impl ErrorCode for Probe {
        fn code(&self) -> &'static str {
            "PROBE_FAILED"
        }
    }

    #[test]
    fn question_mark_keeps_the_error_code() {
        fn run() -> CommandResult<()> {
            Err(Probe)?
        }
        let e = run().unwrap_err();
        assert_eq!(e.code, Some("PROBE_FAILED"));
        assert_eq!(e.message, "probe failed");
    }

    #[test]
    fn question_mark_on_feature_locked_sets_code_and_payload() {
        fn run() -> CommandResult<()> {
            Err(FeatureLocked {
                feature: Feature::new("document_export"),
                tier: TierId::FREE,
            })?
        }
        let e = run().unwrap_err();
        assert_eq!(e.code, Some("FEATURE_LOCKED"));
        assert!(e.feature_locked.is_some());
    }
}
