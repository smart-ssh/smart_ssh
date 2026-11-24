//! Leaf module (Issue #868): the deterministic keychain slot names of a
//! server, so `dto` can ask for them without depending on
//! `server_credentials`. `server_credentials` re-exports both functions.

use ssh_manager_core::profiles::CredentialRef;
use ssh_manager_core::shared::ServerId;

/// Deterministischer `CredentialRef` pro `(server_id, slot)` — kein
/// zusätzlicher Zustand nötig, um sich "den Ref von vorhin" zu merken; bei
/// `update_server` wird derselbe String einfach erneut berechnet.
///
/// `pub(crate)` seit Spec 0082, damit die Tests des Bearbeiten-Ablaufs in
/// [`crate::servers`] dieselbe Berechnung benutzen wie der Produktivcode,
/// statt das Format `server:{id}:{slot}` ein zweites Mal hinzuschreiben —
/// eine Kopie würde bei einer Änderung hier still danebenliegen und die
/// Tests gegen Slots prüfen lassen, die es gar nicht gibt.
pub(crate) fn credential_ref(server_id: ServerId, slot: &str) -> CredentialRef {
    CredentialRef::new(format!("server:{}:{slot}", server_id.0))
}

/// Spec 0018, Abschnitt 4: eigener, vom Login-Auth-Secret unabhängiger Slot
/// — ein Server hat unabhängig von seiner `AuthMethod` (Passwort/Key/Agent/
/// Zertifikat) höchstens **ein** optionales Sudo-Passwort. Deterministisch
/// wie die übrigen Slots: kein eigenes DB-Feld nötig, "ist eines
/// hinterlegt" wird per `CredentialStore::get(...).is_ok()` ermittelt (s.
/// `crate::dto::ServerDto::has_sudo_password`).
pub fn sudo_password_credential_ref(server_id: ServerId) -> CredentialRef {
    credential_ref(server_id, "sudo_password")
}
