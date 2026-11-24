//! `host-key-verification-needed` event (Spec 0100).

use serde::Serialize;

use super::{emit, EventEmitter};
use crate::event_names as names;
use crate::state::SessionId;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum HostKeyKind {
    Unknown,
    Mismatch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub(crate) struct HostKeyVerificationNeededPayload {
    session_id: SessionId,
    /// Issue #37: Kennung genau dieser Abfrage (Generation der
    /// Registrierung in `pending_host_key_confirmations`). Ein
    /// `connect()`-Retry kann unter derselben `session_id` eine neue Abfrage
    /// anlegen; `host-key-verification-ended` trägt dieselbe Kennung, damit
    /// das Frontend nur die passende Abfrage schließt.
    prompt_id: u64,
    host: String,
    port: u16,
    kind: HostKeyKind,
    fingerprint: String,
    /// Nur bei `kind: Mismatch` gesetzt. Für den Mismatch-Fall (Spec 0005
    /// Abschnitt 6.3, Warnung „Bekannt"/„Jetzt angeboten") reicht ein
    /// einzelnes `fingerprint`-Feld nicht, um dem Frontend alten und neuen
    /// Fingerprint nebeneinander zeigen zu lassen.
    expected_fingerprint: Option<String>,
    /// Algorithm name of the key the server offers now (e.g.
    /// `ssh-ed25519`); `null` when it cannot be determined. Display-only.
    key_type: Option<String>,
    /// Name the user gave the saved server this connection belongs to;
    /// `None` when there is none. Display-only: never used for the trust
    /// decision or the known-hosts entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    server_name: Option<String>,
}

#[expect(clippy::too_many_arguments, reason = "one argument per input")]
pub fn emit_host_key_verification_needed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    prompt_id: u64,
    host: String,
    port: u16,
    kind: HostKeyKind,
    fingerprint: String,
    expected_fingerprint: Option<String>,
    key_type: Option<String>,
    server_name: Option<String>,
) {
    emit(
        emitter,
        names::HOST_KEY_VERIFICATION_NEEDED,
        &HostKeyVerificationNeededPayload {
            session_id,
            prompt_id,
            host,
            port,
            kind,
            fingerprint,
            expected_fingerprint,
            key_type,
            server_name,
        },
    );
}
